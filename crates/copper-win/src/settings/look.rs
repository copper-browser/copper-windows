//! How Settings looks (`Fork/SettingsLook.swift`, `Settings.swift` 83–117):
//! its own greys, a touch apart from the rest of the app's, and the
//! panel's size against the window. Platform-free so the numbers are
//! tested on any host.
//!
//! The greys are `NSColor(white: light, dark)` pairs, resolved for the
//! window's appearance when the panel draws (and again when the window
//! goes light or dark), not tokens: the toolkit's palette has no slot for
//! them, and `Swatch::Fixed` of the right one is exactly what is meant.

use copper_win_ui::Color;

/// One grey for light windows and one for dark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    pub light: Color,
    pub dark: Color,
}

impl Pair {
    const fn white(light: f32, dark: f32) -> Pair {
        Pair {
            light: Color::grey(light),
            dark: Color::grey(dark),
        }
    }

    /// The one for this appearance.
    pub const fn get(self, dark: bool) -> Color {
        if dark { self.dark } else { self.light }
    }
}

/// `SettingsInk`: the rail behind the pages.
pub const RAIL: Pair = Pair::white(0.952, 0.128);
/// The page behind the cards.
pub const CONTENT: Pair = Pair::white(0.976, 0.108);
/// A card.
pub const CARD: Pair = Pair::white(1.0, 0.150);
/// A card's outline, and the rules between its lines.
pub const CARD_EDGE: Pair = Pair::white(0.905, 0.205);
/// The sentence under a line's title.
pub const DETAIL: Pair = Pair::white(0.40, 0.64);
/// A section's title.
pub const HEADING: Pair = Pair::white(0.26, 0.80);
/// The search field's well.
pub const FIELD: Pair = Pair::white(0.905, 0.185);
/// The page picked in the rail.
pub const CHOSEN: Pair = Pair::white(1.0, 0.205);
/// A search hit's highlight: black at 7% on light, white at 12% on dark.
pub const MARK: Pair = Pair {
    light: Color::rgba(0.0, 0.0, 0.0, 0.07),
    dark: Color::rgba(1.0, 1.0, 1.0, 0.12),
};
/// Words that say something went wrong (`Design.swift` 45–83's red).
pub const ERROR: Pair = Pair {
    light: Color::rgba(0.80, 0.18, 0.16, 1.0),
    dark: Color::rgba(1.0, 0.45, 0.42, 1.0),
};
/// Words that say something worked.
pub const GOOD: Pair = Pair {
    light: Color::rgba(0.13, 0.52, 0.27, 1.0),
    dark: Color::rgba(0.42, 0.82, 0.52, 1.0),
};

/// Every Settings grey for one appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inks {
    pub dark: bool,
    pub rail: Color,
    pub content: Color,
    pub card: Color,
    pub card_edge: Color,
    pub detail: Color,
    pub heading: Color,
    pub field: Color,
    pub chosen: Color,
    pub mark: Color,
    pub error: Color,
    pub good: Color,
}

impl Inks {
    pub const fn new(dark: bool) -> Inks {
        Inks {
            dark,
            rail: RAIL.get(dark),
            content: CONTENT.get(dark),
            card: CARD.get(dark),
            card_edge: CARD_EDGE.get(dark),
            detail: DETAIL.get(dark),
            heading: HEADING.get(dark),
            field: FIELD.get(dark),
            chosen: CHOSEN.get(dark),
            mark: MARK.get(dark),
            error: ERROR.get(dark),
            good: GOOD.get(dark),
        }
    }
}

/// `SettingsPanel.span`: as big as reads well, never past the window —
/// at most `most`, at least `least` while the window has room for it
/// with 24 to spare, and otherwise 80 short of the window.
pub fn span(length: f32, most: f32, least: f32) -> f32 {
    most.min(least.min(length - 24.0).max(length - 80.0))
        .max(0.0)
}

/// The panel's width in a window `w` wide.
pub fn width(w: f32) -> f32 {
    span(w, 920.0, 600.0)
}

/// The panel's height in a window `h` tall. The frame gets it from its
/// layout clamps (`frame::build`); this states the numbers it must match.
#[cfg(test)]
pub fn height(h: f32) -> f32 {
    span(h, 700.0, 440.0)
}

/// The rail's width for a panel `w` wide (`Settings.swift` 92).
pub fn rail(w: f32) -> f32 {
    if w < 760.0 { 190.0 } else { 220.0 }
}

/// The metrics every page shares (`SettingsMetrics`, `Plate.swift`).
pub mod metric {
    /// The panel's corners.
    pub const RADIUS: f32 = 16.0;
    /// A card's corners.
    pub const CARD_RADIUS: f32 = 11.0;
    /// A line's text inset, and the rule's.
    pub const LINE_INSET: f32 = 16.0;
    /// A line's vertical padding.
    pub const LINE_PAD_V: f32 = 12.0;
    pub const TITLE: f32 = 13.0;
    pub const DETAIL: f32 = 12.0;
    /// A section's title over its card.
    pub const CAPTION: f32 = 12.5;
    /// The gap between a section's title and its card.
    pub const SECTION_GAP: f32 = 8.0;
    /// The gap between sections.
    pub const SECTIONS: f32 = 26.0;
    /// Cards stacked in one section (Password managers).
    pub const STACK: f32 = 12.0;
    /// Page padding: sides, top, bottom.
    pub const PAGE_H: f32 = 32.0;
    pub const PAGE_TOP: f32 = 4.0;
    pub const PAGE_BOTTOM: f32 = 36.0;
    /// The page header's padding.
    pub const HEAD_TOP: f32 = 22.0;
    pub const HEAD_BOTTOM: f32 = 16.0;
    pub const HEAD_TITLE: f32 = 20.0;
    pub const HEAD_BLURB: f32 = 13.0;
    /// The rail.
    pub const RAIL_INSET: f32 = 12.0;
    pub const RAIL_TITLE: f32 = 15.0;
    pub const RAIL_GROUPS: f32 = 14.0;
    pub const RAIL_GROUP_LABEL: f32 = 11.5;
    pub const RAIL_ROW: f32 = 30.0;
    pub const RAIL_ROW_RADIUS: f32 = 8.0;
    /// The search field.
    pub const SEARCH_H: f32 = 30.0;
    pub const SEARCH_TEXT: f32 = 13.0;
    /// A rail page with no hits while searching.
    pub const DIMMED: f32 = 0.36;
    /// A line that can't be used now (`.opacity(0.45)`).
    pub const DISABLED: f32 = 0.45;
    /// Controls.
    pub const SWITCH_W: f32 = 30.0;
    pub const SWITCH_H: f32 = 18.0;
    pub const PILL_TEXT: f32 = 11.5;
    pub const PILL_PAD_H: f32 = 10.0;
    pub const PILL_PAD_V: f32 = 5.0;
    pub const SEGMENT_RADIUS: f32 = 9.0;
    pub const SEGMENT_ITEM_RADIUS: f32 = 7.0;
    /// A text field in a line.
    pub const FIELD_H: f32 = 26.0;
    pub const FIELD_W: f32 = 220.0;
    /// How long a search landing stays lit (`SettingsFinder.landed`).
    pub const FLASH_SECONDS: f32 = 1.5;
    /// The panel arrives from this scale (`App.swift` 404–418).
    pub const ENTER_SCALE: f32 = 0.97;
    /// The panel's shadow (`Settings.swift` 117–121).
    pub const SHADOW_ALPHA: f32 = 0.18;
    pub const SHADOW_RADIUS: f32 = 40.0;
    pub const SHADOW_Y: f32 = 14.0;
}

/// `text` cut into runs of lit (`true`) and plain letters for one line
/// that can only be cut at its end (`SettingsMarks`). A match deep in a
/// long line is brought into view — the line starts `…` a few words before
/// it — and past `budget` letters the rest is one plain run, the only one
/// that gets cut short, so a run is never cut in its middle. A run's
/// trailing spaces move to the start of the next, where text measurement
/// counts them.
pub fn runs(
    text: &str,
    marks: &std::collections::BTreeSet<usize>,
    budget: usize,
) -> Vec<(String, bool)> {
    let chars: Vec<char> = text.chars().collect();
    // Where the line starts: a word boundary a little before a late match.
    let start = match marks.first() {
        Some(&m) if m > budget / 2 && m < chars.len() => {
            let from = m.saturating_sub(budget / 4);
            (0..=from)
                .rev()
                .find(|&i| i == 0 || chars[i - 1] == ' ')
                .unwrap_or(0)
        }
        _ => 0,
    };
    let mut out: Vec<(String, bool)> = Vec::new();
    if start > 0 {
        out.push(("…".into(), false));
    }
    let mut used = 0;
    let mut rest = false;
    for (i, c) in chars.iter().enumerate().skip(start) {
        let on = !rest && marks.contains(&i);
        match out.last_mut() {
            Some((run, lit)) if *lit == on => run.push(*c),
            _ => {
                if used >= budget {
                    rest = true;
                    out.push((c.to_string(), false));
                    continue;
                }
                out.push((c.to_string(), on))
            }
        }
        used += 1;
    }
    for i in 1..out.len() {
        let trimmed = out[i - 1].0.trim_end().len();
        let tail = out[i - 1].0.split_off(trimmed);
        out[i].0.insert_str(0, &tail);
    }
    out.retain(|(r, _)| !r.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_is_as_big_as_reads_well() {
        // A roomy window: the most.
        assert_eq!(width(1920.0), 920.0);
        assert_eq!(height(1080.0), 700.0);
        // The default 1180×780 window.
        assert_eq!(width(1180.0), 920.0);
        assert_eq!(height(780.0), 700.0);
        // Between: 80 short of the window, but never under the least.
        assert_eq!(width(800.0), 720.0);
        assert_eq!(width(660.0), 600.0);
        // Too small for the least: 24 to spare.
        assert_eq!(width(620.0), 596.0);
        assert_eq!(height(420.0), 396.0);
    }

    #[test]
    fn the_rail_narrows_with_the_panel() {
        assert_eq!(rail(920.0), 220.0);
        assert_eq!(rail(700.0), 190.0);
    }

    #[test]
    fn greys_follow_the_appearance() {
        assert_eq!(Inks::new(false).rail, Color::grey(0.952));
        assert_eq!(Inks::new(true).rail, Color::grey(0.128));
        assert_eq!(Inks::new(true).mark.a, 0.12);
    }
    #[test]
    fn a_late_match_is_brought_into_view() {
        let text = "From Chrome, Edge, Brave, Vivaldi or Arc on this PC — passwords come through each browser's own export, and nothing leaves this PC";
        let start = text.find("export").unwrap();
        let start = text[..start].chars().count();
        let marks = (start..start + 6).collect();
        let r = runs(text, &marks, 56);
        assert!(r[0].0.starts_with('…'), "{r:?}");
        let lit: Vec<_> = r
            .iter()
            .filter(|(_, on)| *on)
            .map(|(s, _)| s.trim())
            .collect();
        assert_eq!(lit, ["export"]);
        // The lit word sits within the budget, so only the tail is cut.
        let before: usize = r
            .iter()
            .take_while(|(_, on)| !on)
            .map(|(s, _)| s.chars().count())
            .sum();
        assert!(before < 40, "{r:?}");
    }

    #[test]
    fn an_early_match_keeps_the_line_whole() {
        let marks = (0..6).collect();
        let r = runs("Export to a CSV file", &marks, 56);
        assert_eq!(
            r,
            vec![
                ("Export".to_string(), true),
                (" to a CSV file".to_string(), false)
            ]
        );
    }

    #[test]
    fn past_the_budget_the_rest_is_one_plain_run() {
        let text = "aa bb cc dd ee ff";
        let marks = [0, 1, 6, 7, 12, 13].into_iter().collect();
        let r = runs(text, &marks, 8);
        // "aa bb cc" fits; "ee", lit, is past it and goes plain.
        assert_eq!(r.last().unwrap(), &(" dd ee ff".to_string(), false));
        assert_eq!(r.iter().filter(|(_, on)| *on).count(), 2);
    }
}
