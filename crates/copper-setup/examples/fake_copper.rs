//! A stand-in for a running Copper browser, for testing how setup closes
//! one on upgrade and uninstall without the real browser.
//!
//! It opens `--windows N` (default 1) top-level windows of the browser's
//! class, `CopperWindow`, and appends one line per event to `--log FILE`:
//! `start`, `close <n>` for each `WM_CLOSE` a window receives, and `exit`
//! when the last window is gone. With `--stubborn` it ignores `WM_CLOSE`
//! (a browser stuck in a dialog), so setup has to end it after its grace
//! period. Copy it over the installed `copper.exe` to make setup see it as
//! the installed Copper.
//!
//! ```text
//! fake_copper.exe --windows 3 --log C:\Users\Public\setup\fake.log
//! fake_copper.exe --stubborn --log C:\Users\Public\setup\fake.log
//! ```

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    imp::main();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("fake_copper is a Windows test helper");
}

#[cfg(windows)]
mod imp {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GetMessageW, MSG, PostQuitMessage, RegisterClassExW, SW_SHOWNOACTIVATE, ShowWindow,
        TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
    };
    use windows_core::{HSTRING, PCWSTR, w};

    static LOG: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
    static STARTED: OnceLock<Instant> = OnceLock::new();
    static STUBBORN: AtomicBool = AtomicBool::new(false);
    static OPEN: AtomicUsize = AtomicUsize::new(0);

    fn log(line: &str) {
        let ms = STARTED.get().map_or(0, |t| t.elapsed().as_millis());
        if let Some(Ok(mut guard)) = LOG.get().map(Mutex::lock)
            && let Some(f) = guard.as_mut()
        {
            let _ = writeln!(f, "{ms:>6} ms pid {} {line}", std::process::id());
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_CLOSE => {
                log(&format!("close {:?}", hwnd.0));
                if !STUBBORN.load(Ordering::Relaxed) {
                    // SAFETY: our window.
                    let _ = unsafe { DestroyWindow(hwnd) };
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                if OPEN.fetch_sub(1, Ordering::Relaxed) == 1 {
                    log("exit");
                    // SAFETY: ends this thread's loop.
                    unsafe { PostQuitMessage(0) };
                }
                LRESULT(0)
            }
            // SAFETY: default handling.
            _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
        }
    }

    pub fn main() {
        let _ = STARTED.set(Instant::now());
        let args: Vec<String> = std::env::args().skip(1).collect();
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let count: usize = value("--windows").and_then(|n| n.parse().ok()).unwrap_or(1);
        STUBBORN.store(args.iter().any(|a| a == "--stubborn"), Ordering::Relaxed);
        let file = value("--log").and_then(|p| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .ok()
        });
        let _ = LOG.set(Mutex::new(file));
        log(&format!(
            "start windows={count} stubborn={}",
            STUBBORN.load(Ordering::Relaxed)
        ));

        // SAFETY: plain window class registration and creation; the loop
        // below is the standard one.
        unsafe {
            let module = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("CopperWindow");
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: module.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            for i in 0..count {
                let title = HSTRING::from(format!("Fake Copper {}", i + 1));
                if let Ok(hwnd) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class,
                    PCWSTR(title.as_ptr()),
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    480,
                    320,
                    None,
                    None,
                    Some(module.into()),
                    None,
                ) {
                    OPEN.fetch_add(1, Ordering::Relaxed);
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
