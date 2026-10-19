//! The bars that rise from the bottom of the window to say one thing
//! (`App.swift` `bars`): the announcement (a toast, gone after long enough
//! to read it) and above it a question waiting for an answer.

use copper_win_ui::tokens::{Shadow, motion};
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Insets, Justify, Node, NodeId, Prop, Role, Token, Weight};
use std::time::Duration;

/// How long an announcement stays: long enough to read, 1.7–6 s
/// (`Fork/Announcements.swift`).
pub fn seconds(text: &str) -> Duration {
    let s = (text.chars().count() as f64 / 15.0).clamp(1.7, 6.0);
    Duration::from_secs_f64(s)
}

/// The pill's look: the page's ground in a capsule, a hairline, a soft
/// shadow (`App.swift` 519–531).
fn pill(n: Node) -> Node {
    n.fill(Token::Ground)
        .stroke(Token::Hairline, 1.0)
        .radius(18.0)
        .shadow(Shadow::new(0.10, 18.0, 6.0))
        .layer()
}

/// The bars and the toast in them.
pub struct Toasts {
    bars: NodeId,
    toast: NodeId,
    text: NodeId,
    /// Bumped by every toast, so an older one's timer leaves a newer alone.
    pub generation: u64,
}

pub fn build(ui: &mut Ui, parent: NodeId) -> Toasts {
    let bars = ui.tree.add(
        parent,
        Node::column()
            .absolute(Abs {
                left: Some(0.0),
                top: None,
                right: Some(0.0),
                bottom: Some(30.0),
            })
            .align(Align::Center)
            .justify(Justify::End)
            .gap(8.0),
    );
    let toast = ui.tree.add(
        bars,
        pill(Node::row())
            .padding(Insets::hv(15.0, 9.0))
            .hidden(true)
            .role(Role::ToolTip),
    );
    let text = ui.tree.add(toast, Node::text("", 12.0).ink(Token::Ink));
    Toasts {
        bars,
        toast,
        text,
        generation: 0,
    }
}

impl Toasts {
    /// The bars' stack: the question sheet goes in it too.
    pub fn bars(&self) -> NodeId {
        self.bars
    }

    /// Show `text`; returns its generation, for [`Toasts::hide`].
    pub fn show(&mut self, ui: &mut Ui, text: &str) -> u64 {
        self.generation += 1;
        ui.tree.set_text(self.text, text);
        let was_hidden = ui.tree.get(self.toast).is_some_and(|n| n.style.hidden);
        ui.tree.set_hidden(self.toast, false);
        if was_hidden {
            ui.tree.set_prop(self.toast, Prop::Y, 12.0);
            ui.tree.set_prop(self.toast, Prop::Opacity, 0.0);
        }
        ui.tree.animate(self.toast, Prop::Y, 0.0, motion::SETTLE);
        ui.tree
            .animate(self.toast, Prop::Opacity, 1.0, motion::QUICK);
        self.generation
    }

    /// Take toast `generation` away (if it is still the one showing).
    pub fn hide(&mut self, ui: &mut Ui, generation: u64) {
        if generation == self.generation {
            ui.tree.set_hidden(self.toast, true);
        }
    }
}

/// The ask bar's look, for [`super::sheet`]: the same pill, with a
/// primary (ink) and a quiet button.
pub fn ask_pill() -> Node {
    Node::row()
        .fill(Token::Ground)
        .stroke(Token::Hairline, 1.0)
        .radius(18.0)
        .shadow(Shadow::new(0.12, 20.0, 6.0))
        .layer()
        .padding(Insets::new(16.0, 9.0, 10.0, 9.0))
        .gap(12.0)
        .align(Align::Center)
}

/// A capsule button: `primary` in ink, else quiet.
pub fn capsule(label: &str, primary: bool) -> (Node, Node) {
    let b = Node::row()
        .padding(Insets::hv(11.0, 5.0))
        .radius(12.0)
        .align(Align::Center)
        .button(label);
    let b = if primary {
        b.fill(Token::Ink)
    } else {
        b.hover_fill(Token::Hover)
    };
    let t = Node::text(label, 12.0)
        .weight(if primary {
            Weight::Medium
        } else {
            Weight::Regular
        })
        .ink(if primary { Token::Ground } else { Token::Muted })
        .role(Role::None);
    (b, t)
}
