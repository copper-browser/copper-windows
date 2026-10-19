//! The glue between the toolkit's page slot and the engine's page
//! (CONTRACT §2): the toolkit hit-tests, lays out and composes; the page
//! draws into the slot's visual and takes the input that lands there.
//! Neither crate knows the other; this is the only place they meet.

use copper_engine_webview2::WebView2Page;
use copper_engine_webview2::hosting::{
    FocusReason as PageFocus, MouseEventKind, PointerEventKind, VirtualKeys,
};
use copper_win_ui::win::{FocusReason, SlotHost, SlotMouse, SlotPointer};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::DirectComposition::IDCompositionVisual;
use windows::Win32::UI::Input::Pointer::{GetPointerInfo, POINTER_INFO};
use windows::Win32::UI::WindowsAndMessaging::HCURSOR;
use windows::core::IUnknown;

/// A tab's page, as its slot's host.
pub struct PageHost {
    page: WebView2Page,
}

impl PageHost {
    pub fn new(page: WebView2Page) -> PageHost {
        PageHost { page }
    }
}

impl SlotHost for PageHost {
    fn attach(&self, hwnd: HWND, visual: &IDCompositionVisual) {
        tracing::debug!(target: "copper::slot", "attach");
        self.page.attach(hwnd, visual);
    }

    fn detach(&self) {
        tracing::debug!(target: "copper::slot", "detach");
        self.page.detach();
    }

    fn set_bounds(&self, rect: RECT) {
        tracing::debug!(
            target: "copper::slot",
            "bounds {},{} {}x{}",
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top
        );
        self.page.set_bounds(rect);
    }

    fn set_visible(&self, visible: bool) {
        tracing::debug!(target: "copper::slot", "visible {visible}");
        self.page.set_visible(visible);
    }

    fn set_scale(&self, scale: f64) {
        self.page.set_rasterization_scale(scale);
    }

    fn mouse(&self, m: SlotMouse) {
        if let Some(kind) = MouseEventKind::from_window_message(m.kind) {
            self.page.send_mouse_input(
                kind,
                VirtualKeys::from_wparam(m.keys as usize),
                m.data,
                m.point,
            );
        }
    }

    fn pointer(&self, p: SlotPointer) {
        let Some(kind) = PointerEventKind::from_window_message(p.kind) else {
            return;
        };
        let mut info = POINTER_INFO::default();
        // SAFETY: plain out-parameter read for the message being handled.
        if unsafe { GetPointerInfo(p.pointer_id, &mut info) }.is_ok() {
            self.page.send_pointer_input(kind, &info);
        }
    }

    fn focus(&self, reason: FocusReason) {
        self.page.move_focus(match reason {
            FocusReason::Programmatic => PageFocus::Programmatic,
            FocusReason::Next => PageFocus::Next,
            FocusReason::Previous => PageFocus::Previous,
        });
    }

    fn cursor(&self) -> Option<HCURSOR> {
        let c = self.page.cursor();
        (!c.is_invalid()).then_some(c)
    }

    fn automation(&self) -> Option<IUnknown> {
        self.page.automation_provider()
    }

    fn window_moved(&self) {
        self.page.notify_parent_window_moved();
    }
}
