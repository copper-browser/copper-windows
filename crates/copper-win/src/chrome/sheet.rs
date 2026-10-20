//! A question for the user (`ShellView::ask`, a site's permission): a bar
//! rising from the foot of the window, an optional mark, the question in
//! its own words and two answers (`App.swift` `captureAsking`). One at a
//! time; more wait their turn. Escape refuses; focus starts on the
//! refusal, so a stray Enter never grants anything.
//!
//! The answers are kept here and handed out when a button is pressed, but
//! never called from here: the caller ([`super::events`]) takes the answer
//! out and calls it once the window is free, so an answer may reach back
//! into the shell (and the shell back into the view) freely.

use super::toast::{ask_pill, capsule};
use copper_shell_core::Question;
use copper_win_ui::tokens::motion;
use copper_win_ui::win::Ui;
use copper_win_ui::{Node, NodeId, Prop, Role, Token, Trim, Weight};
use std::collections::VecDeque;

/// What answering does.
pub type Answer = Box<dyn FnOnce(bool)>;

/// The question on show and those waiting.
pub struct Sheet {
    parent: NodeId,
    bar: Option<Bar>,
    waiting: VecDeque<Waiting>,
    next: u64,
}

/// A question not yet on show.
struct Waiting {
    ticket: u64,
    q: Question,
    mark: Option<char>,
    answer: Answer,
}

struct Bar {
    ticket: u64,
    node: NodeId,
    mark: Option<NodeId>,
    title: NodeId,
    allow: NodeId,
    deny: NodeId,
    answer: Answer,
}

/// The mark's size (`Image(systemName:)` 11 pt medium, muted).
const MARK: f32 = 11.0;

pub fn build(_ui: &mut Ui, parent: NodeId) -> Sheet {
    Sheet {
        parent,
        bar: None,
        waiting: VecDeque::new(),
        next: 0,
    }
}

impl Sheet {
    /// Ask `q` (now, or after the questions already asked). The ticket
    /// takes it back ([`Sheet::withdraw`]).
    pub fn ask(&mut self, ui: &mut Ui, q: Question, answer: Answer) -> u64 {
        self.ask_marked(ui, q, None, answer)
    }

    /// [`Sheet::ask`], with a glyph before the words (a site's camera,
    /// microphone, location or notifications).
    pub fn ask_marked(
        &mut self,
        ui: &mut Ui,
        q: Question,
        mark: Option<char>,
        answer: Answer,
    ) -> u64 {
        self.next += 1;
        let ticket = self.next;
        let w = Waiting {
            ticket,
            q,
            mark,
            answer,
        };
        if self.bar.is_some() {
            self.waiting.push_back(w);
        } else {
            self.show(ui, w);
        }
        ticket
    }

    /// Reword question `ticket` (a site asking for the camera, then the
    /// microphone too, is asked about both at once).
    pub fn retitle(&mut self, ui: &mut Ui, ticket: u64, title: &str, mark: Option<char>) {
        if let Some(b) = self.bar.as_ref().filter(|b| b.ticket == ticket) {
            ui.tree.set_text(b.title, title);
            if let (Some(m), Some(ch)) = (b.mark, mark) {
                ui.tree.set_text(m, &ch.to_string());
            }
            let name = title.to_string();
            ui.tree.edit(b.node, |n| n.a11y.name = name);
        } else if let Some(w) = self.waiting.iter_mut().find(|w| w.ticket == ticket) {
            w.q.title = title.to_string();
            w.mark = mark.or(w.mark);
        }
    }

    /// Take question `ticket` back unanswered (its answer is dropped).
    pub fn withdraw(&mut self, ui: &mut Ui, ticket: u64) {
        if self.bar.as_ref().is_some_and(|b| b.ticket == ticket) {
            drop(self.dismiss(ui));
        } else {
            self.waiting.retain(|w| w.ticket != ticket);
        }
    }

    pub fn is_open(&self) -> bool {
        self.bar.is_some()
    }

    fn show(&mut self, ui: &mut Ui, w: Waiting) {
        let Waiting {
            ticket,
            q,
            mark,
            answer,
        } = w;
        let node = ui
            .tree
            .insert(self.parent, 0, ask_pill().role(Role::Group).name(&q.title));
        let mark = mark.map(|ch| {
            ui.tree.add(
                node,
                Node::glyph(ch, MARK, MARK + 4.0)
                    .weight(Weight::Medium)
                    .ink(Token::Muted)
                    .role(Role::None),
            )
        });
        let words = ui
            .tree
            .add(node, Node::column().shrink(1.0).max_width(520.0));
        let title = ui.tree.add(
            words,
            Node::text(&q.title, 12.5).ink(Token::Ink).trim(Trim::Wrap),
        );
        if let Some(d) = q.detail.as_deref().filter(|d| !d.is_empty()) {
            ui.tree.add(
                words,
                Node::text(d, 11.5).ink(Token::Muted).trim(Trim::Wrap),
            );
        }
        let (b, t) = capsule(&q.allow, true);
        let allow = ui.tree.add(node, b);
        ui.tree.add(allow, t);
        let (b, t) = capsule(&q.deny, false);
        let deny = ui.tree.add(node, b);
        ui.tree.add(deny, t);
        // Up from the foot, fading in (`.move(edge: .bottom)` + opacity,
        // `Motion.settle`).
        ui.tree.set_prop(node, Prop::Y, 12.0);
        ui.tree.set_prop(node, Prop::Opacity, 0.0);
        ui.tree.animate(node, Prop::Y, 0.0, motion::SETTLE);
        ui.tree.animate(node, Prop::Opacity, 1.0, motion::QUICK);
        ui.focus(Some(deny));
        self.bar = Some(Bar {
            ticket,
            node,
            mark,
            title,
            allow,
            deny,
            answer,
        });
    }

    /// A click on `id`: if it answers the question, take the bar down (and
    /// put up the next) and return the answer to call with its value.
    pub fn answered(&mut self, ui: &mut Ui, id: NodeId) -> Option<(Answer, bool)> {
        let yes = match &self.bar {
            Some(b) if b.allow == id => true,
            Some(b) if b.deny == id => false,
            _ => return None,
        };
        self.dismiss(ui).map(|a| (a, yes))
    }

    /// Escape: the question is refused. Returns the answer to call with
    /// `false`.
    pub fn refuse(&mut self, ui: &mut Ui) -> Option<Answer> {
        self.dismiss(ui)
    }

    fn dismiss(&mut self, ui: &mut Ui) -> Option<Answer> {
        let bar = self.bar.take()?;
        ui.tree.remove(bar.node);
        if let Some(w) = self.waiting.pop_front() {
            self.show(ui, w);
        }
        Some(bar.answer)
    }
}
