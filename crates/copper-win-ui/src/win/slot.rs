//! The WebSlot: where a page shows inside the window (`docs/CONTRACT.md` §2).
//!
//! The toolkit never knows what a page is. A [`Node::slot`] node gets a
//! DirectComposition visual of its own; the app implements [`SlotHost`] to
//! hand that visual, the slot's bounds, scale and visibility, and the
//! mouse, touch and pen input that lands on it to its engine (WebView2's
//! composition controller). Keyboard and IME input do not pass through
//! here: while the page has focus they go to the engine's own child
//! window.
//!
//! [`Node::slot`]: crate::node::Node::slot

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::DirectComposition::IDCompositionVisual;
use windows::Win32::UI::WindowsAndMessaging::HCURSOR;
use windows::core::IUnknown;

/// Why focus moves into the page (WebView2's
/// `COREWEBVIEW2_MOVE_FOCUS_REASON`: the same values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FocusReason {
    Programmatic = 0,
    /// Tab from the chrome: the page's first element.
    Next = 1,
    /// Shift+Tab from the chrome: the page's last element.
    Previous = 2,
}

/// One mouse message for the page.
///
/// `kind` is the window message (`WM_MOUSEMOVE`, `WM_LBUTTONDOWN`, …,
/// `WM_MOUSEWHEEL`, `WM_MOUSEHWHEEL`, `WM_MOUSELEAVE`), which is exactly
/// WebView2's `COREWEBVIEW2_MOUSE_EVENT_KIND`; `keys` is the message's
/// `MK_*` state (`COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS`); `data` is the
/// wheel delta or the X button; `point` is in slot-local physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotMouse {
    pub kind: u32,
    pub keys: u32,
    pub data: u32,
    pub point: POINT,
}

/// One touch or pen message for the page: `kind` is the window message
/// (`WM_POINTERDOWN`, …), which is WebView2's
/// `COREWEBVIEW2_POINTER_EVENT_KIND`; read the rest with `GetPointerInfo`
/// (or `GetPointerTouchInfo` / `GetPointerPenInfo`) on `pointer_id`
/// while the message is being handled, and pass it to
/// `CreateCoreWebView2PointerInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotPointer {
    pub kind: u32,
    pub pointer_id: u32,
}

/// What the app does with a slot. Every method is called on the UI
/// thread, from inside the window's message handling: do not call back
/// into the window (`Window::with` returns `None` while it is busy).
pub trait SlotHost {
    /// The slot's visual exists (or was rebuilt after a GPU reset): host the
    /// page under it.
    fn attach(&self, hwnd: HWND, visual: &IDCompositionVisual);
    /// The slot left the window.
    fn detach(&self);
    /// The slot moved or resized: physical pixels, client-relative.
    fn set_bounds(&self, rect: RECT);
    fn set_visible(&self, visible: bool);
    /// Physical pixels per DIP.
    fn set_scale(&self, scale: f64);
    fn mouse(&self, m: SlotMouse);
    fn pointer(&self, p: SlotPointer);
    /// Give the page keyboard focus.
    fn focus(&self, reason: FocusReason);
    /// The cursor the page wants, if it has said.
    fn cursor(&self) -> Option<HCURSOR>;
    /// The page's UI Automation root (an `IRawElementProviderSimple`).
    fn automation(&self) -> Option<IUnknown>;
}

/// What the window last told a slot, so it only says what changed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Told {
    pub bounds: Option<(i32, i32, i32, i32)>,
    pub visible: Option<bool>,
    pub scale: Option<f32>,
}
