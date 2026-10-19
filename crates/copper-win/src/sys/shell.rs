//! Handing things to the Windows shell: an address to the default browser
//! (`copper cloud sso`), a file to its app, a folder to Explorer.

use super::wide;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

/// Open `target` (an address, a file or a folder) the way a double-click
/// in Explorer would.
pub fn open(target: &str) -> Result<(), String> {
    let t = wide(target);
    // SAFETY: NUL-terminated strings that outlive the call.
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(t.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute's own convention: greater than 32 is success.
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("couldn't open {target} (error {})", r.0 as isize))
    }
}
