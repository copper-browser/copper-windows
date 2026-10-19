//! Copper's colours for the setup window, from the Mac's `Design.swift`.
//!
//! Every colour there is a pair of greys — one for a light window, one for a
//! dark — and so is every colour here. The pair is picked by the system's
//! app mode (Settings › Personalisation › Colours), and High Contrast swaps
//! the whole palette for the user's own system colours.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

use crate::sys::reg::{self, HKEY_CURRENT_USER};

/// A resolved palette.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub high_contrast: bool,
    /// The window's ground (drawn when there is no Mica behind it).
    pub ground: D2D1_COLOR_F,
    /// Text and the primary button.
    pub ink: D2D1_COLOR_F,
    /// Secondary text.
    pub muted: D2D1_COLOR_F,
    /// Text drawn on `ink` (the primary button's label, the tick).
    pub on_ink: D2D1_COLOR_F,
    /// Thin rules, the progress track (an alpha over the ground).
    pub hairline: D2D1_COLOR_F,
    /// Accent for focus rings in High Contrast; `ink` otherwise.
    pub focus: D2D1_COLOR_F,
}

/// `NSColor(white:alpha: 1)`.
const fn grey(w: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: w,
        g: w,
        b: w,
        a: 1.0,
    }
}

/// Same colour, different opacity.
pub fn alpha(c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * a, ..c }
}

/// Blend two colours (straight alpha) by `t`.
pub fn mix(a: D2D1_COLOR_F, b: D2D1_COLOR_F, t: f32) -> D2D1_COLOR_F {
    let l = |x: f32, y: f32| x + (y - x) * t;
    D2D1_COLOR_F {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    }
}

fn sys_color(index: windows::Win32::Graphics::Gdi::SYS_COLOR_INDEX) -> D2D1_COLOR_F {
    // SAFETY: plain query.
    let c = unsafe { GetSysColor(index) };
    D2D1_COLOR_F {
        r: (c & 0xff) as f32 / 255.0,
        g: ((c >> 8) & 0xff) as f32 / 255.0,
        b: ((c >> 16) & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// Does the system ask apps to be dark?
pub fn system_is_dark() -> bool {
    reg::read_dword(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "AppsUseLightTheme",
    ) == Some(0)
}

/// Is High Contrast on?
pub fn high_contrast() -> bool {
    let mut hc = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: sized struct for this query.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some((&mut hc as *mut HIGHCONTRASTW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    ok.is_ok() && hc.dwFlags.contains(HCF_HIGHCONTRASTON)
}

impl Palette {
    /// The palette for the system as it is right now.
    pub fn current() -> Palette {
        if high_contrast() {
            let ink = sys_color(COLOR_WINDOWTEXT);
            return Palette {
                dark: false,
                high_contrast: true,
                ground: sys_color(COLOR_WINDOW),
                ink,
                muted: sys_color(COLOR_GRAYTEXT),
                on_ink: sys_color(COLOR_WINDOW),
                hairline: sys_color(COLOR_BTNFACE),
                focus: sys_color(COLOR_HIGHLIGHT),
            };
        }
        let dark = system_is_dark();
        // Design.swift's pairs: (light, dark).
        let pick = |light: f32, dark_: f32| grey(if dark { dark_ } else { light });
        let ink = pick(0.09, 0.93);
        Palette {
            dark,
            high_contrast: false,
            ground: pick(1.0, 0.11),
            ink,
            muted: pick(0.55, 0.58),
            on_ink: pick(1.0, 0.11),
            hairline: alpha(ink, if dark { 0.16 } else { 0.10 }),
            focus: ink,
        }
    }
}
