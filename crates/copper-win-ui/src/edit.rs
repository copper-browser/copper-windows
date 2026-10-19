//! The one field's editing model: text, selection, IME composition, undo.
//!
//! Positions are byte offsets into the UTF-8 text that always sit on
//! grapheme-cluster boundaries, so the caret never lands inside an emoji, a
//! flag or a letter with its accents. DirectWrite and IMM32 count UTF-16
//! code units; [`utf16_of`] and [`byte_of_utf16`] translate at that edge.
//!
//! Words follow what a browser's address bar does on Windows: a word is a
//! run of letters and digits, a run of punctuation is a word of its own,
//! and spaces are skipped — so Ctrl+Backspace in `example.com/some/path`
//! takes `path`, then `/`, then `some`.
//!
//! The field is single-line: pasted newlines become spaces and control
//! characters are dropped.

use std::ops::Range;
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// One clause of an IME composition (a segment the IME converts as a unit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    /// Byte range in the composition string.
    pub range: Range<usize>,
    /// The clause being converted right now (drawn with a thick underline).
    pub target: bool,
}

/// An IME composition in progress, shown at the caret and not yet text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Composition {
    pub text: String,
    /// Byte offset of the IME's cursor inside `text`.
    pub caret: usize,
    pub clauses: Vec<Clause>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    anchor: usize,
    caret: usize,
}

/// What the last change was, so a run of typing undoes as one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    None,
    Typing,
    Deleting,
}

/// The field's text and everything about editing it.
#[derive(Clone, Debug)]
pub struct TextEdit {
    text: String,
    anchor: usize,
    caret: usize,
    composition: Option<Composition>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    run: Run,
    /// Steps of undo kept.
    pub max_undo: usize,
}

impl Default for TextEdit {
    fn default() -> TextEdit {
        TextEdit::new()
    }
}

/// What a character is, for word movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class_of(g: &str) -> Class {
    let c = g.chars().next().unwrap_or(' ');
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' || c as u32 > 0x2E7F && !c.is_ascii() {
        // Letters, digits, and CJK / emoji and the like (which have no
        // spaces between words) count as word material.
        Class::Word
    } else {
        Class::Punct
    }
}

/// Strip what a one-line field cannot hold.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for c in s.chars() {
        if c == '\r' || c == '\n' || c == '\t' {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            continue;
        }
        if c.is_control() {
            continue;
        }
        last_space = c == ' ';
        out.push(c);
    }
    out
}

impl TextEdit {
    pub fn new() -> TextEdit {
        TextEdit {
            text: String::new(),
            anchor: 0,
            caret: 0,
            composition: None,
            undo: Vec::new(),
            redo: Vec::new(),
            run: Run::None,
            max_undo: 200,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn anchor(&self) -> usize {
        self.anchor
    }

    /// The selection, start to end.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.caret)..self.anchor.max(self.caret)
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.caret
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Replace everything (the app showing a new address). History is
    /// cleared: undo never goes back to another page's address.
    pub fn set_text(&mut self, s: &str, select_all: bool) {
        self.text = clean(s);
        self.caret = self.text.len();
        self.anchor = if select_all { 0 } else { self.caret };
        self.composition = None;
        self.undo.clear();
        self.redo.clear();
        self.run = Run::None;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            anchor: self.anchor,
            caret: self.caret,
        }
    }

    /// Record the state before a change, unless it continues the last run.
    fn checkpoint(&mut self, run: Run) {
        if run == Run::None || run != self.run {
            self.undo.push(self.snapshot());
            if self.undo.len() > self.max_undo {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.run = run;
    }

    /// Break the typing run (a caret move, a paste).
    fn break_run(&mut self) {
        self.run = Run::None;
    }

    // — boundaries —

    /// The grapheme boundary before `pos`.
    pub fn prev_boundary(&self, pos: usize) -> usize {
        let mut c = GraphemeCursor::new(pos.min(self.text.len()), self.text.len(), true);
        c.prev_boundary(&self.text, 0).ok().flatten().unwrap_or(0)
    }

    /// The grapheme boundary after `pos`.
    pub fn next_boundary(&self, pos: usize) -> usize {
        let len = self.text.len();
        let mut c = GraphemeCursor::new(pos.min(len), len, true);
        c.next_boundary(&self.text, 0).ok().flatten().unwrap_or(len)
    }

    /// Snap an arbitrary byte offset (e.g. from a hit test) to a boundary.
    pub fn snap(&self, pos: usize) -> usize {
        let pos = pos.min(self.text.len());
        let mut c = GraphemeCursor::new(pos, self.text.len(), true);
        if c.is_boundary(&self.text, 0).unwrap_or(true) {
            pos
        } else {
            self.prev_boundary(pos)
        }
    }

    fn class_before(&self, pos: usize) -> Option<Class> {
        (pos > 0).then(|| class_of(&self.text[self.prev_boundary(pos)..pos]))
    }

    fn class_after(&self, pos: usize) -> Option<Class> {
        (pos < self.text.len()).then(|| class_of(&self.text[pos..self.next_boundary(pos)]))
    }

    /// The start of the word before `pos`: skip spaces, then one run.
    pub fn word_start_before(&self, mut pos: usize) -> usize {
        while self.class_before(pos) == Some(Class::Space) {
            pos = self.prev_boundary(pos);
        }
        if let Some(run) = self.class_before(pos) {
            while self.class_before(pos) == Some(run) {
                pos = self.prev_boundary(pos);
            }
        }
        pos
    }

    /// The start of the next word after `pos` (Windows' Ctrl+Right): past
    /// this run, then past the spaces after it.
    pub fn word_start_after(&self, mut pos: usize) -> usize {
        if let Some(run) = self.class_after(pos)
            && run != Class::Space
        {
            while self.class_after(pos) == Some(run) {
                pos = self.next_boundary(pos);
            }
        }
        while self.class_after(pos) == Some(Class::Space) {
            pos = self.next_boundary(pos);
        }
        pos
    }

    /// The word under `pos` (double-click).
    pub fn word_at(&self, pos: usize) -> Range<usize> {
        let pos = self.snap(pos);
        let Some(run) = self.class_after(pos).or(self.class_before(pos)) else {
            return pos..pos;
        };
        let mut a = pos;
        while self.class_before(a) == Some(run) {
            a = self.prev_boundary(a);
        }
        let mut b = pos;
        while self.class_after(b) == Some(run) {
            b = self.next_boundary(b);
        }
        a..b
    }

    // — moving —

    /// Put the caret at `pos`, extending the selection or not.
    pub fn move_to(&mut self, pos: usize, extend: bool) {
        let pos = self.snap(pos);
        self.caret = pos;
        if !extend {
            self.anchor = pos;
        }
        self.break_run();
    }

    /// Select `range` with the caret at its end.
    pub fn select(&mut self, range: Range<usize>) {
        self.anchor = self.snap(range.start);
        self.caret = self.snap(range.end);
        self.break_run();
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
        self.break_run();
    }

    /// Left in logical order (the backend swaps for right-to-left text).
    pub fn back(&mut self, extend: bool, word: bool) {
        let to = if !extend && !word && self.has_selection() {
            self.selection().start
        } else if word {
            self.word_start_before(self.caret)
        } else {
            self.prev_boundary(self.caret)
        };
        self.move_to(to, extend);
    }

    pub fn forward(&mut self, extend: bool, word: bool) {
        let to = if !extend && !word && self.has_selection() {
            self.selection().end
        } else if word {
            self.word_start_after(self.caret)
        } else {
            self.next_boundary(self.caret)
        };
        self.move_to(to, extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.move_to(self.text.len(), extend);
    }

    // — editing —

    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.text.replace_range(range.clone(), with);
        self.caret = range.start + with.len();
        self.anchor = self.caret;
    }

    /// Type `s` over the selection.
    pub fn insert(&mut self, s: &str) {
        let s = clean(s);
        if s.is_empty() {
            return;
        }
        // A space after a word ends the typing run, so undo takes back one
        // word at a time — what Windows' own edit controls do.
        let breaks = s.chars().all(char::is_whitespace)
            && self
                .class_before(self.caret)
                .is_some_and(|c| c != Class::Space);
        if breaks {
            // The space starts the next word's run.
            self.checkpoint(Run::None);
            self.run = Run::Typing;
        } else {
            let run = if self.has_selection() || s.graphemes(true).count() > 1 {
                Run::None
            } else {
                Run::Typing
            };
            self.checkpoint(run);
        }
        self.replace(self.selection(), &s);
    }

    /// Paste: one undo step, newlines flattened.
    pub fn paste(&mut self, s: &str) {
        let s = clean(s);
        if s.is_empty() {
            return;
        }
        self.checkpoint(Run::None);
        self.replace(self.selection(), &s);
        self.break_run();
    }

    /// Backspace; with `word`, Ctrl+Backspace.
    pub fn backspace(&mut self, word: bool) -> bool {
        let range = if self.has_selection() {
            self.selection()
        } else if self.caret == 0 {
            return false;
        } else if word {
            self.word_start_before(self.caret)..self.caret
        } else {
            self.prev_boundary(self.caret)..self.caret
        };
        self.checkpoint(if word || self.has_selection() {
            Run::None
        } else {
            Run::Deleting
        });
        self.replace(range, "");
        true
    }

    /// Delete; with `word`, Ctrl+Delete.
    pub fn delete(&mut self, word: bool) -> bool {
        let range = if self.has_selection() {
            self.selection()
        } else if self.caret >= self.text.len() {
            return false;
        } else if word {
            self.caret..self.word_start_after(self.caret)
        } else {
            self.caret..self.next_boundary(self.caret)
        };
        self.checkpoint(if word || self.has_selection() {
            Run::None
        } else {
            Run::Deleting
        });
        self.replace(range, "");
        true
    }

    /// Ctrl+X: the selection, removed. `None` with nothing selected.
    pub fn cut(&mut self) -> Option<String> {
        if !self.has_selection() {
            return None;
        }
        let out = self.selected_text().to_owned();
        self.checkpoint(Run::None);
        self.replace(self.selection(), "");
        Some(out)
    }

    /// Ctrl+C: the selection.
    pub fn copy(&self) -> Option<String> {
        self.has_selection()
            .then(|| self.selected_text().to_owned())
    }

    pub fn undo(&mut self) -> bool {
        if self.composition.is_some() {
            return false;
        }
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        let now = self.snapshot();
        self.redo.push(now);
        self.restore(prev);
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.composition.is_some() {
            return false;
        }
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let now = self.snapshot();
        self.undo.push(now);
        self.restore(next);
        true
    }

    fn restore(&mut self, s: Snapshot) {
        self.text = s.text;
        self.anchor = s.anchor;
        self.caret = s.caret;
        self.run = Run::None;
    }

    // — IME —

    /// The IME's composition string changed. The selection it replaces is
    /// removed when the composition starts, as one undoable step together
    /// with the text it commits.
    pub fn set_composition(&mut self, text: &str, caret: usize, clauses: Vec<Clause>) {
        if self.composition.is_none() {
            self.checkpoint(Run::None);
            if self.has_selection() {
                self.replace(self.selection(), "");
            }
        }
        if text.is_empty() {
            self.composition = Some(Composition::default());
            return;
        }
        let caret = caret.min(text.len());
        self.composition = Some(Composition {
            text: text.to_owned(),
            caret,
            clauses,
        });
    }

    /// The IME produced final text (`GCS_RESULTSTR`).
    pub fn commit_composition(&mut self, result: &str) {
        let had = self.composition.take().is_some();
        let s = clean(result);
        if !had {
            self.checkpoint(Run::None);
        }
        if !s.is_empty() {
            self.replace(self.selection(), &s);
        }
        self.break_run();
    }

    /// The composition was abandoned (Escape in the IME, focus lost).
    pub fn cancel_composition(&mut self) {
        if self.composition.take().is_some() {
            // Nothing was committed: drop the checkpoint if nothing changed.
            if self.undo.last().is_some_and(|s| s.text == self.text) {
                self.undo.pop();
            }
        }
    }

    /// The text as drawn: the composition spliced in at the caret. Returns
    /// the string, the caret in it, and the composition's range in it.
    pub fn display(&self) -> (String, usize, Option<Range<usize>>) {
        match &self.composition {
            Some(c) if !c.text.is_empty() => {
                let at = self.caret;
                let mut s = String::with_capacity(self.text.len() + c.text.len());
                s.push_str(&self.text[..at]);
                s.push_str(&c.text);
                s.push_str(&self.text[at..]);
                (s, at + c.caret, Some(at..at + c.text.len()))
            }
            _ => (self.text.clone(), self.caret, None),
        }
    }

    /// Whether the text reads right to left (its first strong character).
    pub fn is_rtl(&self) -> bool {
        first_strong_rtl(&self.text)
    }
}

/// The paragraph direction by the first strong character (Unicode's P2):
/// Hebrew, Arabic, Syriac, Thaana, N'Ko and friends are right to left.
pub fn first_strong_rtl(s: &str) -> bool {
    for c in s.chars() {
        let u = c as u32;
        let rtl = matches!(u, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF);
        if rtl {
            return true;
        }
        if c.is_alphabetic() {
            return false;
        }
    }
    false
}

/// UTF-16 offset of byte offset `pos` in `s`.
pub fn utf16_of(s: &str, pos: usize) -> usize {
    s[..pos.min(s.len())].encode_utf16().count()
}

/// Byte offset of UTF-16 offset `u` in `s`, rounded down to a char.
pub fn byte_of_utf16(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= u {
            return i;
        }
        n += c.len_utf16();
        if n > u {
            return i;
        }
    }
    s.len()
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
    fn typing_and_undo_by_words() {
        let mut e = TextEdit::new();
        for c in "copper browser".chars() {
            e.insert(&c.to_string());
        }
        assert_eq!(e.text(), "copper browser");
        assert!(e.undo());
        assert_eq!(e.text(), "copper");
        assert!(e.undo());
        assert_eq!(e.text(), "");
        assert!(e.redo());
        assert_eq!(e.text(), "copper");
        assert!(e.redo());
        assert_eq!(e.text(), "copper browser");
        assert!(!e.redo());
    }

    #[test]
    fn ctrl_backspace_takes_url_words() {
        let mut e = field("example.com/some/path");
        e.backspace(true);
        assert_eq!(e.text(), "example.com/some/");
        e.backspace(true);
        assert_eq!(e.text(), "example.com/some");
        e.backspace(true);
        assert_eq!(e.text(), "example.com/");
        let mut e = field("hello   world  ");
        e.backspace(true);
        assert_eq!(e.text(), "hello   ");
    }

    #[test]
    fn ctrl_delete_and_word_moves() {
        let mut e = field("one two three");
        e.home(false);
        e.forward(false, true);
        assert_eq!(e.caret(), 4);
        e.delete(true);
        assert_eq!(e.text(), "one three");
        e.end(false);
        e.back(false, true);
        assert_eq!(e.caret(), 4);
    }

    #[test]
    fn graphemes_are_atomic() {
        let family = "👨‍👩‍👧";
        let mut e = field(&format!("a{family}b"));
        e.end(false);
        e.back(false, false);
        e.back(false, false);
        assert_eq!(e.caret(), 1);
        e.forward(true, false);
        assert_eq!(e.selected_text(), family);
        let mut e = field("e\u{301}"); // e + combining acute
        e.backspace(false);
        assert_eq!(e.text(), "");
        assert_eq!(field("🇯🇵x").next_boundary(0), "🇯🇵".len());
    }

    #[test]
    fn selection_replace_and_clipboard() {
        let mut e = field("hello world");
        e.select(6..11);
        assert_eq!(e.copy().as_deref(), Some("world"));
        assert_eq!(e.cut().as_deref(), Some("world"));
        assert_eq!(e.text(), "hello ");
        e.paste("there\nfriend");
        assert_eq!(e.text(), "hello there friend");
        e.undo();
        assert_eq!(e.text(), "hello ");
        e.undo();
        assert_eq!(e.text(), "hello world");
        assert_eq!(e.selected_text(), "world");
    }

    #[test]
    fn arrows_collapse_a_selection() {
        let mut e = field("abcdef");
        e.select(1..4);
        e.back(false, false);
        assert_eq!((e.caret(), e.anchor()), (1, 1));
        e.select(1..4);
        e.forward(false, false);
        assert_eq!(e.caret(), 4);
    }

    #[test]
    fn ime_composition_then_commit() {
        let mut e = field("go to ");
        e.set_composition("にほ", "にほ".len(), vec![]);
        let (shown, caret, range) = e.display();
        assert_eq!(shown, "go to にほ");
        assert_eq!(caret, shown.len());
        assert_eq!(range, Some(6..6 + "にほ".len()));
        assert_eq!(e.text(), "go to ");
        e.set_composition(
            "日本",
            "日本".len(),
            vec![Clause {
                range: 0..6,
                target: true,
            }],
        );
        e.commit_composition("日本");
        assert_eq!(e.text(), "go to 日本");
        assert!(e.composition().is_none());
        e.undo();
        assert_eq!(e.text(), "go to ");
    }

    #[test]
    fn ime_replaces_selection_and_cancel_restores_nothing_extra() {
        let mut e = field("abc");
        e.select_all();
        e.set_composition("x", 1, vec![]);
        assert_eq!(e.text(), "");
        e.cancel_composition();
        // The selection was removed by the composition's start; undo brings
        // it back.
        assert!(e.undo());
        assert_eq!(e.text(), "abc");
        let mut e = field("abc");
        e.set_composition("x", 1, vec![]);
        e.cancel_composition();
        assert!(!e.can_undo());
    }

    #[test]
    fn utf16_round_trip() {
        let s = "a😀é";
        assert_eq!(utf16_of(s, s.len()), 1 + 2 + 1);
        assert_eq!(byte_of_utf16(s, 1), 1);
        assert_eq!(byte_of_utf16(s, 2), 1); // inside the surrogate pair
        assert_eq!(byte_of_utf16(s, 3), 5);
        assert_eq!(byte_of_utf16(s, 4), s.len());
    }

    #[test]
    fn rtl_by_first_strong() {
        assert!(first_strong_rtl("123 שלום"));
        assert!(!first_strong_rtl("hello שלום"));
        assert!(field("مرحبا").is_rtl());
    }

    #[test]
    fn double_click_word() {
        let e = field("copper.browser/x");
        assert_eq!(e.word_at(2), 0..6);
        assert_eq!(e.word_at(6), 6..7);
    }

    #[test]
    fn control_characters_never_enter() {
        let mut e = TextEdit::new();
        e.insert("\u{7f}");
        e.insert("\u{8}");
        assert_eq!(e.text(), "");
        e.insert("a\tb");
        assert_eq!(e.text(), "a b");
    }
}
