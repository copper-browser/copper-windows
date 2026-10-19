//! The engine's own windows: the hidden *park* window that parents
//! controllers no shell window hosts, and the backstage *stage* windows.

use std::sync::OnceLock;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics,
    MA_NOACTIVATE, RegisterClassExW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, ShowWindow,
    WINDOW_EX_STYLE, WM_MOUSEACTIVATE, WNDCLASSEXW, WS_CLIPCHILDREN, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("CopperWebView2Host");

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_MOUSEACTIVATE {
        // Never take activation from the shell window.
        return LRESULT(MA_NOACTIVATE as isize);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None).map(Into::into).unwrap_or_default() }
}

fn register() -> bool {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    *REGISTERED.get_or_init(|| {
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc) != 0 }
    })
}

/// An engine-owned window, destroyed on drop.
pub(crate) struct OwnedWindow(pub HWND);

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

fn create(
    ex: WINDOW_EX_STYLE,
    owner: Option<HWND>,
    rect: RECT,
) -> windows::core::Result<OwnedWindow> {
    if !register() {
        return Err(windows::core::Error::from_thread());
    }
    let hwnd = unsafe {
        CreateWindowExW(
            ex | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("Copper"),
            WS_POPUP | WS_CLIPCHILDREN,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            owner,
            None,
            Some(instance()),
            None,
        )?
    };
    Ok(OwnedWindow(hwnd))
}

/// The hidden window controllers live under while no shell window hosts
/// them (WebView2 needs a parent window for every controller). Never shown.
pub(crate) fn park_window() -> windows::core::Result<OwnedWindow> {
    create(
        WINDOW_EX_STYLE(0),
        None,
        RECT {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        },
    )
}

/// Where a stage window goes: just past the virtual screen's bottom-right
/// corner, so it never overlaps a monitor (not the minimised-window
/// position, which Windows treats specially).
fn off_screen(width: i32, height: i32) -> RECT {
    let (x, y, w, h) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let left = x + w + 64;
    let top = y + h + 64;
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

/// The backstage's stage window: an off-screen, non-activating tool window
/// (no taskbar button, never focused) that stays "visible" so Chromium
/// keeps rendering the pages in it. Unowned, so minimizing or closing a
/// shell window never hides or destroys it (or the pages in it).
pub(crate) fn stage_window(width: i32, height: i32) -> windows::core::Result<OwnedWindow> {
    let win = create(WS_EX_NOREDIRECTIONBITMAP, None, off_screen(width, height))?;
    unsafe {
        let _ = ShowWindow(win.0, SW_SHOWNOACTIVATE);
    }
    Ok(win)
}

/// Resize a stage window (keeping it off screen).
pub(crate) fn resize_stage(win: &OwnedWindow, width: i32, height: i32) {
    let r = off_screen(width, height);
    unsafe {
        let _ = SetWindowPos(
            win.0,
            None,
            r.left,
            r.top,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}
