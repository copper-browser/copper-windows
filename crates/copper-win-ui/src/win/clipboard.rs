//! Plain-text clipboard (`CF_UNICODETEXT`).

use windows::Win32::Foundation::GlobalFree;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

/// `CF_UNICODETEXT`.
const CF_UNICODETEXT: u32 = 13;

/// Another process may hold the clipboard for a moment; try a few times.
fn open(hwnd: HWND) -> bool {
    for _ in 0..5 {
        // SAFETY: opening with our own window as owner.
        if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    false
}

/// The clipboard's text, if it has any.
pub fn get(hwnd: HWND) -> Option<String> {
    if !open(hwnd) {
        return None;
    }
    // SAFETY: the clipboard is open; the handle stays valid until it closes,
    // and the locked memory is read only up to its NUL.
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

/// Put `text` on the clipboard.
pub fn set(hwnd: HWND, text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    if !open(hwnd) {
        return false;
    }
    // SAFETY: the clipboard is open; the global block is sized for the
    // string and handed to the system on success (freed by us on failure).
    unsafe {
        let ok = (|| {
            EmptyClipboard().ok()?;
            let g = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let p = GlobalLock(g) as *mut u16;
            if p.is_null() {
                let _ = GlobalFree(Some(g));
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(g);
            if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))).is_err() {
                let _ = GlobalFree(Some(g));
                return None;
            }
            Some(())
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}
