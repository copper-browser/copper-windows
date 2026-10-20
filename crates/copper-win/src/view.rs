//! The shell's view of the window (`ShellView`): every notification the
//! shell sends is turned into an edit of the chrome. None blocks; none
//! calls back into the shell from inside the window's state (see
//! [`crate::chrome`] on re-entrancy).

use crate::chrome::Chrome;
use crate::chrome::model::Model;
use crate::commands::Registry;
use crate::frame::WinFrame;
use crate::panels::Panels;
use crate::picture;
use crate::platform::{Icon, Shell, Win};
use crate::sys;
use copper_core::browser::TabId;
use copper_core::settings::{Look, TabsPosition};
use copper_engine::Engine;
use copper_engine_webview2::WebView2Page;
use copper_shell_core::palette::complete::Purpose;
use copper_shell_core::shell::pages::PageSlot;
use copper_shell_core::{Question, Refresh, ShellView};
use copper_win_ui::Appearance;
use copper_win_ui::win::FocusReason;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

/// The window, as the shell sees it.
pub struct WinView {
    chrome: Rc<Chrome>,
    commands: Registry,
    panels: Panels,
    layout: Cell<TabsPosition>,
    /// The window before it went full screen.
    fullscreen: RefCell<Option<sys::window::Saved>>,
    /// Full screen because a page asked (a video): the chrome steps away.
    immersed: Cell<bool>,
    presented: Cell<bool>,
}

impl WinView {
    pub fn new(chrome: Rc<Chrome>) -> WinView {
        WinView {
            chrome,
            commands: Registry::default(),
            panels: Panels::default(),
            layout: Cell::new(TabsPosition::Left),
            fullscreen: RefCell::new(None),
            immersed: Cell::new(false),
            presented: Cell::new(false),
        }
    }

    pub fn chrome(&self) -> &Rc<Chrome> {
        &self.chrome
    }

    pub fn commands(&self) -> &Registry {
        &self.commands
    }

    pub fn panels(&self) -> &Panels {
        &self.panels
    }

    pub fn layout(&self) -> TabsPosition {
        self.layout.get()
    }

    /// Put `text` on the clipboard.
    pub fn copy_text(&self, text: &str) {
        let text = text.to_string();
        self.chrome.edit(move |ui, _| {
            ui.copy_text(&text);
        });
    }

    /// Whether the chrome has something open over the page changed: note
    /// it (Escape from the page is the chrome's while it does).
    pub fn overlay_changed(&self) {
        let c = self.chrome.clone();
        let panel = self.panels.showing().is_some();
        self.chrome.edit(move |_, st| {
            c.set_overlay(st.card.is_open() || st.sheet.is_open() || panel);
        });
    }

    /// Give the shown page keyboard focus (unless the card is up).
    pub fn focus_page(&self) {
        self.chrome.edit(|ui, st| {
            if st.card.is_open() {
                return;
            }
            if let Some(slot) = st.stage.shown().and_then(|t| st.stage.slot_of(t)) {
                ui.focus_page(slot, FocusReason::Programmatic);
            }
        });
    }

    /// Escape: close what is on top, else stop the page.
    pub fn escape(&self, shell: &Shell) {
        let Some((card, sheet)) = self.chrome.read(|ui, st| {
            if st.card.is_open() {
                st.card.close(ui);
                return (true, None);
            }
            (false, st.sheet.refuse(ui))
        }) else {
            return;
        };
        if card {
            self.overlay_changed();
            self.focus_page();
            return;
        }
        if let Some(answer) = sheet {
            self.overlay_changed();
            answer(false);
            return;
        }
        if crate::find::close(shell) {
            return;
        }
        if self.panels.hide(shell) {
            return;
        }
        if self.immersed.get() || self.fullscreen.borrow().is_some() {
            self.set_fullscreen(false, false);
            return;
        }
        shell.stop_active();
    }

    /// Fold the column away, or bring it back.
    pub fn toggle_fold(&self) {
        self.chrome.edit(|ui, st| {
            st.folded = !st.folded;
            crate::chrome::events::apply_layout(ui, st);
        });
    }

    /// F11: the window covers its monitor, or comes back.
    pub fn toggle_fullscreen(&self) {
        let on = self.fullscreen.borrow().is_none();
        self.set_fullscreen(on, false);
    }

    /// A page went full screen (`immersed`: the chrome steps away) or the
    /// user asked for it.
    pub fn set_fullscreen(&self, on: bool, immersed: bool) {
        let hwnd = self.chrome.window().hwnd();
        if on {
            if self.fullscreen.borrow().is_none() {
                let saved = sys::window::enter_fullscreen(hwnd);
                *self.fullscreen.borrow_mut() = saved;
            }
        } else if let Some(saved) = self.fullscreen.borrow_mut().take() {
            sys::window::leave_fullscreen(hwnd, saved);
        }
        let immersed = on && immersed;
        if immersed != self.immersed.get() {
            self.immersed.set(immersed);
            self.chrome.edit(move |ui, st| {
                st.immersed = immersed;
                crate::chrome::events::apply_layout(ui, st);
            });
        }
    }

    /// Redraw the column / strip from the shell's state.
    pub fn sync(&self, shell: &Shell) {
        let model = Model::of(shell);
        let title = model
            .active
            .as_ref()
            .and_then(|id| shell.tab_view(id))
            .map(|t| t.label)
            .filter(|t| !t.is_empty());
        self.chrome.edit(move |ui, st| {
            ui.set_space(model.tint);
            if st.side {
                st.column.sync(ui, &mut st.images, &model);
            } else {
                st.strip.sync(ui, &mut st.images, &model);
            }
            st.images.sweep(ui);
        });
        let title = match title {
            Some(t) => format!("{t} — Copper"),
            None => "Copper".to_string(),
        };
        self.chrome.window().set_title(&title);
    }

    /// Wire a new page's keys, focus and full screen to the chrome.
    fn hook_page(&self, shell: &Shell, tab: &TabId, page: &WebView2Page) {
        crate::chrome::events::hook_page(shell, tab, page);
    }
}

impl ShellView<Win> for WinView {
    fn new_frame(&self, shell: &Shell, tab: &TabId, page: &WebView2Page) -> WinFrame {
        self.hook_page(shell, tab, page);
        WinFrame::new(self.chrome.clone(), tab.clone())
    }

    fn page_added(&self, _shell: &Shell, slot: &Rc<PageSlot<Win>>) {
        let tab = slot.id.clone();
        let page = slot.page.clone();
        self.chrome
            .edit(move |ui, st| st.stage.add_page(ui, &tab, page));
    }

    fn page_removed(&self, _shell: &Shell, slot: &Rc<PageSlot<Win>>) {
        let tab = slot.id.clone();
        self.chrome
            .edit(move |ui, st| st.stage.remove_page(ui, &tab));
    }

    fn show_tab(&self, shell: &Shell, tab: Option<&TabId>) {
        let tab = tab.cloned();
        self.chrome.watch("tab switch");
        let backstage = shell.engine().backstage().clone();
        let chrome = self.chrome.clone();
        let panel = self.panels.showing().is_some();
        self.chrome.edit(move |ui, st| {
            st.stage.show(ui, tab.as_ref(), &backstage);
            match &tab {
                // The first tab puts away the card the empty stage opened.
                Some(_) if st.card.is_auto() => {
                    st.card.close(ui);
                }
                Some(_) => {}
                None if !st.card.is_open() => {
                    st.card.open(ui, Purpose::Launch, false, "", true);
                }
                None => {}
            }
            chrome.set_overlay(st.card.is_open() || st.sheet.is_open() || panel);
            if let Some(t) = &tab
                && !st.card.is_open()
                && let Some(slot) = st.stage.slot_of(t)
            {
                ui.focus_page(slot, FocusReason::Programmatic);
            }
        });
        crate::find::tab_changed(shell);
    }

    fn refresh(&self, shell: &Shell, _what: &Refresh) {
        self.sync(shell);
    }

    fn set_layout(&self, shell: &Shell, position: TabsPosition) {
        self.layout.set(position);
        let side = position == TabsPosition::Left;
        self.chrome.edit(move |ui, st| {
            st.side = side;
            crate::chrome::events::apply_layout(ui, st);
        });
        self.sync(shell);
    }

    fn set_progress(&self, loading: bool, fraction: f64) {
        self.chrome
            .edit(move |ui, st| st.stage.set_progress(ui, loading, fraction));
        if let Some(shell) = self.chrome.shell() {
            // The reload door becomes stop and back.
            let nav = Model::nav_of(&shell);
            self.chrome.edit(move |ui, st| {
                st.column.sync_nav(ui, nav);
                st.strip.sync_nav(ui, nav);
            });
        }
    }

    fn set_hover(&self, url: Option<&str>) {
        let url = url.map(str::to_string);
        self.chrome
            .edit(move |ui, st| st.stage.set_hover(ui, url.as_deref()));
    }

    fn apply_look(&self, look: Look) {
        let a = match look {
            Look::Light => Appearance::Light,
            Look::Dark => Appearance::Dark,
            Look::System => Appearance::System,
        };
        let chrome = self.chrome.clone();
        self.chrome.edit(move |ui, _| {
            ui.set_appearance(a);
            chrome.set_dark(ui.is_dark());
        });
    }

    fn is_dark(&self) -> bool {
        self.chrome.is_dark()
    }

    fn toast(&self, text: &str) {
        let text = text.to_string();
        let wait = crate::chrome::toast::seconds(&text);
        let shown = Rc::new(Cell::new(0));
        let g = shown.clone();
        self.chrome
            .edit(move |ui, st| g.set(st.toast.show(ui, &text)));
        let chrome = Rc::downgrade(&self.chrome);
        copper_win_rt::spawn_local(async move {
            copper_win_rt::sleep(wait).await;
            if let Some(c) = chrome.upgrade() {
                let g = shown.get();
                c.edit(move |ui, st| st.toast.hide(ui, g));
            }
        });
    }

    fn ask(&self, _shell: &Shell, question: Question, answer: Box<dyn FnOnce(bool)>) {
        self.chrome.edit(move |ui, st| {
            st.sheet.ask(ui, question, answer);
        });
        self.overlay_changed();
    }

    fn present(&self, shell: &Shell) {
        if self.presented.replace(true) {
            crate::sys::window::bring_forward(self.chrome.window().hwnd());
            return;
        }
        crate::app::show_window(shell, &self.chrome);
    }

    fn is_online(&self) -> bool {
        sys::network::is_online()
    }

    fn showing_downloads(&self) -> bool {
        self.panels.showing() == Some("downloads")
    }

    fn hide_panel(&self) {
        if let Some(shell) = self.chrome.shell() {
            self.panels.hide(&shell);
        }
    }

    fn screenshot(&self) -> Option<Vec<u8>> {
        sys::screenshot::capture(self.chrome.window().hwnd())
    }

    fn open_settings(&self, shell: &Shell, page: &str) {
        crate::settings::open(shell, page);
    }

    fn resize_window(&self, width: i32, height: i32) {
        sys::window::resize(self.chrome.window().hwnd(), width, height);
    }

    fn decode_icon(&self, png: &[u8]) -> Option<Icon> {
        picture::icon(png).map(Rc::new)
    }

    fn load_icon(&self, path: &Path) -> Option<Icon> {
        let bytes = std::fs::read(path).ok()?;
        picture::icon(&bytes).map(Rc::new)
    }
}
