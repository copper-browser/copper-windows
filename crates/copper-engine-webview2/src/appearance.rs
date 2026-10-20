//! How pages look before and around their own content: the colour a webview
//! shows until (and wherever) its page paints, and the colour scheme pages
//! are told to use (`prefers-color-scheme`).
//!
//! Platform-free: the Windows side passes the raw values to
//! `ICoreWebView2Controller2::DefaultBackgroundColor` and
//! `ICoreWebView2Profile::PreferredColorScheme`.

/// The colour scheme pages are asked to use, as `prefers-color-scheme`
/// (and the colours of form controls and scroll bars) sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    /// Whatever Windows uses (Copper's Look set to System).
    #[default]
    Auto,
    /// Light, whatever Windows uses.
    Light,
    /// Dark, whatever Windows uses.
    Dark,
}

impl ColorScheme {
    /// `COREWEBVIEW2_PREFERRED_COLOR_SCHEME` (`AUTO` 0, `LIGHT` 1, `DARK` 2).
    pub(crate) fn raw(self) -> i32 {
        match self {
            ColorScheme::Auto => 0,
            ColorScheme::Light => 1,
            ColorScheme::Dark => 2,
        }
    }
}

/// The colour a webview shows where its page has painted nothing yet: a
/// cold start, a new tab, a blank page, the gap while a page loads.
/// Opaque: WebView2 takes no other alpha in visual hosting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ground {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Ground {
    /// White, WebView2's own.
    pub const WHITE: Ground = Ground::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Ground {
        Ground { r, g, b }
    }

    /// From colour channels in 0…1 (a toolkit colour); out-of-range values
    /// are clamped.
    pub fn from_unit(r: f32, g: f32, b: f32) -> Ground {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Ground::rgb(c(r), c(g), c(b))
    }
}

impl Default for Ground {
    fn default() -> Self {
        Ground::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_are_webview2s_numbers() {
        assert_eq!(ColorScheme::Auto.raw(), 0);
        assert_eq!(ColorScheme::Light.raw(), 1);
        assert_eq!(ColorScheme::Dark.raw(), 2);
        assert_eq!(ColorScheme::default(), ColorScheme::Auto);
    }

    #[test]
    fn grounds_round_and_clamp() {
        assert_eq!(Ground::from_unit(1.0, 1.0, 1.0), Ground::WHITE);
        // Design.swift's dark ground, grey 0.11.
        assert_eq!(Ground::from_unit(0.11, 0.11, 0.11), Ground::rgb(28, 28, 28));
        assert_eq!(Ground::from_unit(-1.0, 2.0, 0.5), Ground::rgb(0, 255, 128));
        assert_eq!(Ground::default(), Ground::WHITE);
    }
}
