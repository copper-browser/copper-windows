//! The window as a PNG (`ShellView::screenshot`: the agents' `shot`, the
//! `--screenshot` hook).
//!
//! Copper's window has no redirection bitmap (`WS_EX_NOREDIRECTIONBITMAP`:
//! everything is DirectComposition), so `BitBlt` from its DC reads black.
//! `PrintWindow` with `PW_RENDERFULLCONTENT` asks DWM for the composed
//! content instead — the chrome and the pages' visuals together.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

/// `PW_RENDERFULLCONTENT` (Windows 8.1+), missing from the bindings' enum.
const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);
/// `PW_CLIENTONLY`.
const PW_CLIENTONLY: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(1);

/// The client area of `hwnd` as a PNG, in physical pixels.
pub fn capture(hwnd: HWND) -> Option<Vec<u8>> {
    let mut rc = RECT::default();
    // SAFETY: a query on a live window.
    unsafe { GetClientRect(hwnd, &mut rc) }.ok()?;
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    if w <= 0 || h <= 0 {
        return None;
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            // Negative: top-down rows.
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: a memory DC and a DIB section owned here and released on
    // every path below; `bits` is valid while the section lives.
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        ReleaseDC(None, screen);
        let mut bits = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            return None;
        };
        let old = SelectObject(dc, HGDIOBJ(bmp.0));
        let ok = PrintWindow(
            hwnd,
            dc,
            PRINT_WINDOW_FLAGS(PW_CLIENTONLY.0 | PW_RENDERFULLCONTENT.0),
        )
        .as_bool();
        let stride = w as usize * 4;
        let png = if ok && !bits.is_null() {
            let px = std::slice::from_raw_parts(bits as *const u8, stride * h as usize);
            crate::picture::encode_png(px, w as u32, h as u32, stride)
        } else {
            None
        };
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(dc);
        png
    }
}
