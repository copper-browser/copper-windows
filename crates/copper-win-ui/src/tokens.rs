//! Copper's design tokens, transcribed from the Mac app — the golden
//! standard — with the line each value comes from.
//!
//! Sources (all under `copper-mac-ref/Sources/Search/`):
//! * `Design.swift` — `Palette`, `Metrics`, `Motion`.
//! * `Side.swift` — the column's measures (rows, pins, the address, doors).
//! * `Fork/SpaceTint.swift`, `Fork/SpaceTheme.swift` — the column's surfaces.
//! * `Fork/CommandPalette.swift` — the ⌘K card (Ctrl+K here).
//! * `Fork/SpacesUI.swift`, `Fork/SpaceSlide.swift`, `Fold.swift`.
//!
//! Sizes are Mac points, which are Windows DIPs one for one. The only values
//! that are not the Mac's are the Windows conventions the platform demands —
//! the caption buttons (Windows 11's own measures, from WinUI's theme
//! resources) and the fonts (Segoe UI Variable for SF, Segoe Fluent Icons
//! for SF Symbols) — and each says so.

use crate::geom::Color;
use crate::spring::{Bezier, Motion};

/// One colour for a light window, one for a dark (`Design.swift` 35–40:
/// every colour is a pair that resolves against the window's appearance).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    pub light: Color,
    pub dark: Color,
}

impl Pair {
    /// `pair(light, dark)` of whites, as `Design.swift` writes them.
    pub const fn white(light: f32, dark: f32) -> Pair {
        Pair {
            light: Color::grey(light),
            dark: Color::grey(dark),
        }
    }

    pub const fn get(&self, dark: bool) -> Color {
        if dark { self.dark } else { self.light }
    }
}

/// `Design.swift` `Palette.NS` — the app's neutrals.
pub mod palette {
    use super::Pair;

    /// The page's ground; everything the browser draws gets out of its way.
    /// `Design.swift` 25: `pair(1.0, 0.11)`.
    pub const GROUND: Pair = Pair::white(1.0, 0.11);
    /// Titles and text: neutral-900 · neutral-100. `Design.swift` 26.
    pub const INK: Pair = Pair::white(0.09, 0.93);
    /// Secondary text: neutral-500. `Design.swift` 27.
    pub const MUTED: Pair = Pair::white(0.55, 0.58);
    /// Placeholders and resting glyphs: neutral-300 · neutral-700.
    /// `Design.swift` 28.
    pub const FAINT: Pair = Pair::white(0.83, 0.32);
    /// The one line: neutral-200 · neutral-800. `Design.swift` 29.
    pub const HAIRLINE: Pair = Pair::white(0.91, 0.20);
    /// The live tab. `Design.swift` 30.
    pub const WASH: Pair = Pair::white(0.937, 0.175);
    /// The one under the pointer. `Design.swift` 31.
    pub const HOVER: Pair = Pair::white(0.965, 0.15);
    /// The resting traffic lights (here: caption glyphs while the window is
    /// behind). `Design.swift` 33.
    pub const RESTING: Pair = Pair::white(0.80, 0.30);
}

/// `Design.swift` `Metrics` (90–127).
pub mod metrics {
    /// The tab strip's height (tabs on top). `Design.swift` 90.
    pub const STRIP: f32 = 52.0;
    /// Where the first tab starts, clear of the traffic lights.
    /// `Design.swift` 94. (Windows has no lights on the left; kept for the
    /// strip layout, which starts at the column inset instead.)
    pub const LIGHTS: f32 = 100.0;
    /// Back, forward and reload: three 26 doors, 2 apart, and 8 of air.
    /// `Design.swift` 97.
    pub const HELM: f32 = 3.0 * 26.0 + 2.0 * 2.0 + 8.0;
    /// The lights' edge less the column's padding, in the column.
    /// `Design.swift` 104.
    pub const SIDE_LIGHTS: f32 = 72.0;
    /// The band left at the top when there is no strip: "just enough for the
    /// traffic lights to sit in" — on Windows, the caption buttons' band over
    /// the page. `Design.swift` 107.
    pub const BARE: f32 = 34.0;
    /// A tab's fixed width in the strip. `Design.swift` 114.
    pub const TAB_WIDTH: f32 = 186.0;
    /// Narrower than this a strip tab shows its mark alone. `Design.swift` 115.
    pub const TAB_TITLED: f32 = 80.0;
    /// A strip tab's minimum: the mark and its air. `Design.swift` 116.
    pub const TAB_MIN_WIDTH: f32 = 36.0;
    /// Between strip tabs. `Design.swift` 117.
    pub const TAB_GAP: f32 = 2.0;
    /// A pinned tab in the strip. `Design.swift` 119.
    pub const PIN_WIDTH: f32 = 30.0;
    /// The new-page square at the end of the strip. `Design.swift` 121.
    pub const PLUS_WIDTH: f32 = 30.0;
    /// The address field, where it stands on its own. `Design.swift` 123.
    pub const FIELD_WIDTH: f32 = 560.0;
    /// The column's default width. `Design.swift` 125.
    pub const SIDE: f32 = 232.0;
    /// The column's narrowest. `Design.swift` 126.
    pub const SIDE_MIN: f32 = 176.0;
    /// The column's widest. `Design.swift` 127.
    pub const SIDE_MAX: f32 = 440.0;
}

/// `Design.swift` `Motion` (134–136) and the other timings the chrome uses.
pub mod motion {
    use super::{Bezier, Motion};

    /// Anything that moves between two places: the live pill, the column
    /// folding. `Design.swift` 134: `spring(response: 0.34, dampingFraction: 0.82)`.
    pub const GLIDE: Motion = Motion::spring(0.34, 0.82);
    /// Anything that arrives or leaves: rows coming and going, the card.
    /// `Design.swift` 135: `spring(response: 0.30, dampingFraction: 0.86)`.
    pub const SETTLE: Motion = Motion::spring(0.30, 0.86);
    /// Hover and press. `Design.swift` 136: `easeOut(duration: 0.14)`.
    pub const QUICK: Motion = Motion::ease_out(0.14);
    /// A space switch's slide. `Fork/SpaceSlide.swift` 146–147:
    /// `timingCurve(0.2, 0.9, 0.3, 1, duration: 0.22)`.
    pub const SLIDE: Motion = Motion::Curve {
        curve: Bezier::new(0.2, 0.9, 0.3, 1.0),
        seconds: 0.22,
    };
    /// The live row's reading bar. `Side.swift` 708: `easeOut(duration: 0.15)`.
    pub const READING: Motion = Motion::ease_out(0.15);
    /// The chrome leaving and coming back as a page takes the screen and
    /// gives it back. `App.swift` 349: `easeOut(duration: 0.12)`.
    pub const IMMERSE: Motion = Motion::ease_out(0.12);
    /// The folded column's grace before it goes back in. `Fold.swift` 45.
    pub const FOLD_GRACE: f32 = 0.3;
    /// How much of the left edge brings a folded column out. `Fold.swift` 43.
    pub const FOLD_EDGE: f32 = 6.0;
    /// The wrong-address shake: `Side.swift` 691, `easeOut(duration: 0.5)`,
    /// three there-and-backs of 7 tapering to nothing (`Design.swift` 250).
    pub const SHAKE_SECONDS: f32 = 0.5;
    pub const SHAKE_TRAVEL: f32 = 7.0;
    /// Before a tooltip shows. The Mac leaves `.help` to AppKit; this is
    /// Windows' own initial delay (`TTDT_INITIAL`, the double-click time's
    /// default), the platform convention.
    pub const TOOLTIP_DELAY: f32 = 0.5;
}

/// The column: `Side.swift` and `Fork/GroupsUI.swift`.
pub mod side {
    /// Windows convention: how opaque the column's ground is over Mica, so
    /// the desktop's tint shows through it the way the Mac's sidebar
    /// material shows the desktop (`Side.swift` draws the column on the
    /// window's ground; Windows 11 apps let Mica through their navigation
    /// pane). Solid (1) wherever Mica is not available.
    pub const MICA_ALPHA: f32 = 0.82;
    /// A row every 37. `Fork/GroupsUI.swift` 39 (`SideBar.row`, `Side.swift` 53).
    pub const ROW: f32 = 37.0;
    /// Between rows. `Fork/GroupsUI.swift` 40 (`SideBar.gap`, `Side.swift` 54).
    pub const GAP: f32 = 4.0;
    /// The column's own edges. `Side.swift` 57.
    pub const INSET: f32 = 8.0;
    /// A row's mark from the row's edge. `Side.swift` 58.
    pub const ROW_INSET: f32 = 10.0;
    /// Between favourites. `Side.swift` 59.
    pub const PIN_GAP: f32 = 8.0;
    /// The lights' band at the top of the column. `Side.swift` 171.
    pub const HEAD: f32 = 46.0;
    /// The sidebar door beside the lights: 28 square, glyph 15.
    /// `Side.swift` 163.
    pub const HEAD_DOOR: f32 = 28.0;
    /// Back / forward / reload in the column: 30 square, glyph 15, 6 from the
    /// edge. `Side.swift` 167–168.
    pub const HELM_DOOR: f32 = 30.0;
    pub const HEAD_GLYPH: f32 = 15.0;
    /// Between the helm's doors. `TabBar.swift` 261.
    pub const HELM_GAP: f32 = 2.0;
    /// A row's corners. `Side.swift` 673, 711, 719.
    pub const ROW_RADIUS: f32 = 10.0;
    /// A row's mark and the space between it and the title. `Side.swift` 599, 605.
    pub const MARK: f32 = 16.0;
    pub const MARK_GAP: f32 = 10.0;
    /// A row's title: 15, semibold when live. `Side.swift` 620.
    pub const TITLE_SIZE: f32 = 15.0;
    /// A row's trailing padding. `Side.swift` 668.
    pub const ROW_TRAILING: f32 = 8.0;
    /// The close cross: 8 semibold in a 15 circle of ink at 0.07.
    /// `Side.swift` 633–636.
    pub const CLOSE_GLYPH: f32 = 8.0;
    pub const CLOSE: f32 = 15.0;
    pub const CLOSE_FILL: f32 = 0.07;
    /// A resting (not live) row's mark is drawn at 0.9. `Side.swift` 606.
    pub const MARK_RESTING: f32 = 0.9;
    /// The live row's shadow: radius 4, y 1, `tint.lift`. `Side.swift` 716.
    pub const PILL_SHADOW_RADIUS: f32 = 4.0;
    pub const PILL_SHADOW_Y: f32 = 1.0;
    /// The live favourite's shadow: radius 5, y 1.5. `Side.swift` 560.
    pub const PIN_SHADOW_RADIUS: f32 = 5.0;
    pub const PIN_SHADOW_Y: f32 = 1.5;
    /// The column's right edge: `tint.hairline` at 0.6, one wide.
    /// `Side.swift` 113.
    pub const EDGE_ALPHA: f32 = 0.6;
    /// The fade at the ends of the scroll: 6 at the top, 14 at the foot.
    /// `Side.swift` 414–416.
    pub const FADE_TOP: f32 = 6.0;
    pub const FADE_BOTTOM: f32 = 14.0;
    /// The rows' scroll padding: 2 on top, 12 at the foot. `Side.swift` 345–346.
    pub const SCROLL_TOP: f32 = 2.0;
    pub const SCROLL_BOTTOM: f32 = 12.0;
    /// The divider: inset by the row inset, 6 above and below. `Side.swift` 427–428.
    pub const DIVIDER_PAD: f32 = 6.0;
    /// "New Tab" and other quiet rows: glyph 14 in 16, title 15, ink at 0.82
    /// at rest. `Side.swift` 754–762.
    pub const QUIET_GLYPH: f32 = 14.0;
    pub const QUIET_REST: f32 = 0.82;
    /// The address at the top of the column: 36 high, corners 10, 12 in on
    /// the left and 5 on the right, host at 14.5, ink at 0.82, 10 below it.
    /// `Side.swift` 853–857, 873–874, 74.
    pub const ADDRESS_HEIGHT: f32 = 36.0;
    pub const ADDRESS_RADIUS: f32 = 10.0;
    pub const ADDRESS_LEADING: f32 = 12.0;
    pub const ADDRESS_TRAILING: f32 = 5.0;
    pub const ADDRESS_TEXT: f32 = 14.5;
    pub const ADDRESS_INK: f32 = 0.82;
    pub const ADDRESS_BELOW: f32 = 10.0;
    /// Below the favourites. `Side.swift` 82.
    pub const PINS_BELOW: f32 = 8.0;
    /// The space strip at the foot: 8 either side, 4 above, 8 below; chips
    /// 28 square, 5 apart, corners 8. `Fork/SpacesUI.swift` 237–239, 411–412, 422.
    pub const FOOT_PAD_H: f32 = 8.0;
    pub const FOOT_PAD_TOP: f32 = 4.0;
    pub const FOOT_PAD_BOTTOM: f32 = 8.0;
    pub const CHIP: f32 = 28.0;
    pub const CHIP_GAP: f32 = 5.0;
    pub const CHIP_RADIUS: f32 = 8.0;
    /// A chip that is not the current space. `Fork/SpacesUI.swift` 419.
    pub const CHIP_RESTING: f32 = 0.42;
    pub const CHIP_HOVER: f32 = 0.8;
    /// The foot's doors: 28 square, glyph 14. `Side.swift` 474.
    pub const FOOT_DOOR: f32 = 28.0;
    pub const FOOT_GLYPH: f32 = 14.0;
    /// The space's name: 15 semibold in the muted ink. `Fork/SpacesUI.swift` 171.
    pub const SPACE_NAME: f32 = 15.0;

    /// Three favourites to a row, four once the column is 400 wide.
    /// `Side.swift` 190–192.
    pub fn pin_columns(side_width: f32) -> usize {
        if side_width >= 400.0 { 4 } else { 3 }
    }

    /// A favourite's width: the column's width split between the columns.
    /// `Side.swift` 196–200.
    pub fn pin_width(side_width: f32) -> f32 {
        let cols = pin_columns(side_width) as f32;
        let available = side_width - 2.0 * INSET - (cols - 1.0) * PIN_GAP;
        (available / cols).max(20.0)
    }

    /// Wide and short, never a big square. `Side.swift` 205–207.
    pub fn pin_height(pin_width: f32) -> f32 {
        (pin_width * 0.52).clamp(34.0, 48.0)
    }

    /// `Side.swift` 536: `min(12, min(width, height) * 0.28)`.
    pub fn pin_radius(w: f32, h: f32) -> f32 {
        (w.min(h) * 0.28).min(12.0)
    }

    /// The mark inside a favourite. `Side.swift` 539.
    pub fn pin_mark(h: f32) -> f32 {
        (h * 0.46).clamp(14.0, 20.0)
    }

    /// A door's corners: 0.3 of its side. `Side.swift` 802.
    pub fn door_radius(size: f32) -> f32 {
        size * 0.3
    }

    /// A letter mark's corners and letter: 0.24 and 0.52 of its side,
    /// semibold, ink at 0.6. `Side.swift` 505–510.
    pub fn mark_radius(size: f32) -> f32 {
        size * 0.24
    }
    pub fn mark_letter(size: f32) -> f32 {
        size * 0.52
    }
    pub const MARK_LETTER_INK: f32 = 0.6;
}

/// The ⌘K card — Ctrl+K on Windows. `Fork/CommandPalette.swift`.
pub mod card {
    /// Wide enough for a title, a host and a hint. Line 24.
    pub const WIDTH: f32 = 640.0;
    /// The card's top a third of the way down the window. Line 29.
    pub const DROP: f32 = 0.3;
    /// Fixed rows so the height is known before drawing. Lines 49–50.
    pub const ROW_HEIGHT: f32 = 40.0;
    pub const HEADING_HEIGHT: f32 = 28.0;
    /// The page behind, told to be quiet: black at 0.16 over a page, 0.05
    /// over an empty tab. Line 56.
    pub const SCRIM_OVER: f32 = 0.16;
    pub const SCRIM_EMPTY: f32 = 0.05;
    /// Under the field: the hairline at 0.55. Line 97.
    pub const RULE_ALPHA: f32 = 0.55;
    /// Corners. Lines 103, 110.
    pub const RADIUS: f32 = 13.0;
    /// Two shadows, no outline: a tight one for the edge, a wide one for the
    /// lift. Lines 114–115.
    pub const EDGE_SHADOW: super::Shadow = super::Shadow::new(0.09, 3.0, 1.0);
    pub const LIFT_SHADOW: super::Shadow = super::Shadow::new(0.26, 48.0, 20.0);
    /// The field row: magnifier 15 medium, 13 to the field, field text 17.5,
    /// 20 either side, 56 high. Lines 120–133.
    pub const FIELD_SPACING: f32 = 13.0;
    pub const GLASS: f32 = 15.0;
    pub const FIELD_TEXT: f32 = 17.5;
    pub const FIELD_PAD: f32 = 20.0;
    pub const FIELD_HEIGHT: f32 = 56.0;
    /// Rows 1 apart, the list 8 in. Lines 142, 165–166.
    pub const ROW_SPACING: f32 = 1.0;
    pub const LIST_PAD: f32 = 8.0;
    /// A heading: 11 semibold, muted, 12 in. Lines 150–153.
    pub const HEADING: f32 = 11.0;
    /// A row: mark 18, 11 between parts, title 13.5 medium, detail and badge
    /// 12.5 muted, hint 12 medium, 12 in, corners 9. Lines 202–270.
    pub const ROW_GAP: f32 = 11.0;
    pub const ROW_MARK: f32 = 18.0;
    pub const ROW_TITLE: f32 = 13.5;
    pub const ROW_DETAIL: f32 = 12.5;
    pub const ROW_HINT: f32 = 12.0;
    pub const ROW_PAD: f32 = 12.0;
    pub const ROW_RADIUS: f32 = 9.0;
    /// The picked row: the space's colour at 0.17. Line 270.
    pub const PICKED_ALPHA: f32 = 0.17;
    /// The card arrives scaled from 0.985 at its top. Line 116.
    pub const ENTER_SCALE: f32 = 0.985;
    /// "Arc shows five or six; past eight the card is taller than it is
    /// wide". `Fork/CommandBar.swift` 15.
    pub const LIMIT: usize = 8;
}

/// Menus and tooltips. The Mac uses AppKit's own; these are Copper's surfaces
/// for them, built from the card above and Copper for Linux's dropdown and
/// hover label (`copper-app/src/style.rs` `.copper-dropdown`,
/// `.copper-hover`), which are themselves cut from the same cloth.
pub mod popup {
    /// The menu card: 6 in, corners 12. Linux `style.rs` `.copper-dropdown`.
    pub const MENU_PAD: f32 = 6.0;
    pub const MENU_RADIUS: f32 = 12.0;
    /// A menu row: the card's row cut down to a menu's density — corners 7,
    /// 10 in, 13.5 text (the card's title size), 30 high.
    pub const MENU_ROW: f32 = 30.0;
    pub const MENU_ROW_RADIUS: f32 = 7.0;
    pub const MENU_ROW_PAD: f32 = 10.0;
    pub const MENU_TEXT: f32 = 13.5;
    pub const MENU_MIN_WIDTH: f32 = 200.0;
    /// The rule between groups: the card's hairline. `Fork/CommandPalette.swift` 97.
    pub const MENU_RULE_PAD: f32 = 5.0;
    /// The shadow: Linux `.copper-dropdown` `0 8px 28px` (blur 28 ≈ radius 14).
    pub const MENU_SHADOW: super::Shadow = super::Shadow::new(0.18, 14.0, 8.0);
    /// Tooltip: 12, 9 by 3 in, corners 7, a hairline ring and a small lift.
    /// Linux `style.rs` `.copper-hover`.
    pub const TIP_TEXT: f32 = 12.0;
    pub const TIP_PAD_H: f32 = 9.0;
    pub const TIP_PAD_V: f32 = 4.0;
    pub const TIP_RADIUS: f32 = 7.0;
    pub const TIP_SHADOW: super::Shadow = super::Shadow::new(0.10, 3.0, 2.0);
}

/// The browser window.
pub mod window {
    /// Opening size. `App.swift` 27 (`.defaultSize(width: 1180, height: 780)`).
    pub const DEFAULT_W: f32 = 1180.0;
    pub const DEFAULT_H: f32 = 780.0;
    /// Smallest size. `App.swift` 24 (`.frame(minWidth: 640, minHeight: 420)`).
    pub const MIN_W: f32 = 640.0;
    pub const MIN_H: f32 = 420.0;
}

/// Windows 11's caption buttons, which stand in for the traffic lights. Not
/// a Mac value: WinUI's `TitleBar` measures and theme resources, so the
/// buttons feel like the platform's own.
pub mod caption {
    use super::{Color, Pair};

    /// Each button is 46 wide (WinUI `TitleBar` caption buttons).
    pub const BUTTON_WIDTH: f32 = 46.0;
    /// Segoe Fluent Icons at 10 (WinUI `CaptionButtonGlyphSize`).
    pub const GLYPH: f32 = 10.0;
    /// `SubtleFillColorSecondary`: #09000000 light, #0FFFFFFF dark.
    pub const HOVER: Pair = Pair {
        light: Color::rgba(0.0, 0.0, 0.0, 0.0353),
        dark: Color::rgba(1.0, 1.0, 1.0, 0.0588),
    };
    /// `SubtleFillColorTertiary`: #06000000 light, #0AFFFFFF dark.
    pub const PRESS: Pair = Pair {
        light: Color::rgba(0.0, 0.0, 0.0, 0.0235),
        dark: Color::rgba(1.0, 1.0, 1.0, 0.0392),
    };
    /// Close: #C42B1C under the pointer, at 0.9 pressed, white glyph.
    pub const CLOSE: Color = Color::rgb(0.769, 0.169, 0.110);
    pub const CLOSE_PRESS_ALPHA: f32 = 0.9;
    /// `TextFillColorPrimary`: #E4000000 light, #FFFFFF dark.
    pub const GLYPH_INK: Pair = Pair {
        light: Color::rgba(0.0, 0.0, 0.0, 0.894),
        dark: Color::rgba(1.0, 1.0, 1.0, 1.0),
    };
    /// `TextFillColorDisabled` for a window that is behind:
    /// #5C000000 light, #5DFFFFFF dark.
    pub const GLYPH_INACTIVE: Pair = Pair {
        light: Color::rgba(0.0, 0.0, 0.0, 0.361),
        dark: Color::rgba(1.0, 1.0, 1.0, 0.365),
    };
    /// Segoe Fluent Icons code points: ChromeMinimize, ChromeMaximize,
    /// ChromeRestore, ChromeClose.
    pub const MINIMIZE: char = '\u{E921}';
    pub const MAXIMIZE: char = '\u{E922}';
    pub const RESTORE: char = '\u{E923}';
    pub const CLOSE_GLYPH: char = '\u{E8BB}';
    /// The resize border inside the top edge of a captionless window, in
    /// DIPs (the system's `SM_CYSIZEFRAME` + padded border is used where it
    /// is larger).
    pub const TOP_RESIZE: f32 = 4.0;
}

/// The keyboard's focus ring and the text caret.
pub mod focus {
    /// The ring sits this far outside the control's shape
    /// (`Fork/SettingsControls.swift` 53: `.inset(by: -3)`).
    pub const RING_OUTSET: f32 = 3.0;
    /// `Fork/SettingsControls.swift` 54: `lineWidth: 1.75`.
    pub const RING_WIDTH: f32 = 1.75;
    /// `Fork/SettingsControls.swift` 54: `Palette.ink.opacity(0.42)`.
    pub const RING_ALPHA: f32 = 0.42;
    /// Windows convention: the caret is as wide as the user's
    /// `SPI_GETCARETWIDTH` setting (1 pixel unless they changed it) and in
    /// the text's own ink, blinking at `GetCaretBlinkTime`.
    pub const CARET_PIXELS: u32 = 1;
}

/// The overlay scroll indicator (SwiftUI's `.scrollIndicators(.automatic)`
/// draws AppKit's overlay scroller: a slim rounded knob that shows while
/// scrolling and fades out, `Fork/Flow/FlowUI.swift` 28).
pub mod scroll {
    /// AppKit's overlay knob at rest.
    pub const KNOB: f32 = 6.0;
    /// Gap between the knob and the edge.
    pub const INSET: f32 = 3.0;
    /// The knob's ink: `Palette.ink` thinned, as the overlay scroller draws
    /// (`NSScroller.knobStyle = .default`).
    pub const ALPHA: f32 = 0.38;
}

/// The type: SF on the Mac, Segoe UI Variable here (the platform's own
/// variable UI face, with its optical sizes), at the Mac's sizes.
pub mod font {
    /// The text optical size, for everything up to 20.
    pub const TEXT: &str = "Segoe UI Variable Text";
    /// The display optical size, from 20 up.
    pub const DISPLAY: &str = "Segoe UI Variable Display";
    /// Windows 10 has no Segoe UI Variable.
    pub const FALLBACK: &str = "Segoe UI";
    /// SF Symbols' stand-in: Windows 11's icon font, then Windows 10's.
    pub const ICONS: &str = "Segoe Fluent Icons";
    pub const ICONS_FALLBACK: &str = "Segoe MDL2 Assets";
    /// From this size up the display cut reads better.
    pub const DISPLAY_FROM: f32 = 20.0;
}

/// A drop shadow as SwiftUI's `.shadow(color: .black.opacity(a), radius: r, y: y)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// Black at this opacity.
    pub alpha: f32,
    /// SwiftUI's radius: the blur's extent. Direct2D's Gaussian takes a
    /// standard deviation, which is half of it.
    pub radius: f32,
    pub y: f32,
}

impl Shadow {
    pub const fn new(alpha: f32, radius: f32, y: f32) -> Shadow {
        Shadow { alpha, radius, y }
    }

    /// How far the shadow reaches past the shape it is cast by.
    pub fn reach(&self) -> f32 {
        self.radius * 1.5 + self.y.abs()
    }
}

/// A space's theme, as far as the column needs it: one colour and how
/// strongly it washes the column (`Fork/SpaceTheme.swift`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub color: Color,
    pub intensity: f32,
}

impl Theme {
    /// Graphite: no colour at all. `Fork/SpaceTheme.swift` 205.
    pub const PLAIN: Theme = Theme {
        color: Color::rgb(0.62, 0.62, 0.64),
        intensity: 0.34,
    };
    /// Copper's own column, the default a space wears.
    /// `Fork/SpaceTheme.swift` 210 (the picture over it is left out).
    pub const COPPER: Theme = Theme {
        color: Color::rgb(0.90, 0.76, 0.67),
        intensity: 0.45,
    };

    /// The column's own colour. In the light: pulled from white by the
    /// intensity, then dimmed; in the dark: the same hue, deep and quiet.
    /// `Fork/SpaceTheme.swift` 222–228.
    pub fn ground(&self, dark: bool) -> Color {
        if dark {
            let (h, s, _) = to_hsb(self.color);
            Color::hsb(h, (s * 0.8).min(1.0), 0.17 + 0.09 * self.intensity)
        } else {
            let t = 0.8 * self.intensity;
            let c = Color::WHITE.mix(self.color, t);
            Color::rgb(c.r * 0.96, c.g * 0.96, c.b * 0.96)
        }
    }

    /// The hue the space is known by, or none for a grey.
    /// `Fork/SpaceTheme.swift` 214–217.
    pub fn hue(&self) -> Option<f32> {
        let (h, s, _) = to_hsb(self.color);
        (s > 0.12).then_some(h)
    }
}

fn to_hsb(c: Color) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let s = if max > 0.0 { d / max } else { 0.0 };
    let h = if d <= 0.0 {
        0.0
    } else if max == c.r {
        ((c.g - c.b) / d).rem_euclid(6.0) / 6.0
    } else if max == c.g {
        ((c.b - c.r) / d + 2.0) / 6.0
    } else {
        ((c.r - c.g) / d + 4.0) / 6.0
    };
    (h, s, max)
}

/// Every surface of the column, resolved for one appearance: either the
/// app's own neutrals ([`Column::paper`], the look of the Mac screenshot) or
/// a space's tint ([`Column::tinted`], `Fork/SpaceTint.swift`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    /// The column's ground.
    pub ground: Color,
    /// Whether the column reads as dark: light ink on it.
    pub dark: bool,
    /// Titles. `Fork/SpaceTint.swift` 136.
    pub ink: Color,
    /// `ink` at 0.62. `Fork/SpaceTint.swift` 137.
    pub muted: Color,
    /// `ink` at 0.45. `Fork/SpaceTint.swift` 138.
    pub faint: Color,
    /// A favourite at rest, the address field. `Fork/SpaceTint.swift` 85.
    pub square: Color,
    /// Under the pointer. `Fork/SpaceTint.swift` 89.
    pub hover: Color,
    /// The live row and favourite. `Fork/SpaceTint.swift` 93.
    pub pill: Color,
    /// The live pill's edge. `Fork/SpaceTint.swift` 96.
    pub rim: Color,
    /// The live pill's shadow. `Fork/SpaceTint.swift` 99.
    pub lift: Color,
    /// The only line. `Fork/SpaceTint.swift` 105.
    pub hairline: Color,
    /// A letter's chip. `Fork/SpaceTint.swift` 108.
    pub chip: Color,
    /// The space's colour at full strength. `Fork/SpaceTint.swift` 112.
    pub dot: Color,
}

impl Column {
    /// Where the ink changes sides. `Fork/SpaceTint.swift` 36.
    pub const TURN: f32 = 0.2;

    /// The column on the app's own ground — white, the live tab in the wash,
    /// the one under the pointer in the hover (`Design.swift` 14–20 and the
    /// screenshot): what a space with no theme looks like.
    pub fn paper(dark: bool) -> Column {
        let ink = palette::INK.get(dark);
        Column {
            ground: palette::GROUND.get(dark),
            dark,
            ink,
            muted: palette::MUTED.get(dark),
            faint: palette::FAINT.get(dark),
            square: palette::WASH.get(dark),
            hover: palette::HOVER.get(dark),
            pill: palette::WASH.get(dark),
            rim: Color::CLEAR,
            lift: Color::CLEAR,
            hairline: palette::HAIRLINE.get(dark),
            chip: palette::WASH.get(dark),
            dot: palette::MUTED.get(dark),
        }
    }

    /// A space's column (`Fork/SpaceTint.swift` 42–139).
    pub fn tinted(theme: Theme, night: bool) -> Column {
        let ground = theme.ground(night);
        let dark = ground.luminance() < Self::TURN;
        let hue = theme.hue();
        let mix =
            |s: f32, b: f32| Color::hsb(hue.unwrap_or(0.0), if hue.is_some() { s } else { 0.0 }, b);
        let ink = if dark {
            mix(0.06, 0.96)
        } else {
            mix(0.9, 0.19)
        };
        let (w, k) = (Color::WHITE, Color::BLACK);
        Column {
            ground,
            dark,
            ink,
            muted: ink.alpha(0.62),
            faint: ink.alpha(0.45),
            square: if dark { w.alpha(0.07) } else { k.alpha(0.045) },
            hover: if dark { w.alpha(0.09) } else { k.alpha(0.06) },
            pill: if dark { w.alpha(0.16) } else { w.alpha(0.48) },
            rim: if dark { w.alpha(0.14) } else { w.alpha(0.6) },
            lift: k.alpha(if dark { 0.32 } else { 0.07 }),
            hairline: if dark { w.alpha(0.1) } else { k.alpha(0.08) },
            chip: if dark { w.alpha(0.1) } else { k.alpha(0.07) },
            dot: mix(0.62, if night { 0.86 } else { 0.70 }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_match_the_mac_at_default_width() {
        let w = side::pin_width(metrics::SIDE);
        assert!((w - 66.666).abs() < 0.01, "{w}");
        let h = side::pin_height(w);
        assert!((h - 34.666).abs() < 0.01, "{h}");
        assert!((side::pin_radius(w, h) - 9.706).abs() < 0.01);
        assert_eq!(side::pin_columns(400.0), 4);
    }

    #[test]
    fn copper_column_is_pale_and_takes_dark_ink() {
        let c = Column::tinted(Theme::COPPER, false);
        assert!(!c.dark);
        assert!(
            c.ground.r > 0.9 && c.ground.r > c.ground.b,
            "{:?}",
            c.ground
        );
        assert!(c.ink.luminance() < 0.05);
        let night = Column::tinted(Theme::COPPER, true);
        assert!(night.dark && night.ink.luminance() > 0.8);
    }

    #[test]
    fn plain_has_no_hue() {
        assert_eq!(Theme::PLAIN.hue(), None);
        assert!(Theme::COPPER.hue().is_some());
    }

    #[test]
    fn paper_is_the_screenshot() {
        let p = Column::paper(false);
        assert_eq!(p.ground, Color::WHITE);
        assert_eq!(p.pill, Color::grey(0.937));
    }
}
