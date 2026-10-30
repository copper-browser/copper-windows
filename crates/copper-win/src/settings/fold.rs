//! Folds (`SettingsAdvanced`, Mac Copper#118): rows most people never
//! touch, folded at the foot of a card under one Advanced row, and the
//! Spaces page's Customize… and About's shortcuts. A fold is named by its
//! anchor; its row's button sends `fold:<anchor>`, which the panel takes
//! ([`ACTION`]) — no page hears it.
//!
//! Search still finds what a fold holds: the index is made with every fold
//! open ([`indexing`]), and a pick on a row a fold holds opens the fold
//! before the page draws ([`reveal`]). Folds close with Settings.

use super::form::Section;
use std::cell::{Cell, RefCell};

/// The action a fold's button sends, before the fold's anchor.
pub const ACTION: &str = "fold:";

thread_local! {
    /// The folds open now.
    static OPEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// What each fold holds: `(row anchor, fold anchor)`.
    static HOLDS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// Every fold open: the index is being made.
    static ALL: Cell<bool> = const { Cell::new(false) };
}

/// Whether fold `fold`'s rows are drawn.
pub fn is_open(fold: &str) -> bool {
    ALL.get() || OPEN.with_borrow(|o| o.iter().any(|f| f == fold))
}

/// Open fold `fold` if it is closed, close it if it is open.
pub fn toggle(fold: &str) {
    OPEN.with_borrow_mut(|o| match o.iter().position(|f| f == fold) {
        Some(i) => {
            o.remove(i);
        }
        None => o.push(fold.to_string()),
    });
}

/// Fold `fold` holds the rows anchored `rows`.
pub fn holds(fold: &str, rows: &[&str]) {
    HOLDS.with_borrow_mut(|h| {
        for r in rows {
            if !h.iter().any(|(row, f)| row == r && f == fold) {
                h.push((r.to_string(), fold.to_string()));
            }
        }
    });
}

/// A search is landing on `row`: open the fold that holds it.
pub fn reveal(row: &str) {
    let fold = HOLDS.with_borrow(|h| h.iter().find(|(r, _)| r == row).map(|(_, f)| f.clone()));
    if let Some(f) = fold
        && !is_open(&f)
    {
        toggle(&f);
    }
}

/// Settings closed: every fold closes.
pub fn close_all() {
    OPEN.with_borrow_mut(Vec::clear);
}

/// Run `f` with every fold open (making the search index).
pub fn indexing<T>(f: impl FnOnce() -> T) -> T {
    ALL.set(true);
    let out = f();
    ALL.set(false);
    out
}

/// The Advanced row at the foot of `sec`, folding the rows anchored
/// `rows`; `true` when they are to be drawn after it.
pub fn advanced(sec: &mut Section, fold: &str, rows: &[&str]) -> bool {
    holds(fold, rows);
    let open = is_open(fold);
    let more = format!(
        "{} more setting{}",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" }
    );
    sec.button(
        "Advanced",
        (!open).then_some(more.as_str()),
        &format!("{ACTION}{fold}"),
        if open { "Hide" } else { "Show" },
        false,
    )
    .anchor(fold);
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::form::{Control, Form};

    #[test]
    fn a_fold_opens_closes_and_opens_for_search() {
        close_all();
        let mut f = Form::default();
        assert!(!advanced(f.section("S"), "t.more", &["t.a", "t.b"]));
        let line = &f.sections[0].lines[0];
        assert_eq!(line.detail.as_deref(), Some("2 more settings"));
        assert!(matches!(&line.control, Control::Button { action, .. } if action == "fold:t.more"));
        toggle("t.more");
        assert!(is_open("t.more"));
        toggle("t.more");
        assert!(!is_open("t.more"));
        assert!(indexing(|| is_open("t.more")));
        assert!(!is_open("t.more"));
        reveal("t.b");
        assert!(is_open("t.more"));
        close_all();
        assert!(!is_open("t.more"));
    }
}
