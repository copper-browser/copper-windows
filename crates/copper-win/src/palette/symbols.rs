//! A space's mark (`Fork/Spaces.swift` `Space.symbol/emoji/title`,
//! `Fork/SpacesUI.swift` `SpaceGlyph`): its emoji, its symbol, or a dot —
//! never a letter. The Mac's symbols are SF Symbols, stored `sf:<name>`;
//! Windows draws each with the nearest Segoe Fluent Icons glyph.

/// The symbols on offer, in the Mac's order, with the name a person reads
/// (`SpacePage.symbols`, `symbolName`) and the Windows glyph.
pub const SYMBOLS: [(&str, &str, char); 26] = [
    ("house", "Home", '\u{E80F}'),
    ("briefcase", "Briefcase", '\u{E821}'),
    ("book", "Book", '\u{E736}'),
    ("hammer", "Hammer", '\u{E90F}'),
    ("gamecontroller", "Game controller", '\u{E7FC}'),
    ("cart", "Cart", '\u{E7BF}'),
    ("heart", "Heart", '\u{EB51}'),
    ("star", "Star", '\u{E734}'),
    ("flask", "Flask", '\u{E9D9}'),
    ("graduationcap", "Graduation cap", '\u{E7BE}'),
    ("music.note", "Music", '\u{E8D6}'),
    ("airplane", "Airplane", '\u{E709}'),
    ("paintbrush", "Paintbrush", '\u{E771}'),
    ("leaf", "Leaf", '\u{E913}'),
    ("terminal", "Terminal", '\u{E756}'),
    ("globe", "Globe", '\u{E774}'),
    ("camera", "Camera", '\u{E722}'),
    ("film", "Film", '\u{E714}'),
    ("bolt", "Bolt", '\u{E945}'),
    ("flame", "Flame", '\u{ECAD}'),
    ("moon", "Moon", '\u{E708}'),
    ("sun.max", "Sun", '\u{E706}'),
    ("cup.and.saucer", "Cup", '\u{EC32}'),
    ("dumbbell", "Dumbbell", '\u{E805}'),
    ("chart.bar", "Chart", '\u{E9D2}'),
    ("folder", "Folder", '\u{E8B7}'),
];

/// What a space wears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    Emoji(String),
    /// A Segoe Fluent glyph standing for an SF Symbol.
    Symbol(char),
    /// No icon: a dot.
    Dot,
}

/// The SF Symbol a stored icon names, if it is one (`Space.symbol`).
pub fn symbol(icon: Option<&str>) -> Option<&str> {
    icon?.strip_prefix("sf:")
}

/// The Windows glyph for an SF Symbol name (a symbol from a Mac we have no
/// drawing of falls back to the grid mark).
pub fn glyph_of(name: &str) -> char {
    SYMBOLS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map_or('\u{E80A}', |(_, _, g)| *g)
}

/// What a person calls a symbol (`SpacePage.symbolName`).
pub fn symbol_name(name: &str) -> String {
    if let Some((_, n, _)) = SYMBOLS.iter().find(|(s, _, _)| *s == name) {
        return (*n).to_string();
    }
    let words = name.replace('.', " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Whether a character starts an emoji presentation (the Mac's
/// `isEmojiPresentation`, for the ranges that matter to a space's name).
pub fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F300..=0x1F5FF
        | 0x1F600..=0x1F64F
        | 0x1F680..=0x1F6FF
        | 0x1F900..=0x1F9FF
        | 0x1FA70..=0x1FAFF
        | 0x2600..=0x26FF
        | 0x2700..=0x27BF
        | 0x1F1E6..=0x1F1FF
        | 0x2B50
        | 0x2B55
        | 0x231A..=0x231B
        | 0x23E9..=0x23F3)
}

/// The first emoji in `text` with whatever joins it (modifiers, ZWJ
/// sequences, variation selectors) — the emoji field keeps only that.
pub fn first_emoji(text: &str) -> Option<String> {
    let mut chars = text.chars().skip_while(|c| !is_emoji(*c)).peekable();
    let first = chars.next()?;
    let mut out = String::from(first);
    let mut join = false;
    while let Some(&c) = chars.peek() {
        let u = c as u32;
        let modifier =
            matches!(u, 0x1F3FB..=0x1F3FF | 0xFE0F | 0xFE0E | 0x20E3 | 0xE0020..=0xE007F);
        if c == '\u{200D}' {
            join = true;
            out.push(c);
            chars.next();
        } else if modifier
            || join
            || (is_regional(first) && is_regional(c) && out.chars().count() == 1)
        {
            join = false;
            out.push(c);
            chars.next();
        } else {
            break;
        }
    }
    Some(out)
}

fn is_regional(c: char) -> bool {
    matches!(c as u32, 0x1F1E6..=0x1F1FF)
}

/// The emoji a space shows: its own, or the one its name begins with
/// (`Space.emoji`).
pub fn emoji(icon: Option<&str>, name: &str) -> Option<String> {
    if let Some(i) = icon.filter(|i| !i.is_empty() && !i.starts_with("sf:")) {
        return Some(i.to_string());
    }
    let first = name.chars().next()?;
    is_emoji(first).then(|| first_emoji(name)).flatten()
}

/// The space's mark (`SpaceGlyph`): emoji, else symbol, else a dot.
pub fn mark(icon: Option<&str>, name: &str) -> Mark {
    if let Some(s) = symbol(icon) {
        return Mark::Symbol(glyph_of(s));
    }
    match emoji(icon, name) {
        Some(e) => Mark::Emoji(e),
        None => Mark::Dot,
    }
}

/// The name without the emoji the mark already shows (`Space.title`), so
/// a header does not read "🏠 🏠 Home".
pub fn title(icon: Option<&str>, name: &str) -> String {
    if icon.is_some_and(|i| !i.is_empty()) {
        return name.to_string();
    }
    match name.chars().next() {
        Some(c) if is_emoji(c) => {
            let lead = first_emoji(name).unwrap_or_default();
            let rest = name[lead.len()..].trim();
            if rest.is_empty() {
                name.to_string()
            } else {
                rest.to_string()
            }
        }
        _ => name.to_string(),
    }
}

/// "1 tab", "3 tabs".
pub fn tabs(n: usize) -> String {
    if n == 1 {
        "1 tab".into()
    } else {
        format!("{n} tabs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_emoji_symbol_or_dot() {
        assert_eq!(mark(None, "Work"), Mark::Dot, "no letter");
        assert_eq!(mark(None, "🏠 Home"), Mark::Emoji("🏠".into()));
        assert_eq!(mark(Some("🔥"), "Work"), Mark::Emoji("🔥".into()));
        assert_eq!(mark(Some("sf:house"), "🏠 Home"), Mark::Symbol('\u{E80F}'));
        assert_eq!(
            mark(Some("sf:unknown.thing"), "x"),
            Mark::Symbol('\u{E80A}')
        );
    }

    #[test]
    fn titles_drop_the_shown_emoji() {
        assert_eq!(title(None, "🏠 Home"), "Home");
        assert_eq!(title(None, "🏠"), "🏠");
        assert_eq!(title(Some("🔥"), "🏠 Home"), "🏠 Home");
        assert_eq!(title(None, "Work"), "Work");
    }

    #[test]
    fn first_emoji_keeps_its_joins() {
        assert_eq!(first_emoji("abc 👍🏽 x").as_deref(), Some("👍🏽"));
        assert_eq!(first_emoji("👩‍💻🔥").as_deref(), Some("👩‍💻"));
        assert_eq!(first_emoji("🇫🇷🇩🇪").as_deref(), Some("🇫🇷"));
        assert_eq!(first_emoji("plain").as_deref(), None);
    }

    #[test]
    fn names() {
        assert_eq!(symbol_name("gamecontroller"), "Game controller");
        assert_eq!(symbol_name("sun.max"), "Sun");
        assert_eq!(symbol_name("person.crop"), "Person crop");
        assert_eq!(tabs(1), "1 tab");
        assert_eq!(tabs(0), "0 tabs");
    }
}
