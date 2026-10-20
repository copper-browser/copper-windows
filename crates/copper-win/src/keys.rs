//! Keyboard shortcuts: which chord runs which command.
//!
//! One table serves both ways a key arrives: from the chrome
//! (`Event::Key`, when a field, a row or nothing has focus) and from a page
//! (WebView2's `AcceleratorKeyPressed`, when the page has focus). A key the
//! table names is Copper's wherever focus is; any other key belongs to
//! whoever has focus.
//!
//! The bindings mirror copper-app's `actions.rs` (Ctrl where the Mac has
//! ⌘), with Windows' own spellings added where Windows has them (F3 to find
//! again, Ctrl+F4 to close, the keyboard's Back and Forward keys).

use copper_win_ui::input::{Key, Mods};

/// A key with the modifiers held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    pub mods: Mods,
}

impl Chord {
    pub const fn new(key: Key, mods: Mods) -> Chord {
        Chord { key, mods }
    }
}

const fn c(key: Key) -> Chord {
    Chord::new(
        key,
        Mods {
            ctrl: true,
            shift: false,
            alt: false,
        },
    )
}
const fn cs(key: Key) -> Chord {
    Chord::new(
        key,
        Mods {
            ctrl: true,
            shift: true,
            alt: false,
        },
    )
}
const fn ca(key: Key) -> Chord {
    Chord::new(
        key,
        Mods {
            ctrl: true,
            shift: false,
            alt: true,
        },
    )
}
const fn a(key: Key) -> Chord {
    Chord::new(
        key,
        Mods {
            ctrl: false,
            shift: false,
            alt: true,
        },
    )
}
const fn s(key: Key) -> Chord {
    Chord::new(
        key,
        Mods {
            ctrl: false,
            shift: true,
            alt: false,
        },
    )
}
const fn k(key: Key) -> Chord {
    Chord::new(key, Mods::NONE)
}
const fn l(ch: char) -> Key {
    Key::Letter(ch)
}

// Virtual-key codes the toolkit has no name for.
const VK_OEM_PLUS: u32 = 0xBB;
const VK_OEM_COMMA: u32 = 0xBC;
const VK_OEM_MINUS: u32 = 0xBD;
const VK_OEM_4: u32 = 0xDB; // [
const VK_OEM_6: u32 = 0xDD; // ]
const VK_ADD: u32 = 0x6B;
const VK_SUBTRACT: u32 = 0x6D;
const VK_NUMPAD0: u32 = 0x60;
const VK_BROWSER_BACK: u32 = 0xA6;
const VK_BROWSER_FORWARD: u32 = 0xA7;
const VK_BROWSER_REFRESH: u32 = 0xA8;
const VK_BROWSER_SEARCH: u32 = 0xAA;

/// Every binding: command id → its chords, the first one the one shown.
pub const BINDINGS: &[(&str, &[Chord])] = &[
    // Tabs.
    ("new-tab", &[c(l('T'))]),
    ("new-private-tab", &[cs(l('N'))]),
    ("new-canvas", &[cs(l('E'))]),
    ("close-tab", &[c(l('W')), c(Key::F(4))]),
    ("reopen-tab", &[cs(l('T'))]),
    ("next-tab", &[c(Key::Tab), c(Key::PageDown)]),
    ("previous-tab", &[cs(Key::Tab), c(Key::PageUp)]),
    ("select-tab-1", &[c(Key::Digit(1)), a(Key::Digit(1))]),
    ("select-tab-2", &[c(Key::Digit(2)), a(Key::Digit(2))]),
    ("select-tab-3", &[c(Key::Digit(3)), a(Key::Digit(3))]),
    ("select-tab-4", &[c(Key::Digit(4)), a(Key::Digit(4))]),
    ("select-tab-5", &[c(Key::Digit(5)), a(Key::Digit(5))]),
    ("select-tab-6", &[c(Key::Digit(6)), a(Key::Digit(6))]),
    ("select-tab-7", &[c(Key::Digit(7)), a(Key::Digit(7))]),
    ("select-tab-8", &[c(Key::Digit(8)), a(Key::Digit(8))]),
    ("select-tab-9", &[c(Key::Digit(9)), a(Key::Digit(9))]),
    ("pin-tab", &[cs(l('D'))]),
    // The field and the palette.
    ("focus-field", &[c(l('L')), a(l('D')), k(Key::F(6))]),
    ("palette", &[c(l('K'))]),
    ("escape", &[k(Key::Escape)]),
    // The page.
    (
        "reload",
        &[c(l('R')), k(Key::F(5)), k(Key::Other(VK_BROWSER_REFRESH))],
    ),
    ("hard-reload", &[cs(l('R')), s(Key::F(5)), c(Key::F(5))]),
    (
        "back",
        &[
            a(Key::Left),
            c(Key::Other(VK_OEM_4)),
            k(Key::Other(VK_BROWSER_BACK)),
        ],
    ),
    (
        "forward",
        &[
            a(Key::Right),
            c(Key::Other(VK_OEM_6)),
            k(Key::Other(VK_BROWSER_FORWARD)),
        ],
    ),
    ("find", &[c(l('F'))]),
    ("find-next", &[c(l('G')), k(Key::F(3))]),
    ("find-previous", &[cs(l('G')), s(Key::F(3))]),
    (
        "zoom-in",
        &[
            c(Key::Other(VK_OEM_PLUS)),
            cs(Key::Other(VK_OEM_PLUS)),
            c(Key::Other(VK_ADD)),
        ],
    ),
    (
        "zoom-out",
        &[c(Key::Other(VK_OEM_MINUS)), c(Key::Other(VK_SUBTRACT))],
    ),
    ("zoom-reset", &[c(Key::Digit(0)), c(Key::Other(VK_NUMPAD0))]),
    ("inspector", &[cs(l('I')), k(Key::F(12))]),
    ("bookmark-page", &[cs(l('B'))]),
    ("hide-anything", &[cs(l('H'))]),
    ("reader", &[ca(l('R'))]),
    // Panels.
    ("show-history", &[c(l('H'))]),
    ("show-downloads", &[cs(l('J'))]),
    ("show-hidden", &[cs(l('U'))]),
    // Spaces.
    ("next-space", &[ca(Key::Right)]),
    ("previous-space", &[ca(Key::Left)]),
    // Agents.
    ("agent", &[c(l('E'))]),
    // Window.
    ("toggle-sidebar", &[c(l('S'))]),
    ("toggle-layout", &[cs(l('S'))]),
    ("fullscreen", &[k(Key::F(11))]),
    ("settings", &[c(Key::Other(VK_OEM_COMMA))]),
    ("search", &[k(Key::Other(VK_BROWSER_SEARCH))]),
    ("quit", &[c(l('Q'))]),
];

/// The command `chord` runs, if any.
pub fn command(chord: Chord) -> Option<&'static str> {
    BINDINGS
        .iter()
        .find(|(_, chords)| chords.contains(&chord))
        .map(|(id, _)| *id)
}

/// How a command's first chord is written (`Ctrl+Shift+T`), for tooltips
/// and the palette.
pub fn shortcut(id: &str) -> Option<String> {
    let (_, chords) = BINDINGS.iter().find(|(i, _)| *i == id)?;
    chords.first().map(|c| label(*c))
}

/// `Ctrl+Shift+T`.
pub fn label(chord: Chord) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(4);
    if chord.mods.ctrl {
        parts.push("Ctrl".into());
    }
    if chord.mods.alt {
        parts.push("Alt".into());
    }
    if chord.mods.shift {
        parts.push("Shift".into());
    }
    parts.push(key_name(chord.key));
    parts.join("+")
}

fn key_name(key: Key) -> String {
    match key {
        Key::Left => "Left".into(),
        Key::Right => "Right".into(),
        Key::Up => "Up".into(),
        Key::Down => "Down".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "PgUp".into(),
        Key::PageDown => "PgDn".into(),
        Key::Backspace => "Backspace".into(),
        Key::Delete => "Del".into(),
        Key::Insert => "Ins".into(),
        Key::Enter => "Enter".into(),
        Key::Escape => "Esc".into(),
        Key::Tab => "Tab".into(),
        Key::Space => "Space".into(),
        Key::Menu => "Menu".into(),
        Key::Letter(ch) => ch.to_string(),
        Key::Digit(d) => d.to_string(),
        Key::F(n) => format!("F{n}"),
        Key::Other(VK_OEM_PLUS) => "=".into(),
        Key::Other(VK_OEM_MINUS) => "-".into(),
        Key::Other(VK_OEM_COMMA) => ",".into(),
        Key::Other(VK_OEM_4) => "[".into(),
        Key::Other(VK_OEM_6) => "]".into(),
        Key::Other(VK_ADD) => "Num+".into(),
        Key::Other(VK_SUBTRACT) => "Num-".into(),
        Key::Other(VK_NUMPAD0) => "Num0".into(),
        Key::Other(VK_BROWSER_BACK) => "Browser Back".into(),
        Key::Other(VK_BROWSER_FORWARD) => "Browser Forward".into(),
        Key::Other(VK_BROWSER_REFRESH) => "Browser Refresh".into(),
        Key::Other(VK_BROWSER_SEARCH) => "Browser Search".into(),
        Key::Other(vk) => format!("0x{vk:02X}"),
    }
}

/// The toolkit's [`Key`] for virtual-key code `vk` (what WebView2's
/// accelerator event carries) — the same mapping the toolkit applies to its
/// own `WM_KEYDOWN`.
pub fn key_of(vk: u32) -> Key {
    match vk {
        0x25 => Key::Left,
        0x27 => Key::Right,
        0x26 => Key::Up,
        0x28 => Key::Down,
        0x24 => Key::Home,
        0x23 => Key::End,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x08 => Key::Backspace,
        0x2E => Key::Delete,
        0x2D => Key::Insert,
        0x0D => Key::Enter,
        0x1B => Key::Escape,
        0x09 => Key::Tab,
        0x20 => Key::Space,
        0x5D => Key::Menu,
        0x41..=0x5A => Key::Letter(char::from(vk as u8)),
        0x30..=0x39 => Key::Digit((vk - 0x30) as u8),
        0x70..=0x87 => Key::F((vk - 0x70 + 1) as u8),
        _ => Key::Other(vk),
    }
}

/// Whether a key pressed in a page is one Copper takes from it: only the
/// table's chords, and Escape only while the chrome has something open to
/// close (a page's own Escape — leaving a dialog, stopping a video — wins
/// otherwise).
pub fn page_takes(chord: Chord, chrome_has_overlay: bool) -> Option<&'static str> {
    let id = command(chord)?;
    match id {
        "escape" if !chrome_has_overlay => None,
        // The page's own find bar is never shown: Copper's is.
        _ => Some(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(key: Key, ctrl: bool, shift: bool, alt: bool) -> Chord {
        Chord::new(key, Mods { ctrl, shift, alt })
    }

    #[test]
    fn the_briefs_keys_are_bound() {
        let t = |k, ctrl, shift, alt| command(chord(k, ctrl, shift, alt));
        assert_eq!(t(Key::Letter('T'), true, false, false), Some("new-tab"));
        assert_eq!(t(Key::Letter('W'), true, false, false), Some("close-tab"));
        assert_eq!(t(Key::Letter('T'), true, true, false), Some("reopen-tab"));
        assert_eq!(t(Key::Letter('L'), true, false, false), Some("focus-field"));
        assert_eq!(t(Key::Letter('K'), true, false, false), Some("palette"));
        assert_eq!(t(Key::Tab, true, false, false), Some("next-tab"));
        assert_eq!(t(Key::Tab, true, true, false), Some("previous-tab"));
        assert_eq!(t(Key::Digit(1), true, false, false), Some("select-tab-1"));
        assert_eq!(t(Key::Digit(9), true, false, false), Some("select-tab-9"));
        assert_eq!(t(Key::Letter('R'), true, false, false), Some("reload"));
        assert_eq!(t(Key::F(5), false, false, false), Some("reload"));
        assert_eq!(t(Key::Left, false, false, true), Some("back"));
        assert_eq!(t(Key::Right, false, false, true), Some("forward"));
        assert_eq!(t(Key::Letter('F'), true, false, false), Some("find"));
        assert_eq!(
            t(Key::Letter('B'), true, true, false),
            Some("bookmark-page")
        );
        assert_eq!(
            t(Key::Letter('H'), true, false, false),
            Some("show-history")
        );
        assert_eq!(
            t(Key::Letter('J'), true, true, false),
            Some("show-downloads")
        );
        assert_eq!(t(Key::Letter('U'), true, true, false), Some("show-hidden"));
        // Ctrl+Y is Redo in Windows text fields and pages.
        assert_eq!(t(Key::Letter('Y'), true, false, false), None);
        assert_eq!(
            t(Key::Other(VK_OEM_COMMA), true, false, false),
            Some("settings")
        );
        assert_eq!(t(Key::Letter('E'), true, false, false), Some("agent"));
        assert_eq!(t(Key::Letter('E'), true, true, false), Some("new-canvas"));
        assert_eq!(
            t(Key::Letter('N'), true, true, false),
            Some("new-private-tab")
        );
        assert_eq!(t(Key::F(11), false, false, false), Some("fullscreen"));
        assert_eq!(t(Key::Escape, false, false, false), Some("escape"));
    }

    #[test]
    fn typing_is_never_a_command() {
        for ch in 'A'..='Z' {
            assert_eq!(command(chord(Key::Letter(ch), false, false, false)), None);
            assert_eq!(command(chord(Key::Letter(ch), false, true, false)), None);
        }
        assert_eq!(command(chord(Key::Enter, false, false, false)), None);
        assert_eq!(command(chord(Key::Tab, false, false, false)), None);
        // Copy, paste, select all, undo are the field's and the page's.
        for ch in ['A', 'C', 'V', 'X', 'Z'] {
            assert_eq!(command(chord(Key::Letter(ch), true, false, false)), None);
        }
    }

    #[test]
    fn no_chord_is_bound_twice() {
        let mut seen: Vec<Chord> = Vec::new();
        for (id, chords) in BINDINGS {
            for ch in *chords {
                assert!(!seen.contains(ch), "{id}: {} bound twice", label(*ch));
                seen.push(*ch);
            }
        }
    }

    #[test]
    fn escape_stays_with_the_page_unless_the_chrome_has_something_open() {
        let esc = chord(Key::Escape, false, false, false);
        assert_eq!(page_takes(esc, false), None);
        assert_eq!(page_takes(esc, true), Some("escape"));
        let t = chord(Key::Letter('T'), true, false, false);
        assert_eq!(page_takes(t, false), Some("new-tab"));
    }

    #[test]
    fn labels() {
        assert_eq!(shortcut("reopen-tab").as_deref(), Some("Ctrl+Shift+T"));
        assert_eq!(shortcut("settings").as_deref(), Some("Ctrl+,"));
        assert_eq!(shortcut("back").as_deref(), Some("Alt+Left"));
        assert_eq!(shortcut("fullscreen").as_deref(), Some("F11"));
        assert_eq!(shortcut("nope"), None);
    }

    #[test]
    fn virtual_keys_map_like_the_toolkits() {
        assert_eq!(key_of(0x54), Key::Letter('T'));
        assert_eq!(key_of(0x31), Key::Digit(1));
        assert_eq!(key_of(0x74), Key::F(5));
        assert_eq!(key_of(0x7A), Key::F(11));
        assert_eq!(key_of(0x25), Key::Left);
        assert_eq!(key_of(0x09), Key::Tab);
        assert_eq!(key_of(0xBC), Key::Other(VK_OEM_COMMA));
    }
}
