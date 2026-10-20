//! The pills at the window's foot (`CanvasInvitePill`, `CanvasMentionPill`):
//! "<who> invited you to “<name>”  Open  ×" and "<who> mentioned you in
//! “<canvas>”  Open  ×", the mention above the invite, both above the
//! toast — in the chrome's bars, over the panels.

use super::copy::{self, Mention};
use super::{inbox, look, ops};
use crate::chrome::glyph;
use crate::chrome::toast::{ask_pill, capsule};
use crate::platform::{Shell, Win};
use copper_cloud::canvas::Invite;
use copper_shell_core::canvas::{self as core, chatprefs, host, registry};
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Justify, Node, NodeId, Role, Token, Trim, Weight};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Act {
    Join(String),
    NotNowInvite,
    Open(String),
    NotNowMention,
}

#[derive(Default)]
struct Pills {
    invite: Option<Invite>,
    mention: Option<Mention>,
    joining: bool,
    opening: bool,
    stack: Option<NodeId>,
    hits: HashMap<NodeId, Act>,
}

thread_local! {
    static PILLS: RefCell<Pills> = RefCell::new(Pills::default());
}

fn with<R>(f: impl FnOnce(&mut Pills) -> R) -> R {
    PILLS.with(|p| f(&mut p.borrow_mut()))
}

/// The invites were read: `fresh` (new or reminded) comes up; one
/// withdrawn or answered elsewhere goes.
pub fn sync(shell: &Shell, fresh: Option<&Invite>) {
    let now = inbox::invites();
    with(|p| {
        p.joining = false;
        if let Some(f) = fresh {
            p.invite = Some(f.clone());
        } else if let Some(shown) = &p.invite {
            p.invite = now.iter().find(|i| i.id == shown.id).cloned();
        }
    });
    draw(shell);
}

/// A new mention of you comes up (the newest wins).
pub fn mention(shell: &Shell, m: &Mention) {
    with(|p| p.mention = Some(m.clone()));
    draw(shell);
}

/// Mentions read: their pill goes.
pub fn mentions_read(shell: &Shell, ids: &[String]) {
    let gone = with(|p| {
        let gone = p.mention.as_ref().is_some_and(|m| ids.contains(&m.id));
        if gone {
            p.mention = None;
        }
        gone
    });
    if gone {
        draw(shell);
    }
}

/// Another account: nothing of the old one's stays up.
pub fn clear(shell: &Shell) {
    with(|p| {
        p.invite = None;
        p.mention = None;
    });
    draw(shell);
}

/// A click on a pill. `true` if it was one of theirs.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let Some(act) = with(|p| p.hits.get(&id).cloned()) else {
        return false;
    };
    match act {
        Act::Join(invite) => {
            if with(|p| std::mem::replace(&mut p.joining, true)) {
                return true;
            }
            draw(shell);
            // The answer takes the pill down (or, failed, gives it back).
            ops::answer(shell, &invite, true);
        }
        Act::NotNowInvite => {
            with(|p| p.invite = None);
            draw(shell);
        }
        Act::Open(mention) => {
            let m = with(|p| p.mention.clone()).filter(|m| m.id == mention);
            if let Some(m) = m {
                open_mention(shell, &m);
            }
        }
        Act::NotNowMention => {
            with(|p| p.mention = None);
            draw(shell);
        }
    }
    true
}

/// Open the canvas a mention is in, at the message (an assignment: at its
/// card), and mark it read (`CanvasChat.open`).
pub fn open_mention(shell: &Shell, m: &Mention) {
    if with(|p| std::mem::replace(&mut p.opening, true)) {
        return;
    }
    with(|p| {
        if p.mention.as_ref().is_some_and(|x| x.id == m.id) {
            p.mention = None;
        }
    });
    draw(shell);
    super::raise(shell);
    let Some(entry) = registry::find(&m.canvas_id) else {
        with(|p| p.opening = false);
        inbox::mark_read(shell, std::slice::from_ref(&m.id), None);
        shell.toast("That canvas isn't yours to open any more.");
        return;
    };
    if !m.is_assignment()
        && let Some(account) = entry.account.clone()
    {
        chatprefs::update(&account, &entry.id, |p| p.open = Some(true));
    }
    let tab = core::show::<Win>(shell, &entry.id, true);
    let weak = shell.downgrade();
    let m = m.clone();
    copper_win_rt::spawn_local(async move {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut found = None;
        while Instant::now() < deadline {
            if let Some(h) = host::for_tab::<Win>(&tab).filter(|h| h.is_ready()) {
                found = Some(h);
                break;
            }
            copper_win_rt::sleep(Duration::from_millis(60)).await;
        }
        with(|p| p.opening = false);
        let Some(shell) = weak.upgrade() else { return };
        inbox::mark_read(&shell, std::slice::from_ref(&m.id), None);
        let Some(h) = found else {
            shell.toast("The canvas didn't open in time.");
            return;
        };
        let _ = h
            .run(&copper_pagejs::canvas::reveal_chat(&m.message_id))
            .await;
    });
}

// MARK: drawing

fn draw(shell: &Shell) {
    let (invite, mention, joining) = with(|p| (p.invite.clone(), p.mention.clone(), p.joining));
    shell.view().chrome().edit(move |ui, st| {
        let bars = st.toast.bars();
        PILLS.with(|p| {
            let mut p = p.borrow_mut();
            let stack = match p.stack.filter(|s| ui.tree.contains(*s)) {
                Some(s) => {
                    ui.tree.clear_children(s);
                    s
                }
                None => {
                    let s = ui.tree.insert(
                        bars,
                        0,
                        Node::column()
                            .gap(8.0)
                            .align(Align::Center)
                            .role(Role::None),
                    );
                    p.stack = Some(s);
                    s
                }
            };
            p.hits.clear();
            if let Some(m) = &mention {
                mention_pill(ui, stack, m, &mut p.hits);
            }
            if let Some(i) = &invite {
                invite_pill(ui, stack, i, joining, &mut p.hits);
            }
            ui.tree
                .set_hidden(stack, mention.is_none() && invite.is_none());
        });
    });
}

fn not_now(ui: &mut Ui, pill: NodeId, help: &str) -> NodeId {
    let x = ui.tree.add(
        pill,
        Node::stack()
            .size(18.0, 18.0)
            .shrink(0.0)
            .radius(9.0)
            .hover_fill(Token::Hover)
            .align(Align::Center)
            .justify(Justify::Center)
            .button("Not now")
            .tooltip(help),
    );
    ui.tree.add(
        x,
        Node::glyph(glyph::CLOSE, 9.0, 18.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    x
}

fn invite_pill(
    ui: &mut Ui,
    stack: NodeId,
    i: &Invite,
    joining: bool,
    hits: &mut HashMap<NodeId, Act>,
) {
    let from = inbox::sender(i);
    let canvas = i.canvas_name.clone().unwrap_or_else(|| "a canvas".into());
    let pill = ui.tree.add(
        stack,
        ask_pill()
            .gap(10.0)
            .role(Role::Group)
            .name(&if from.is_empty() {
                format!("Invitation to {canvas}")
            } else {
                format!("Invitation to {canvas} from {from}")
            }),
    );
    ui.tree.add(
        pill,
        Node::glyph(glyph::PERSON_ADD, 12.0, 16.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    ui.tree.add(
        pill,
        Node::text(
            &copy::invite_pill(&from, &canvas, i.nudged_at.is_some()),
            12.5,
        )
        .ink(Token::Ink)
        .trim(Trim::Middle)
        .max_width(420.0),
    );
    if joining {
        ui.tree.add(pill, Node::text("…", 12.0).ink(Token::Muted));
    }
    let (b, t) = capsule("Open", true);
    let mut b = b.tooltip(&format!("Join “{canvas}” and open it"));
    if joining {
        b.flags.disabled = true;
    }
    let open = ui.tree.add(pill, b);
    ui.tree.add(open, t);
    hits.insert(open, Act::Join(i.id.clone()));
    let x = not_now(
        ui,
        pill,
        "Not now — it stays under Invitations in the Canvas card",
    );
    hits.insert(x, Act::NotNowInvite);
}

fn mention_pill(ui: &mut Ui, stack: NodeId, m: &Mention, hits: &mut HashMap<NodeId, Act>) {
    let said = if m.excerpt.is_empty() {
        format!("{} {} in {}", m.who(), m.verb(), m.canvas_name)
    } else {
        format!(
            "{} {} in {}: {}",
            m.who(),
            m.verb(),
            m.canvas_name,
            m.excerpt
        )
    };
    let pill = ui.tree.add(
        stack,
        ask_pill()
            .padding(Insets::new(9.0, 8.0, 10.0, 8.0))
            .gap(10.0)
            .role(Role::Group)
            .name(&said),
    );
    let seed = if m.from_id.is_empty() {
        m.who()
    } else {
        m.from_id.as_str()
    };
    let face = look::face(
        ui,
        pill,
        m.who(),
        copy::hex(&core::stable_color(seed)),
        22.0,
    );
    ui.tree.edit(face, |n| n.a11y.role = Role::None);
    let words = ui.tree.add(
        pill,
        Node::column()
            .gap(1.0)
            .max_width(380.0)
            .shrink(1.0)
            .role(Role::None),
    );
    let line = ui.tree.add(words, Node::row().role(Role::None));
    ui.tree.add(
        line,
        Node::text(m.who(), 12.5)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .shrink(0.0)
            .role(Role::None),
    );
    ui.tree.add(
        line,
        Node::text(&m.line(), 12.5)
            .ink(Token::Ink)
            .trim(Trim::Middle)
            .shrink(1.0)
            .role(Role::None),
    );
    if !m.excerpt.is_empty() {
        ui.tree.add(
            words,
            Node::text(&m.excerpt, 11.5)
                .ink(Token::Muted)
                .role(Role::None),
        );
    }
    let (b, t) = capsule("Open", true);
    let b = b.tooltip(&format!(
        "Open “{}” at {}",
        m.canvas_name,
        if m.is_assignment() {
            "the checklist"
        } else {
            "the message"
        }
    ));
    let open = ui.tree.add(pill, b);
    ui.tree.add(open, t);
    hits.insert(open, Act::Open(m.id.clone()));
    let x = not_now(ui, pill, "Not now — the canvas keeps it until you read it");
    hits.insert(x, Act::NotNowMention);
}
