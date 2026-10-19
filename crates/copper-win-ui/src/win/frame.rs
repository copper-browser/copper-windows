//! The custom frame: no system caption, the client area to the window's
//! edges, Mica behind it, rounded corners, and the system's resize borders.
//!
//! `WM_NCCALCSIZE` keeps the left, right and bottom borders the system
//! computes (they are invisible on Windows 10 and 11 and carry the resize
//! hit areas) and gives the top back to the client; `WM_NCHITTEST` then
//! puts a resize band inside the top edge and maps the caption buttons and
//! drag regions. Mica (`DWMWA_SYSTEMBACKDROP_TYPE`) shows through wherever
//! nothing is drawn, because the window has no redirection bitmap and the
//! frame is extended over the whole client area.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE, DWMSBT_MAINWINDOW, DWMSBT_NONE,
    DWMSBT_TABBEDWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, IsZoomed, NCCALCSIZE_PARAMS, SM_CXPADDEDBORDER, SM_CYSIZEFRAME,
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    WM_NCCALCSIZE,
};
use windows::core::{BOOL, w};

/// What shows through the window where nothing is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    /// Mica: the desktop's colour, softly (Windows 11's main-window material).
    #[default]
    Mica,
    /// Mica Alt: the stronger tint Windows uses behind tabbed windows.
    MicaAlt,
    /// No material; the app's own ground everywhere.
    Solid,
}

impl Backdrop {
    fn system(self) -> DWM_SYSTEMBACKDROP_TYPE {
        match self {
            Backdrop::Mica => DWMSBT_MAINWINDOW,
            Backdrop::MicaAlt => DWMSBT_TABBEDWINDOW,
            Backdrop::Solid => DWMSBT_NONE,
        }
    }
}

fn set_attr<T>(
    hwnd: HWND,
    attr: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE,
    value: &T,
) -> bool {
    // SAFETY: `value` points at a live T of the size passed.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attr,
            (value as *const T).cast(),
            std::mem::size_of::<T>() as u32,
        )
        .is_ok()
    }
}

/// Extend the frame over the client area and ask for rounded corners.
pub fn init(hwnd: HWND) {
    let m = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    // SAFETY: a valid window and margins.
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
    }
    set_attr(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &DWM_WINDOW_CORNER_PREFERENCE(DWMWCP_ROUND.0),
    );
}

/// Apply the backdrop and the light/dark frame. Returns whether the
/// material is live (Windows 11 22H2 and later); when it is not the app
/// paints its own ground (the Windows 10 fallback).
pub fn backdrop(hwnd: HWND, b: Backdrop, dark: bool) -> bool {
    set_attr(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &BOOL::from(dark));
    let ok = set_attr(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &b.system());
    ok && b != Backdrop::Solid
}

/// The system's resize border at `dpi`, in pixels.
pub fn border(dpi: u32) -> i32 {
    // SAFETY: plain metric queries.
    unsafe {
        GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    }
}

pub fn is_maximized(hwnd: HWND) -> bool {
    // SAFETY: a state query.
    unsafe { IsZoomed(hwnd).as_bool() }
}

/// `WM_NCCALCSIZE`: the system's borders, but no caption.
pub fn nccalcsize(hwnd: HWND, wparam: WPARAM, lparam: LPARAM, dpi: u32) -> LRESULT {
    if wparam.0 == 0 {
        // SAFETY: default handling of the rect-only form.
        return unsafe { DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam) };
    }
    // SAFETY: with wParam TRUE, lParam points at NCCALCSIZE_PARAMS.
    unsafe {
        let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
        let top = params.rgrc[0].top;
        let r = DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam);
        if r.0 != 0 {
            return r;
        }
        params.rgrc[0].top = top;
        // A maximised window hangs its borders off the monitor; keep the
        // client's top on screen.
        if is_maximized(hwnd) {
            params.rgrc[0].top += border(dpi);
        }
        LRESULT(0)
    }
}

/// Whether the system is in dark mode for apps.
pub fn system_dark() -> bool {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reading a DWORD into a local of the right size.
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
        .is_ok()
    };
    ok && value == 0
}

/// Whether Windows' "Animation effects" are on (Reduce Motion when off).
pub fn animations_on() -> bool {
    let mut on = BOOL(1);
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes a BOOL.
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut on as *mut BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    on.as_bool()
}

/// A rectangle's size.
pub fn rect_size(r: &RECT) -> (i32, i32) {
    (r.right - r.left, r.bottom - r.top)
}
