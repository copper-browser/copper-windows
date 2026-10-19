//! A plain-text trail of what setup did, for when something goes wrong.
//!
//! The window says what failed in a sentence; this file has the detail
//! (`%TEMP%\CopperSetup.log`). Appended to, capped so it never grows past a
//! few hundred kilobytes, and never a reason for setup itself to fail.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Start over once the log passes this size.
const CAP_BYTES: u64 = 256 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

/// Where the log lives.
pub fn path() -> PathBuf {
    std::env::temp_dir().join("CopperSetup.log")
}

/// Append one line. Errors are swallowed: logging must never break setup.
pub fn line(message: &str) {
    let _guard = LOCK.lock();
    let path = path();
    let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > CAP_BYTES);
    let file = OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(&path);
    if let Ok(mut file) = file {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let _ = writeln!(file, "[{secs}] [pid {}] {message}", std::process::id());
    }
}

/// `log!("copied {} bytes", n)`: format and append one line.
macro_rules! log {
    ($($arg:tt)*) => { $crate::log::line(&format!($($arg)*)) };
}
pub(crate) use log;
