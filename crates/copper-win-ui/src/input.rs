//! Keys, modifiers, and what a key does to the field.
//!
//! The backend turns `WM_KEYDOWN` into a [`Key`] and [`Mods`]; this module
//! decides what that means for a [`TextEdit`] so the whole of the field's
//! keyboard behaviour is tested off Windows. Text itself arrives through
//! `WM_CHAR` and the IME, not here.
//!
//! The bindings are Windows' own (Ctrl where the Mac has ⌘): Ctrl+A, C, X,
//! V, Z, Y and Ctrl+Shift+Z; Ctrl+Backspace and Ctrl+Delete by word;
//! Shift+Insert, Ctrl+Insert and Shift+Delete; Home and End.

use crate::edit::TextEdit;

/// Modifier keys held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };
    pub const CTRL: Mods = Mods {
        ctrl: true,
        shift: false,
        alt: false,
    };
    pub const SHIFT: Mods = Mods {
        ctrl: false,
        shift: true,
        alt: false,
    };
    pub const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };
}

/// A key, by what it is rather than which scan code it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Insert,
    Enter,
    Escape,
    Tab,
    Space,
    /// The context-menu key (or Shift+F10).
    Menu,
    /// A letter, as an uppercase ASCII character.
    Letter(char),
    Digit(u8),
    F(u8),
    /// Anything else, by virtual-key code.
    Other(u32),
}

/// What a key did to the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKey {
    /// Not the field's key: offer it to the app.
    Ignored,
    /// The text changed.
    Edited,
    /// Only the caret or selection moved.
    Moved,
    /// Put this on the clipboard (copy).
    Copy(String),
    /// Put this on the clipboard; the text changed (cut).
    Cut(String),
    /// Read the clipboard and [`TextEdit::paste`] it.
    Paste,
    /// Enter.
    Submit,
    /// Escape.
    Cancel,
}

/// Apply `key` to the field. `rtl`: the paragraph reads right to left, so
/// Left and Right swap their logical meaning.
pub fn field_key(edit: &mut TextEdit, key: Key, mods: Mods, rtl: bool) -> FieldKey {
    if mods.alt {
        return FieldKey::Ignored;
    }
    // While the IME composes, it owns the keys.
    if edit.composition().is_some() {
        return FieldKey::Ignored;
    }
    let word = mods.ctrl;
    let extend = mods.shift;
    match key {
        Key::Left if rtl => {
            edit.forward(extend, word);
            FieldKey::Moved
        }
        Key::Left => {
            edit.back(extend, word);
            FieldKey::Moved
        }
        Key::Right if rtl => {
            edit.back(extend, word);
            FieldKey::Moved
        }
        Key::Right => {
            edit.forward(extend, word);
            FieldKey::Moved
        }
        Key::Home => {
            edit.home(extend);
            FieldKey::Moved
        }
        Key::End => {
            edit.end(extend);
            FieldKey::Moved
        }
        Key::Backspace => edited(edit.backspace(word)),
        Key::Delete if mods.shift && !mods.ctrl => match edit.cut() {
            Some(s) => FieldKey::Cut(s),
            None => edited(edit.delete(false)),
        },
        Key::Delete => edited(edit.delete(word)),
        Key::Insert if mods.shift && !mods.ctrl => FieldKey::Paste,
        Key::Insert if mods.ctrl => edit.copy().map_or(FieldKey::Moved, FieldKey::Copy),
        Key::Enter => FieldKey::Submit,
        Key::Escape => FieldKey::Cancel,
        Key::Letter(c) if mods.ctrl => match (c, mods.shift) {
            ('A', false) => {
                edit.select_all();
                FieldKey::Moved
            }
            ('C', false) => edit.copy().map_or(FieldKey::Moved, FieldKey::Copy),
            ('X', false) => edit.cut().map_or(FieldKey::Moved, FieldKey::Cut),
            ('V', false) => FieldKey::Paste,
            ('Z', false) => edited(edit.undo()),
            ('Y', false) | ('Z', true) => edited(edit.redo()),
            _ => FieldKey::Ignored,
        },
        _ => FieldKey::Ignored,
    }
}

fn edited(changed: bool) -> FieldKey {
    if changed {
        FieldKey::Edited
    } else {
        FieldKey::Moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(s: &str) -> TextEdit {
        let mut e = TextEdit::new();
        e.set_text(s, false);
        e
    }

    #[test]
    fn clipboard_keys() {
        let mut e = field("copper");
        assert_eq!(
            field_key(&mut e, Key::Letter('A'), Mods::CTRL, false),
            FieldKey::Moved
        );
        assert_eq!(
            field_key(&mut e, Key::Letter('C'), Mods::CTRL, false),
            FieldKey::Copy("copper".into())
        );
        assert_eq!(
            field_key(&mut e, Key::Letter('X'), Mods::CTRL, false),
            FieldKey::Cut("copper".into())
        );
        assert_eq!(e.text(), "");
        assert_eq!(
            field_key(&mut e, Key::Letter('V'), Mods::CTRL, false),
            FieldKey::Paste
        );
        assert_eq!(
            field_key(&mut e, Key::Insert, Mods::SHIFT, false),
            FieldKey::Paste
        );
        assert_eq!(
            field_key(&mut e, Key::Letter('Z'), Mods::CTRL, false),
            FieldKey::Edited
        );
        assert_eq!(e.text(), "copper");
        assert_eq!(
            field_key(&mut e, Key::Letter('Z'), Mods::CTRL_SHIFT, false),
            FieldKey::Edited
        );
        assert_eq!(e.text(), "");
    }

    #[test]
    fn ctrl_backspace_by_word() {
        let mut e = field("copper browser");
        assert_eq!(
            field_key(&mut e, Key::Backspace, Mods::CTRL, false),
            FieldKey::Edited
        );
        assert_eq!(e.text(), "copper ");
        assert_eq!(
            field_key(&mut e, Key::Backspace, Mods::NONE, false),
            FieldKey::Edited
        );
        assert_eq!(e.text(), "copper");
    }

    #[test]
    fn arrows_follow_the_paragraph() {
        let mut e = field("שלום");
        e.home(false);
        field_key(&mut e, Key::Left, Mods::NONE, true);
        assert_eq!(e.caret(), "ש".len());
        field_key(&mut e, Key::Right, Mods::NONE, true);
        assert_eq!(e.caret(), 0);
    }

    #[test]
    fn composition_owns_the_keys() {
        let mut e = field("a");
        e.set_composition("か", 3, vec![]);
        assert_eq!(
            field_key(&mut e, Key::Backspace, Mods::NONE, false),
            FieldKey::Ignored
        );
        assert_eq!(
            field_key(&mut e, Key::Enter, Mods::NONE, false),
            FieldKey::Ignored
        );
    }

    #[test]
    fn app_keys_pass_through() {
        let mut e = field("a");
        assert_eq!(
            field_key(&mut e, Key::Letter('K'), Mods::CTRL, false),
            FieldKey::Ignored
        );
        assert_eq!(
            field_key(&mut e, Key::Up, Mods::NONE, false),
            FieldKey::Ignored
        );
        assert_eq!(
            field_key(&mut e, Key::Enter, Mods::NONE, false),
            FieldKey::Submit
        );
    }
}
