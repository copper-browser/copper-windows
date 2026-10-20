//! The top-level window's place on screen: remembered across runs, full
//! screen and back, brought forward for a second launch, resized for
//! automation. Each call sends the window messages synchronously, so they
//! are only made with the window's state free (never inside an edit).

use std::path::Path;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, HWND_TOP, IsIconic,
    SW_RESTORE, SW_SHOWMAXIMIZED, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
    SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPlacement,
    SetWindowPos, ShowWindow, WINDOWPLACEMENT, WPF_ASYNCWINDOWPLACEMENT, WS_CAPTION, WS_THICKFRAME,
};

/// Windows 11's 1-DIP window border and round corners: off while the
/// window covers the monitor (a light hairline round a black frame),
/// back after.
fn bare_edges(hwnd: HWND, bare: bool) {
    // DWMWA_COLOR_NONE / DWMWA_COLOR_DEFAULT.
    let colour: u32 = if bare { 0xFFFF_FFFE } else { 0xFFFF_FFFF };
    let corner = if bare {
        DWMWCP_DONOTROUND
    } else {
        DWMWCP_ROUND
    };
    // SAFETY: plain attribute writes with correctly sized values; failures
    // (Windows 10) are harmless.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            (&colour as *const u32).cast(),
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            size_of::<i32>() as u32,
        );
    }
}

/// The window as it was before going full screen.
#[derive(Clone, Copy)]
pub struct Saved {
    style: isize,
    placement: WINDOWPLACEMENT,
}

/// Cover `hwnd`'s monitor with it: no frame, no caption.
pub fn enter_fullscreen(hwnd: HWND) -> Option<Saved> {
    // SAFETY: plain Win32 calls on our own window with valid out-params.
    unsafe {
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        GetWindowPlacement(hwnd, &mut placement).ok()?;
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let bare = style & !((WS_CAPTION.0 | WS_THICKFRAME.0) as isize);
        SetWindowLongPtrW(hwnd, GWL_STYLE, bare);
        bare_edges(hwnd, true);
        let r = info.rcMonitor;
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
        );
        Some(Saved { style, placement })
    }
}

/// Back to where it was before [`enter_fullscreen`].
pub fn leave_fullscreen(hwnd: HWND, saved: Saved) {
    // SAFETY: as above; the placement is the one Windows gave us.
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_STYLE, saved.style);
        bare_edges(hwnd, false);
        let _ = SetWindowPlacement(hwnd, &saved.placement);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER,
        );
    }
}

/// Bring the window forward (a second launch handed it a page).
pub fn bring_forward(hwnd: HWND) {
    // SAFETY: our own window.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
}

/// Physical pixels per DIP.
pub fn scale(hwnd: HWND) -> f64 {
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
}

/// Make the window `w` × `h` DIPs, outer size (`browser_resize`).
pub fn resize(hwnd: HWND, w: i32, h: i32) {
    let s = scale(hwnd);
    // SAFETY: our own window.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            (f64::from(w) * s).round() as i32,
            (f64::from(h) * s).round() as i32,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// The window's place, as remembered on disk: the normal (restored)
/// rectangle in screen pixels, and whether it was maximised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub maximized: bool,
}

impl Place {
    /// `left top right bottom maximized` on one line.
    pub fn to_line(self) -> String {
        format!(
            "{} {} {} {} {}",
            self.left,
            self.top,
            self.right,
            self.bottom,
            u8::from(self.maximized)
        )
    }

    pub fn from_line(s: &str) -> Option<Place> {
        let v: Vec<i32> = s
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let [left, top, right, bottom, max] = v.as_slice() else {
            return None;
        };
        (right - left >= 200 && bottom - top >= 150).then_some(Place {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
            maximized: *max != 0,
        })
    }
}

/// Where the window is now.
pub fn place(hwnd: HWND) -> Option<Place> {
    // SAFETY: valid out-param.
    unsafe {
        let mut p = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        GetWindowPlacement(hwnd, &mut p).ok()?;
        let r = p.rcNormalPosition;
        Some(Place {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
            maximized: p.showCmd == SW_SHOWMAXIMIZED.0 as u32,
        })
    }
}

/// Put the (still hidden) window where it was last time, if that place is
/// still on a monitor. Returns whether it should open maximised.
pub fn restore(hwnd: HWND, place: Place) -> bool {
    // SAFETY: plain calls on our own hidden window.
    unsafe {
        let mut p = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        if GetWindowPlacement(hwnd, &mut p).is_err() {
            return false;
        }
        p.flags = WPF_ASYNCWINDOWPLACEMENT;
        // Hidden: placing it must not show it.
        p.showCmd = 0;
        p.rcNormalPosition = RECT {
            left: place.left,
            top: place.top,
            right: place.right,
            bottom: place.bottom,
        };
        let _ = SetWindowPlacement(hwnd, &p);
        // Off every monitor (one unplugged since): back on the nearest.
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_ok() {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                let w = info.rcWork;
                let visible = r.left < w.right - 64
                    && r.right > w.left + 64
                    && r.top < w.bottom - 32
                    && r.top >= w.top - 8;
                if !visible {
                    let width = (r.right - r.left).min(w.right - w.left);
                    let height = (r.bottom - r.top).min(w.bottom - w.top);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        w.left + (w.right - w.left - width) / 2,
                        w.top + (w.bottom - w.top - height) / 2,
                        width,
                        height,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
        }
        place.maximized
    }
}

/// Read the remembered place from `path`.
pub fn load(path: &Path) -> Option<Place> {
    Place::from_line(std::fs::read_to_string(path).ok()?.trim())
}

/// Remember `place` in `path` (best effort).
pub fn save(path: &Path, place: Place) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, place.to_line()) {
        tracing::warn!("could not remember the window's place: {e}");
    }
}
