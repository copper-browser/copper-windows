//! Other processes: closing a running Copper, starting Copper, and
//! the short-lived `cmd.exe` that deletes an uninstalled copper.exe.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_TIMEOUT, WPARAM};
use windows::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};
use windows_core::{BOOL, HSTRING, PCWSTR, PWSTR};

use crate::files::is_inside;
use crate::log::log;

/// The browser's top-level window class (copper-win-ui registers it).
pub const COPPER_WINDOW_CLASS: &str = "CopperWindow";

/// The exit code of a Copper that setup had to end after the grace period.
const ENDED_BY_SETUP: u32 = 1;

/// Top-level windows of `class` owned by other processes: (window, pid).
fn windows_of_class(class: &str) -> Vec<(HWND, u32)> {
    struct Search {
        class: Vec<u16>,
        me: u32,
        found: Vec<(HWND, u32)>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Search passed to EnumWindows below,
        // alive for the whole (synchronous) enumeration.
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut buf = [0u16; 64];
        // SAFETY: valid window handle from the enumeration; sized buffer.
        let n = unsafe { GetClassNameW(hwnd, &mut buf) };
        if n > 0 && buf[..n as usize] == search.class[..] {
            let mut pid = 0u32;
            // SAFETY: as above.
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != 0 && pid != search.me {
                search.found.push((hwnd, pid));
            }
        }
        BOOL(1)
    }
    let mut search = Search {
        class: class.encode_utf16().collect(),
        // SAFETY: trivial.
        me: unsafe { GetCurrentProcessId() },
        found: Vec::new(),
    };
    // SAFETY: `visit` only touches `search`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    search.found
}

/// The full path of process `pid`'s executable, if it can be read.
fn image_path(process: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: a handle opened with PROCESS_QUERY_LIMITED_INFORMATION; sized
    // buffer, its length in and the written length out.
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// A running Copper that has to go: its process and the windows it was
/// asked to close.
struct Running {
    pid: u32,
    handle: HANDLE,
    asked: Vec<HWND>,
}

impl Running {
    fn exited(&self) -> bool {
        // SAFETY: a handle we opened with SYNCHRONIZE.
        let r = unsafe { WaitForSingleObject(self.handle, 0) };
        r != WAIT_TIMEOUT
    }
}

/// Post `WM_CLOSE` to every `CopperWindow` of the processes in `running`
/// not yet asked, and pick up processes started meanwhile from `dir`.
fn ask_to_close(dir: &Path, running: &mut Vec<Running>) {
    let dir = dir.to_string_lossy();
    for (hwnd, pid) in windows_of_class(COPPER_WINDOW_CLASS) {
        let at = match running.iter().position(|r| r.pid == pid) {
            Some(i) => i,
            None => {
                // SAFETY: a handle to wait on, end, and read the exe path of.
                let opened = unsafe {
                    OpenProcess(
                        PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                        false,
                        pid,
                    )
                };
                let Ok(handle) = opened else {
                    continue;
                };
                // Only a Copper started from the install folder holds the
                // files being replaced or removed. A portable copy or a
                // developer's build elsewhere is someone else's session.
                let path = image_path(handle);
                if !path.as_deref().is_some_and(|p| is_inside(p, &dir)) {
                    // SAFETY: the handle just opened.
                    let _ = unsafe { CloseHandle(handle) };
                    continue;
                }
                log!(
                    "Copper is running: pid {pid} ({})",
                    path.unwrap_or_default()
                );
                running.push(Running {
                    pid,
                    handle,
                    asked: Vec::new(),
                });
                running.len() - 1
            }
        };
        let r = &mut running[at];
        if !r.asked.contains(&hwnd) {
            // SAFETY: posting is fire-and-forget; a window gone by now is fine.
            let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            r.asked.push(hwnd);
        }
    }
}

/// Close every Copper running from `dir` before its files change.
///
/// Each of its windows (class `CopperWindow`, however many there are) gets
/// `WM_CLOSE`, so the browser shuts down the way it does when the user
/// closes it: session saved, WebView2 released. Windows it opens while
/// closing are asked too. Whatever is still running after `grace` is ended
/// with `TerminateProcess` — an upgrade must not wait on a browser stuck
/// in a dialog. Returns how many processes were still running at the very
/// end (a process that can't be ended); the caller then moves the running
/// exe aside instead of overwriting it. `found` is called once, before any
/// waiting, if there is a Copper to close at all (for "Closing Copper…").
pub fn close_running_copper(dir: &Path, grace: Duration, found: impl FnOnce()) -> usize {
    let mut running = Vec::new();
    ask_to_close(dir, &mut running);
    if running.is_empty() {
        return 0;
    }
    found();
    let pids: Vec<u32> = running.iter().map(|r| r.pid).collect();
    let windows: usize = running.iter().map(|r| r.asked.len()).sum();
    log!("asked {windows} Copper window(s) in {pids:?} to close");

    let started = Instant::now();
    while started.elapsed() < grace {
        if running.iter().all(Running::exited) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
        ask_to_close(dir, &mut running);
    }

    let mut ended = Vec::new();
    for r in running.iter().filter(|r| !r.exited()) {
        // SAFETY: a handle opened with PROCESS_TERMINATE.
        match unsafe { TerminateProcess(r.handle, ENDED_BY_SETUP) } {
            Ok(()) => ended.push(r.pid),
            Err(e) => log!("couldn't end Copper (pid {}): {e}", r.pid),
        }
    }
    if !ended.is_empty() {
        log!("ended Copper {ended:?}: still running after {grace:?}");
        // TerminateProcess is asynchronous; give the handles a moment to
        // signal so the files are free when the caller copies.
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until && !running.iter().all(Running::exited) {
            std::thread::sleep(Duration::from_millis(50));
        }
    } else {
        log!("Copper closed in {:?}", started.elapsed());
    }

    let left = running.iter().filter(|r| !r.exited()).count();
    for r in running {
        // SAFETY: handles we opened, closed once.
        let _ = unsafe { CloseHandle(r.handle) };
    }
    if left > 0 {
        log!("{left} Copper process(es) still running");
    }
    left
}

/// Start Copper, detached, from its own folder.
pub fn launch(exe: &Path) -> anyhow::Result<()> {
    let dir = exe.parent().unwrap_or(Path::new("."));
    Command::new(exe).current_dir(dir).spawn()?;
    Ok(())
}

/// `C:\Windows\System32` — asked for, not taken from PATH.
fn system_dir() -> PathBuf {
    let mut buf = [0u16; 260];
    // SAFETY: sized buffer.
    let n = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return PathBuf::from(r"C:\Windows\System32");
    }
    PathBuf::from(String::from_utf16_lossy(&buf[..n]))
}

/// Leave behind a hidden `cmd.exe` that waits for this process to exit and
/// then deletes `exe` and `dir`. Falls back to scheduling both for deletion
/// at the next reboot (which needs rights a per-user process may not have;
/// logged either way).
pub fn delete_after_exit(exe: &Path, dir: &Path) {
    let args = crate::files::self_delete_args(&exe.to_string_lossy(), &dir.to_string_lossy());
    let spawned = Command::new(system_dir().join("cmd.exe"))
        .raw_arg(&args)
        // Not the folder being deleted: a process's current directory keeps
        // it from being removed.
        .current_dir(std::env::temp_dir())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn();
    match spawned {
        Ok(_) => log!("self-delete scheduled: cmd.exe {args}"),
        Err(e) => {
            log!("self-delete via cmd.exe failed ({e}); trying MoveFileEx at reboot");
            for path in [exe, dir] {
                // SAFETY: valid path; a null new name means delete.
                let r = unsafe {
                    MoveFileExW(
                        &HSTRING::from(path),
                        PCWSTR::null(),
                        MOVEFILE_DELAY_UNTIL_REBOOT,
                    )
                };
                if let Err(e) = r {
                    log!("MoveFileEx({}) failed: {e}", path.display());
                }
            }
        }
    }
}
