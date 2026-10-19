//! A question for the user (`ShellView::ask`): a bar rising from the foot
//! of the window, the site's question in its own words and two answers
//! (`App.swift` `captureAsking`). One at a time; more wait their turn.
//!
//! The answers are kept here and handed out when a button is pressed, but
//! never called from here: the caller ([`super::events`]) takes the answer
//! out and calls it once the window is free, so an answer may reach back
//! into the shell (and the shell back into the view) freely.

use super::toast::{ask_pill, capsule};
use copper_shell_core::Question;
use copper_win_ui::win::Ui;
use copper_win_ui::{Node, NodeId, Role, Token, Trim};
use std::collections::VecDeque;

/// What answering does.
pub type Answer = Box<dyn FnOnce(bool)>;

/// The question on show and those waiting.
pub struct Sheet {
    parent: NodeId,
    bar: Option<Bar>,
    waiting: VecDeque<(u64, Question, Answer)>,
    next: u64,
}

struct Bar {
    ticket: u64,
    node: NodeId,
    allow: NodeId,
    deny: NodeId,
    answer: Answer,
}

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
        self.next += 1;
        let ticket = self.next;
        if self.bar.is_some() {
            self.waiting.push_back((ticket, q, answer));
        } else {
            self.show(ui, ticket, q, answer);
        }
        ticket
    }

    /// Take question `ticket` back unanswered (its answer is dropped).
    pub fn withdraw(&mut self, ui: &mut Ui, ticket: u64) {
        if self.bar.as_ref().is_some_and(|b| b.ticket == ticket) {
            drop(self.dismiss(ui));
        } else {
            self.waiting.retain(|(t, _, _)| *t != ticket);
        }
    }

    pub fn is_open(&self) -> bool {
        self.bar.is_some()
    }

    fn show(&mut self, ui: &mut Ui, ticket: u64, q: Question, answer: Answer) {
        let node = ui
            .tree
            .insert(self.parent, 0, ask_pill().role(Role::Group).name(&q.title));
        let words = ui
            .tree
            .add(node, Node::column().shrink(1.0).max_width(520.0));
        ui.tree.add(
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
        ui.focus(Some(allow));
        self.bar = Some(Bar {
            ticket,
            node,
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
        if let Some((t, q, a)) = self.waiting.pop_front() {
            self.show(ui, t, q, a);
        }
        Some(bar.answer)
    }
}
