//! DOM `KeyboardEvent.key` values → what CDP `Input.dispatchKeyEvent` needs
//! (`code`, Windows virtual-key code, text), for a US layout.
//!
//! CDP delivers key events through the renderer's own input path, so they
//! are trusted and run the editor's default actions (typing, Enter submits,
//! Ctrl+A selects all); the virtual-key code is what Chromium's Windows
//! editing behaviour keys on, so it must be right.

/// One key as CDP wants it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeyDef {
    pub key: String,
    pub code: String,
    /// Windows virtual-key code (`windowsVirtualKeyCode`).
    pub vk: u32,
    /// What pressing it types with the given shift state; `None` for keys
    /// that type nothing.
    pub text: Option<String>,
    /// `location`: 1 for left modifiers, 3 for the numeric keypad.
    pub location: u32,
    /// Whether Shift is part of producing `key` (US layout).
    pub shifted: bool,
}

const NAMED: &[(&str, &str, u32, Option<&str>)] = &[
    ("Enter", "Enter", 0x0D, Some("\r")),
    ("Tab", "Tab", 0x09, None),
    ("Backspace", "Backspace", 0x08, None),
    ("Delete", "Delete", 0x2E, None),
    ("Escape", "Escape", 0x1B, None),
    ("ArrowLeft", "ArrowLeft", 0x25, None),
    ("ArrowUp", "ArrowUp", 0x26, None),
    ("ArrowRight", "ArrowRight", 0x27, None),
    ("ArrowDown", "ArrowDown", 0x28, None),
    ("Home", "Home", 0x24, None),
    ("End", "End", 0x23, None),
    ("PageUp", "PageUp", 0x21, None),
    ("PageDown", "PageDown", 0x22, None),
    ("Insert", "Insert", 0x2D, None),
    ("Shift", "ShiftLeft", 0x10, None),
    ("Control", "ControlLeft", 0x11, None),
    ("Alt", "AltLeft", 0x12, None),
    ("Meta", "MetaLeft", 0x5B, None),
    ("CapsLock", "CapsLock", 0x14, None),
    ("ContextMenu", "ContextMenu", 0x5D, None),
    ("Pause", "Pause", 0x13, None),
    ("PrintScreen", "PrintScreen", 0x2C, None),
    ("ScrollLock", "ScrollLock", 0x91, None),
    ("NumLock", "NumLock", 0x90, None),
    (" ", "Space", 0x20, Some(" ")),
];

/// US-layout punctuation: (unshifted, shifted, code, vk).
const PUNCT: &[(char, char, &str, u32)] = &[
    ('`', '~', "Backquote", 0xC0),
    ('-', '_', "Minus", 0xBD),
    ('=', '+', "Equal", 0xBB),
    ('[', '{', "BracketLeft", 0xDB),
    (']', '}', "BracketRight", 0xDD),
    ('\\', '|', "Backslash", 0xDC),
    (';', ':', "Semicolon", 0xBA),
    ('\'', '"', "Quote", 0xDE),
    (',', '<', "Comma", 0xBC),
    ('.', '>', "Period", 0xBE),
    ('/', '?', "Slash", 0xBF),
];

/// Shifted digit row, `!` (Digit1) … `)` (Digit0).
const SHIFTED_DIGITS: &str = ")!@#$%^&*(";

fn modifier_location(key: &str) -> u32 {
    match key {
        "Shift" | "Control" | "Alt" | "Meta" => 1,
        _ => 0,
    }
}

/// The definition of DOM key `key`, or `None` when it is not on a US
/// keyboard (type such characters with `Input.insertText`).
pub(crate) fn lookup(key: &str) -> Option<KeyDef> {
    if let Some((k, code, vk, text)) = NAMED.iter().find(|(k, ..)| *k == key) {
        return Some(KeyDef {
            key: (*k).into(),
            code: (*code).into(),
            vk: *vk,
            text: text.map(Into::into),
            location: modifier_location(k),
            shifted: false,
        });
    }
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return Some(KeyDef {
            key: key.into(),
            code: key.into(),
            vk: 0x70 + n - 1,
            text: None,
            location: 0,
            shifted: false,
        });
    }
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let def = |code: String, vk: u32, shifted: bool| KeyDef {
        key: key.into(),
        code,
        vk,
        text: Some(key.into()),
        location: 0,
        shifted,
    };
    if c.is_ascii_lowercase() {
        let up = c.to_ascii_uppercase();
        return Some(def(format!("Key{up}"), up as u32, false));
    }
    if c.is_ascii_uppercase() {
        return Some(def(format!("Key{c}"), c as u32, true));
    }
    if c.is_ascii_digit() {
        return Some(def(format!("Digit{c}"), c as u32, false));
    }
    if let Some(i) = SHIFTED_DIGITS.find(c) {
        let d = char::from(b'0' + i as u8);
        return Some(def(format!("Digit{d}"), d as u32, true));
    }
    if let Some((lo, _, code, vk)) = PUNCT.iter().find(|(lo, hi, ..)| *lo == c || *hi == c) {
        return Some(def((*code).into(), *vk, *lo != c));
    }
    None
}

/// CDP's `modifiers` bit field.
pub(crate) fn modifier_bits(m: copper_engine::Modifiers) -> u32 {
    (u32::from(m.alt))
        | (u32::from(m.ctrl) << 1)
        | (u32::from(m.meta) << 2)
        | (u32::from(m.shift) << 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use copper_engine::Modifiers;

    #[test]
    fn named_keys() {
        let e = lookup("Enter").unwrap();
        assert_eq!(
            (e.code.as_str(), e.vk, e.text.as_deref()),
            ("Enter", 13, Some("\r"))
        );
        assert_eq!(lookup("ArrowDown").unwrap().vk, 0x28);
        assert_eq!(lookup("Shift").unwrap().location, 1);
        assert_eq!(lookup("F5").unwrap().vk, 0x74);
        assert_eq!(lookup("F12").unwrap().code, "F12");
        assert!(lookup("F25").is_none());
        assert_eq!(lookup(" ").unwrap().code, "Space");
    }

    #[test]
    fn printable_keys() {
        let a = lookup("a").unwrap();
        assert_eq!((a.code.as_str(), a.vk, a.shifted), ("KeyA", 0x41, false));
        let b = lookup("B").unwrap();
        assert_eq!((b.code.as_str(), b.vk, b.shifted), ("KeyB", 0x42, true));
        let bang = lookup("!").unwrap();
        assert_eq!(
            (bang.code.as_str(), bang.vk, bang.shifted),
            ("Digit1", 0x31, true)
        );
        let rp = lookup(")").unwrap();
        assert_eq!(rp.code, "Digit0");
        let q = lookup("?").unwrap();
        assert_eq!((q.code.as_str(), q.vk, q.shifted), ("Slash", 0xBF, true));
        assert!(!lookup("/").unwrap().shifted);
        assert!(lookup("é").is_none());
        assert!(lookup("Unidentified").is_none());
    }

    #[test]
    fn modifiers() {
        let all = Modifiers {
            shift: true,
            ctrl: true,
            alt: true,
            meta: true,
        };
        assert_eq!(modifier_bits(all), 15);
        assert_eq!(
            modifier_bits(Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }),
            2
        );
        assert_eq!(modifier_bits(Modifiers::NONE), 0);
    }
}
