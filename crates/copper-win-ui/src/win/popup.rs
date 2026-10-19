//! Context menus and tooltips: small windows of their own, drawn through
//! DirectComposition with transparent margins for their shadows.
//!
//! They never take activation (`WS_EX_NOACTIVATE`): the main window keeps
//! focus and routes keys to the open menu, the way Windows' own menus
//! behave. A menu closes on a choice, Escape, a click anywhere else, or the
//! app losing the foreground. Tooltips ignore the mouse entirely.

use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, HTCLIENT, HTTRANSPARENT,
    IDC_ARROW, LoadCursorW, MA_NOACTIVATE, RegisterClassExW, SW_SHOWNOACTIVATE, SetCursor,
    SetWindowLongPtrW, ShowWindow, WM_GETOBJECT, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_NCHITTEST, WM_SETCURSOR, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Result, w};

use super::compose::Scene;
use super::gfx::Gfx;
use super::paint::Painter;
use super::text::{Fonts, Measurer};
use super::ui::Ui;
use super::window::{Host, host_of};
use crate::event::Event;
use crate::geom::{Point, Rect, Size};
use crate::menu::{self, Built, Menu};
use crate::theme::Theme;
use crate::tokens::{motion, popup};
use crate::tree::{NodeId, Prop, Tree};

/// Layout ids of the menu's tree and the tooltip's (see `Fonts::set_space`).
const MENU_SPACE: u32 = 1;
const TIP_SPACE: u32 = 2;
/// Gap between a tooltip and what it explains.
const TIP_GAP: f32 = 6.0;

const CLASS: windows::core::PCWSTR = w!("CopperPopup");

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: registering a class whose procedure lives for the process.
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: hinst.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
    });
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: the user data is the owning window's host while the popup
    // lives (popups are destroyed before their owner).
    if let Some(host) = unsafe { host_of(hwnd) }
        && let Some(r) = host.popup_message(hwnd, msg, wp, lp)
    {
        return r;
    }
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // SAFETY: default handling.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// The work area of the monitor nearest `p` (screen pixels).
fn work_area(p: POINT) -> RECT {
    // SAFETY: plain monitor queries.
    unsafe {
        let m = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(m, &mut info);
        info.rcWork
    }
}

/// A popup window and its tree.
pub(super) struct Popup {
    pub hwnd: HWND,
    pub tree: Tree,
    scene: Scene,
    space: u32,
    size: Size,
    pub card: NodeId,
    dirty: Vec<NodeId>,
    new_slots: Vec<NodeId>,
}

impl Popup {
    /// Lay `tree` out at its natural size and make a hidden window for it.
    fn new(ui: &mut Ui, mut tree: Tree, space: u32, margin: f32, mouse: bool) -> Result<Popup> {
        register();
        let Some(root) = tree.root() else {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        };
        let card = tree.children(root).first().copied().unwrap_or(root);
        ui.fonts.set_space(space);
        tree.layout(
            Size::new(4096.0, 4096.0),
            &mut Measurer {
                fonts: &mut ui.fonts,
            },
        );
        let c = tree.get(card).map(|n| n.frame).unwrap_or_default();
        let size = Size::new((c.w + 2.0 * margin).ceil(), (c.h + 2.0 * margin).ceil());
        tree.mark_layout();
        tree.layout(
            size,
            &mut Measurer {
                fonts: &mut ui.fonts,
            },
        );
        ui.fonts.set_space(0);

        let s = ui.scale();
        let mut ex =
            WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
        if !mouse {
            ex |= WS_EX_TRANSPARENT;
        }
        // SAFETY: creating our own popup, owned by the main window.
        let hwnd = unsafe {
            let hinst = GetModuleHandleW(None)?;
            CreateWindowExW(
                ex,
                CLASS,
                w!(""),
                WS_POPUP,
                0,
                0,
                (size.w * s).ceil() as i32,
                (size.h * s).ceil() as i32,
                Some(ui.hwnd),
                None,
                Some(hinst.into()),
                None,
            )?
        };
        if let Some(h) = ui.host.upgrade() {
            // SAFETY: a non-owning pointer to the host, which outlives the
            // popup (it is destroyed in the host's teardown at the latest).
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, std::rc::Rc::as_ptr(&h) as isize) };
        }
        let none = DWM_WINDOW_CORNER_PREFERENCE(DWMWCP_DONOTROUND.0);
        // SAFETY: a valid attribute of the right size.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&none as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
        }
        let mut scene = Scene::new(hwnd);
        scene.set_scale(s);
        // Fade in (QUICK), as the Mac's menus and help tags do.
        tree.set_prop(root, Prop::Opacity, 0.0);
        tree.animate(root, Prop::Opacity, 1.0, motion::QUICK);
        Ok(Popup {
            hwnd,
            tree,
            scene,
            space,
            size,
            card,
            dirty: Vec::with_capacity(4),
            new_slots: Vec::new(),
        })
    }

    /// Put the window at `x`, `y` (screen pixels of its top-left) and show it.
    fn show_at(&self, x: i32, y: i32) {
        // SAFETY: moving and showing our own popup without activating it.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    fn step(&mut self, dt: f32) {
        self.tree.step(dt);
    }

    fn needs_frame(&self) -> bool {
        self.tree.has_dirty_layers() || self.tree.is_animating() || self.tree.needs_layout()
    }

    fn render(
        &mut self,
        gfx: &Gfx,
        painter: &mut Painter,
        fonts: &mut Fonts,
        theme: &Theme,
    ) -> Result<u32> {
        fonts.set_space(self.space);
        self.tree.layout(self.size, &mut Measurer { fonts });
        self.new_slots.clear();
        self.scene
            .sync_structure(gfx, &self.tree, &mut self.new_slots)?;
        if self.tree.take_visuals_dirty() {
            self.scene.sync_visuals(&self.tree)?;
        }
        self.tree.take_dirty_layers(&mut self.dirty);
        let n = self
            .scene
            .draw(gfx, &self.tree, &self.dirty, painter, fonts, theme, true)?;
        fonts.set_space(0);
        Ok(n)
    }

    fn reset(&mut self) {
        self.scene.reset();
        self.tree.mark_visuals();
        if let Some(r) = self.tree.root() {
            self.tree.mark_paint(r);
        }
    }

    fn close(self, fonts: &mut Fonts) {
        fonts.forget_space(self.space);
        // SAFETY: destroying our own popup.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    /// The card in popup DIPs.
    fn card_rect(&self) -> Rect {
        self.tree.window_rect(self.card)
    }
}

/// An open context menu.
pub(super) struct MenuPopup {
    pub id: u32,
    menu: Menu,
    built: Built,
    popup: Popup,
    pub highlighted: Option<usize>,
    tracking: bool,
}

impl MenuPopup {
    /// Build and show `menu` with its top-left at `at` (main window DIPs).
    pub fn open(ui: &mut Ui, id: u32, menu: &Menu, at: Point) -> Result<MenuPopup> {
        let margin = popup::MENU_SHADOW.reach().ceil();
        let mut tree = Tree::new();
        let built = menu::build(&mut tree, menu, margin);
        let popup = Popup::new(ui, tree, MENU_SPACE, margin, true)?;
        let s = ui.scale();
        let mut p = POINT {
            x: (at.x * s).round() as i32,
            y: (at.y * s).round() as i32,
        };
        // SAFETY: converting a point in our own window.
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(ui.hwnd, &mut p);
        }
        let area = work_area(p);
        let card = popup.card_rect();
        let (cw, ch) = ((card.w * s).round() as i32, (card.h * s).round() as i32);
        let m = (margin * s).round() as i32;
        let mut x = p.x;
        let mut y = p.y;
        if x + cw > area.right {
            x = (p.x - cw).max(area.left);
        }
        if y + ch > area.bottom {
            y = (p.y - ch).max(area.top);
        }
        popup.show_at(x - m, y - m);
        Ok(MenuPopup {
            id,
            menu: menu.clone(),
            built,
            popup,
            highlighted: None,
            tracking: false,
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.popup.hwnd
    }

    /// Highlight row `i`; whether that changed anything.
    pub fn highlight(&mut self, i: Option<usize>) -> bool {
        if self.highlighted == i {
            return false;
        }
        self.highlighted = i;
        menu::highlight(&mut self.popup.tree, &self.built, i);
        true
    }

    /// Up or Down; whether the highlight moved.
    pub fn step_highlight(&mut self, forward: bool) -> bool {
        let next = self.menu.step(self.highlighted, forward);
        self.highlight(next)
    }

    /// The highlighted row's node.
    pub fn highlighted_row(&self) -> Option<NodeId> {
        self.highlighted
            .and_then(|i| self.built.rows.get(i).copied().flatten())
    }

    /// The item id behind row node `row`, if it can be chosen.
    pub fn item_of_row(&self, row: NodeId) -> Option<u32> {
        let i = self.built.rows.iter().position(|r| *r == Some(row))?;
        self.menu.id_at(i)
    }

    /// The card (`Role::Menu`).
    pub fn card(&self) -> NodeId {
        self.popup.card
    }

    /// The item an access key picks.
    pub fn access(&self, c: char) -> Option<usize> {
        self.menu.access(c, self.highlighted)
    }

    /// The item id at index `i`, if it can be chosen.
    pub fn id_at(&self, i: usize) -> Option<u32> {
        self.menu.id_at(i)
    }

    /// The row index under `p` (popup DIPs).
    fn row_at(&self, p: Point) -> Option<usize> {
        let target = self.popup.tree.pick(p).target?;
        self.built
            .rows
            .iter()
            .position(|r| r.is_some_and(|r| r == target || self.popup.tree.is_within(target, r)))
    }

    pub fn needs_frame(&self) -> bool {
        self.popup.needs_frame()
    }

    pub fn step(&mut self, dt: f32) {
        self.popup.step(dt);
    }

    pub fn render(
        &mut self,
        gfx: &Gfx,
        painter: &mut Painter,
        fonts: &mut Fonts,
        theme: &Theme,
    ) -> Result<u32> {
        self.popup.render(gfx, painter, fonts, theme)
    }

    pub fn reset(&mut self) {
        self.popup.reset();
    }

    pub fn repaint_all(&mut self) {
        if let Some(r) = self.popup.tree.root() {
            self.popup.tree.mark_paint(r);
        }
    }

    pub fn close(self, fonts: &mut Fonts) {
        self.popup.close(fonts);
    }

    pub fn tree(&self) -> &Tree {
        &self.popup.tree
    }
}

/// An open tooltip.
pub(super) struct TipPopup {
    popup: Popup,
}

impl TipPopup {
    /// Show `text` under `anchor` (main window DIPs).
    pub fn open(ui: &mut Ui, text: &str, anchor: Rect) -> Result<TipPopup> {
        let margin = popup::TIP_SHADOW.reach().ceil();
        let mut tree = Tree::new();
        menu::build_tooltip(&mut tree, text, margin);
        let popup = Popup::new(ui, tree, TIP_SPACE, margin, false)?;
        let s = ui.scale();
        let card = popup.card_rect();
        let (cw, ch) = ((card.w * s).round() as i32, (card.h * s).round() as i32);
        let gap = (TIP_GAP * s).round() as i32;
        let mut tl = POINT {
            x: (anchor.x * s).round() as i32,
            y: (anchor.y * s).round() as i32,
        };
        let mut br = POINT {
            x: (anchor.right() * s).round() as i32,
            y: (anchor.bottom() * s).round() as i32,
        };
        // SAFETY: converting points in our own window.
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(ui.hwnd, &mut tl);
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(ui.hwnd, &mut br);
        }
        let area = work_area(POINT {
            x: (tl.x + br.x) / 2,
            y: br.y,
        });
        let mut x = (tl.x + br.x) / 2 - cw / 2;
        let mut y = br.y + gap;
        if y + ch > area.bottom {
            y = tl.y - gap - ch;
        }
        x = x.clamp(area.left, (area.right - cw).max(area.left));
        y = y.max(area.top);
        let m = (margin * s).round() as i32;
        popup.show_at(x - m, y - m);
        Ok(TipPopup { popup })
    }

    pub fn needs_frame(&self) -> bool {
        self.popup.needs_frame()
    }

    pub fn step(&mut self, dt: f32) {
        self.popup.step(dt);
    }

    pub fn render(
        &mut self,
        gfx: &Gfx,
        painter: &mut Painter,
        fonts: &mut Fonts,
        theme: &Theme,
    ) -> Result<u32> {
        self.popup.render(gfx, painter, fonts, theme)
    }

    pub fn reset(&mut self) {
        self.popup.reset();
    }

    pub fn close(self, fonts: &mut Fonts) {
        self.popup.close(fonts);
    }
}

impl Ui {
    /// A message for one of the popups; `None` lets the default run.
    pub(super) fn popup_message(
        &mut self,
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> Option<LRESULT> {
        if msg == WM_GETOBJECT {
            return self.popup_object(hwnd, wp, lp);
        }
        let Some(m) = self.menu.as_mut().filter(|m| m.hwnd() == hwnd) else {
            return match msg {
                WM_NCHITTEST => Some(LRESULT(HTTRANSPARENT as isize)),
                WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
                _ => None,
            };
        };
        let s = self.dpi as f32 / 96.0;
        let local = |lp: LPARAM| {
            let x = (lp.0 & 0xffff) as i16 as f32;
            let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
            Point::new(x / s, y / s)
        };
        match msg {
            WM_NCHITTEST => {
                let mut p = POINT {
                    x: (lp.0 & 0xffff) as i16 as i32,
                    y: ((lp.0 >> 16) & 0xffff) as i16 as i32,
                };
                // SAFETY: converting a point for our own popup.
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
                }
                let at = Point::new(p.x as f32 / s, p.y as f32 / s);
                let inside = m.popup.card_rect().contains(at);
                Some(LRESULT(if inside {
                    HTCLIENT as isize
                } else {
                    HTTRANSPARENT as isize
                }))
            }
            WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_SETCURSOR => {
                // SAFETY: the stock arrow.
                unsafe { SetCursor(LoadCursorW(None, IDC_ARROW).ok()) };
                Some(LRESULT(1))
            }
            WM_MOUSEMOVE => {
                if !m.tracking {
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: tracking our own popup.
                    m.tracking = unsafe { TrackMouseEvent(&mut t).is_ok() };
                }
                let row = m.row_at(local(lp)).filter(|&i| m.id_at(i).is_some());
                if m.highlight(row) {
                    self.announce_highlight();
                }
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                m.tracking = false;
                m.highlight(None);
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                let row = m.row_at(local(lp));
                if let Some(item) = row.and_then(|i| m.id_at(i)) {
                    self.choose(item);
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    /// Close the menu with `item` chosen.
    pub(super) fn choose(&mut self, item: u32) {
        if let Some(menu) = self.drop_menu() {
            self.events.push_back(Event::MenuChosen { menu, item });
        }
    }

    /// Tell UI Automation which menu row is highlighted.
    pub(super) fn announce_highlight(&mut self) {
        if let Some(r) = self.menu.as_ref().and_then(|m| m.highlighted_row()) {
            self.uia.menu_highlight(r);
        }
    }

    /// Show the tooltip of `id` now.
    pub(super) fn show_tip(&mut self, id: NodeId) {
        self.hide_tip();
        let Some(text) = self.tree.get(id).and_then(|n| n.tooltip.clone()) else {
            return;
        };
        if self.menu.is_some() || !self.tree.is_shown(id) {
            return;
        }
        let anchor = self.tree.visible_rect(id);
        match TipPopup::open(self, &text, anchor) {
            Ok(t) => self.tip = Some(t),
            Err(e) => self.recover(&e),
        }
    }

    pub(super) fn hide_tip(&mut self) {
        if let Some(t) = self.tip.take() {
            t.close(&mut self.fonts);
        }
    }
}

impl Host {
    /// Run a popup's message against the shared state.
    pub(super) fn popup_message(
        &self,
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> Option<LRESULT> {
        self.with_ui(|ui| ui.popup_message(hwnd, msg, wp, lp))
            .flatten()
    }
}
