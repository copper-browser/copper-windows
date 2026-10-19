//! The smoke window: a top-level Win32 window composed with
//! DirectComposition (no redirection bitmap), its whole client area one
//! page slot. It does what `copper-win-ui`'s `WebSlot` does for the real
//! shell: forwards mouse and pointer input, sets the page's cursor, keeps
//! bounds and scale current, and gives the page focus.

use std::cell::{Cell, RefCell};

use copper_engine_webview2::WebView2Page;
use copper_engine_webview2::hosting::{
    FocusExit, FocusReason, KeyEventKind, MouseEventKind, PointerEventKind, VirtualKeys,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionTarget,
    IDCompositionVisual2,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Input::Pointer::{GetPointerInfo, GetPointerType, POINTER_INFO};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, IUnknown, w};

thread_local! {
    static PAGE: RefCell<Option<WebView2Page>> = const { RefCell::new(None) };
    /// `TrackMouseEvent` is armed (so `WM_MOUSELEAVE` will come).
    static TRACKING: Cell<bool> = const { Cell::new(false) };
}

pub struct Window {
    pub hwnd: HWND,
    device: IDCompositionDesktopDevice,
    _target: IDCompositionTarget,
    root: IDCompositionVisual2,
}

fn page() -> Option<WebView2Page> {
    PAGE.with(|p| p.borrow().clone())
}

fn lo(v: usize) -> i32 {
    i32::from(v as u16 as i16)
}

fn hi(v: usize) -> i32 {
    i32::from((v >> 16) as u16 as i16)
}

fn client_rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    r
}

fn scale_of(hwnd: HWND) -> f64 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 1.0,
        dpi => f64::from(dpi) / 96.0,
    }
}

impl Window {
    pub fn create(title: &str) -> windows::core::Result<Self> {
        let instance = unsafe { GetModuleHandleW(None)? };
        let class = w!("CopperSmoke");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
            lpszClassName: class,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc) };
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                class,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1280,
                800,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        let device: IDCompositionDesktopDevice =
            unsafe { DCompositionCreateDevice2(None::<&IUnknown>)? };
        let target = unsafe { device.CreateTargetForHwnd(hwnd, true)? };
        let root = unsafe { device.CreateVisual()? };
        unsafe {
            target.SetRoot(&root)?;
            device.Commit()?;
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        }
        Ok(Window {
            hwnd,
            device,
            _target: target,
            root,
        })
    }

    /// Put `page` in the client area and give it input.
    pub fn show_page(&self, page: &WebView2Page) {
        PAGE.with(|p| *p.borrow_mut() = Some(page.clone()));
        page.attach(self.hwnd, &self.root);
        page.set_bounds(client_rect(self.hwnd));
        page.set_rasterization_scale(scale_of(self.hwnd));
        page.set_visible(true);
        unsafe {
            let _ = self.device.Commit();
        }
        let hwnd = self.hwnd.0 as isize;
        page.on_cursor_changed(|c| unsafe {
            SetCursor(Some(c));
        });
        page.on_accelerator_key(move |k| {
            // Ctrl+W closes the window: the page never sees it.
            let close =
                k.kind == KeyEventKind::KeyDown && k.ctrl && k.virtual_key == u32::from(b'W');
            if close {
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            close
        });
        // No shell controls to tab to: wrap around inside the page. (The
        // page is looked up, not captured: a callback the page owns must
        // not own the page.)
        page.on_focus_exit(|exit| {
            if let Some(p) = self::page() {
                p.move_focus(match exit {
                    FocusExit::Next => FocusReason::Next,
                    FocusExit::Previous => FocusReason::Previous,
                });
            }
        });
        page.move_focus(FocusReason::Programmatic);
    }

    pub fn close(&self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// A mouse message for the page: slot-local point and `mouseData`.
fn mouse(hwnd: HWND, kind: MouseEventKind, wp: WPARAM, lp: LPARAM) -> (POINT, u32) {
    let mut pt = POINT {
        x: lo(lp.0 as usize),
        y: hi(lp.0 as usize),
    };
    let data = match kind {
        MouseEventKind::Wheel | MouseEventKind::HorizontalWheel => {
            // Wheel messages carry screen coordinates.
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            hi(wp.0) as u32
        }
        MouseEventKind::XButtonDown
        | MouseEventKind::XButtonUp
        | MouseEventKind::XButtonDoubleClick => (wp.0 >> 16) as u32 & 0xFFFF,
        MouseEventKind::Leave => {
            pt = POINT::default();
            0
        }
        _ => 0,
    };
    (pt, data)
}

fn on_mouse(hwnd: HWND, kind: MouseEventKind, wp: WPARAM, lp: LPARAM) {
    let Some(page) = page() else { return };
    use MouseEventKind as M;
    match kind {
        M::Move if !TRACKING.with(Cell::get) => {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            if unsafe { TrackMouseEvent(&mut tme) }.is_ok() {
                TRACKING.with(|t| t.set(true));
            }
        }
        M::Leave => TRACKING.with(|t| t.set(false)),
        M::LeftButtonDown | M::RightButtonDown | M::MiddleButtonDown | M::XButtonDown => unsafe {
            SetCapture(hwnd);
        },
        M::LeftButtonUp | M::RightButtonUp | M::MiddleButtonUp | M::XButtonUp => unsafe {
            let _ = ReleaseCapture();
        },
        _ => {}
    }
    let (pt, data) = mouse(hwnd, kind, wp, lp);
    page.send_mouse_input(kind, VirtualKeys::from_wparam(wp.0), data, pt);
}

/// Touch and pen go to the page as pointer input; a mouse pointer message
/// is left to `DefWindowProc` (which makes the `WM_MOUSE*` message).
fn on_pointer(kind: PointerEventKind, wp: WPARAM) -> bool {
    let Some(page) = page() else { return false };
    let id = (wp.0 & 0xFFFF) as u32;
    let mut ty = POINTER_INPUT_TYPE::default();
    if unsafe { GetPointerType(id, &mut ty) }.is_err() || (ty != PT_TOUCH && ty != PT_PEN) {
        return false;
    }
    let mut info = POINTER_INFO::default();
    if unsafe { GetPointerInfo(id, &mut info) }.is_err() {
        return false;
    }
    page.send_pointer_input(kind, &info);
    true
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(kind) = MouseEventKind::from_window_message(msg) {
        on_mouse(hwnd, kind, wp, lp);
        return LRESULT(0);
    }
    if let Some(kind) = PointerEventKind::from_window_message(msg)
        && on_pointer(kind, wp)
    {
        return LRESULT(0);
    }
    match msg {
        WM_SIZE => {
            if let Some(p) = page() {
                p.set_bounds(client_rect(hwnd));
            }
            LRESULT(0)
        }
        WM_MOVE => {
            if let Some(p) = page() {
                p.notify_parent_window_moved();
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let r = unsafe { *(lp.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            if let Some(p) = page() {
                p.set_rasterization_scale(f64::from((wp.0 & 0xFFFF) as u32) / 96.0);
            }
            LRESULT(0)
        }
        WM_SETCURSOR if (lp.0 as u32 & 0xFFFF) == HTCLIENT => match page() {
            Some(p) => {
                unsafe { SetCursor(Some(p.cursor())) };
                LRESULT(1)
            }
            None => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        },
        WM_SETFOCUS => {
            if let Some(p) = page() {
                p.move_focus(FocusReason::Programmatic);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // Let the page go before the loop ends (its webview closes).
            PAGE.with(|p| p.borrow_mut().take());
            copper_win_rt::quit(0);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
