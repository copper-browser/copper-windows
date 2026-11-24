//! Tidy on Windows (Mac `Fork/Tidy.swift`, `ClearTabsDivider`'s "Tidy",
//! copper-browser/Copper#200).
//!
//! The shell plans and files ([`copper_shell_core::tidy`]); this draws
//! "Tidy" at the end of the seam above New Tab from three loose tabs
//! ("Tidying…" while the model thinks), offers Tidy Tabs in the palette,
//! and shows "Tidied N tabs into M folders · Undo" on Clear's pill
//! ([`crate::clear::show_pill`]). Ctrl+Z undoes while it is up — otherwise
//! the key stays the page's.

use crate::chrome::glyph;
use crate::chrome::toast::CAPSULE;
use crate::commands::Command;
use crate::keys::Chord;
use crate::platform::Shell;
use copper_shell_core::tidy::{LEAST, UNDO_FOR, tidied_words};
use copper_win_ui::Weight;
use copper_win_ui::input::{Key, Mods};
use copper_win_ui::tokens::side;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Justify, Node, NodeId, Role, Token};
use std::cell::RefCell;

/// The seam's "Tidy" and its words.
#[derive(Clone, Copy)]
struct Button {
    node: NodeId,
    text: NodeId,
}

thread_local! {
    static BUTTON: RefCell<Option<Button>> = const { RefCell::new(None) };
}

/// Add "Tidy" to the end of the column's seam (after Clear's insides).
pub fn seam(ui: &mut Ui, seam: NodeId) {
    let node = ui.tree.add(
        seam,
        Node::row()
            .height(18.0)
            .padding(Insets::hv(6.0, 0.0))
            .gap(3.0)
            .align(Align::Center)
            .justify(Justify::Center)
            .shrink(0.0)
            .radius(CAPSULE)
            .hover_fill(Token::ColumnHover)
            .hidden(true)
            .button("Tidy tabs")
            .tooltip("Sort these tabs into folders"),
    );
    ui.tree.add(
        node,
        Node::glyph(glyph::INTELLIGENCE, 9.0, 11.0)
            .weight(Weight::Semibold)
            .ink(Token::ColumnMuted)
            .hover_ink(Token::ColumnInk)
            .role(Role::None),
    );
    let text = ui.tree.add(
        node,
        Node::text("Tidy", 11.0)
            .weight(Weight::Medium)
            .ink(Token::ColumnMuted)
            .hover_ink(Token::ColumnInk)
            .role(Role::None),
    );
    BUTTON.with(|b| *b.borrow_mut() = Some(Button { node, text }));
}

/// Show "Tidy" while there are `loose` tabs enough, "Tidying…" while
/// `busy`; the seam ends 4 px after it rather than a row inset away.
pub fn sync_seam(ui: &mut Ui, seam: NodeId, loose: usize, busy: bool) {
    let Some(b) = BUTTON.with(|b| *b.borrow()) else {
        return;
    };
    let on = loose >= LEAST;
    ui.tree.set_hidden(b.node, !on);
    let words = if busy { "Tidying…" } else { "Tidy" };
    if ui.tree.get(b.text).and_then(|n| n.text_str()) != Some(words) {
        ui.tree.set_text(b.text, words);
    }
    let trailing = if on { 4.0 } else { side::ROW_INSET };
    if ui
        .tree
        .get(seam)
        .is_some_and(|n| n.style.padding.right != trailing)
    {
        ui.tree.edit(seam, |n| n.style.padding.right = trailing);
    }
}

/// Tidy Tabs (the palette; Mac `CommandBar` `tidy-tabs`), and the key
/// Ctrl+Z runs while its Undo is offered.
pub fn commands() -> [Command; 2] {
    [
        Command::new("tidy-tabs", "Tidy Tabs", glyph::INTELLIGENCE, run)
            .when(|s| s.tidy_candidates().len() >= 2),
        Command::key("undo-tidy", |s| {
            undo(s);
        }),
    ]
}

/// Ctrl+Z while Tidy's Undo is offered.
pub fn takes(shell: &Shell, chord: Chord) -> Option<&'static str> {
    let ctrl_z = chord.key == Key::Letter('Z')
        && chord.mods
            == (Mods {
                ctrl: true,
                shift: false,
                alt: false,
            });
    (ctrl_z && shell.tidy_undo_line().is_some()).then_some("undo-tidy")
}

/// A click on the seam's "Tidy".
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let hit = BUTTON.with(|b| b.borrow().is_some_and(|b| b.node == id));
    if hit {
        run(shell);
    }
    hit
}

/// Tidy now; the pill follows when anything was filed.
pub fn run(shell: &Shell) {
    let s = shell.clone();
    copper_win_rt::spawn_local(async move {
        let done = s.tidy_tabs(false).await;
        if done.tabs > 0 {
            let words = tidied_words(done.tabs, done.folders.len());
            crate::clear::show_pill(&s, &words, "undo-tidy", UNDO_FOR);
        }
    });
}

/// Undo the last Tidy, while offered.
pub fn undo(shell: &Shell) -> usize {
    let n = shell.undo_tidy();
    crate::clear::hide_pill(shell);
    n
}
