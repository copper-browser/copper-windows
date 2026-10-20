//! Reading the clipboard's text for the Connect card's Paste button.
//!
//! The chrome toolkit gains `Ui::clipboard_text` in its next merge; this
//! is the same `CF_UNICODETEXT` read until then, behind the one function
//! the page calls.

use windows::Win32::Foundation::{HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

/// `CF_UNICODETEXT`.
const CF_UNICODETEXT: u32 = 13;

/// The clipboard's text, if it has any. Another program may hold the
/// clipboard for a moment: a few tries, 4 ms apart.
pub fn text(hwnd: HWND) -> Option<String> {
    // SAFETY: opening with our own window as owner.
    let opened = (0..5).any(|i| {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        unsafe { OpenClipboard(Some(hwnd)) }.is_ok()
    });
    if !opened {
        return None;
    }
    // SAFETY: the clipboard is open; the handle stays valid until it
    // closes, and the locked memory is read only up to its NUL.
    unsafe {
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0usize;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        });
        let _ = CloseClipboard();
        text
    }
}
