//! The account list under a sign-in or code box — or the vaults'
//! identities and cards under an address or card box (`Accounts.swift`): it
//! hangs 6 below the box's left edge, as wide as the box within 240–360,
//! six 44-high rows and then a scroll, and a footer saying where the rows
//! come from. It follows the box as the page scrolls or zooms, turns up
//! above it near the foot of the page and moves in from the sides rather
//! than be cut (`settings::fill::place`).
//!
//! The rows take no keyboard focus — the caret stays in the page's box —
//! and hovering one does nothing but light it. A click fetches that row's
//! password or code (only then) and fills it; the row says `Fetching…`
//! meanwhile and a second click does nothing.

use super::WinForms;
use crate::chrome::{Chrome, glyph};
use crate::platform::Shell;
use crate::settings::fill::{self, Pending, Rect};
use copper_core::browser::TabId;
use copper_creds::Source;
use copper_pagejs::messages::FieldRect;
use copper_shell_core::forms;
use copper_shell_core::forms::view::{PickerView, Row, RowKind};
use copper_win_ui::tokens::Shadow;
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// The account list on show.
pub struct Picker {
    ticket: u64,
    users: Vec<String>,
    busy: Rc<Cell<bool>>,
}

/// The list's nodes and rows.
struct Shown {
    ticket: u64,
    tab: TabId,
    rows: Vec<Row>,
    popup: NodeId,
    buttons: Vec<NodeId>,
    /// Each row's `Fetching…`, shown once it is clicked.
    fetching: Vec<NodeId>,
    busy: Rc<Cell<bool>>,
    chrome: Weak<Chrome>,
}

thread_local! {
    static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
    static TICKETS: Cell<u64> = const { Cell::new(0) };
}

impl PickerView for Picker {
    fn remove(&self) {
        let shown = SHOWN.with(|s| {
            let mut s = s.borrow_mut();
            if s.as_ref().is_some_and(|x| x.ticket == self.ticket) {
                s.take()
            } else {
                None
            }
        });
        if let Some(s) = shown
            && let Some(c) = s.chrome.upgrade()
        {
            let popup = s.popup;
            c.edit(move |ui, _| ui.tree.remove(popup));
        }
    }

    fn place(&self, rect: FieldRect, zoom: f64) {
        let Some((tab, popup, n, chrome)) = SHOWN.with(|s| {
            let s = s.borrow();
            let s = s.as_ref().filter(|x| x.ticket == self.ticket)?;
            Some((s.tab.clone(), s.popup, s.rows.len(), s.chrome.upgrade()?))
        }) else {
            return;
        };
        chrome.edit(move |ui, st| {
            let Some(page) = st.stage.slot_of(&tab).map(|s| ui.tree.window_rect(s)) else {
                return;
            };
            let at = spot(page, rect, zoom, n);
            ui.tree.edit(popup, |p| p.style.absolute = Some(abs(at)));
        });
    }

    fn busy(&self) -> bool {
        self.busy.get()
    }

    fn users(&self) -> Vec<String> {
        self.users.clone()
    }
}

/// Where the list goes for the box at `rect` (page pixels at `zoom`) on
/// the page drawn at `page` (window pixels).
fn spot(page: copper_win_ui::Rect, rect: FieldRect, zoom: f64, rows: usize) -> Rect {
    let z = zoom as f32;
    let field = Rect {
        x: page.x + rect.x as f32 * z,
        y: page.y + rect.y as f32 * z,
        w: rect.w as f32 * z,
        h: rect.h as f32 * z,
    };
    let page = Rect {
        x: page.x,
        y: page.y,
        w: page.w,
        h: page.h,
    };
    fill::place(field, page, rows)
}

fn abs(at: Rect) -> Abs {
    Abs {
        left: Some(at.x),
        top: Some(at.y),
        right: None,
        bottom: None,
    }
}

/// Segoe Fluent Icons `ContactInfo` and `PaymentCard` (the Mac's
/// `person.text.rectangle` and `creditcard`).
const CONTACT: char = '\u{E779}';
const CARD: char = '\u{E8C7}';

/// A row's mark: where it is from, that it is a code, an identity or a card.
fn mark(r: &Row) -> char {
    match (r.kind, r.credential.source) {
        (RowKind::Code, _) => glyph::DIALPAD,
        (RowKind::Identity, _) => CONTACT,
        (RowKind::Card, _) => CARD,
        (_, Source::Keychain) => glyph::KEY,
        (_, Source::Bitwarden) => glyph::SHIELD,
        (_, Source::OnePassword) => glyph::LOCK,
        (_, Source::BotPass) => glyph::KEY,
    }
}

/// Put the list up under the box at `rect` on `tab`'s page.
pub fn show(shell: &Shell, tab: &TabId, rect: FieldRect, zoom: f64, rows: Vec<Row>) -> Picker {
    let ticket = TICKETS.with(|t| {
        t.set(t.get() + 1);
        t.get()
    });
    let users: Vec<String> = rows.iter().map(|r| r.credential.user.clone()).collect();
    let pending = forms::vault()
        .map(|v| {
            let s = v.snapshot();
            Pending {
                bitwarden: s.bw_locked(),
                onepassword: s.op_locked(),
            }
        })
        .unwrap_or_default();
    let footer = fill::footer(&rows, pending);
    let busy = Rc::new(Cell::new(false));
    let chrome = shell.view().chrome().clone();
    let weak = Rc::downgrade(&chrome);
    let (tab, b) = (tab.clone(), busy.clone());
    chrome.edit(move |ui, st| {
        let Some(page) = st.stage.slot_of(&tab).map(|s| ui.tree.window_rect(s)) else {
            tracing::debug!("forms: the picker's tab has no page on show");
            return;
        };
        let at = spot(page, rect, zoom, rows.len());
        tracing::debug!(rows = rows.len(), ?page, ?at, zoom, "forms: picker up");
        let (popup, buttons, fetching) = build(ui, at, &rows, &footer);
        let old = SHOWN.with(|s| {
            s.borrow_mut().replace(Shown {
                ticket,
                tab,
                rows,
                popup,
                buttons,
                fetching,
                busy: b,
                chrome: weak,
            })
        });
        if let Some(old) = old {
            ui.tree.remove(old.popup);
        }
    });
    Picker {
        ticket,
        users,
        busy,
    }
}

/// The list's nodes: the popup, each row's button and `Fetching…`.
fn build(ui: &mut Ui, at: Rect, rows: &[Row], footer: &str) -> (NodeId, Vec<NodeId>, Vec<NodeId>) {
    let root = ui.content();
    let popup = ui.tree.add(
        root,
        Node::column()
            .absolute(abs(at))
            .width(at.w)
            .fill(Token::Ground)
            .radius(10.0)
            .stroke(Token::Hairline, 1.0)
            .shadow(Shadow::new(0.14, 22.0, 8.0))
            .layer()
            .clip()
            .hit()
            .role(Role::List)
            .name(match rows.first().map(|r| r.kind) {
                Some(RowKind::Identity) => "Addresses",
                Some(RowKind::Card) => "Cards",
                _ => "Accounts",
            }),
    );
    let list = if rows.len() > fill::ROWS_SHOWN {
        ui.tree
            .add(popup, Node::scroll().max_height(fill::LIST_MAX))
    } else {
        ui.tree.add(popup, Node::column())
    };
    let mut buttons = Vec::with_capacity(rows.len());
    let mut fetching = Vec::with_capacity(rows.len());
    for r in rows {
        let (user, under) = fill::row_lines(r);
        let row = ui.tree.add(
            list,
            Node::row()
                .height(fill::ROW_H)
                .shrink(0.0)
                .padding(Insets::hv(10.0, 7.0))
                .gap(10.0)
                .align(Align::Center)
                .hover_fill(Token::Hover)
                .press_fill(Token::Hover)
                // Clickable, never focused: the caret stays in the page.
                .hit()
                .role(Role::ListItem)
                .name(&format!("{user}, {under}")),
        );
        let tile = ui.tree.add(
            row,
            Node::stack()
                .size(22.0, 22.0)
                .shrink(0.0)
                .radius(6.0)
                .fill(Token::Wash)
                .align(Align::Center)
                .justify(Justify::Center)
                .role(Role::None),
        );
        ui.tree.add(
            tile,
            Node::glyph(mark(r), 11.0, 22.0)
                .ink(Token::Muted)
                .role(Role::None),
        );
        let words = ui.tree.add(
            row,
            Node::column()
                .gap(1.0)
                .grow(1.0)
                .shrink(1.0)
                .role(Role::None),
        );
        ui.tree.add(
            words,
            Node::text(&user, 12.5)
                .ink(Token::Ink)
                .trim(Trim::End)
                .role(Role::None),
        );
        ui.tree.add(
            words,
            Node::text(&under, 10.5)
                .ink(Token::Muted)
                .trim(Trim::End)
                .role(Role::None),
        );
        let f = ui.tree.add(
            row,
            Node::text("Fetching…", 10.5)
                .ink(Token::Muted)
                .shrink(0.0)
                .hidden(true)
                .role(Role::None),
        );
        buttons.push(row);
        fetching.push(f);
    }
    let foot = ui.tree.add(
        popup,
        Node::row()
            .height(fill::FOOT_H)
            .shrink(0.0)
            .padding(Insets::hv(12.0, 7.0))
            .gap(6.0)
            .align(Align::Center)
            .fill(Swatch::Fade(Token::Wash, 0.5))
            .role(Role::None),
    );
    ui.tree.add(
        foot,
        Node::glyph(glyph::KEY, 9.0, 12.0)
            .ink(Token::Faint)
            .role(Role::None),
    );
    ui.tree.add(
        foot,
        Node::text(footer, 10.5)
            .ink(Token::Faint)
            .trim(Trim::End)
            .shrink(1.0)
            .role(Role::Text),
    );
    (popup, buttons, fetching)
}

/// A click on a row: fill it. A click on the list between rows is the
/// list's too, and does nothing.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let row = SHOWN.with(|s| {
        let s = s.borrow();
        let s = s.as_ref()?;
        match s.buttons.iter().position(|b| *b == id) {
            Some(i) => Some(Some(i)),
            None => (s.popup == id).then_some(None),
        }
    });
    match row {
        None => false,
        Some(None) => true,
        Some(Some(i)) => {
            pick_at(i);
            shell.view().focus_page();
            true
        }
    }
}

/// Fill row `i` of the list on show, once.
pub fn pick_at(i: usize) {
    let Some((tab, row, f, chrome)) = SHOWN.with(|s| {
        let s = s.borrow();
        let s = s.as_ref()?;
        if s.busy.get() {
            return None;
        }
        let row = s.rows.get(i)?.clone();
        s.busy.set(true);
        Some((
            s.tab.clone(),
            row,
            s.fetching.get(i).copied(),
            s.chrome.upgrade(),
        ))
    }) else {
        return;
    };
    if let (Some(f), Some(c)) = (f, chrome) {
        c.edit(move |ui, _| ui.tree.set_hidden(f, false));
    }
    if let Some(forms) = forms::get::<WinForms>() {
        forms.pick(&tab, row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use copper_creds::credential::{AutofillCard, AutofillIdentity};

    #[test]
    fn identity_and_card_rows_look_like_the_mac() {
        let who = Row::identity(&AutofillIdentity {
            id: "bw-1".into(),
            name: "Home".into(),
            first_name: "Ana".into(),
            city: "Springfield".into(),
            ..Default::default()
        });
        let card = Row::card(&AutofillCard {
            id: "op:2".into(),
            name: "Travel".into(),
            brand: "Visa".into(),
            number: "4111111111111234".into(),
            cardholder_name: "Ana Lima".into(),
            ..Default::default()
        });
        assert_eq!(mark(&who), CONTACT);
        assert_eq!(mark(&card), CARD);
        assert_eq!(
            fill::row_lines(&who),
            ("Ana".to_string(), "Springfield".to_string())
        );
        assert_eq!(
            fill::row_lines(&card),
            ("Visa •••• 1234".to_string(), "Ana Lima".to_string())
        );
        let none = Pending::default();
        assert_eq!(
            fill::footer(std::slice::from_ref(&who), none),
            "From Bitwarden"
        );
        assert_eq!(
            fill::footer(&[who, card], none),
            "From Bitwarden and 1Password"
        );
    }
}
