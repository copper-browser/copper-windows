//! The main window's messages: frame and size, the custom caption, mouse,
//! touch and pen, keyboard, IME, focus and activation.

use std::time::Instant;

use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ScreenToClient, ValidateRect};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetDoubleClickTime, ReleaseCapture, SetCapture, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
    TrackMouseEvent, VK_F10, VK_PROCESSKEY,
};
use windows::Win32::UI::Input::Pointer::GetPointerType;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetSystemMetrics, HTCAPTION, HTCLIENT, HTCLOSE, HTMAXBUTTON, HTMINBUTTON,
    HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, IDC_HAND, IDC_IBEAM, IDC_SIZENS, IDC_SIZEWE,
    KillTimer, LoadCursorW, POINTER_INPUT_TYPE, PT_PEN, PT_TOUCH, SM_CXDOUBLECLK,
    SPI_GETWHEELSCROLLLINES, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetCursor, SetTimer,
    SystemParametersInfoW, WA_INACTIVE, WM_ACTIVATE, WM_ACTIVATEAPP, WM_CAPTURECHANGED, WM_CHAR,
    WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DPICHANGED, WM_IME_CHAR, WM_IME_COMPOSITION,
    WM_IME_ENDCOMPOSITION, WM_IME_SETCONTEXT, WM_IME_STARTCOMPOSITION, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
    WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP, WM_PAINT, WM_POINTERDOWN,
    WM_POINTERENTER, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SETFOCUS, WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_TIMER,
    WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use super::clock::WM_FRAME;
use super::slot::{SlotMouse, SlotPointer};
use super::ui::{Capture, Command, Pan, TIMER_CARET, TIMER_TIP, Ui};
use super::{clipboard, frame, ime, keys};
use crate::chrome::CaptionButton;
use crate::event::Event;
use crate::geom::Point;
use crate::input::{FieldKey, Key, field_key};
use crate::node::Cursor;
use crate::tokens::motion;
use crate::tree::NodeId;

/// Movement past which a touch is a pan, not a tap (DIPs).
const PAN_SLOP: f32 = 6.0;
/// `MK_*` buttons.
const MK_BUTTONS: usize = 0x0001 | 0x0002 | 0x0010 | 0x0020 | 0x0040;

fn lo(v: usize) -> u32 {
    (v & 0xffff) as u32
}

fn hi(v: usize) -> u32 {
    ((v >> 16) & 0xffff) as u32
}

/// Signed coordinates packed in an `LPARAM`.
fn xy(lp: LPARAM) -> (i32, i32) {
    (
        (lp.0 & 0xffff) as i16 as i32,
        ((lp.0 >> 16) & 0xffff) as i16 as i32,
    )
}

impl Ui {
    fn dip(&self, x: i32, y: i32) -> Point {
        let s = self.scale();
        Point::new(x as f32 / s, y as f32 / s)
    }

    fn screen_to_client(&self, lp: LPARAM) -> POINT {
        let (x, y) = xy(lp);
        let mut p = POINT { x, y };
        // SAFETY: converting a point for our own window.
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut p);
        }
        p
    }

    fn is_slot(&self, id: NodeId) -> bool {
        self.tree.get(id).is_some_and(|n| n.is_slot())
    }

    fn is_field(&self, id: NodeId) -> bool {
        self.tree.get(id).is_some_and(|n| n.field_ref().is_some())
    }

    /// Handle a main-window message; `None` runs the default procedure.
    pub(super) fn message(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let r = LRESULT(0);
        match msg {
            WM_FRAME => {
                self.clock.handled();
                self.render();
                Some(r)
            }
            WM_PAINT => {
                // SAFETY: the content is composed; there is nothing to paint.
                unsafe {
                    let _ = ValidateRect(Some(self.hwnd), None);
                }
                self.tree.mark_visuals();
                Some(r)
            }
            WM_SIZE => {
                self.sync_client_size();
                // Draw at the new size right away so a live resize keeps up.
                self.render();
                Some(r)
            }
            WM_DPICHANGED => {
                self.set_dpi(hi(wp.0));
                // SAFETY: lParam points at the suggested window rectangle.
                let rect = unsafe { *(lp.0 as *const RECT) };
                self.commands.push(Command::SetPos(rect));
                Some(r)
            }
            WM_NCMOUSEMOVE => self.nc_move(wp),
            WM_NCMOUSELEAVE => {
                self.nc_tracking = false;
                self.nc_leave();
                None
            }
            WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN => {
                self.hide_tip();
                self.close_menu();
                let b = self.nc_button(wp.0 as u32)?;
                if msg == WM_NCLBUTTONDOWN {
                    self.tree.press(Some(b));
                }
                Some(r)
            }
            WM_NCLBUTTONUP | WM_NCRBUTTONUP => {
                let Some(b) = self.nc_button(wp.0 as u32) else {
                    self.nc_leave();
                    return None;
                };
                if msg == WM_NCLBUTTONUP && self.tree.pressed() == Some(b) {
                    self.tree.release(Some(b));
                    match self.caption.which(b) {
                        Some(CaptionButton::Minimize) => self.commands.push(Command::Minimize),
                        Some(CaptionButton::Maximize) => {
                            self.commands.push(Command::ToggleMaximize)
                        }
                        Some(CaptionButton::Close) => self.events.push_back(Event::CloseRequested),
                        None => {}
                    }
                }
                Some(r)
            }
            WM_NCLBUTTONDBLCLK => self.nc_button(wp.0 as u32).map(|_| r),
            WM_MOUSEMOVE => {
                self.mouse_move(wp, lp);
                Some(r)
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                self.mouse_leave();
                Some(r)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_MBUTTONDOWN
            | WM_XBUTTONDOWN => {
                self.button_down(msg, wp, lp);
                Some(LRESULT(if msg == WM_XBUTTONDOWN { 1 } else { 0 }))
            }
            WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
                // Right-button up must reach the default procedure, which
                // turns it into WM_CONTEXTMENU, unless a page took it.
                let page = self.button_up(msg, wp, lp);
                if msg == WM_RBUTTONUP && !page {
                    None
                } else {
                    Some(LRESULT(if msg == WM_XBUTTONUP { 1 } else { 0 }))
                }
            }
            WM_CAPTURECHANGED => {
                if lp.0 != self.hwnd.0 as isize && self.capture != Capture::None {
                    if let Capture::Node(_) = self.capture {
                        self.tree.release(None);
                    }
                    self.capture = Capture::None;
                }
                Some(r)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                self.wheel(msg, wp, lp);
                Some(r)
            }
            WM_CONTEXTMENU => {
                self.context_menu(lp);
                Some(r)
            }
            WM_SETCURSOR => self.set_cursor(lp),
            WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERENTER
            | WM_POINTERLEAVE => self.pointer(msg, wp, lp),
            WM_KEYDOWN | WM_SYSKEYDOWN => self.key_down(msg, wp),
            WM_CHAR => {
                self.char(wp.0 as u16);
                Some(r)
            }
            WM_IME_STARTCOMPOSITION => {
                let id = self.tree.focused().filter(|&i| self.is_field(i))?;
                self.place_ime(id);
                Some(r)
            }
            WM_IME_COMPOSITION => {
                let id = self.tree.focused().filter(|&i| self.is_field(i))?;
                self.ime_composition(id, lp);
                Some(r)
            }
            WM_IME_ENDCOMPOSITION => {
                if let Some(id) = self.tree.focused().filter(|&i| self.is_field(i))
                    && self
                        .tree
                        .get(id)
                        .and_then(|n| n.field_ref())
                        .is_some_and(|f| f.edit.composition().is_some())
                {
                    self.tree.with_field(id, |f| f.edit.cancel_composition());
                    self.fit_field(id);
                }
                None
            }
            WM_IME_CHAR => Some(r),
            WM_IME_SETCONTEXT => {
                // We draw the composition inline; keep the IME's own
                // composition window hidden (its candidate list still shows).
                // SAFETY: default handling with the flag cleared.
                Some(unsafe { DefWindowProcW(self.hwnd, msg, wp, ime::context_lparam(lp)) })
            }
            WM_SETFOCUS => {
                self.focused_window = true;
                if let Some(id) = self.tree.focused() {
                    self.tree.mark_paint(id);
                    let field = self.is_field(id);
                    self.ime_on = field;
                    ime::enable(self.hwnd, field);
                } else {
                    ime::enable(self.hwnd, false);
                }
                self.restart_caret();
                Some(r)
            }
            WM_KILLFOCUS => {
                self.focused_window = false;
                // SAFETY: our own timer.
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_CARET);
                }
                if let Some(id) = self.tree.focused() {
                    if self.is_field(id) {
                        ime::complete(self.hwnd);
                    }
                    self.tree.mark_paint(id);
                }
                Some(r)
            }
            WM_ACTIVATE => {
                let active = lo(wp.0) != WA_INACTIVE;
                if !active {
                    self.close_menu();
                    self.hide_tip();
                }
                self.events.push_back(Event::Activated(active));
                if active != self.active {
                    self.active = active;
                    self.theme.active = active;
                    self.tree.mark_paint(self.caption.bar);
                }
                None
            }
            WM_ACTIVATEAPP => {
                if wp.0 == 0 {
                    self.close_menu();
                    self.hide_tip();
                }
                None
            }
            WM_SYSCOMMAND => {
                self.close_menu();
                self.hide_tip();
                None
            }
            WM_TIMER => {
                self.timer(wp.0);
                Some(r)
            }
            WM_SETTINGCHANGE => {
                self.system_dark = frame::system_dark();
                self.tree.reduce_motion = !frame::animations_on();
                self.apply_theme();
                None
            }
            WM_CLOSE => {
                self.events.push_back(Event::CloseRequested);
                Some(r)
            }
            WM_DESTROY => {
                self.teardown();
                Some(r)
            }
            _ => None,
        }
    }

    // — the caption —

    fn nc_button(&self, ht: u32) -> Option<NodeId> {
        match ht {
            HTMINBUTTON => Some(self.caption.min),
            HTMAXBUTTON => Some(self.caption.max),
            HTCLOSE => Some(self.caption.close),
            _ => None,
        }
    }

    fn nc_move(&mut self, wp: WPARAM) -> Option<LRESULT> {
        if !self.nc_tracking {
            let mut t = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE | TME_NONCLIENT,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: tracking our own window.
            self.nc_tracking = unsafe { TrackMouseEvent(&mut t).is_ok() };
        }
        let b = self.nc_button(wp.0 as u32);
        self.tree.set_hovered(b);
        b.map(|_| LRESULT(0))
    }

    fn nc_leave(&mut self) {
        let caption = |id: Option<NodeId>| id.is_some_and(|i| self.caption.which(i).is_some());
        if caption(self.tree.hovered()) {
            self.tree.set_hovered(None);
        }
        if caption(self.tree.pressed()) {
            self.tree.release(None);
        }
    }

    /// `WM_NCHITTEST`: resize band, caption buttons, drag regions.
    pub(super) fn hit_test(&mut self, wp: WPARAM, lp: LPARAM) -> LRESULT {
        // SAFETY: the default hit test (borders outside the client area).
        let def = unsafe {
            DefWindowProcW(
                self.hwnd,
                windows::Win32::UI::WindowsAndMessaging::WM_NCHITTEST,
                wp,
                lp,
            )
        };
        if def.0 != HTCLIENT as isize {
            return def;
        }
        let c = self.screen_to_client(lp);
        let border = frame::border(self.dpi);
        let w = (self.size.w * self.scale()).round() as i32;
        if !frame::is_maximized(self.hwnd) && c.y < border {
            let ht = if c.x < 2 * border {
                HTTOPLEFT
            } else if c.x >= w - 2 * border {
                HTTOPRIGHT
            } else {
                HTTOP
            };
            return LRESULT(ht as isize);
        }
        let pick = self.tree.pick(self.dip(c.x, c.y));
        if let Some(t) = pick.target {
            let ht = match self.caption.which(t) {
                Some(CaptionButton::Minimize) => HTMINBUTTON,
                Some(CaptionButton::Maximize) => HTMAXBUTTON,
                Some(CaptionButton::Close) => HTCLOSE,
                None => HTCLIENT,
            };
            return LRESULT(ht as isize);
        }
        LRESULT(if pick.drag { HTCAPTION } else { HTCLIENT } as isize)
    }

    // — the mouse —

    fn track_leave(&mut self) {
        if self.tracking {
            return;
        }
        let mut t = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        // SAFETY: tracking our own window.
        self.tracking = unsafe { TrackMouseEvent(&mut t).is_ok() };
    }

    /// Send a mouse message to the page in `slot`, at client pixel `c`.
    fn to_slot(&self, slot: NodeId, kind: u32, wp: WPARAM, c: POINT) {
        let Some((h, _)) = self.slots.get(&slot) else {
            return;
        };
        let s = self.scale();
        let r = self.tree.visible_rect(slot);
        let data = match kind {
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL | WM_XBUTTONDOWN | WM_XBUTTONUP => hi(wp.0),
            _ => 0,
        };
        h.mouse(SlotMouse {
            kind,
            keys: lo(wp.0),
            data,
            point: POINT {
                x: c.x - (r.x * s).round() as i32,
                y: c.y - (r.y * s).round() as i32,
            },
        });
    }

    fn mouse_move(&mut self, wp: WPARAM, lp: LPARAM) {
        self.track_leave();
        let (x, y) = xy(lp);
        let c = POINT { x, y };
        let p = self.dip(x, y);
        match self.capture {
            Capture::Slot(s) => {
                self.to_slot(s, WM_MOUSEMOVE, wp, c);
                return;
            }
            Capture::Node(id) if self.is_field(id) => {
                self.field_drag(id, p);
                return;
            }
            _ => {}
        }
        let pick = self.tree.pointer_move(p);
        let over = pick.target.filter(|&t| self.is_slot(t));
        if over != self.over_slot {
            if let Some(old) = self.over_slot {
                self.to_slot(old, WM_MOUSELEAVE, WPARAM(0), c);
            }
            self.over_slot = over;
        }
        if let Some(s) = over {
            self.to_slot(s, WM_MOUSEMOVE, wp, c);
        }
        self.update_tip(pick.target);
    }

    fn mouse_leave(&mut self) {
        if let Some(old) = self.over_slot.take() {
            self.to_slot(old, WM_MOUSELEAVE, WPARAM(0), POINT::default());
        }
        if self.capture == Capture::None {
            self.tree.pointer_leave();
        }
        self.update_tip(None);
    }

    /// Schedule the tooltip of whatever is under the pointer.
    fn update_tip(&mut self, target: Option<NodeId>) {
        let mut cur = target;
        let mut with_tip = None;
        while let Some(c) = cur {
            if self.tree.get(c).is_some_and(|n| n.tooltip.is_some()) {
                with_tip = Some(c);
                break;
            }
            cur = self.tree.parent(c);
        }
        if with_tip == self.tip_for {
            return;
        }
        self.tip_for = with_tip;
        self.hide_tip();
        // SAFETY: our own timer.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TIP);
            if with_tip.is_some() && self.menu.is_none() {
                SetTimer(
                    Some(self.hwnd),
                    TIMER_TIP,
                    (motion::TOOLTIP_DELAY * 1000.0) as u32,
                    None,
                );
            }
        }
    }

    fn button_down(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) {
        self.hide_tip();
        if self.menu.is_some() {
            // A click outside the menu only closes it, as Windows' do.
            self.close_menu();
            return;
        }
        let (x, y) = xy(lp);
        let c = POINT { x, y };
        let p = self.dip(x, y);
        let pick = self.tree.pick(p);
        if let Some(s) = pick.target.filter(|&t| self.is_slot(t)) {
            // SAFETY: capturing for our own window.
            unsafe { SetCapture(self.hwnd) };
            self.capture = Capture::Slot(s);
            // The page takes focus itself when clicked.
            if self.tree.focused().is_some() {
                self.set_focus(None, false);
            }
            self.to_slot(s, msg, wp, c);
            return;
        }
        if msg != WM_LBUTTONDOWN && msg != WM_LBUTTONDBLCLK {
            return;
        }
        let target = pick.target;
        self.tree.press(target);
        if let Some(t) = target {
            // SAFETY: capturing for our own window.
            unsafe { SetCapture(self.hwnd) };
            self.capture = Capture::Node(t);
            if self
                .tree
                .get(t)
                .is_some_and(|n| n.flags.focusable && !n.flags.disabled)
            {
                self.set_focus(Some(t), false);
            }
        }
        let count = self.click_count(p);
        if let Some(t) = target.filter(|&t| self.is_field(t)) {
            let shift = keys::mods().shift;
            let pos = self.field_pos(t, p);
            self.tree.with_field(t, |f| match count {
                1 => f.edit.move_to(pos, shift),
                2 => {
                    let w = f.edit.word_at(pos);
                    f.edit.select(w);
                }
                _ => f.edit.select_all(),
            });
            self.fit_field(t);
            self.restart_caret();
        }
    }

    /// 1 for a click, 2 for a double click, 3 for a triple.
    fn click_count(&mut self, p: Point) -> u32 {
        let now = Instant::now();
        // SAFETY: plain system queries.
        let (time, slop) = unsafe { (GetDoubleClickTime(), GetSystemMetrics(SM_CXDOUBLECLK)) };
        let (at, from, n) = self.clicks;
        let near =
            (p.x - from.x).abs() <= slop as f32 / 2.0 && (p.y - from.y).abs() <= slop as f32 / 2.0;
        let n = if near && now.duration_since(at).as_millis() <= u128::from(time) {
            n + 1
        } else {
            1
        };
        self.clicks = (now, p, n);
        n
    }

    /// The text position under `p` in field `id`.
    fn field_pos(&mut self, id: NodeId, p: Point) -> usize {
        self.fonts.set_space(0);
        let r = self.tree.window_rect(id);
        let Some(f) = self.tree.get(id).and_then(|n| n.field_ref()) else {
            return 0;
        };
        let x = p.x - r.x + f.scroll_x;
        self.fonts.field_hit(id, f, x, r.w, r.h)
    }

    fn field_drag(&mut self, id: NodeId, p: Point) {
        let pos = self.field_pos(id, p);
        if self
            .tree
            .get(id)
            .and_then(|n| n.field_ref())
            .is_some_and(|f| f.edit.caret() != pos)
        {
            self.tree.with_field(id, |f| f.edit.move_to(pos, true));
            self.fit_field(id);
        }
    }

    /// Returns whether a page took the button.
    fn button_up(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> bool {
        let (x, y) = xy(lp);
        let c = POINT { x, y };
        let p = self.dip(x, y);
        match self.capture {
            Capture::Slot(s) => {
                self.to_slot(s, msg, wp, c);
                if wp.0 & MK_BUTTONS == 0 {
                    self.capture = Capture::None;
                    // SAFETY: releasing our own capture.
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                true
            }
            Capture::Node(_) if msg == WM_LBUTTONUP => {
                self.capture = Capture::None;
                // SAFETY: as above.
                unsafe {
                    let _ = ReleaseCapture();
                }
                let target = self.tree.pick(p).target;
                if let Some(id) = self.tree.release(target)
                    && self.caption.which(id).is_none()
                    && !self.is_field(id)
                {
                    self.events.push_back(Event::Click(id));
                    self.uia.invoked(&self.tree, id);
                }
                false
            }
            _ => {
                if let Some(s) = self.tree.pick(p).target.filter(|&t| self.is_slot(t)) {
                    self.to_slot(s, msg, wp, c);
                    return true;
                }
                false
            }
        }
    }

    fn wheel(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) {
        self.hide_tip();
        let c = self.screen_to_client(lp);
        let p = self.dip(c.x, c.y);
        let target = self.tree.pick(p).target;
        if let Some(s) = target.filter(|&t| self.is_slot(t)) {
            self.to_slot(s, msg, wp, c);
            return;
        }
        if msg != WM_MOUSEWHEEL {
            return;
        }
        let Some(sc) = target.and_then(|t| self.tree.scroller_of(t)) else {
            return;
        };
        let delta = hi(wp.0) as u16 as i16 as f32;
        let mut lines: u32 = 3;
        // SAFETY: SPI_GETWHEELSCROLLLINES writes a UINT.
        unsafe {
            let _ = SystemParametersInfoW(
                SPI_GETWHEELSCROLLLINES,
                0,
                Some((&mut lines as *mut u32).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }
        let lines = if lines == u32::MAX { 3 } else { lines.max(1) } as f32;
        // A notch scrolls NOTCH DIPs at Windows' default of three lines;
        // touchpads send fractions of a notch.
        let precise = delta % 120.0 != 0.0;
        let dips = -delta / 120.0 * crate::scroll::NOTCH * lines / 3.0;
        self.tree.scroll(sc, |s| s.wheel(dips, precise));
    }

    fn context_menu(&mut self, lp: LPARAM) {
        self.hide_tip();
        let keyboard = lp.0 as i32 == -1;
        let (target, at) = if keyboard {
            let f = self.tree.focused();
            let at = f.map_or(Point::new(self.size.w / 2.0, self.size.h / 3.0), |id| {
                let r = self.tree.window_rect(id);
                Point::new(r.x + 8.0, r.bottom())
            });
            (f, at)
        } else {
            let c = self.screen_to_client(lp);
            let p = self.dip(c.x, c.y);
            (self.tree.pick(p).target, p)
        };
        if target.is_some_and(|t| self.is_slot(t)) {
            return;
        }
        self.events.push_back(Event::ContextMenu { target, at });
    }

    fn set_cursor(&mut self, lp: LPARAM) -> Option<LRESULT> {
        if lo(lp.0 as usize) != HTCLIENT {
            return None;
        }
        if let Some(s) = self.over_slot
            && let Some(h) = self.slots.get(&s).and_then(|(h, _)| h.cursor())
        {
            // SAFETY: the page's cursor handle.
            unsafe { SetCursor(Some(h)) };
            return Some(LRESULT(1));
        }
        let cursor = self
            .tree
            .hovered()
            .and_then(|h| self.tree.get(h))
            .map_or(Cursor::Arrow, |n| n.cursor);
        let id = match cursor {
            Cursor::Arrow => IDC_ARROW,
            Cursor::Hand => IDC_HAND,
            Cursor::IBeam => IDC_IBEAM,
            Cursor::ResizeEw => IDC_SIZEWE,
            Cursor::ResizeNs => IDC_SIZENS,
        };
        // SAFETY: a stock cursor.
        unsafe { SetCursor(LoadCursorW(None, id).ok()) };
        Some(LRESULT(1))
    }

    // — touch and pen —

    fn pointer(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let id = lo(wp.0);
        let mut ty = POINTER_INPUT_TYPE::default();
        // SAFETY: querying the pointer of the message being handled.
        if unsafe { GetPointerType(id, &mut ty) }.is_err() || (ty != PT_TOUCH && ty != PT_PEN) {
            return None;
        }
        let c = self.screen_to_client(lp);
        let p = self.dip(c.x, c.y);
        let slot = match self.capture {
            Capture::Slot(s) => Some(s),
            _ if self.pan.is_none() => self.tree.pick(p).target.filter(|&t| self.is_slot(t)),
            _ => None,
        };
        if let Some(s) = slot {
            if msg == WM_POINTERDOWN {
                self.capture = Capture::Slot(s);
                self.set_focus(None, false);
            } else if msg == WM_POINTERUP {
                self.capture = Capture::None;
            }
            if let Some((h, _)) = self.slots.get(&s) {
                h.pointer(SlotPointer {
                    kind: msg,
                    pointer_id: id,
                });
            }
            return Some(LRESULT(0));
        }
        let now = Instant::now();
        match msg {
            WM_POINTERDOWN => {
                self.hide_tip();
                self.close_menu();
                let target = self.tree.pick(p).target;
                self.tree.press(target);
                self.pan = Some(Pan {
                    id,
                    scroller: target.and_then(|t| self.tree.scroller_of(t)),
                    start: p,
                    last: p,
                    last_at: now,
                    velocity: 0.0,
                    moved: false,
                });
                Some(LRESULT(0))
            }
            WM_POINTERUPDATE => {
                let mut pan = self.pan.filter(|g| g.id == id)?;
                let dy = p.y - pan.last.y;
                if !pan.moved
                    && ((p.x - pan.start.x).abs() > PAN_SLOP
                        || (p.y - pan.start.y).abs() > PAN_SLOP)
                {
                    pan.moved = true;
                    self.tree.release(None);
                }
                if pan.moved
                    && let Some(sc) = pan.scroller
                {
                    self.tree.scroll(sc, |s| s.drag(dy));
                    let dt = now.duration_since(pan.last_at).as_secs_f32().max(0.001);
                    // Smooth the finger's speed over the last few samples.
                    pan.velocity = pan.velocity * 0.3 + dy / dt * 0.7;
                }
                pan.last = p;
                pan.last_at = now;
                self.pan = Some(pan);
                Some(LRESULT(0))
            }
            WM_POINTERUP => {
                let pan = self.pan.take().filter(|g| g.id == id)?;
                if pan.moved {
                    if let Some(sc) = pan.scroller {
                        let v = if now.duration_since(pan.last_at).as_millis() > 80 {
                            0.0
                        } else {
                            pan.velocity
                        };
                        self.tree.scroll(sc, |s| s.release(v));
                    }
                } else {
                    let target = self.tree.pick(p).target;
                    if let Some(t) = target
                        && self
                            .tree
                            .get(t)
                            .is_some_and(|n| n.flags.focusable && !n.flags.disabled)
                    {
                        self.set_focus(Some(t), false);
                    }
                    if let Some(c) = self.tree.release(target) {
                        if let Some(b) = self.caption.which(c) {
                            match b {
                                CaptionButton::Minimize => self.commands.push(Command::Minimize),
                                CaptionButton::Maximize => {
                                    self.commands.push(Command::ToggleMaximize)
                                }
                                CaptionButton::Close => {
                                    self.events.push_back(Event::CloseRequested)
                                }
                            }
                        } else if self.is_field(c) {
                            let pos = self.field_pos(c, p);
                            self.tree.with_field(c, |f| f.edit.move_to(pos, false));
                            self.fit_field(c);
                        } else {
                            self.events.push_back(Event::Click(c));
                            self.uia.invoked(&self.tree, c);
                        }
                    }
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    // — the keyboard —

    fn key_down(&mut self, msg: u32, wp: WPARAM) -> Option<LRESULT> {
        let vk = wp.0 as u32;
        if vk == u32::from(VK_PROCESSKEY.0) {
            return None;
        }
        self.hide_tip();
        let key = keys::key(vk);
        let mods = keys::mods();
        if self.menu.is_some() {
            self.menu_key(key);
            return Some(LRESULT(0));
        }
        let focused = self.tree.focused();
        if let Some(id) = focused.filter(|&i| self.is_field(i))
            && !mods.alt
        {
            let rtl = self
                .tree
                .get(id)
                .and_then(|n| n.field_ref())
                .is_some_and(|f| f.edit.is_rtl());
            let out = self
                .tree
                .with_field(id, |f| field_key(&mut f.edit, key, mods, rtl))
                .unwrap_or(FieldKey::Ignored);
            match out {
                FieldKey::Ignored => {}
                FieldKey::Edited => {
                    self.field_changed(id);
                    return Some(LRESULT(0));
                }
                FieldKey::Moved => {
                    self.fit_field(id);
                    self.restart_caret();
                    return Some(LRESULT(0));
                }
                FieldKey::Copy(s) => {
                    clipboard::set(self.hwnd, &s);
                    return Some(LRESULT(0));
                }
                FieldKey::Cut(s) => {
                    clipboard::set(self.hwnd, &s);
                    self.field_changed(id);
                    return Some(LRESULT(0));
                }
                FieldKey::Paste => {
                    if let Some(s) = clipboard::get(self.hwnd) {
                        self.tree.with_field(id, |f| f.edit.paste(&s));
                        self.field_changed(id);
                    }
                    return Some(LRESULT(0));
                }
                FieldKey::Submit => {
                    self.events.push_back(Event::FieldSubmit(id));
                    return Some(LRESULT(0));
                }
                FieldKey::Cancel => {
                    self.events.push_back(Event::FieldCancel(id));
                    return Some(LRESULT(0));
                }
            }
        }
        match key {
            Key::Tab if !mods.ctrl && !mods.alt => {
                let next = self.tree.next_focus(!mods.shift);
                if next.is_some() {
                    self.set_focus(next, true);
                }
                return Some(LRESULT(0));
            }
            Key::Enter | Key::Space if !mods.ctrl && !mods.alt => {
                if let Some(id) = focused.filter(|&i| !self.is_field(i)) {
                    self.events.push_back(Event::Click(id));
                    self.uia.invoked(&self.tree, id);
                    return Some(LRESULT(0));
                }
            }
            Key::Menu => {
                self.context_menu(LPARAM(-1));
                return Some(LRESULT(0));
            }
            Key::F(10) if mods.shift => {
                self.context_menu(LPARAM(-1));
                return Some(LRESULT(0));
            }
            _ => {}
        }
        self.events.push_back(Event::Key { key, mods, focused });
        if msg == WM_SYSKEYDOWN {
            // F10 alone would enter the (absent) menu bar; Alt+F4 and
            // Alt+Space must reach the default procedure.
            if vk == u32::from(VK_F10.0) {
                return Some(LRESULT(0));
            }
            return None;
        }
        Some(LRESULT(0))
    }

    fn menu_key(&mut self, key: Key) {
        let Some(m) = self.menu.as_mut() else {
            return;
        };
        let moved = match key {
            Key::Up => m.step_highlight(false),
            Key::Down => m.step_highlight(true),
            Key::Home => {
                m.highlight(None);
                m.step_highlight(true)
            }
            Key::End => {
                m.highlight(None);
                m.step_highlight(false)
            }
            _ => false,
        };
        if moved {
            self.announce_highlight();
            return;
        }
        let Some(m) = self.menu.as_mut() else {
            return;
        };
        match key {
            Key::Enter | Key::Space => {
                if let Some(item) = m.highlighted.and_then(|i| m.id_at(i)) {
                    self.choose(item);
                }
            }
            Key::Escape | Key::Left | Key::Tab => self.close_menu(),
            Key::Letter(c) => {
                if let Some(item) = m.access(c).and_then(|i| m.id_at(i)) {
                    self.choose(item);
                }
            }
            Key::Digit(d) => {
                let c = char::from(b'0' + d);
                if let Some(item) = m.access(c).and_then(|i| m.id_at(i)) {
                    self.choose(item);
                }
            }
            _ => {}
        }
    }

    fn char(&mut self, unit: u16) {
        let Some(id) = self.tree.focused().filter(|&i| self.is_field(i)) else {
            return;
        };
        let ch = match unit {
            0xD800..=0xDBFF => {
                self.high_surrogate = Some(unit);
                return;
            }
            0xDC00..=0xDFFF => {
                let Some(h) = self.high_surrogate.take() else {
                    return;
                };
                char::decode_utf16([h, unit]).next().and_then(Result::ok)
            }
            _ => char::from_u32(u32::from(unit)),
        };
        self.high_surrogate = None;
        let Some(ch) = ch.filter(|c| !c.is_control()) else {
            return;
        };
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        self.tree.with_field(id, |f| f.edit.insert(s));
        self.field_changed(id);
    }

    fn field_changed(&mut self, id: NodeId) {
        self.fit_field(id);
        self.restart_caret();
        self.events.push_back(Event::FieldChanged(id));
    }

    fn ime_composition(&mut self, id: NodeId, lp: LPARAM) {
        let u = ime::read(self.hwnd, lp);
        let mut changed = false;
        if let Some(result) = &u.result {
            self.tree
                .with_field(id, |f| f.edit.commit_composition(result));
            changed = !result.is_empty();
        }
        if let Some((text, caret, clauses)) = u.composition {
            self.tree.with_field(id, |f| {
                if text.is_empty() {
                    f.edit.cancel_composition();
                } else {
                    f.edit.set_composition(&text, caret, clauses);
                }
            });
        }
        self.fit_field(id);
        self.restart_caret();
        if changed {
            self.events.push_back(Event::FieldChanged(id));
        }
    }

    // — timers and teardown —

    fn timer(&mut self, id: usize) {
        match id {
            TIMER_CARET => {
                if let Some(f) = self.tree.focused().filter(|&i| self.is_field(i)) {
                    let on = self
                        .tree
                        .get(f)
                        .and_then(|n| n.field_ref())
                        .is_some_and(|x| x.caret_on);
                    self.set_caret_on(f, !on);
                }
            }
            TIMER_TIP => {
                // SAFETY: our own timer.
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_TIP);
                }
                if let Some(t) = self.tip_for {
                    self.show_tip(t);
                }
            }
            _ => {}
        }
    }

    fn teardown(&mut self) {
        self.close_menu();
        self.hide_tip();
        // SAFETY: our own timers.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CARET);
            let _ = KillTimer(Some(self.hwnd), TIMER_TIP);
        }
        for (h, _) in self.slots.values() {
            h.detach();
        }
        self.slots.clear();
        self.clock.rest();
        self.uia.teardown();
        self.events.push_back(Event::Closed);
    }
}
