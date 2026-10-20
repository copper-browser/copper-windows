//! The saved-password list's rows (`Passwords.swift` `Site`, `Account`):
//! a site with a letter mark, its account count or its one account, and a
//! chevron; opened, each account with its password as dots and Show/Hide,
//! Copy and Remove — under the pointer, or while its password is shown.

use super::Hit;
use crate::chrome::glyph;
use crate::settings::sites::{self, Site};
use copper_creds::Login;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Color, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim};

/// A shown password: whose, and what it is.
pub struct Shown<'a> {
    pub host: &'a str,
    pub user: &'a str,
    pub secret: &'a str,
}

/// Remove's red (`.red.opacity(0.75)`).
const RED: Color = Color::rgba(0.85, 0.2, 0.2, 0.75);

/// One site, and its accounts when `open`.
pub fn site(
    ui: &mut Ui,
    card: NodeId,
    s: &Site,
    open: bool,
    shown: Option<&Shown>,
    hits: &mut Vec<(NodeId, Hit)>,
) {
    let mut head = Node::row()
        .padding(Insets::hv(14.0, 10.0))
        .gap(10.0)
        .align(Align::Center)
        .hover_fill(Token::Hover)
        .button(&s.host);
    if open {
        head = head.fill(Swatch::Fade(Token::Wash, 0.6));
    }
    let head = ui.tree.add(card, head);
    hits.push((head, Hit::Toggle(s.host.clone())));
    let letter = s
        .host
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "•".into());
    let mark = ui.tree.add(
        head,
        Node::stack()
            .size(16.0, 16.0)
            .radius(4.0)
            .fill(Token::Wash)
            .align(Align::Center)
            .justify(Justify::Center)
            .shrink(0.0)
            .role(Role::None),
    );
    ui.tree.add(
        mark,
        Node::text(&letter, 9.0).ink(Token::Muted).role(Role::None),
    );
    ui.tree.add(
        head,
        Node::text(&s.host, 13.0)
            .ink(Token::Ink)
            .trim(Trim::End)
            .shrink(1.0)
            .role(Role::None),
    );
    let aside = if s.logins.len() > 1 {
        sites::accounts(s.logins.len())
    } else if !open {
        s.logins.first().map(|l| l.user.clone()).unwrap_or_default()
    } else {
        String::new()
    };
    if !aside.is_empty() {
        ui.tree.add(
            head,
            Node::text(&aside, 11.5)
                .ink(Token::Muted)
                .trim(Trim::Middle)
                .shrink(1.0)
                .role(Role::None),
        );
    }
    ui.tree.add(head, Node::new().grow(1.0).role(Role::None));
    ui.tree.add(
        head,
        Node::glyph(
            if open {
                glyph::CHEVRON_DOWN
            } else {
                glyph::CHEVRON_RIGHT
            },
            9.0,
            12.0,
        )
        .ink(Token::Muted)
        .role(Role::None),
    );
    if !open {
        return;
    }
    let under = ui.tree.add(
        card,
        Node::column().padding(Insets::new(30.0, 4.0, 6.0, 4.0)),
    );
    for l in &s.logins {
        let mine = shown.filter(|x| x.host == l.host && x.user == l.user);
        account(ui, under, l, mine.map(|x| x.secret), hits);
    }
}

/// One account: the name, the password as dots (or shown), and what to
/// do with it.
fn account(
    ui: &mut Ui,
    parent: NodeId,
    l: &Login,
    secret: Option<&str>,
    hits: &mut Vec<(NodeId, Hit)>,
) {
    let row = ui.tree.add(
        parent,
        Node::row()
            .padding(Insets::hv(10.0, 7.0))
            .gap(10.0)
            .radius(8.0)
            .align(Align::Center)
            .hover_fill(Token::Hover),
    );
    let named = !l.user.is_empty();
    ui.tree.add(
        row,
        Node::text(if named { &l.user } else { "No username" }, 12.0)
            .ink(if named { Token::Ink } else { Token::Faint })
            .trim(Trim::Middle)
            .min_width(120.0)
            .shrink(1.0),
    );
    let dots = "\u{2022}".repeat(8);
    let (text, size) = match secret {
        Some(s) => (s, 12.5),
        None => (dots.as_str(), 10.0),
    };
    ui.tree.add(
        row,
        Node::text(text, size)
            .ink(if secret.is_some() {
                Token::Ink
            } else {
                Token::Muted
            })
            .trim(Trim::End)
            .shrink(1.0)
            .name(if secret.is_some() {
                "Password"
            } else {
                "Password, hidden"
            }),
    );
    ui.tree.add(row, Node::new().grow(1.0).role(Role::None));
    let mut acts = Node::row().gap(4.0).align(Align::Center).shrink(0.0);
    if secret.is_none() {
        // Under the pointer only, as on the Mac; a shown password keeps
        // its Hide in sight.
        acts = acts.reveal();
    }
    let acts = ui.tree.add(row, acts);
    let shown = secret.is_some();
    for (label, hit, red) in [
        (
            if shown { "Hide" } else { "Show" },
            if shown {
                Hit::Hide
            } else {
                Hit::Show(l.clone())
            },
            false,
        ),
        ("Copy", Hit::Copy(l.clone()), false),
        ("Remove", Hit::Remove(l.clone()), true),
    ] {
        let b = quick(ui, acts, label, red, &l.host);
        hits.push((b, hit));
    }
}

/// A small text button (`Quick`).
fn quick(ui: &mut Ui, parent: NodeId, label: &str, red: bool, host: &str) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::row()
            .height(22.0)
            .padding(Insets::hv(7.0, 0.0))
            .radius(6.0)
            .align(Align::Center)
            .hover_fill(Token::Hover)
            .press_fill(Token::Hover)
            .button(&format!("{label} — {host}")),
    );
    ui.tree.add(
        b,
        Node::text(label, 11.5)
            .ink(if red {
                Swatch::Fixed(RED)
            } else {
                Swatch::Token(Token::Ink)
            })
            .role(Role::None),
    );
    b
}

/// A line of words in the list's card where rows would be (reading,
/// nothing kept, nothing found, an error), inset like the rows.
pub fn note(ui: &mut Ui, card: NodeId, text: &str) -> NodeId {
    let row = ui.tree.add(
        card,
        Node::row().padding(Insets::hv(14.0, 16.0)).role(Role::None),
    );
    ui.tree.add(
        row,
        Node::text(text, 12.5)
            .ink(Token::Muted)
            .trim(Trim::Wrap)
            .grow(1.0)
            .shrink(1.0)
            .role(Role::Text),
    );
    row
}
