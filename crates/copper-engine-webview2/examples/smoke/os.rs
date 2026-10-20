//! Real OS input and the system dialogs, for the checks that need what a
//! user does: a right click, the Menu key, Shift+F10, answering the Save As
//! dialog. Input goes in with `SendInput`, so the smoke window must be the
//! foreground window ([`wait_foreground`]).

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, IsWindowEnabled, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT,
    SendInput, VIRTUAL_KEY, VK_APPS, VK_ESCAPE, VK_F10, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, EnumChildWindows, EnumWindows, GW_OWNER, GetClassNameW, GetForegroundWindow,
    GetWindow, GetWindowTextW, IsWindowVisible, PostMessageW, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, SetCursorPos, SetForegroundWindow, WM_CLOSE, WM_COMMAND, WM_GETTEXT,
};
use windows::core::BOOL;

/// `IDOK` / `IDCANCEL` as `WM_COMMAND` ids.
const IDOK: usize = 1;
const IDCANCEL: usize = 2;
/// How long a cross-thread `WM_GETTEXT`/`WM_SETTEXT` may take.
const SEND_TIMEOUT_MS: u32 = 2000;

/// Wait (polling, never blocking the loop) until `hwnd` is the foreground
/// window, asking for it once a second. Each ask taps Alt first: Windows
/// lets the process that received the last input take the foreground.
pub async fn wait_foreground(hwnd: HWND, limit: Duration) -> bool {
    let t = Instant::now();
    let mut asked = None::<Instant>;
    while t.elapsed() < limit {
        if unsafe { GetForegroundWindow() } == hwnd {
            return true;
        }
        if asked.is_none_or(|a| a.elapsed() >= Duration::from_secs(1)) {
            send(&[key(VK_MENU, false), key(VK_MENU, true)]);
            unsafe {
                let _ = SetForegroundWindow(hwnd);
            }
            asked = Some(Instant::now());
        }
        copper_win_rt::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// A real right click at client point `at` (physical px) of `hwnd`.
pub fn right_click(hwnd: HWND, at: (i32, i32)) {
    let mut pt = POINT { x: at.0, y: at.1 };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut pt);
        let _ = SetCursorPos(pt.x, pt.y);
    }
    send(&[mouse(MOUSEEVENTF_RIGHTDOWN), mouse(MOUSEEVENTF_RIGHTUP)]);
}

/// The Menu (Apps) key.
pub fn menu_key() {
    send(&[key(VK_APPS, false), key(VK_APPS, true)]);
}

/// Shift+F10.
pub fn shift_f10() {
    send(&[
        key(VK_SHIFT, false),
        key(VK_F10, false),
        key(VK_F10, true),
        key(VK_SHIFT, true),
    ]);
}

/// Type `text` into the focused control, as a user would: Ctrl+A first
/// (replacing what is there), each character as a Unicode key stroke.
pub fn type_over(text: &str) {
    let ctrl = VIRTUAL_KEY(0x11);
    let a = VIRTUAL_KEY(u16::from(b'A'));
    send(&[
        key(ctrl, false),
        key(a, false),
        key(a, true),
        key(ctrl, true),
    ]);
    let strokes: Vec<INPUT> = text
        .encode_utf16()
        .flat_map(|unit| [unicode(unit, false), unicode(unit, true)])
        .collect();
    send(&strokes);
}

/// Enter.
pub fn enter() {
    let ret = VIRTUAL_KEY(0x0D);
    send(&[key(ret, false), key(ret, true)]);
}

/// Escape (closes a native menu).
pub fn escape() {
    send(&[key(VK_ESCAPE, false), key(VK_ESCAPE, true)]);
}

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn mouse(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    }
}

fn unicode(unit: u16, up: bool) -> INPUT {
    let flags = if up {
        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_UNICODE
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wScan: unit,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

/// Whether `hwnd` takes input (a modal dialog disables its owner).
pub fn enabled(hwnd: HWND) -> bool {
    unsafe { IsWindowEnabled(hwnd).as_bool() }
}

/// The visible dialog (`#32770`) `owner` owns, if one is open.
pub fn owned_dialog(owner: HWND) -> Option<HWND> {
    struct Find {
        owner: HWND,
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let find = unsafe { &mut *(lp.0 as *mut Find) };
        let owned = unsafe { GetWindow(hwnd, GW_OWNER) }.ok() == Some(find.owner);
        if owned && unsafe { IsWindowVisible(hwnd) }.as_bool() && class(hwnd) == "#32770" {
            find.found = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut find = Find { owner, found: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut find as *mut Find as isize));
    }
    find.found
}

/// Wait (polling) for the dialog `owner` owns.
pub async fn wait_dialog(owner: HWND, limit: Duration) -> Option<HWND> {
    let t = Instant::now();
    while t.elapsed() < limit {
        if let Some(d) = owned_dialog(owner) {
            return Some(d);
        }
        copper_win_rt::sleep(Duration::from_millis(100)).await;
    }
    None
}

/// Wait (polling) until `dialog` is gone.
pub async fn wait_closed(dialog: HWND, owner: HWND, limit: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        if owned_dialog(owner) != Some(dialog) {
            return true;
        }
        copper_win_rt::sleep(Duration::from_millis(100)).await;
    }
    false
}

fn class(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The children of `dialog` with window class `want`, in z order.
fn children(dialog: HWND, want: &str) -> Vec<HWND> {
    struct Collect<'a> {
        want: &'a str,
        out: Vec<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let c = unsafe { &mut *(lp.0 as *mut Collect) };
        if class(hwnd) == c.want {
            c.out.push(hwnd);
        }
        BOOL(1)
    }
    let mut c = Collect {
        want,
        out: Vec::new(),
    };
    unsafe {
        let _ = EnumChildWindows(
            Some(dialog),
            Some(each),
            LPARAM(&mut c as *mut Collect as isize),
        );
    }
    c.out
}

/// The Save As dialog's "File name" box: its first edit control.
fn name_box(dialog: HWND) -> Option<HWND> {
    children(dialog, "Edit").into_iter().next()
}

/// The text of the dialog's "File name" box.
pub fn file_name(dialog: HWND) -> Option<String> {
    let edit = name_box(dialog)?;
    let mut buf = vec![0u16; 1024];
    let mut copied = 0usize;
    unsafe {
        SendMessageTimeoutW(
            edit,
            WM_GETTEXT,
            WPARAM(buf.len()),
            LPARAM(buf.as_mut_ptr() as isize),
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            Some(&mut copied),
        );
    }
    buf.truncate(copied);
    Some(String::from_utf16_lossy(&buf))
}

/// How to answer the dialog.
#[derive(Clone, Copy, Debug)]
pub enum Answer {
    /// The Save button (`IDOK`; the button itself if that does nothing).
    Save,
    /// The Cancel button.
    Cancel,
    /// The title bar's close box (`WM_CLOSE`).
    Close,
}

/// Answer `dialog`; resolves to whether it closed.
pub async fn answer(dialog: HWND, owner: HWND, how: Answer) -> bool {
    let post = |msg, wp: usize| unsafe {
        let _ = PostMessageW(Some(dialog), msg, WPARAM(wp), LPARAM(0));
    };
    match how {
        Answer::Save => post(WM_COMMAND, IDOK),
        Answer::Cancel => post(WM_COMMAND, IDCANCEL),
        Answer::Close => post(WM_CLOSE, 0),
    }
    if wait_closed(dialog, owner, Duration::from_secs(3)).await {
        return true;
    }
    // A Save the dialog did not take as `IDOK`: its button.
    if let Answer::Save = how
        && let Some(button) = children(dialog, "Button")
            .into_iter()
            .find(|b| title(*b).replace('&', "").eq_ignore_ascii_case("save"))
    {
        unsafe {
            let _ = PostMessageW(Some(button), BM_CLICK, WPARAM(0), LPARAM(0));
        }
        return wait_closed(dialog, owner, Duration::from_secs(3)).await;
    }
    false
}
