//! The Canvas door at the column's foot and its card (`CanvasDoor`,
//! `CanvasPopover`): one 320-DIP card in the window clicked, over a
//! see-through layer that closes it. The list (Personal, Invitations, My
//! canvases, Shared with me, Signed out, then New canvas) and the card's
//! other modes — rename, invite, share, members, delete — draw in place.
//!
//! The card is drawn afresh from a snapshot whenever something it shows
//! changes; the text typed into its fields and the focused control carry
//! over.

use super::copy::{self, RowFacts};
use super::look::{self, Kind};
use super::state::{self, Act, Card, FieldKind, Mode};
use super::{inbox, ops, share};
use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use copper_core::canvas::CanvasEntry;
use copper_shell_core::canvas::{self as core, registry};
use copper_win_ui::tokens::{Shadow, side};
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim, Weight};
use std::cell::Cell;

pub const WIDTH: f32 = 320.0;

thread_local! {
    /// The door and its dot.
    static DOOR: Cell<Option<(NodeId, NodeId)>> = const { Cell::new(None) };
}

/// Put the door in the column's foot (its tooltip and dot follow
/// [`refresh_door`]). Returns the door.
pub fn mount(ui: &mut Ui, foot: NodeId) -> NodeId {
    let (d, _) = crate::chrome::build::door(
        ui,
        foot,
        glyph::CANVAS,
        side::FOOT_DOOR,
        side::FOOT_GLYPH,
        "Canvas",
        &copy::door_help(0, 0, 0),
    );
    let dot = ui.tree.add(
        d,
        look::dot(Token::ColumnInk)
            .absolute(Abs {
                left: None,
                top: Some(4.0),
                right: Some(4.0),
                bottom: None,
            })
            .hidden(true),
    );
    DOOR.with(|c| c.set(Some((d, dot))));
    d
}

/// The door's dot and tooltip, from what is waiting: a mention of you is
/// the accent dot, people live the green, an invite alone the column's ink.
pub fn refresh_door(shell: &Shell) {
    let Some((door, dot)) = DOOR.with(Cell::get) else {
        return;
    };
    let mentions = inbox::mention_total();
    let invites = inbox::invites().len();
    let others: usize = inbox::presence_now().peers.values().sum();
    let help = copy::door_help(mentions, invites, others);
    let name = copy::door_name(mentions, invites);
    let colour: Option<Swatch> = copy::door_dot(mentions, others, invites).map(|d| match d {
        copy::Dot::Mention => Token::Accent.into(),
        copy::Dot::Live => look::GREEN.into(),
        copy::Dot::Invite => Token::ColumnInk.into(),
    });
    shell.view().chrome().edit(move |ui, _| {
        ui.tree.edit(door, |n| {
            n.tooltip = Some(help.clone());
            n.a11y.help = help;
            n.a11y.name = name;
        });
        match colour {
            Some(c) => {
                ui.tree.edit(dot, |n| n.look.fill = c);
                ui.tree.set_hidden(dot, false);
            }
            None => ui.tree.set_hidden(dot, true),
        }
    });
}

/// The door was clicked (or Ctrl+Shift+O): the card, or away with it.
pub fn toggle(shell: &Shell) {
    if state::is_open() {
        close(shell);
    } else {
        open(shell, Mode::List);
    }
}

/// Show the card in `mode`.
pub fn open(shell: &Shell, mode: Mode) {
    super::rename::close(shell);
    state::with(|c| {
        c.open = true;
        c.note = None;
    });
    set_mode(shell, mode);
    inbox::refresh_soon(shell);
    core::refresh_soon::<Win>();
    shell.view().overlay_changed();
}

/// Put the card away.
pub fn close(shell: &Shell) {
    let scrim = state::with(|c| {
        let s = c.scrim.take();
        *c = Card::default();
        s
    });
    if let Some(s) = scrim {
        shell.view().chrome().edit(move |ui, _| ui.tree.remove(s));
    }
    shell.view().overlay_changed();
    shell.view().focus_page();
}

/// Change what the card shows (and start reading what the mode needs).
pub fn set_mode(shell: &Shell, mode: Mode) {
    let start_name = match &mode {
        Mode::Rename(id) => registry::entry(id).map(|e| e.name),
        _ => Some(String::new()),
    };
    state::with(|c| {
        c.mode = mode.clone();
        c.working = false;
        c.failure = None;
        c.members = None;
        if !matches!(mode, Mode::Share(_)) {
            c.share = None;
        }
        c.texts.clear();
        if let Some(n) = start_name {
            c.texts.insert(FieldKind::Name, n);
        }
        c.focus = match &mode {
            Mode::List => None,
            Mode::New | Mode::Rename(_) => Some(Act::Field(FieldKind::Name)),
            Mode::Invite(_) => Some(Act::Field(FieldKind::Email)),
            Mode::Share(_) => Some(Act::Field(FieldKind::Query)),
            Mode::Members(_) => Some(Act::Back),
            // Cancel is where the keyboard starts: Return confirms nothing.
            Mode::Delete(_) => Some(Act::Cancel),
        };
    });
    match &mode {
        Mode::Members(id) => ops::load_members(shell, id),
        Mode::Share(id) => share::start(shell, id),
        _ => {}
    }
    render(shell);
}

/// Escape: a form goes back to the list, the list closes. `true` if the
/// card was up.
pub fn escape(shell: &Shell) -> bool {
    if !state::is_open() {
        return false;
    }
    if state::with(|c| c.mode.is_form()) {
        set_mode(shell, Mode::List);
    } else {
        close(shell);
    }
    true
}

// MARK: snapshot

/// One canvas row as drawn.
#[derive(Clone, Debug)]
pub struct RowV {
    pub id: String,
    pub name: String,
    pub glyph: char,
    pub detail: String,
    pub front: bool,
    pub open: bool,
    pub peers: usize,
    pub mentions: usize,
}

/// An invitation as drawn.
#[derive(Clone, Debug)]
pub struct InviteV {
    pub id: String,
    pub canvas: String,
    pub from: String,
    pub detail: String,
}

/// The canvas a mode is about.
#[derive(Clone, Debug)]
pub struct Subject {
    pub name: String,
    pub personal: bool,
    pub shared: bool,
    pub leaving: bool,
}

/// What the card shows, read from the shell before drawing.
pub struct Snap {
    pub status: String,
    pub cloud_ready: bool,
    pub signed_in: bool,
    pub personal: RowV,
    pub invites: Vec<InviteV>,
    pub mine: Vec<RowV>,
    pub shared: Vec<RowV>,
    pub away: Vec<RowV>,
    pub problem: Option<String>,
    pub subject: Option<Subject>,
    pub share: Option<share::View>,
    /// Where the door is: the card hangs over it.
    pub anchor: Option<NodeId>,
}

fn me() -> Option<String> {
    copper_shell_core::cloud::get()
        .and_then(|c| c.client.account())
        .map(|a| a.user_id.to_ascii_lowercase())
}

fn glyph_of(e: &CanvasEntry) -> char {
    if registry::is_personal(e) {
        glyph::PERSON
    } else if registry::is_shared(e) {
        glyph::PEOPLE
    } else {
        glyph::CANVAS
    }
}

fn row_v(e: &CanvasEntry, front: Option<&str>, tabs: &[String], me: Option<&str>) -> RowV {
    let p = inbox::presence_now();
    let facts = RowFacts {
        personal: registry::is_personal(e),
        shared: registry::is_shared(e),
        owner: e.is_owner(),
        away: registry::is_shared(e) && e.account.as_deref() != me,
        cloud_ready: registry::cloud_ready(),
        personal_syncs: registry::personal_syncs(),
        open: p.open.contains(&e.id),
        synced: p.synced.contains(&e.id),
        members: e.members.unwrap_or(1),
    };
    RowV {
        id: e.id.clone(),
        name: e.name.clone(),
        glyph: glyph_of(e),
        detail: copy::row_detail(&facts, e.owner.as_deref()),
        front: front == Some(e.id.as_str()),
        open: tabs.contains(&e.id),
        peers: p.peers.get(&e.id).copied().unwrap_or(0),
        mentions: inbox::mention_count(&e.id),
    }
}

/// The ids of the canvases open in a tab.
fn open_canvases(shell: &Shell) -> Vec<String> {
    shell
        .browser()
        .all_tabs()
        .filter(|t| !t.private)
        .filter_map(|t| core::id_from_url(&t.url))
        .collect()
}

pub fn subject(id: &str) -> Option<Subject> {
    let e = registry::entry(id)?;
    let shared = registry::is_shared(&e);
    Some(Subject {
        name: e.name.clone(),
        personal: registry::is_personal(&e),
        shared,
        leaving: shared && !e.is_owner(),
    })
}

impl Snap {
    pub fn gather(shell: &Shell, mode: &Mode) -> Snap {
        let me = me();
        let front = core::active::<Win>(shell);
        let tabs = open_canvases(shell);
        let account = copper_shell_core::cloud::get().and_then(|c| c.client.account());
        let cloud_ready = registry::cloud_ready();
        let status = copy::header_status(account.as_ref().filter(|_| cloud_ready).map(|a| {
            if a.display_name.is_empty() {
                a.email.as_str()
            } else {
                a.display_name.as_str()
            }
        }));
        let personal = registry::personal();
        let visible = registry::visible();
        let mut mine: Vec<&CanvasEntry> = visible
            .iter()
            .filter(|e| !registry::is_personal(e) && e.is_owner())
            .collect();
        mine.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
        let mut shared: Vec<&CanvasEntry> = visible
            .iter()
            .filter(|e| registry::is_shared(e) && !e.is_owner())
            .collect();
        shared.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
        let mut away: Vec<CanvasEntry> = registry::with(|r| {
            r.all
                .iter()
                .filter(|e| registry::is_shared(e) && e.account != me)
                .cloned()
                .collect()
        });
        away.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
        let f = front.as_deref();
        let m = me.as_deref();
        let invites = inbox::invites()
            .into_iter()
            .map(|i| {
                let from = inbox::sender(&i);
                InviteV {
                    detail: copy::invite_detail(&from, i.nudged_at.is_some()),
                    canvas: i.canvas_name.clone().unwrap_or_else(|| "a canvas".into()),
                    id: i.id,
                    from,
                }
            })
            .collect();
        let subject = mode.canvas().and_then(subject);
        let share = match mode {
            Mode::Share(id) => Some(share::View::gather(shell, id)),
            _ => None,
        };
        Snap {
            status,
            cloud_ready,
            signed_in: account.is_some(),
            personal: row_v(&personal, f, &tabs, m),
            invites,
            mine: mine.into_iter().map(|e| row_v(e, f, &tabs, m)).collect(),
            shared: shared.into_iter().map(|e| row_v(e, f, &tabs, m)).collect(),
            away: away.iter().map(|e| row_v(e, f, &tabs, m)).collect(),
            problem: registry::with(|r| r.problem.clone()),
            subject,
            share,
            anchor: DOOR.with(Cell::get).map(|(d, _)| d),
        }
    }
}

// MARK: drawing

/// Draw the card afresh (when it is up).
pub fn render(shell: &Shell) {
    if !state::is_open() {
        return;
    }
    let mode = state::with(|c| c.mode.clone());
    let snap = Snap::gather(shell, &mode);
    shell.view().chrome().edit(move |ui, st| {
        state::CARD.with(|c| draw(ui, st.ids.plates, &mut c.borrow_mut(), &snap));
    });
}

fn draw(ui: &mut Ui, plates: NodeId, c: &mut Card, s: &Snap) {
    if !c.open {
        return;
    }
    // What had focus, to give it back.
    let wanted = c
        .focus
        .take()
        .or_else(|| ui.tree.focused().and_then(|f| c.hits.get(&f).cloned()));
    let scrim = match c.scrim.filter(|s| ui.tree.contains(*s)) {
        Some(sc) => {
            ui.tree.clear_children(sc);
            sc
        }
        None => {
            let sc = ui.tree.add(
                plates,
                Node::stack()
                    .absolute(Abs::FILL)
                    .layer()
                    .hit()
                    .modal()
                    .role(Role::None),
            );
            c.scrim = Some(sc);
            sc
        }
    };
    c.hits.clear();
    c.rows.clear();
    c.fields.clear();
    c.submit = None;
    c.first = None;
    c.hits.insert(scrim, Act::Scrim);

    // Over the door, left-aligned with it, inside the window.
    let win = ui.size();
    let (left, bottom) = match s.anchor.filter(|a| ui.tree.is_shown(*a)) {
        Some(a) => {
            let r = ui.tree.window_rect(a);
            let left = r.x.clamp(8.0, (win.w - WIDTH - 8.0).max(8.0));
            (left, (win.h - r.y + 6.0).max(8.0))
        }
        None => (((win.w - WIDTH) / 2.0).max(8.0), (win.h - 420.0).max(8.0)),
    };
    let card = ui.tree.add(
        scrim,
        Node::column()
            .absolute(Abs {
                left: Some(left),
                top: None,
                right: None,
                bottom: Some(bottom),
            })
            .width(WIDTH)
            .max_height((win.h - bottom - 8.0).max(120.0))
            .fill(Token::Ground)
            .radius(13.0)
            .stroke(Token::Hairline, 1.0)
            .shadow(Shadow::new(0.16, 24.0, 8.0))
            .clip()
            .layer()
            .hit()
            .role(Role::Pane)
            .name("Canvas"),
    );
    c.card = Some(card);
    header(ui, c, card, s);
    look::hairline(ui, card);
    match c.mode.clone() {
        Mode::List | Mode::New => list(ui, c, card, s),
        Mode::Rename(_) => match &s.subject {
            Some(sub) => name_form(
                ui,
                c,
                card,
                Some("Rename canvas"),
                Some(&sub.name),
                "Rename",
                s,
            ),
            None => gone(ui, card),
        },
        Mode::Invite(_) => match &s.subject {
            Some(sub) => invite_form(ui, c, card, sub),
            None => gone(ui, card),
        },
        Mode::Members(_) => match &s.subject {
            Some(sub) => members(ui, c, card, sub),
            None => gone(ui, card),
        },
        Mode::Delete(_) => match &s.subject {
            Some(sub) => delete(ui, c, card, sub),
            None => gone(ui, card),
        },
        Mode::Share(_) => match (&s.subject, &s.share) {
            (Some(sub), Some(v)) => share::draw(ui, c, card, sub, v),
            _ => gone(ui, card),
        },
    }

    // Typed text carries over; the focus goes back where it was.
    let fields: Vec<(NodeId, FieldKind)> = c.fields.iter().map(|(n, k)| (*n, *k)).collect();
    for (n, k) in fields {
        if let Some(t) = c.texts.get(&k).filter(|t| !t.is_empty()) {
            ui.set_field_text(
                n,
                t,
                matches!(c.mode, Mode::Rename(_)) && wanted == Some(Act::Field(k)),
            );
        }
    }
    sync_submit(ui, c);
    let target = wanted
        .and_then(|w| c.hits.iter().find(|(_, a)| **a == w).map(|(n, _)| *n))
        // Something in the card holds the keyboard.
        .or(c.first);
    if let Some(t) = target {
        ui.focus(Some(t));
    }
}

/// The form's action is on while there is something to send.
pub fn sync_submit(ui: &mut Ui, c: &Card) {
    if let Some((b, k)) = c.submit {
        let text = c.texts.get(&k).map(String::as_str).unwrap_or("");
        let ok = !c.working
            && match k {
                FieldKind::Email => text.contains('@'),
                _ => copy::submittable(text),
            };
        look::set_enabled(ui, b, ok);
    }
}

fn gone(ui: &mut Ui, card: NodeId) {
    let b = ui
        .tree
        .add(card, Node::column().padding(Insets::hv(look::INSET, 12.0)));
    look::note(ui, b, "That canvas isn't here any more.", false);
}

fn header(ui: &mut Ui, c: &mut Card, card: NodeId, s: &Snap) {
    let h = ui.tree.add(
        card,
        Node::row()
            .height(40.0)
            .shrink(0.0)
            .padding(Insets::hv(look::INSET, 0.0))
            .gap(8.0)
            .align(Align::Center),
    );
    if !matches!(c.mode, Mode::List | Mode::New) {
        let back = ui.tree.add(
            h,
            Node::stack()
                .size(22.0, 22.0)
                .shrink(0.0)
                .radius(5.0)
                .hover_fill(Token::Hover)
                .align(Align::Center)
                .justify(Justify::Center)
                .button("Back to canvases")
                .tooltip("Back"),
        );
        ui.tree.add(
            back,
            Node::glyph(glyph::CHEVRON_LEFT, 11.0, 22.0)
                .ink(Token::Muted)
                .role(Role::None),
        );
        c.hits.insert(back, Act::Back);
    }
    ui.tree.add(
        h,
        Node::text("Canvas", 13.0)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .shrink(0.0),
    );
    ui.tree.add(h, Node::new().grow(1.0).min_width(6.0));
    ui.tree.add(
        h,
        Node::text(&s.status, 11.0)
            .ink(Token::Muted)
            .trim(Trim::Middle)
            .max_width(190.0),
    );
}

fn list(ui: &mut Ui, c: &mut Card, card: NodeId, s: &Snap) {
    let scroll = ui.tree.add(
        card,
        Node::scroll()
            .max_height(380.0)
            .shrink(1.0)
            .padding(Insets::hv(0.0, 4.0))
            .role(Role::List)
            .name("Canvases"),
    );
    row(ui, c, scroll, &s.personal);
    if !s.invites.is_empty() {
        look::section(ui, scroll, "Invitations");
        for i in &s.invites {
            invite_row(ui, c, scroll, i);
        }
    }
    if !s.mine.is_empty() {
        look::section(ui, scroll, "My canvases");
        for r in &s.mine {
            row(ui, c, scroll, r);
        }
    }
    if !s.shared.is_empty() {
        look::section(ui, scroll, "Shared with me");
        for r in &s.shared {
            row(ui, c, scroll, r);
        }
    }
    if !s.away.is_empty() {
        look::section(ui, scroll, "Signed out");
        look::explain(
            ui,
            scroll,
            &copy::signed_out_note(s.away.len(), s.signed_in),
        );
        // Another account signed in: only their count, never their names.
        if !s.signed_in {
            for r in &s.away {
                row(ui, c, scroll, r);
            }
        }
    }
    let note = c
        .note
        .clone()
        .map(|n| (n, false))
        .or_else(|| s.problem.clone().map(|p| (p, true)));
    if let Some((text, failed)) = note {
        let b = ui.tree.add(
            card,
            Node::column().padding(Insets::new(look::INSET, 0.0, look::INSET, 6.0)),
        );
        look::note(ui, b, &text, failed);
    }
    look::hairline(ui, card);
    if c.mode == Mode::New {
        name_form(ui, c, card, None, None, "Create", s);
    } else {
        footer(ui, c, card, s);
    }
}

fn row(ui: &mut Ui, c: &mut Card, parent: NodeId, r: &RowV) {
    let mut said = vec![r.name.clone(), r.detail.clone()];
    if r.peers > 0 {
        said.push(copy::live(r.peers));
    }
    if r.front {
        said.push("the tab in front".into());
    } else if r.open {
        said.push("open in a tab".into());
    }
    let b = ui.tree.add(
        parent,
        Node::row()
            .padding(Insets::hv(look::INSET, 6.0))
            .gap(10.0)
            .align(Align::Center)
            .fill(if r.front {
                Swatch::Token(Token::Wash)
            } else {
                Swatch::Clear
            })
            .hover_fill(Token::Hover)
            .button(&said.join(", "))
            .tooltip(&r.name),
    );
    look::tile(ui, b, r.glyph, r.front);
    let words = ui.tree.add(
        b,
        Node::column()
            .gap(1.0)
            .grow(1.0)
            .shrink(1.0)
            .role(Role::None),
    );
    ui.tree.add(
        words,
        Node::text(&r.name, 12.5)
            .weight(if r.front {
                Weight::Semibold
            } else {
                Weight::Regular
            })
            .ink(Token::Ink)
            .role(Role::None),
    );
    ui.tree.add(
        words,
        Node::text(&r.detail, 11.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    if r.mentions > 0 {
        look::mention_badge(ui, b, r.mentions);
    }
    if r.peers > 0 {
        let live = ui.tree.add(
            b,
            Node::row()
                .gap(4.0)
                .shrink(0.0)
                .align(Align::Center)
                .tooltip(&copy::live(r.peers)),
        );
        ui.tree.add(live, look::dot(look::GREEN));
        ui.tree.add(
            live,
            Node::text(&r.peers.to_string(), 11.0)
                .ink(Token::Muted)
                .role(Role::None),
        );
    }
    if r.open {
        ui.tree.add(
            b,
            Node::glyph(
                if r.front { glyph::CHECK } else { glyph::WINDOW },
                10.5,
                16.0,
            )
            .ink(if r.front { Token::Ink } else { Token::Muted })
            .tooltip(if r.front {
                "The tab in front"
            } else {
                "Open in a tab — click to switch to it"
            })
            .role(Role::None),
        );
    }
    c.hits.insert(b, Act::Open(r.id.clone()));
    c.rows.insert(b, r.id.clone());
    c.first.get_or_insert(b);
}

fn invite_row(ui: &mut Ui, c: &mut Card, parent: NodeId, i: &InviteV) {
    let working = c.answering.get(&i.id).copied();
    let b = ui.tree.add(
        parent,
        Node::column()
            .padding(Insets::hv(look::INSET, 7.0))
            .gap(6.0)
            .role(Role::Group)
            .name(&if i.from.is_empty() {
                format!("Invitation to {}", i.canvas)
            } else {
                format!("Invitation to {} from {}", i.canvas, i.from)
            }),
    );
    let top = ui.tree.add(b, Node::row().gap(10.0).align(Align::Start));
    look::tile(ui, top, glyph::MAIL, false);
    let words = ui
        .tree
        .add(top, Node::column().gap(1.0).grow(1.0).shrink(1.0));
    ui.tree.add(
        words,
        Node::text(&i.canvas, 12.5).ink(Token::Ink).trim(Trim::Wrap),
    );
    ui.tree.add(
        words,
        Node::text(&i.detail, 11.0)
            .ink(Token::Muted)
            .trim(Trim::Wrap),
    );
    if let Some(f) = c.answer_failed.get(&i.id) {
        let w = ui
            .tree
            .add(b, Node::column().padding(Insets::new(36.0, 0.0, 0.0, 0.0)));
        look::note(ui, w, f, true);
    }
    let buttons = ui.tree.add(
        b,
        Node::row()
            .gap(8.0)
            .align(Align::Center)
            .justify(Justify::End),
    );
    ui.tree.add(buttons, Node::new().grow(1.0));
    if let Some(accepting) = working {
        ui.tree.add(
            buttons,
            Node::text(
                if accepting {
                    "Joining…"
                } else {
                    "Declining…"
                },
                11.0,
            )
            .ink(Token::Muted),
        );
    }
    let decline = look::button(ui, buttons, "Decline", Kind::Plain, working.is_none());
    let join = look::button(ui, buttons, "Join", Kind::Primary, working.is_none());
    ui.tree.edit(join, |n| {
        n.tooltip = Some(format!("Join “{}” and open it", i.canvas));
    });
    c.hits.insert(decline, Act::Decline(i.id.clone()));
    c.hits.insert(join, Act::Join(i.id.clone()));
}

fn footer(ui: &mut Ui, c: &mut Card, card: NodeId, s: &Snap) {
    let f = ui.tree.add(
        card,
        Node::column()
            .padding(Insets::hv(look::INSET, 10.0))
            .gap(6.0),
    );
    let new = ui.tree.add(
        f,
        Node::row()
            .gap(6.0)
            .padding(Insets::hv(2.0, 2.0))
            .radius(5.0)
            .align(Align::Center)
            .hover_fill(Token::Hover)
            .button("New canvas")
            .tooltip(&copy::new_help(s.cloud_ready)),
    );
    ui.tree.add(
        new,
        Node::glyph(glyph::PLUS, 11.0, 14.0)
            .ink(Token::Ink)
            .role(Role::None),
    );
    ui.tree.add(
        new,
        Node::text("New canvas", 12.5)
            .ink(Token::Ink)
            .role(Role::None),
    );
    c.hits.insert(new, Act::NewMode);
    if !s.cloud_ready {
        let text = format!("{}{}", copy::FOOTER_CLOUD, copy::FOOTER_SETTINGS);
        let b = ui.tree.add(
            f,
            Node::column()
                .padding(Insets::hv(2.0, 1.0))
                .radius(5.0)
                .hover_fill(Token::Hover)
                .button(&text),
        );
        ui.tree.add(
            b,
            Node::text(&text, 11.0)
                .ink(Token::Muted)
                .trim(Trim::Wrap)
                .role(Role::None),
        );
        c.hits.insert(b, Act::Settings);
    }
}

/// A name, asked for in place: New canvas (`title` none) and Rename.
fn name_form(
    ui: &mut Ui,
    c: &mut Card,
    card: NodeId,
    title: Option<&str>,
    subject: Option<&str>,
    action: &str,
    s: &Snap,
) {
    let f = ui.tree.add(
        card,
        Node::column()
            .padding(Insets::hv(look::INSET, 10.0))
            .gap(8.0),
    );
    if let Some(t) = title {
        look::form_title(ui, f, t);
    }
    if let Some(n) = subject {
        look::subject(ui, f, n);
    }
    let field = look::field(ui, f, "Canvas name");
    c.fields.insert(field, FieldKind::Name);
    c.hits.insert(field, Act::Field(FieldKind::Name));
    if let Some(e) = &c.failure {
        look::note(ui, f, e, true);
    }
    if title.is_none() {
        look::note(ui, f, &copy::new_note(s.cloud_ready), false);
    }
    let submit = buttons(ui, c, f, action, Kind::Primary);
    c.submit = Some((submit, FieldKind::Name));
}

/// Cancel and the action, at the trailing edge; returns the action.
fn buttons(ui: &mut Ui, c: &mut Card, parent: NodeId, action: &str, kind: Kind) -> NodeId {
    let row = ui.tree.add(
        parent,
        Node::row()
            .gap(8.0)
            .align(Align::Center)
            .justify(Justify::End),
    );
    ui.tree.add(row, Node::new().grow(1.0));
    if c.working {
        ui.tree.add(row, Node::text("…", 11.0).ink(Token::Muted));
    }
    let cancel = look::button(ui, row, "Cancel", Kind::Plain, true);
    let go = look::button(ui, row, action, kind, !c.working);
    c.hits.insert(cancel, Act::Cancel);
    c.hits.insert(
        go,
        if kind == Kind::Destructive {
            Act::Confirm
        } else {
            Act::Submit
        },
    );
    go
}

fn invite_form(ui: &mut Ui, c: &mut Card, card: NodeId, sub: &Subject) {
    let f = ui.tree.add(
        card,
        Node::column()
            .padding(Insets::hv(look::INSET, 10.0))
            .gap(8.0),
    );
    look::form_title(ui, f, "Invite to canvas");
    look::subject(ui, f, &sub.name);
    let field = look::field(ui, f, "name@example.com");
    c.fields.insert(field, FieldKind::Email);
    c.hits.insert(field, Act::Field(FieldKind::Email));
    look::note(
        ui,
        f,
        "They'll see it under Invitations once they sign in to the same cloud.",
        false,
    );
    if let Some(e) = &c.failure {
        look::note(ui, f, e, true);
    }
    let submit = buttons(ui, c, f, "Send Invite", Kind::Primary);
    c.submit = Some((submit, FieldKind::Email));
}

fn members(ui: &mut Ui, c: &mut Card, card: NodeId, sub: &Subject) {
    let top = ui.tree.add(
        card,
        Node::column()
            .padding(Insets::new(look::INSET, 10.0, look::INSET, 4.0))
            .gap(8.0),
    );
    look::form_title(ui, top, "Members");
    look::subject(ui, top, &sub.name);
    let body = ui.tree.add(
        card,
        Node::column().padding(Insets::new(0.0, 0.0, 0.0, 8.0)),
    );
    match &c.members {
        None => {
            let w = ui
                .tree
                .add(body, Node::column().padding(Insets::all(look::INSET)));
            look::note(ui, w, "Loading…", false);
        }
        Some(Err(e)) => {
            let w = ui
                .tree
                .add(body, Node::column().padding(Insets::all(look::INSET)));
            look::note(ui, w, e, true);
        }
        Some(Ok(list)) => {
            let scroll = ui.tree.add(
                body,
                Node::scroll()
                    .max_height(300.0)
                    .shrink(1.0)
                    .role(Role::List)
                    .name("Members"),
            );
            for m in list {
                member_row(ui, scroll, m);
            }
        }
    }
}

/// One member: a face, the name and address, the role.
pub fn member_row(ui: &mut Ui, parent: NodeId, m: &copper_cloud::canvas::Member) -> NodeId {
    let name = m
        .display_name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| m.email.clone())
        .unwrap_or_default();
    let email = m.email.clone().unwrap_or_default();
    let r = ui.tree.add(
        parent,
        Node::row()
            .padding(Insets::hv(look::INSET, 5.0))
            .gap(10.0)
            .align(Align::Center)
            .role(Role::ListItem)
            .name(&name),
    );
    look::face(
        ui,
        r,
        &name,
        copy::hex(&core::stable_color(&m.user_id.to_ascii_lowercase())),
        22.0,
    );
    let words = ui
        .tree
        .add(r, Node::column().gap(1.0).grow(1.0).shrink(1.0));
    ui.tree.add(words, Node::text(&name, 12.5).ink(Token::Ink));
    if !email.is_empty() && email != name {
        ui.tree
            .add(words, Node::text(&email, 11.0).ink(Token::Muted));
    }
    ui.tree.add(
        r,
        Node::text(&copy::role_word(&m.role), 11.0)
            .ink(Token::Faint)
            .shrink(0.0),
    );
    r
}

fn delete(ui: &mut Ui, c: &mut Card, card: NodeId, sub: &Subject) {
    let f = ui.tree.add(
        card,
        Node::column()
            .padding(Insets::hv(look::INSET, 10.0))
            .gap(8.0),
    );
    let what = if sub.leaving {
        "Leave canvas"
    } else {
        "Delete canvas"
    };
    look::form_title(ui, f, what);
    look::subject(ui, f, &sub.name);
    let consequence = copy::consequence(sub.leaving, sub.shared);
    look::note(ui, f, &consequence, false);
    if let Some(e) = &c.failure {
        look::note(ui, f, e, true);
    }
    let go = buttons(ui, c, f, what, Kind::Destructive);
    ui.tree.edit(go, |n| n.a11y.help = consequence);
}
