//! Clear and its Undo on Windows (Mac `Fork/ClearTabs.swift`, `Side.swift`
//! `ClearTabsDivider`, `App.swift` `clearTabsToast`).
//!
//! The shell decides which rows go and keeps the Undo
//! ([`copper_shell_core::clear`]); this draws the two places it shows:
//!
//! * the seam above New Tab: while anything is clearable it grows from the
//!   plain 13 px hairline to the Mac's 26 px, and "↓ Clear" appears in it
//!   while the pointer is over it;
//! * "Cleared N tabs · Undo" where the bars stand at the foot of the
//!   window (over everything), for the six seconds Undo is offered.
//!
//! Ctrl+Shift+K clears (the palette's Clear Tabs too); Ctrl+Z undoes while
//! the pill is up — otherwise it stays the page's.

use crate::chrome::build::tip;
use crate::chrome::glyph;
use crate::chrome::toast::CAPSULE;
use crate::commands::Command;
use crate::keys::Chord;
use crate::platform::Shell;
use copper_shell_core::clear::{UNDO_FOR, cleared_words};
use copper_win_ui::Weight;
use copper_win_ui::input::{Key, Mods};
use copper_win_ui::tokens::{Shadow, motion, side};
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Abs, Align, Color, Dim, Insets, Justify, Node, NodeId, Prop, Role, Swatch, Token,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The seam at rest: a hairline with 6 px above and below (`Side.swift`
/// `divider`).
pub const PLAIN: f32 = 1.0 + 2.0 * side::DIVIDER_PAD;
/// The seam with something to clear: 4 px above and below the 18 px
/// "↓ Clear" capsule (`ClearTabsDivider`).
pub const CLEARING: f32 = 26.0;
const CLEARING_PAD: f32 = 4.0;
// The capsule (2 + 14 + 2) fits between the pads.
const _: () = assert!(CLEARING - 2.0 * CLEARING_PAD >= 18.0 && PLAIN == 13.0);

/// The Undo's colour (`DriveStyle.accent`).
const UNDO_INK: Color = Color::rgb(0.78, 0.45, 0.24);

/// The nodes this owns. One window per process.
#[derive(Default)]
struct Ids {
    seam: Option<NodeId>,
    button: Option<NodeId>,
    pill: Option<Pill>,
    /// Bumped by every pill shown, so an older one's timer leaves a newer
    /// alone.
    generation: u64,
}

#[derive(Clone, Copy)]
struct Pill {
    /// The pill's own foot-of-the-window stack, beside the bars: it goes
    /// over everything when shown, the empty stage's standing field too
    /// (whose scrim would otherwise take the click meant for Undo).
    holder: NodeId,
    node: NodeId,
    text: NodeId,
    undo: NodeId,
}

thread_local! {
    static IDS: RefCell<Ids> = RefCell::default();
}

/// Build the seam's insides into the column's divider `seam`: hairline,
/// the hover-only "↓ Clear", hairline. The divider is a pointer target, so
/// the capsule shows while the pointer is anywhere on it.
pub fn seam(ui: &mut Ui, seam: NodeId) {
    ui.tree.edit(seam, |n| {
        n.style.dir = copper_win_ui::Dir::Row;
        n.style.align = Align::Center;
        n.flags.hit = true;
    });
    let line = || {
        Node::new()
            .height(1.0)
            .grow(1.0)
            .fill(Token::ColumnHairline)
    };
    ui.tree.add(seam, line());
    let button = ui.tree.add(
        seam,
        Node::row()
            .padding(Insets::hv(4.0, 2.0))
            .gap(3.0)
            .align(Align::Center)
            .justify(Justify::Center)
            .shrink(0.0)
            .radius(CAPSULE)
            .fill(Swatch::Fade(Token::ColumnHover, 0.7))
            .reveal()
            .hidden(true)
            .button("Clear")
            .tooltip(&tip("Clear", "clear-tabs")),
    );
    ui.tree.add(
        button,
        Node::glyph(glyph::ARROW_DOWN, 9.0, 11.0)
            .weight(Weight::Semibold)
            .ink(Token::ColumnMuted)
            .role(Role::None),
    );
    ui.tree.add(
        button,
        Node::text("Clear", 11.0)
            .weight(Weight::Medium)
            .ink(Token::ColumnMuted)
            .role(Role::None),
    );
    ui.tree.add(seam, line());
    IDS.with(|ids| {
        let mut ids = ids.borrow_mut();
        ids.seam = Some(seam);
        ids.button = Some(button);
    });
}

/// Bring the seam in line with `clearable` (how many rows Clear would
/// close): its height, its padding, whether "↓ Clear" is there and what it
/// says to a screen reader.
pub fn sync_seam(ui: &mut Ui, seam: NodeId, clearable: usize) {
    let Some(button) = IDS.with(|ids| ids.borrow().button) else {
        return;
    };
    let on = clearable > 0;
    let (height, pad) = if on {
        (CLEARING, CLEARING_PAD)
    } else {
        (PLAIN, side::DIVIDER_PAD)
    };
    if ui
        .tree
        .get(seam)
        .is_some_and(|n| n.style.height != Dim::Px(height))
    {
        ui.tree.edit(seam, |n| {
            n.style.height = Dim::Px(height);
            n.style.padding = Insets::new(side::ROW_INSET, pad, side::ROW_INSET, pad);
        });
    }
    ui.tree.set_hidden(button, !on);
    let name = format!("Clear {clearable} tabs");
    if on && ui.tree.get(button).is_some_and(|n| n.a11y.name != name) {
        ui.tree.edit(button, |n| n.a11y.name = name);
    }
}

/// The seam's height now, for the column's sums.
pub fn seam_height(ui: &Ui, seam: NodeId) -> f32 {
    match ui.tree.get(seam).map(|n| n.style.height) {
        Some(Dim::Px(h)) => h,
        _ => PLAIN,
    }
}

/// Clear Tabs (the palette, Ctrl+Shift+K; Mac `CommandBar` `clear-tabs`),
/// and the key Ctrl+Z runs while Undo is offered.
pub fn commands() -> [Command; 2] {
    [
        Command::new("clear-tabs", "Clear Tabs", glyph::ARROW_DOWN, |s| {
            clear(s);
        })
        .when(|s| !s.clearable_tabs().is_empty()),
        Command::key("undo-clear", |s| {
            undo(s);
        }),
    ]
}

/// Ctrl+Z while Undo is offered.
pub fn takes(shell: &Shell, chord: Chord) -> Option<&'static str> {
    let ctrl_z = chord.key == Key::Letter('Z')
        && chord.mods
            == (Mods {
                ctrl: true,
                shift: false,
                alt: false,
            });
    (ctrl_z && shell.clear_undo_count().is_some()).then_some("undo-clear")
}

/// A click on the seam's "↓ Clear" or the pill's Undo.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let (button, undo) = IDS.with(|ids| {
        let ids = ids.borrow();
        (ids.button, ids.pill.map(|p| p.undo))
    });
    if button == Some(id) {
        clear(shell);
        true
    } else if undo == Some(id) {
        self::undo(shell);
        true
    } else {
        false
    }
}

/// Clear the current space and offer Undo. Returns how many went.
pub fn clear(shell: &Shell) -> usize {
    let n = shell.clear_tabs();
    if n > 0 {
        show_pill(shell, n);
    }
    n
}

/// Undo the last Clear, while offered.
pub fn undo(shell: &Shell) -> usize {
    let n = shell.undo_clear();
    hide_pill(shell);
    n
}

fn show_pill(shell: &Shell, count: usize) {
    let text = cleared_words(count);
    let shown = Rc::new(Cell::new(0));
    let g = shown.clone();
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, st| {
        let Some(pill) = IDS.with(|ids| ids.borrow().pill).or_else(|| {
            let p = build_pill(ui, ui.tree.parent(st.toast.bars())?);
            IDS.with(|ids| ids.borrow_mut().pill = Some(p));
            Some(p)
        }) else {
            return;
        };
        if let Some(parent) = ui.tree.parent(pill.holder) {
            let last = ui.tree.children(parent).len().saturating_sub(1);
            ui.tree.reorder(pill.holder, last);
        }
        ui.tree.set_text(pill.text, &text);
        ui.tree
            .edit(pill.node, |n| n.a11y.name = format!("{text}. Undo"));
        ui.announce(&text);
        let was_hidden = ui.tree.get(pill.node).is_some_and(|n| n.style.hidden);
        ui.tree.set_hidden(pill.node, false);
        if was_hidden {
            ui.tree.set_prop(pill.node, Prop::Y, 12.0);
            ui.tree.set_prop(pill.node, Prop::Opacity, 0.0);
        }
        ui.tree.animate(pill.node, Prop::Y, 0.0, motion::SETTLE);
        ui.tree
            .animate(pill.node, Prop::Opacity, 1.0, motion::QUICK);
        let generation = IDS.with(|ids| {
            let mut ids = ids.borrow_mut();
            ids.generation += 1;
            ids.generation
        });
        g.set(generation);
    });
    let weak = Rc::downgrade(&chrome);
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(UNDO_FOR).await;
        let Some(c) = weak.upgrade() else { return };
        let g = shown.get();
        c.edit(move |ui, _| {
            if IDS.with(|ids| ids.borrow().generation) == g {
                hide(ui);
            }
        });
    });
}

fn hide_pill(shell: &Shell) {
    shell.view().chrome().edit(|ui, _| hide(ui));
}

fn hide(ui: &mut Ui) {
    if let Some(p) = IDS.with(|ids| ids.borrow().pill) {
        ui.tree.set_hidden(p.node, true);
    }
}

/// The pill: the announcement's look, the count in ink and Undo in the
/// agents' copper (`ClearTabsToast`).
fn build_pill(ui: &mut Ui, parent: NodeId) -> Pill {
    // Where the bars stand (`toast::build`), and like them never in the
    // way of a click itself.
    let holder = ui.tree.add(
        parent,
        Node::column()
            .absolute(Abs {
                left: Some(0.0),
                top: None,
                right: Some(0.0),
                bottom: Some(30.0),
            })
            .align(Align::Center)
            .justify(Justify::End),
    );
    let node = ui.tree.add(
        holder,
        Node::row()
            .fill(Token::Ground)
            .stroke(Token::Hairline, 1.0)
            .radius(CAPSULE)
            .shadow(Shadow::new(0.10, 18.0, 6.0))
            .layer()
            .padding(Insets::hv(15.0, 9.0))
            .gap(10.0)
            .align(Align::Center)
            .hidden(true)
            .role(Role::ToolTip),
    );
    let text = ui.tree.add(node, Node::text("", 12.0).ink(Token::Ink));
    let undo = ui.tree.add(
        node,
        Node::row()
            .align(Align::Center)
            .button("Undo")
            .tooltip("Undo (Ctrl+Z)"),
    );
    ui.tree.add(
        undo,
        Node::text("Undo", 12.0)
            .weight(Weight::Semibold)
            .ink(UNDO_INK)
            .role(Role::None),
    );
    Pill {
        holder,
        node,
        text,
        undo,
    }
}
