//! Other processes: asking a running Copper to quit, starting Copper, and
//! the short-lived `cmd.exe` that deletes an uninstalled copper.exe.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_TIMEOUT, WPARAM};
use windows::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, GetCurrentProcessId, OpenProcess, PROCESS_SYNCHRONIZE, WaitForMultipleObjects,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};
use windows_core::{BOOL, HSTRING, PCWSTR};

use crate::log::log;

/// The browser's top-level window class (copper-win-ui registers it).
pub const COPPER_WINDOW_CLASS: &str = "CopperWindow";

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

/// Ask every running Copper to close (`WM_CLOSE` to each `CopperWindow`)
/// and wait up to `timeout` for those processes to exit.
///
/// Returns how many are still running afterwards. Not an error if some
/// are: the caller moves the running exe aside instead of overwriting it.
pub fn close_running_copper(timeout: Duration) -> usize {
    let found = windows_of_class(COPPER_WINDOW_CLASS);
    if found.is_empty() {
        return 0;
    }
    let mut pids: Vec<u32> = found.iter().map(|&(_, pid)| pid).collect();
    pids.sort_unstable();
    pids.dedup();
    log!(
        "asking {} Copper process(es) to close: {pids:?}",
        pids.len()
    );

    let handles: Vec<HANDLE> = pids
        .iter()
        // SAFETY: SYNCHRONIZE-only handle to a process we may wait on.
        .filter_map(|&pid| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok())
        .collect();
    for &(hwnd, _) in &found {
        // SAFETY: posting is fire-and-forget; a window gone by now is fine.
        let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }

    // WaitForMultipleObjects takes at most 64 handles; Copper is one process
    // in practice, but don't fall over on a pathological case.
    let deadline = std::time::Instant::now() + timeout;
    let mut running = 0;
    for chunk in handles.chunks(64) {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        // SAFETY: valid handles.
        let r = unsafe { WaitForMultipleObjects(chunk, true, left.as_millis() as u32) };
        if r == WAIT_TIMEOUT {
            running += chunk.len();
        }
    }
    for h in handles {
        // SAFETY: handles we opened.
        let _ = unsafe { CloseHandle(h) };
    }
    if running > 0 {
        log!("{running} Copper process(es) still running after {timeout:?}");
    }
    running
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
