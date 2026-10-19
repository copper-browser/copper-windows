//! Appearance: light, dark or the system's, and the named colours nodes
//! paint with.
//!
//! Nodes never hold a resolved colour. They hold a [`Swatch`] — a token from
//! [`crate::tokens`], maybe faded — and the theme resolves it at paint time,
//! so switching appearance or space is one repaint and nothing else in the
//! code knows which look it is (the Mac's rule, `Design.swift` 5–12).

use crate::geom::Color;
use crate::tokens::{Column, Theme as SpaceTheme, caption, palette};

/// Light, dark, or whatever Windows is doing (`Design.swift` `Look`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
    #[default]
    System,
}

impl Appearance {
    /// Resolve against the system's own choice.
    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::System => system_dark,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
            Appearance::System => "System",
        }
    }
}

/// A named colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    // The app's neutrals (`Design.swift` `Palette`).
    Ground,
    Ink,
    Muted,
    Faint,
    Hairline,
    Wash,
    Hover,
    // The column (`Fork/SpaceTint.swift`).
    ColumnGround,
    ColumnInk,
    ColumnMuted,
    ColumnFaint,
    Square,
    ColumnHover,
    Pill,
    Rim,
    Lift,
    ColumnHairline,
    Chip,
    Dot,
    /// The space's colour: the picked row, the caret, the focus ring.
    Accent,
    /// Text selection: the accent, thinned.
    Selection,
    /// Keyboard focus ring: ink, thinned (`tokens::focus`).
    Focus,
    // Windows 11 caption buttons (`tokens::caption`).
    CaptionHover,
    CaptionPress,
    CaptionGlyph,
    Close,
    Black,
    White,
}

/// What a node paints with: a token, a token faded, or a fixed colour.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Swatch {
    #[default]
    Clear,
    Token(Token),
    /// The token at this opacity (SwiftUI's `.opacity`).
    Fade(Token, f32),
    Fixed(Color),
}

impl From<Token> for Swatch {
    fn from(t: Token) -> Swatch {
        Swatch::Token(t)
    }
}

impl From<Color> for Swatch {
    fn from(c: Color) -> Swatch {
        Swatch::Fixed(c)
    }
}

impl Swatch {
    pub fn is_clear(&self) -> bool {
        matches!(self, Swatch::Clear) || matches!(self, Swatch::Fixed(c) if c.a <= 0.0)
    }
}

/// Everything colour depends on, resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// The column's surfaces (paper, or a space's tint).
    pub column: Column,
    /// The space theme the column was made from; `None` for paper.
    pub space: Option<SpaceTheme>,
    pub accent: Color,
    /// Whether the window is the foreground one (caption glyphs dim when not).
    pub active: bool,
    /// How opaque the column's ground is drawn over the window's backdrop:
    /// below 1 the Mica shows through, 1 on Windows 10 or with Mica off.
    pub column_alpha: f32,
}

impl Theme {
    /// The column on the app's own ground, as in the Mac screenshot.
    pub fn new(dark: bool) -> Theme {
        Theme::with_space(dark, None)
    }

    /// The column in a space's colour (`None`: the app's paper).
    pub fn with_space(dark: bool, space: Option<SpaceTheme>) -> Theme {
        let column = match space {
            Some(t) => Column::tinted(t, dark),
            None => Column::paper(dark),
        };
        Theme {
            dark,
            column,
            space,
            accent: Column::tinted(SpaceTheme::COPPER, dark).dot,
            active: true,
            column_alpha: 1.0,
        }
    }

    pub fn token(&self, t: Token) -> Color {
        let d = self.dark;
        let c = &self.column;
        match t {
            Token::Ground => palette::GROUND.get(d),
            Token::Ink => palette::INK.get(d),
            Token::Muted => palette::MUTED.get(d),
            Token::Faint => palette::FAINT.get(d),
            Token::Hairline => palette::HAIRLINE.get(d),
            Token::Wash => palette::WASH.get(d),
            Token::Hover => palette::HOVER.get(d),
            Token::ColumnGround => c.ground.alpha(self.column_alpha),
            Token::ColumnInk => c.ink,
            Token::ColumnMuted => c.muted,
            Token::ColumnFaint => c.faint,
            Token::Square => c.square,
            Token::ColumnHover => c.hover,
            Token::Pill => c.pill,
            Token::Rim => c.rim,
            Token::Lift => c.lift,
            Token::ColumnHairline => c.hairline,
            Token::Chip => c.chip,
            Token::Dot => c.dot,
            Token::Accent => self.accent,
            Token::Selection => self.accent.alpha(if d { 0.45 } else { 0.28 }),
            Token::Focus => palette::INK.get(d).alpha(crate::tokens::focus::RING_ALPHA),
            Token::CaptionHover => caption::HOVER.get(d),
            Token::CaptionPress => caption::PRESS.get(d),
            Token::CaptionGlyph => {
                if self.active {
                    caption::GLYPH_INK.get(d)
                } else {
                    caption::GLYPH_INACTIVE.get(d)
                }
            }
            Token::Close => caption::CLOSE,
            Token::Black => Color::BLACK,
            Token::White => Color::WHITE,
        }
    }

    pub fn color(&self, s: Swatch) -> Color {
        match s {
            Swatch::Clear => Color::CLEAR,
            Swatch::Token(t) => self.token(t),
            Swatch::Fade(t, a) => self.token(t).alpha(a),
            Swatch::Fixed(c) => c,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_resolves() {
        assert!(Appearance::System.is_dark(true));
        assert!(!Appearance::Light.is_dark(true));
        assert!(Appearance::Dark.is_dark(false));
    }

    #[test]
    fn swatches_follow_the_look() {
        let light = Theme::new(false);
        let dark = Theme::new(true);
        assert_eq!(light.color(Token::Ground.into()), Color::WHITE);
        assert_eq!(dark.color(Token::Ground.into()), Color::grey(0.11));
        assert_eq!(light.color(Swatch::Fade(Token::Ink, 0.5)).a, 0.5);
        let mut t = light;
        t.column_alpha = 0.8;
        assert!((t.color(Token::ColumnGround.into()).a - 0.8).abs() < 1e-6);
        t.active = false;
        assert!(t.color(Token::CaptionGlyph.into()).a < 0.5);
    }
}
