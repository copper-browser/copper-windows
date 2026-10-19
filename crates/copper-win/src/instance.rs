//! One browser per world (`COPPER_WORLD`): the first start owns a named
//! mutex and a message-only window; a later start finds that window, hands
//! it the addresses it was given (`WM_COPYDATA`) and exits. The window
//! answers on the loop's next turn, so a hand-over never runs inside
//! another message.

use crate::sys::wide;
use std::cell::RefCell;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, FindWindowExW,
    GetWindowThreadProcessId, HWND_MESSAGE, RegisterClassExW, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COPYDATA, WNDCLASSEXW,
};
use windows::core::PCWSTR;

/// The message-only window's class.
const CLASS: &str = "CopperInstance";
/// `dwData` of a hand-over: "COPR".
const TAG: usize = 0x434F_5052;
/// How long a second start waits for the first one's window to appear.
const WAIT: Duration = Duration::from_secs(5);

/// The world's name, for the mutex and the window ("" without a world).
fn world() -> String {
    std::env::var("COPPER_WORLD")
        .map(|w| w.trim().to_string())
        .unwrap_or_default()
}

/// What a start is.
pub enum Role {
    /// The first: keep the guard as long as the process runs.
    First(Guard),
    /// Another runs; the addresses were handed to it.
    Handed,
}

/// The mutex that makes this start the first.
pub struct Guard(HANDLE);

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Claim the world, or hand `open` to whoever has it.
pub fn claim(open: &[String]) -> Role {
    let name = wide(&format!("Local\\Copper.Browser.{}", world()));
    // SAFETY: a named mutex; the name outlives the call.
    let mutex = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) };
    let Ok(mutex) = mutex else {
        // No mutex, no knowing: run on our own.
        return Role::First(Guard(HANDLE::default()));
    };
    // SAFETY: read straight after the call that set it.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if !existed {
        return Role::First(Guard(mutex));
    }
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(hwnd) = find()
            && hand(hwnd, open)
        {
            // SAFETY: our handle.
            unsafe {
                let _ = CloseHandle(mutex);
            }
            return Role::Handed;
        }
        if Instant::now() >= deadline {
            // The first start never came up (or hung): run anyway.
            tracing::warn!("another Copper holds this world but does not answer");
            return Role::First(Guard(mutex));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The first start's message-only window.
fn find() -> Option<HWND> {
    let class = wide(CLASS);
    let title = wide(&world());
    // SAFETY: both strings outlive the call.
    unsafe {
        FindWindowExW(
            Some(HWND_MESSAGE),
            None,
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
        )
        .ok()
    }
}

/// Send `open` (one address per line; empty: just come forward).
fn hand(hwnd: HWND, open: &[String]) -> bool {
    let mut pid = 0u32;
    // SAFETY: valid window and out-param.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            // The first start may take the foreground when it opens these.
            let _ = AllowSetForegroundWindow(pid);
        }
    }
    let body = open.join("\n");
    let data = COPYDATASTRUCT {
        dwData: TAG,
        cbData: body.len() as u32,
        lpData: body.as_ptr() as *mut _,
    };
    let mut result = 0usize;
    // SAFETY: `data` and `body` live until the call returns (WM_COPYDATA
    // is copied into the receiver during it).
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&data as *const COPYDATASTRUCT as isize),
            SMTO_ABORTIFHUNG,
            3000,
            Some(&mut result),
        )
    };
    sent.0 != 0 && result == 1
}

type Receiver = Box<dyn Fn(Vec<String>)>;

thread_local! {
    static RECEIVER: RefCell<Option<Receiver>> = const { RefCell::new(None) };
}

/// Listen for later starts: `receive` gets what each hands over (on the
/// loop's next turn). UI thread, once.
pub fn listen(receive: impl Fn(Vec<String>) + 'static) {
    RECEIVER.with(|r| *r.borrow_mut() = Some(Box::new(receive)));
    let class = wide(CLASS);
    let title = wide(&world());
    // SAFETY: registering and creating a message-only window with a static
    // window procedure; the strings outlive the calls.
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        if let Err(e) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        ) {
            tracing::warn!("a second start cannot reach this one: {e}");
        }
    }
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA {
        // SAFETY: WM_COPYDATA's lparam is a COPYDATASTRUCT valid for the
        // duration of the message.
        let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
        if data.dwData != TAG {
            return LRESULT(0);
        }
        let bytes = if data.cbData == 0 || data.lpData.is_null() {
            &[][..]
        } else {
            // SAFETY: cbData bytes at lpData, per the message.
            unsafe { std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize) }
        };
        let open: Vec<String> = String::from_utf8_lossy(bytes)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect();
        copper_win_rt::spawn_local(async move {
            RECEIVER.with(|r| {
                if let Some(f) = r.borrow().as_ref() {
                    f(open);
                }
            });
        });
        return LRESULT(1);
    }
    // SAFETY: default handling for everything else.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
