//! A mention of you while Copper isn't the app in front (Mac
//! `CanvasChatNotices`): one Windows notification — a notification-area
//! balloon, which Windows 10 and 11 show as a toast under Copper's name —
//! that opens the canvas at the message when clicked. The icon is there
//! only while a notice is.

use super::copy::Mention;
use crate::platform::{Shell, WeakShell};
use crate::sys::wide;
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_RESPECT_QUIET_TIME, NIIF_USER, NIM_ADD,
    NIM_DELETE, NIM_SETVERSION, NIN_BALLOONHIDE, NIN_BALLOONTIMEOUT, NIN_BALLOONUSERCLICK,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetForegroundWindow, GetWindowThreadProcessId, HICON,
    HWND_MESSAGE, LoadIconW, RegisterClassExW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WNDCLASSEXW,
};
use windows::core::PCWSTR;

const CLASS: &str = "CopperCanvasNotices";
const CALLBACK: u32 = WM_APP + 0x63;
const ICON_ID: u32 = 0x6361;

struct Notices {
    hwnd: Option<HWND>,
    shell: Option<WeakShell>,
    shown: Option<Mention>,
}

thread_local! {
    static NOTICES: RefCell<Notices> = const {
        RefCell::new(Notices { hwnd: None, shell: None, shown: None })
    };
}

/// Copper is the app in front.
pub fn frontmost() -> bool {
    // SAFETY: plain queries.
    unsafe {
        let w = GetForegroundWindow();
        if w.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

fn window() -> Option<HWND> {
    if let Some(h) = NOTICES.with(|n| n.borrow().hwnd) {
        return Some(h);
    }
    let class = wide(CLASS);
    // SAFETY: a message-only window with a static procedure; the strings
    // outlive the calls.
    let hwnd = unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(class.as_ptr()),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        )
        .ok()?
    };
    NOTICES.with(|n| n.borrow_mut().hwnd = Some(hwnd));
    Some(hwnd)
}

fn copy_into(dst: &mut [u16], text: &str) {
    let w: Vec<u16> = text.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

/// A mention of you arrived: noticed outside Copper when it isn't in front.
pub fn mention(shell: &Shell, m: &Mention) {
    if frontmost() {
        return;
    }
    let Some(hwnd) = window() else { return };
    remove();
    NOTICES.with(|n| {
        let mut n = n.borrow_mut();
        n.shell = Some(shell.downgrade());
        n.shown = Some(m.clone());
    });
    let title = format!("{} {}", m.who(), m.verb());
    let body = if m.excerpt.is_empty() {
        m.canvas_name.clone()
    } else {
        format!("{}\n{}", m.canvas_name, m.excerpt)
    };
    tracing::info!(target: "copper::canvas", mention = %m.id, "mention notice");
    let mut d = data(hwnd);
    d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_INFO;
    d.uCallbackMessage = CALLBACK;
    // SAFETY: the app icon from our own resources (id 1, copper.rc).
    d.hIcon = unsafe {
        GetModuleHandleW(None)
            .ok()
            .and_then(|m| LoadIconW(Some(m.into()), PCWSTR(1 as _)).ok())
            .unwrap_or(HICON::default())
    };
    copy_into(&mut d.szTip, "Copper");
    copy_into(&mut d.szInfoTitle, &title);
    copy_into(&mut d.szInfo, &body);
    d.dwInfoFlags = NIIF_USER | NIIF_RESPECT_QUIET_TIME;
    d.hBalloonIcon = d.hIcon;
    d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    // SAFETY: a filled-in NOTIFYICONDATAW for our window.
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &d).as_bool() {
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
        }
    }
}

/// Away with the icon (and its notice).
fn remove() {
    let Some(hwnd) = NOTICES.with(|n| n.borrow().hwnd) else {
        return;
    };
    let d = data(hwnd);
    // SAFETY: our own icon, if it is there.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &d);
    }
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == CALLBACK {
        // Version 4: the event is lparam's low word.
        let event = (lparam.0 as u32) & 0xFFFF;
        if event == NIN_BALLOONUSERCLICK {
            let taken = NOTICES.with(|n| {
                let mut n = n.borrow_mut();
                n.shown.take().zip(n.shell.clone())
            });
            remove();
            if let Some((m, weak)) = taken {
                // On the loop's next turn, not inside the notification's call.
                copper_win_rt::spawn_local(async move {
                    if let Some(shell) = weak.upgrade() {
                        super::pills::open_mention(&shell, &m);
                    }
                });
            }
        } else if event == NIN_BALLOONTIMEOUT || event == NIN_BALLOONHIDE {
            NOTICES.with(|n| n.borrow_mut().shown = None);
            remove();
        }
        return LRESULT(0);
    }
    // SAFETY: the default procedure for everything else.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
