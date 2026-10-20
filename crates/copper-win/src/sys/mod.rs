//! What Copper asks of Windows itself, each in its own small module: who
//! the user is and which languages they read, whether the machine is
//! online, the user's calendar, the window's screenshot, opening things
//! with the shell (and a file's icon), slow work off the window's thread,
//! the window's place on screen, Windows' privacy switches, and the few
//! messages shown before there is a window to show them in.

pub mod calendar;
pub mod dialog;
pub mod files;
pub mod network;
pub mod privacy;
pub mod screenshot;
pub mod shell;
pub mod user;
pub mod window;
pub mod work;

/// `s` as a NUL-terminated UTF-16 string.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A UTF-16 buffer up to its first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
