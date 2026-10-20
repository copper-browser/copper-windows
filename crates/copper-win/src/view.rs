//! The shell's view of the window (`ShellView`): every notification the
//! shell sends is turned into an edit of the chrome. None blocks; none
//! calls back into the shell from inside the window's state (see
//! [`crate::chrome`] on re-entrancy).

use crate::chrome::Chrome;
use crate::chrome::model::Model;
use crate::chrome::permit::Permits;
use crate::chrome::toast::{DownloadNews, download_words};
use crate::commands::Registry;
use crate::frame::WinFrame;
use crate::panels::Panels;
use crate::picture;
use crate::platform::{Icon, Shell, Win};
use crate::sys;
use copper_core::browser::TabId;
use copper_core::settings::{Look, TabsPosition};
use copper_engine::{DownloadState, Engine};
use copper_engine_webview2::{ColorScheme, Ground, WebView2Page};
use copper_shell_core::palette::complete::Purpose;
use copper_shell_core::shell::pages::PageSlot;
use copper_shell_core::{Question, Refresh, ShellView};
use copper_win_ui::Appearance;
use copper_win_ui::win::FocusReason;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

/// The window, as the shell sees it.
pub struct WinView {
    chrome: Rc<Chrome>,
    commands: Registry,
    panels: Panels,
    layout: Cell<TabsPosition>,
    /// The window before it went full screen.
    fullscreen: RefCell<Option<sys::window::Saved>>,
    /// F11: the window covers its monitor and keeps its chrome.
    browser_full: Cell<bool>,
    /// The tab whose page is full screen (a video): the window covers its
    /// monitor and the chrome steps away. Independent of F11: the page
    /// leaving gives back whatever F11 had.
    immersed: RefCell<Option<TabId>>,
    presented: Cell<bool>,
    /// A download that just finished or failed, for the announcement the
    /// shell makes of it next ([`WinView::toast`]).
    download_news: RefCell<Option<DownloadNews>>,
    /// The site question on show, if any (`chrome::permit`).
    permits: RefCell<Permits>,
    /// When the last download began (a load that ends just then was
    /// handed to it, [`crate::frame`]).
    download_began: Cell<Option<Instant>>,
}

impl WinView {
    pub fn new(chrome: Rc<Chrome>) -> WinView {
        WinView {
            chrome,
            commands: Registry::default(),
            panels: Panels::default(),
            layout: Cell::new(TabsPosition::Left),
            fullscreen: RefCell::new(None),
            browser_full: Cell::new(false),
            immersed: RefCell::new(None),
            presented: Cell::new(false),
            download_news: RefCell::new(None),
            permits: RefCell::new(Permits::default()),
            download_began: Cell::new(None),
        }
    }

    /// When the last download began.
    pub fn download_began(&self) -> Option<Instant> {
        self.download_began.get()
    }

    /// The site question on show (`chrome::permit`).
    pub fn permits(&self) -> &RefCell<Permits> {
        &self.permits
    }

    /// Listen to the downloads, so the shell's announcements of them come
    /// out in the Mac's words. The listener runs just before the shell's
    /// own handling, which announces synchronously.
    pub fn listen_downloads(shell: &Shell) {
        let weak = shell.downgrade();
        shell.downloads().subscribe(move |_, ev| {
            let Some(shell) = weak.upgrade() else { return };
            if matches!(ev.state, DownloadState::Started { .. }) {
                shell.view().download_began.set(Some(Instant::now()));
            }
            let news = match &ev.state {
                DownloadState::Finished { path, .. } => Some(DownloadNews::Finished(
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )),
                DownloadState::Failed {
                    cancelled: false, ..
                } => Some(DownloadNews::Failed),
                _ => None,
            };
            let pending = news.is_some();
            shell.view().download_news.replace(news);
            if pending {
                // Only for the announcement made right now: by the loop's
                // next turn the shell has spoken or chose not to.
                let weak = shell.downgrade();
                copper_win_rt::spawn_local(async move {
                    if let Some(s) = weak.upgrade() {
                        s.view().download_news.take();
                    }
                });
            }
        });
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

    /// Ask where to save each download, or not, as Settings says
    /// (`downloads.ask`); the Save As dialog belongs to this window.
    /// Settings › Downloads calls this when the switch changes.
    pub fn apply_download_ask(&self, shell: &Shell) {
        let ask = shell.settings().downloads_ask();
        shell
            .engine()
            .set_download_ask(ask, self.chrome.window().hwnd());
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
        if crate::chrome::letter::close(shell) {
            return;
        }
        if crate::find::close(shell) {
            return;
        }
        if self.panels.hide(shell) {
            return;
        }
        // A page's full screen first, then the window's.
        if self.immersed.borrow().is_some() {
            self.end_immersion(shell);
            return;
        }
        if self.browser_full.get() {
            self.browser_full.set(false);
            self.apply_fullscreen();
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

    /// F11: the window covers its monitor with its chrome, or comes back.
    /// Over a page that is full screen, F11 first gives the page back its
    /// place.
    pub fn toggle_fullscreen(&self) {
        let immersed = self.immersed.borrow().is_some();
        if immersed && let Some(shell) = self.chrome.shell() {
            self.end_immersion(&shell);
            return;
        }
        self.browser_full.set(!self.browser_full.get());
        self.apply_fullscreen();
    }

    /// `tab`'s page went full screen or came back. Only the shown tab's
    /// page takes the screen.
    pub fn page_fullscreen(&self, shell: &Shell, tab: &TabId, on: bool) {
        let mine = self.immersed.borrow().as_ref() == Some(tab);
        if on && shell.active_id().as_ref() == Some(tab) {
            *self.immersed.borrow_mut() = Some(tab.clone());
        } else if !on && mine {
            *self.immersed.borrow_mut() = None;
        } else {
            return;
        }
        self.apply_fullscreen();
    }

    /// The page that is full screen leaves it (Escape, F11, another tab):
    /// the page's state ends and the chrome comes back; F11's stays.
    fn end_immersion(&self, shell: &Shell) {
        let Some(tab) = self.immersed.borrow_mut().take() else {
            return;
        };
        if let Some(page) = shell.page(&tab) {
            use copper_engine::PageView;
            let done = page.eval(
                "document.fullscreenElement && document.exitFullscreen()",
                &copper_engine::World::Page,
            );
            copper_win_rt::spawn_local(async move {
                let _ = done.await;
            });
        }
        self.apply_fullscreen();
    }

    /// The window and the chrome as F11 and the page's full screen want.
    fn apply_fullscreen(&self) {
        let hwnd = self.chrome.window().hwnd();
        let immersed = self.immersed.borrow().is_some();
        let on = immersed || self.browser_full.get();
        if on {
            if self.fullscreen.borrow().is_none() {
                let saved = sys::window::enter_fullscreen(hwnd);
                *self.fullscreen.borrow_mut() = saved;
            }
        } else if let Some(saved) = self.fullscreen.borrow_mut().take() {
            sys::window::leave_fullscreen(hwnd, saved);
        }
        self.chrome.edit(move |ui, st| {
            // A hovered address does not follow the page into full screen.
            st.stage.set_hover(ui, None);
            ui.set_fullscreen(on);
            ui.set_blackout(immersed);
            if st.immersed != immersed || st.full != on {
                st.immersed = immersed;
                st.full = on;
                crate::chrome::events::apply_layout(ui, st);
            }
        });
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
        let tint = if shell.active_is_private() {
            Some(crate::chrome::model::private_tint())
        } else {
            model.tint
        };
        self.chrome.edit(move |ui, st| {
            ui.set_space(tint);
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
        if let Some(slot) = tab.and_then(|t| shell.slot(t)) {
            slot.frame.picked();
        }
        // Another tab ends the page's full screen, not F11's.
        let left = self
            .immersed
            .borrow()
            .as_ref()
            .is_some_and(|t| Some(t) != tab);
        if left {
            self.end_immersion(shell);
        }
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
        // Windows' theme changed (`dark_changed`): the pages' ground follows.
        shell
            .engine()
            .set_page_ground(page_ground(self.chrome.is_dark()));
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
        let chrome = self.chrome.clone();
        self.chrome.edit(move |ui, st| {
            // The line rests full for 220 ms before it goes.
            if let Some(ticket) = st.stage.set_progress(ui, loading, fraction) {
                let weak = Rc::downgrade(&chrome);
                copper_win_rt::spawn_local(async move {
                    copper_win_rt::sleep(std::time::Duration::from_millis(220)).await;
                    if let Some(chrome) = weak.upgrade() {
                        chrome.edit(move |ui, st| st.stage.progress_done(ui, ticket));
                    }
                });
            }
        });
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
        // Pages follow the Look too: `prefers-color-scheme`, and the
        // ground a webview shows until its page paints.
        let engine = self.chrome.shell().map(|s| s.engine().clone());
        if let Some(e) = &engine {
            e.set_color_scheme(match look {
                Look::Light => ColorScheme::Light,
                Look::Dark => ColorScheme::Dark,
                Look::System => ColorScheme::Auto,
            });
        }
        let chrome = self.chrome.clone();
        self.chrome.edit(move |ui, _| {
            ui.set_appearance(a);
            chrome.set_dark(ui.is_dark());
            if let Some(e) = engine {
                e.set_page_ground(page_ground(ui.is_dark()));
            }
        });
    }

    fn is_dark(&self) -> bool {
        self.chrome.is_dark()
    }

    fn toast(&self, text: &str) {
        let news = self.download_news.take();
        let text = news
            .and_then(|n| download_words(text, &n))
            .unwrap_or_else(|| text.to_string());
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
        // The camera, microphone, location and notifications are asked
        // about by `chrome::permit`, in the Mac's words and remembered per
        // site; the shell's own question for them is dropped unanswered
        // (permit answers the page).
        if crate::chrome::permit::is_shell_question(&question) {
            drop(answer);
            return;
        }
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
        matches!(self.panels.showing(), Some("downloads" | "downloads-list"))
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

/// The window's ground (`Design.swift` `Palette.NS.ground`), which a
/// webview shows until its page paints.
fn page_ground(dark: bool) -> Ground {
    let c = copper_win_ui::tokens::palette::GROUND.get(dark);
    Ground::from_unit(c.r, c.g, c.b)
}
