//! The few things said before Copper has a window to say them in.

use super::wide;
use windows::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_YESNO, MessageBoxW,
};
use windows::core::PCWSTR;

/// Where Microsoft's Evergreen WebView2 Runtime installer lives.
pub const WEBVIEW2_DOWNLOAD: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// A plain error box.
pub fn error(title: &str, text: &str) {
    let (t, x) = (wide(title), wide(text));
    // SAFETY: NUL-terminated strings that outlive the call.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(x.as_ptr()),
            PCWSTR(t.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// The WebView2 Runtime is missing (or broken): say so in one sentence and
/// offer to open the download page.
pub fn webview2_missing(detail: &str) {
    let text = format!(
        "Copper needs the Microsoft Edge WebView2 Runtime, and it isn't installed on this PC.\n\n\
         Open the download page now? Copper starts once it is installed.\n\n({detail})"
    );
    let (t, x) = (wide("Copper"), wide(&text));
    // SAFETY: as above.
    let answer = unsafe {
        MessageBoxW(
            None,
            PCWSTR(x.as_ptr()),
            PCWSTR(t.as_ptr()),
            MB_YESNO | MB_ICONINFORMATION,
        )
    };
    if answer == IDYES {
        let _ = super::shell::open(WEBVIEW2_DOWNLOAD);
    }
}
