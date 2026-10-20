//! The native Share card (`CanvasShareSheet`, `CanvasShareModel`): who is
//! here, the invite link, the people on the cloud and the field to invite
//! by email, then the members and the invites waiting — a mode of the
//! Canvas card. The page's own Share button opens it.

use super::copy::{self, Doing, Outcome};
use super::door::{self, Subject};
use super::look::{self, Kind};
use super::ops::{self, cloud_call};
use super::state::{self, Act, Card, FieldKind, Mode};
use crate::platform::{Shell, Win};
use copper_cloud::Feature;
use copper_cloud::canvas::{Invite, Member, Person};
use copper_shell_core::canvas::{self as core, host, registry};
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Node, NodeId, Role, Token, Trim, Weight};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::time::Duration;

/// A stand-in invite's id, until the server's copy takes its place.
const LOCAL: &str = "local-";

/// What the card knows about the canvas's sharing.
#[derive(Default)]
pub struct Model {
    pub id: String,
    pub members: Vec<Member>,
    pub pending: Vec<Invite>,
    pub people: Vec<Person>,
    /// The query `people` answer.
    pub searched: Option<String>,
    pub token: Option<String>,
    pub copied: bool,
    /// Making or resetting the link.
    pub working: bool,
    pub link_failure: Option<String>,
    /// Addresses (lowercased) with an invite on its way.
    pub acting: HashSet<String>,
    /// What the last invite came to: the text, and whether it failed.
    pub said: Option<(String, bool)>,
    pub generation: u64,
}

/// Someone in the canvas now.
#[derive(Clone, Debug)]
pub struct Face {
    pub name: String,
    pub colour: u32,
}

/// What the card shows, read before drawing.
pub struct View {
    pub can_share: bool,
    pub signed_in: bool,
    pub owner: bool,
    pub here: bool,
    pub me: Face,
    pub my_email: String,
    pub my_id: String,
    pub others: Vec<Face>,
    /// `Some(false)`: the cloud has no share links.
    pub links: Option<bool>,
    pub directory: bool,
    pub reminders: bool,
    pub host: String,
    pub version: Option<String>,
}

impl View {
    pub fn gather(_shell: &Shell, id: &str) -> View {
        let entry = registry::entry(id);
        let cloud = copper_shell_core::cloud::get();
        let client = cloud.map(|c| &c.client);
        let account = client.and_then(|c| c.account());
        let (me_id, me_name, me_colour) = registry::identity();
        let mut others: Vec<Face> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        let mut here = false;
        for h in host::hosts_of::<Win>(id) {
            here |= h.room_open();
            let d = h.describe();
            for p in d["presence"].as_array().into_iter().flatten() {
                let pid = p["id"].as_str().unwrap_or("").to_ascii_lowercase();
                if pid.is_empty() || pid == me_id || p["kind"].as_str() == Some("agent") {
                    continue;
                }
                if seen.insert(pid) {
                    others.push(Face {
                        name: p["name"].as_str().unwrap_or("Someone").to_string(),
                        colour: copy::hex(p["color"].as_str().unwrap_or("")),
                    });
                }
            }
        }
        View {
            can_share: entry.as_ref().is_some_and(registry::is_shared) && registry::cloud_ready(),
            signed_in: client.is_some_and(|c| c.is_signed_in()),
            owner: entry.as_ref().is_some_and(|e| e.is_owner()),
            here,
            me: Face {
                name: me_name,
                colour: copy::hex(&me_colour),
            },
            my_email: account
                .as_ref()
                .map(|a| a.email.to_ascii_lowercase())
                .unwrap_or_default(),
            my_id: account
                .as_ref()
                .map(|a| a.user_id.to_ascii_lowercase())
                .unwrap_or_default(),
            others,
            links: client.and_then(|c| c.supports(Feature::ShareLinks)),
            directory: client.and_then(|c| c.supports(Feature::People)) != Some(false),
            reminders: client.and_then(|c| c.supports(Feature::InviteReminders)) == Some(true),
            host: copy::cloud_host(client.and_then(|c| c.link()).map(|l| l.host()).as_deref()),
            version: client.and_then(|c| c.server_version()),
        }
    }
}

/// Where someone stands on the canvas.
enum Standing {
    You,
    Member(String),
    Invited(Box<Invite>),
    Invite,
}

fn standing(m: &Model, v: &View, email: &str, person_id: Option<&str>) -> Standing {
    let key = email.trim().to_ascii_lowercase();
    if (!key.is_empty() && key == v.my_email)
        || person_id.is_some_and(|p| p.eq_ignore_ascii_case(&v.my_id))
    {
        return Standing::You;
    }
    if let Some(member) = m.members.iter().find(|x| {
        x.email
            .as_deref()
            .is_some_and(|e| e.eq_ignore_ascii_case(&key))
            || person_id.is_some_and(|p| p.eq_ignore_ascii_case(&x.user_id))
    }) {
        return Standing::Member(member.role.clone());
    }
    if let Some(i) = m.pending.iter().find(|i| {
        i.email
            .as_deref()
            .is_some_and(|e| e.eq_ignore_ascii_case(&key))
    }) {
        return Standing::Invited(Box::new(i.clone()));
    }
    Standing::Invite
}

fn invite_name(i: &Invite) -> String {
    i.invitee
        .as_ref()
        .and_then(|p| p.display_name.clone().filter(|n| !n.is_empty()))
        .or_else(|| i.email.clone())
        .unwrap_or_default()
}

/// The name the card uses for `email`: the person's, if the directory knows them.
fn name_for(m: &Model, email: &str) -> String {
    let key = email.to_ascii_lowercase();
    m.people
        .iter()
        .find(|p| {
            p.email
                .as_deref()
                .is_some_and(|e| e.eq_ignore_ascii_case(&key))
        })
        .and_then(|p| p.display_name.clone().filter(|n| !n.is_empty()))
        .or_else(|| {
            m.pending
                .iter()
                .find(|i| {
                    i.email
                        .as_deref()
                        .is_some_and(|e| e.eq_ignore_ascii_case(&key))
                })
                .map(invite_name)
        })
        .unwrap_or_else(|| email.to_string())
}

// MARK: opening

/// The Share card for canvas `id`, opened over the door.
pub fn show(shell: &Shell, id: &str) {
    if state::is_open() {
        door::set_mode(shell, Mode::Share(id.to_string()));
    } else {
        door::open(shell, Mode::Share(id.to_string()));
    }
}

/// The mode began: read the members, the invites waiting and the link.
pub fn start(shell: &Shell, id: &str) {
    let token = registry::entry(id).and_then(|e| e.remote_id).and_then(|r| {
        cached_tokens(shell)
            .get(&r)
            .and_then(Value::as_str)
            .map(String::from)
    });
    state::with(|c| {
        c.share = Some(Model {
            id: id.to_string(),
            token,
            ..Default::default()
        })
    });
    reload(shell, id);
}

/// Read the members and the invites again.
fn reload(shell: &Shell, id: &str) {
    let Some(remote) = remote_of(id) else {
        return;
    };
    let Some(api) = ops::api() else { return };
    let weak = shell.downgrade();
    let id = id.to_string();
    copper_win_rt::spawn_local(async move {
        let (a, r) = (api.clone(), remote.clone());
        let members = cloud_call(async move { a.members(&r).await }).await;
        let (a, r) = (api.clone(), remote.clone());
        let invites = cloud_call(async move { a.invites(&r).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        let mine = state::with(|c| {
            let Some(m) = c.share.as_mut().filter(|m| m.id == id) else {
                return false;
            };
            if let Ok(list) = members {
                m.members = list;
            }
            if let Ok(list) = invites {
                // Stand-ins still on their way stay until answered.
                let waiting: Vec<Invite> = m
                    .pending
                    .drain(..)
                    .filter(|i| i.id.starts_with(LOCAL))
                    .collect();
                m.pending = waiting;
                m.pending.extend(
                    list.into_iter()
                        .filter(|i| i.status.as_deref().is_none_or(|s| s == "pending")),
                );
            }
            true
        });
        if mine {
            door::render(&shell);
        }
    });
}

fn remote_of(id: &str) -> Option<String> {
    registry::entry(id).and_then(|e| e.remote_id)
}

fn links_path(shell: &Shell) -> std::path::PathBuf {
    shell
        .store()
        .paths()
        .data
        .join("canvas")
        .join("share-links.json")
}

/// Share tokens kept on this device (`share-links.json`, remote → token):
/// the same file the `canvas_share_link` tool keeps.
fn cached_tokens(shell: &Shell) -> Map<String, Value> {
    std::fs::read(links_path(shell))
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

fn save_token(shell: &Shell, remote: &str, token: Option<&str>) {
    let mut saved = cached_tokens(shell);
    match token {
        Some(t) => {
            saved.insert(remote.to_string(), json!(t));
        }
        None => {
            saved.remove(remote);
        }
    }
    let _ = copper_core::store::write_atomic(
        &links_path(shell),
        Value::Object(saved).to_string().as_bytes(),
        true,
        false,
    );
}

// MARK: drawing

pub fn draw(ui: &mut Ui, c: &mut Card, card: NodeId, sub: &Subject, v: &View) {
    let f = ui.tree.add(
        card,
        Node::column()
            .padding(Insets::hv(look::INSET, 10.0))
            .gap(10.0)
            .shrink(1.0),
    );
    look::form_title(ui, f, "Share canvas");
    look::subject(ui, f, &sub.name);
    if !v.can_share {
        look::note(ui, f, &copy::unshareable(sub.personal), false);
        let row = ui.tree.add(f, Node::row().gap(8.0));
        if v.signed_in {
            let b = look::button(ui, row, "New shared canvas", Kind::Primary, true);
            c.hits.insert(b, Act::NewShared);
        } else {
            let b = look::button(ui, row, "Connect to Copper Cloud", Kind::Primary, true);
            c.hits.insert(b, Act::Connect);
        }
        return;
    }
    let Some(m) = c.share.take() else {
        return;
    };
    presence(ui, f, v);
    links(ui, c, f, &m, v);

    // The heading, the field, and what the last invite came to.
    let top = ui.tree.add(f, Node::column().gap(5.0));
    ui.tree.add(
        top,
        Node::text(
            if v.directory {
                "People on this cloud"
            } else {
                "Invite by email"
            },
            11.0,
        )
        .weight(Weight::Semibold)
        .ink(Token::Muted),
    );
    let field = look::field(
        ui,
        top,
        if v.directory {
            "Search people or type an email"
        } else {
            "Type an email to invite"
        },
    );
    c.fields.insert(field, FieldKind::Query);
    c.hits.insert(field, Act::Field(FieldKind::Query));
    if let Some((text, failed)) = &m.said {
        let line = if *failed {
            text.clone()
        } else {
            format!("✓ {text}")
        };
        look::note(ui, top, &line, *failed);
    }

    let scroll = ui.tree.add(
        f,
        Node::scroll()
            .max_height(360.0)
            .shrink(1.0)
            .gap(10.0)
            .role(Role::List)
            .name("People"),
    );
    let query = c.texts.get(&FieldKind::Query).cloned().unwrap_or_default();
    people_rows(ui, c, scroll, &m, v, &query);
    members_section(ui, c, scroll, &m, v);
    invited_section(ui, c, scroll, &m, v);
    if v.owner && v.links != Some(false) {
        let row = ui.tree.add(f, Node::row());
        let b = look::button(ui, row, "Reset link", Kind::Plain, !m.working);
        ui.tree.edit(b, |n| {
            n.tooltip = Some(
                "The link you shared stops working; a new one is made when you copy it".into(),
            );
        });
        c.hits.insert(b, Act::ResetLink);
    }
    c.share = Some(m);
}

fn presence(ui: &mut Ui, parent: NodeId, v: &View) {
    let Some(line) = copy::presence_line(v.others.len(), v.here) else {
        return;
    };
    let row = ui.tree.add(
        parent,
        Node::row()
            .gap(6.0)
            .align(Align::Center)
            .role(Role::Group)
            .name(&line),
    );
    if v.here {
        look::face(ui, row, &v.me.name, v.me.colour, 22.0);
    }
    for o in v.others.iter().take(8) {
        look::face(ui, row, &o.name, o.colour, 22.0);
    }
    ui.tree.add(row, Node::text(&line, 11.0).ink(Token::Muted));
}

fn links(ui: &mut Ui, c: &mut Card, parent: NodeId, m: &Model, v: &View) {
    if v.links == Some(false) {
        look::note(
            ui,
            parent,
            &copy::links_unsupported(&v.host, v.version.as_deref()),
            false,
        );
        return;
    }
    let row = ui
        .tree
        .add(parent, Node::row().gap(8.0).align(Align::Center));
    let copy_b = look::button(
        ui,
        row,
        if m.copied {
            "Copied"
        } else {
            "Copy invite link"
        },
        Kind::Primary,
        !m.working,
    );
    c.hits.insert(copy_b, Act::CopyLink);
    if m.token.is_some() {
        let web = look::button(ui, row, "Copy web link", Kind::Plain, true);
        c.hits.insert(web, Act::CopyWeb);
    }
    if let Some(f) = &m.link_failure {
        look::note(ui, parent, f, true);
    }
}

/// Who a person row is about.
struct Who<'a> {
    name: Option<&'a str>,
    email: &'a str,
    id: Option<&'a str>,
}

fn person_row(ui: &mut Ui, c: &mut Card, parent: NodeId, m: &Model, v: &View, who: Who) {
    let Who { name, email, id } = who;
    let row = ui.tree.add(
        parent,
        Node::row()
            .gap(8.0)
            .align(Align::Center)
            .role(Role::ListItem)
            .name(name.unwrap_or(email)),
    );
    match name {
        Some(n) => {
            let words = ui
                .tree
                .add(row, Node::column().gap(1.0).grow(1.0).shrink(1.0));
            ui.tree.add(words, Node::text(n, 11.5).ink(Token::Ink));
            ui.tree.add(
                words,
                Node::text(email, 10.0).ink(Token::Muted).trim(Trim::Middle),
            );
        }
        None => {
            ui.tree.add(
                row,
                Node::glyph(crate::chrome::glyph::MAIL, 10.5, 14.0)
                    .ink(Token::Muted)
                    .role(Role::None),
            );
            ui.tree.add(
                row,
                Node::text(email, 11.5)
                    .ink(Token::Ink)
                    .trim(Trim::Middle)
                    .grow(1.0)
                    .shrink(1.0),
            );
        }
    }
    match standing(m, v, email, id) {
        Standing::You => {
            ui.tree.add(row, Node::text("You", 10.0).ink(Token::Muted));
        }
        Standing::Member(role) => {
            ui.tree.add(
                row,
                Node::text(if role == "owner" { "Owner" } else { "Member" }, 10.0)
                    .ink(Token::Muted)
                    .tooltip(&format!("{} of this canvas", copy::role_word(&role))),
            );
        }
        Standing::Invited(i) => invited(ui, c, row, m, v, &i, true),
        Standing::Invite => {
            let busy = m.acting.contains(&email.to_ascii_lowercase());
            let b = look::button(ui, row, "Invite", Kind::Plain, !busy);
            c.hits.insert(b, Act::InviteTo(email.to_string()));
        }
    }
}

/// "Invited", then Resend and × where the cloud and the account allow them.
fn invited(ui: &mut Ui, c: &mut Card, row: NodeId, m: &Model, v: &View, i: &Invite, label: bool) {
    let email = i.email.clone().unwrap_or_default();
    let busy = m.acting.contains(&email.to_ascii_lowercase());
    let local = i.id.starts_with(LOCAL);
    let no_account = v.reminders && i.invitee.is_none();
    if label {
        ui.tree.add(
            row,
            Node::text("Invited", 10.0)
                .ink(Token::Muted)
                .tooltip(&copy::waiting_help(no_account, &v.host)),
        );
    }
    if busy || local {
        ui.tree.add(row, Node::text("…", 11.0).ink(Token::Muted));
        return;
    }
    if v.reminders {
        let b = look::button(ui, row, "Resend", Kind::Plain, true);
        ui.tree.edit(b, |n| {
            n.tooltip = Some("Remind them — the invite comes up again in their Copper".into());
        });
        c.hits.insert(b, Act::Resend(email.clone()));
    }
    let from_me = i
        .invited_by
        .as_ref()
        .is_some_and(|p| p.id.eq_ignore_ascii_case(&v.my_id));
    if v.reminders && (v.owner || from_me) {
        let x = ui.tree.add(
            row,
            Node::stack()
                .size(16.0, 16.0)
                .shrink(0.0)
                .radius(4.0)
                .hover_fill(Token::Hover)
                .align(Align::Center)
                .justify(copper_win_ui::Justify::Center)
                .button(&format!("Withdraw the invite to {}", name_for(m, &email)))
                .tooltip("Withdraw the invite"),
        );
        ui.tree.add(
            x,
            Node::glyph(crate::chrome::glyph::CLOSE, 8.0, 16.0)
                .ink(Token::Muted)
                .role(Role::None),
        );
        c.hits.insert(x, Act::Withdraw(i.id.clone()));
    }
}

fn people_rows(ui: &mut Ui, c: &mut Card, parent: NodeId, m: &Model, v: &View, query: &str) {
    let q = query.trim();
    let shown: Vec<&Person> = if v.directory && !q.is_empty() && m.searched.as_deref() == Some(q) {
        m.people.iter().take(6).collect()
    } else {
        vec![]
    };
    let typed = (copy::is_email(q)
        && !shown.iter().any(|p| {
            p.email
                .as_deref()
                .is_some_and(|e| e.eq_ignore_ascii_case(q))
        }))
    .then(|| q.to_string());
    if typed.is_none() && shown.is_empty() {
        return;
    }
    let col = ui.tree.add(parent, Node::column().gap(6.0));
    if let Some(e) = &typed {
        person_row(
            ui,
            c,
            col,
            m,
            v,
            Who {
                name: None,
                email: e,
                id: None,
            },
        );
    }
    for p in shown {
        let email = p.email.clone().unwrap_or_default();
        let name = p.display_name.clone().filter(|n| !n.is_empty());
        let who = Who {
            name: name.as_deref(),
            email: &email,
            id: Some(&p.id),
        };
        person_row(ui, c, col, m, v, who);
    }
}

fn members_section(ui: &mut Ui, c: &mut Card, parent: NodeId, m: &Model, v: &View) {
    let col = ui.tree.add(parent, Node::column().gap(4.0));
    ui.tree.add(
        col,
        Node::text("Members", 11.0)
            .weight(Weight::Semibold)
            .ink(Token::Muted),
    );
    for member in &m.members {
        let name = member
            .display_name
            .clone()
            .filter(|n| !n.is_empty())
            .or_else(|| member.email.clone())
            .unwrap_or_default();
        let row = ui.tree.add(
            col,
            Node::row()
                .gap(8.0)
                .align(Align::Center)
                .role(Role::ListItem)
                .name(&name),
        );
        ui.tree.add(
            row,
            Node::text(&name, 11.5)
                .ink(Token::Ink)
                .grow(1.0)
                .shrink(1.0),
        );
        ui.tree.add(
            row,
            Node::text(&copy::role_word(&member.role), 10.0).ink(Token::Muted),
        );
        if v.owner && member.role != "owner" {
            let b = look::button(ui, row, "Remove", Kind::Plain, true);
            c.hits.insert(b, Act::Remove(member.user_id.clone()));
        }
    }
}

fn invited_section(ui: &mut Ui, c: &mut Card, parent: NodeId, m: &Model, v: &View) {
    if m.pending.is_empty() {
        return;
    }
    let col = ui.tree.add(parent, Node::column().gap(6.0));
    ui.tree.add(
        col,
        Node::text("Invited", 11.0)
            .weight(Weight::Semibold)
            .ink(Token::Muted),
    );
    for i in &m.pending {
        let email = i.email.clone().unwrap_or_default();
        let name = invite_name(i);
        let row = ui.tree.add(
            col,
            Node::row()
                .gap(8.0)
                .align(Align::Center)
                .role(Role::ListItem)
                .name(&name),
        );
        let words = ui
            .tree
            .add(row, Node::column().gap(1.0).grow(1.0).shrink(1.0));
        ui.tree.add(
            words,
            Node::text(&name, 11.5).ink(Token::Ink).trim(Trim::Middle),
        );
        if v.reminders && i.invitee.is_none() && !i.id.starts_with(LOCAL) {
            ui.tree.add(
                words,
                Node::text(&format!("No account on {} yet", v.host), 10.0)
                    .ink(Token::Muted)
                    .trim(Trim::Middle),
            );
        } else if name != email {
            ui.tree.add(
                words,
                Node::text(&email, 10.0)
                    .ink(Token::Muted)
                    .trim(Trim::Middle),
            );
        }
        invited(ui, c, row, m, v, i, false);
    }
}

// MARK: acting

/// The query changed: look people up (after a pause).
pub fn query_changed(shell: &Shell) {
    let (q, generation) = state::with(|c| {
        let q = c.texts.get(&FieldKind::Query).cloned().unwrap_or_default();
        let g = c.share.as_mut().map(|m| {
            m.generation += 1;
            m.generation
        });
        (q.trim().to_string(), g)
    });
    let Some(generation) = generation else { return };
    // The typed address row comes and goes at once.
    door::render(shell);
    let directory = copper_shell_core::cloud::get()
        .and_then(|c| c.client.supports(Feature::People))
        != Some(false);
    if q.is_empty() || !directory {
        return;
    }
    let Some(api) = ops::api() else { return };
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(250)).await;
        let still = state::with(|c| c.share.as_ref().is_some_and(|m| m.generation == generation));
        if !still {
            return;
        }
        let asked = q.clone();
        let found = cloud_call(async move { api.people(Some(&asked), Some(6)).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        let changed = state::with(|c| match c.share.as_mut() {
            Some(m) if m.generation == generation => {
                m.people = found.unwrap_or_default();
                m.searched = Some(q);
                true
            }
            _ => false,
        });
        if changed {
            door::render(&shell);
        }
    });
}

/// Return in the field: invite the address typed (or the one person found).
pub fn submit(shell: &Shell) {
    let target = state::with(|c| {
        let q = c.texts.get(&FieldKind::Query).cloned().unwrap_or_default();
        let q = q.trim();
        if copy::is_email(q) {
            return Some(q.to_string());
        }
        let m = c.share.as_ref()?;
        (m.searched.as_deref() == Some(q) && m.people.len() == 1)
            .then(|| m.people[0].email.clone())
            .flatten()
    });
    match target {
        Some(e) => invite(shell, &e, false),
        None => {
            state::with(|c| {
                if let Some(m) = c.share.as_mut() {
                    m.said = Some(("That isn't an email address.".into(), true));
                }
            });
            door::render(shell);
        }
    }
}

/// A click on one of the card's Share controls. `true` if it was one.
pub fn act(shell: &Shell, a: &Act) -> bool {
    match a {
        Act::CopyLink => copy_link(shell, false),
        Act::CopyWeb => copy_link(shell, true),
        Act::ResetLink => reset(shell),
        Act::InviteTo(email) => invite(shell, email, false),
        Act::Resend(email) => invite(shell, email, true),
        Act::Withdraw(id) => withdraw(shell, id),
        Act::Remove(user) => remove(shell, user),
        Act::NewShared => door::set_mode(shell, Mode::New),
        Act::Connect => {
            door::close(shell);
            crate::settings::open(shell, "cloud");
        }
        _ => return false,
    }
    true
}

fn current(shell: &Shell) -> Option<(String, String, String)> {
    let _ = shell;
    let id = state::with(|c| c.share.as_ref().map(|m| m.id.clone()))?;
    let e = registry::entry(&id)?;
    Some((id, e.remote_id?, e.name))
}

fn host_text() -> String {
    copy::cloud_host(
        copper_shell_core::cloud::get()
            .and_then(|c| c.client.link())
            .map(|l| l.host())
            .as_deref(),
    )
}

fn copy_link(shell: &Shell, web: bool) {
    let Some((id, remote, name)) = current(shell) else {
        return;
    };
    let host = host_text();
    let cached = state::with(|c| c.share.as_ref().and_then(|m| m.token.clone()));
    if let Some(t) = cached {
        let (copper, https) = copy::share_links(&t, &host, &name);
        shell.view().copy_text(if web { &https } else { &copper });
        state::with(|c| {
            if let Some(m) = c.share.as_mut() {
                m.copied = true;
            }
        });
        door::render(shell);
        return;
    }
    let Some(api) = ops::api() else { return };
    state::with(|c| {
        if let Some(m) = c.share.as_mut() {
            m.working = true;
            m.link_failure = None;
        }
    });
    door::render(shell);
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let r = remote.clone();
        let made = cloud_call(async move { api.create_share_link(&r).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        let token = match made {
            Ok(l) => {
                save_token(&shell, &remote, Some(&l.token));
                let (copper, _) = copy::share_links(&l.token, &host, &name);
                shell.view().copy_text(&copper);
                Ok(l.token)
            }
            Err(f) if f.status == 404 => Err(copy::links_unsupported(
                &host,
                copper_shell_core::cloud::get()
                    .and_then(|c| c.client.server_version())
                    .as_deref(),
            )),
            Err(f) => Err(copy::plain(&f, &Doing::Linking, &host)),
        };
        state::with(|c| {
            if let Some(m) = c.share.as_mut().filter(|m| m.id == id) {
                m.working = false;
                match token {
                    Ok(t) => {
                        m.token = Some(t);
                        m.copied = true;
                    }
                    Err(e) => m.link_failure = Some(e),
                }
            }
        });
        door::render(&shell);
    });
}

fn reset(shell: &Shell) {
    let Some((id, remote, _)) = current(shell) else {
        return;
    };
    let Some(api) = ops::api() else { return };
    state::with(|c| {
        if let Some(m) = c.share.as_mut() {
            m.working = true;
            m.link_failure = None;
        }
    });
    door::render(shell);
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let r = remote.clone();
        let done = cloud_call(async move { api.revoke_share_links(&r, None).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        let host = host_text();
        state::with(|c| {
            if let Some(m) = c.share.as_mut().filter(|m| m.id == id) {
                m.working = false;
                match &done {
                    Ok(()) => {
                        m.token = None;
                        m.copied = false;
                    }
                    Err(f) => m.link_failure = Some(copy::plain(f, &Doing::Resetting, &host)),
                }
            }
        });
        if done.is_ok() {
            save_token(&shell, &remote, None);
        }
        door::render(&shell);
    });
}

/// Invite `email` (or, `again`, remind them): the row says Invited at once.
pub fn invite(shell: &Shell, email: &str, again: bool) {
    let Some((id, remote, canvas_name)) = current(shell) else {
        return;
    };
    let address = email.trim().to_string();
    let key = address.to_ascii_lowercase();
    let host = host_text();
    let Some(cloud) = copper_shell_core::cloud::get() else {
        return;
    };
    let (who, nobody, nudged_before) = state::with(|c| {
        let Some(m) = c.share.as_mut() else {
            return (address.clone(), false, false);
        };
        let who = name_for(m, &address);
        let nobody = cloud.client.supports(Feature::People) == Some(true)
            && m.searched.as_deref().map(|s| s.trim().to_ascii_lowercase()) == Some(key.clone())
            && !m.people.iter().any(|p| {
                p.email
                    .as_deref()
                    .is_some_and(|e| e.eq_ignore_ascii_case(&key))
            });
        let before = m.pending.iter().any(|i| {
            i.email
                .as_deref()
                .is_some_and(|e| e.eq_ignore_ascii_case(&key))
                && i.nudged_at.is_some()
        });
        m.acting.insert(key.clone());
        m.said = None;
        if !again {
            m.pending.insert(
                0,
                Invite {
                    id: format!("{LOCAL}{key}"),
                    canvas_id: Some(remote.clone()),
                    canvas_name: Some(canvas_name.clone()),
                    email: Some(address.clone()),
                    status: Some("pending".into()),
                    invited_by: None,
                    invitee: None,
                    created_at: None,
                    nudged_at: None,
                    nudged: None,
                },
            );
        }
        (who, nobody, before)
    });
    door::render(shell);
    let weak = shell.downgrade();
    let client = cloud.client.clone();
    let reminders = client.supports(Feature::InviteReminders) == Some(true);
    copper_win_rt::spawn_local(async move {
        let path = format!("/v1/canvases/{}/invites", ops::path_id(&remote));
        let body = json!({"email": address});
        let answer =
            cloud_call(async move { client.request("POST", &path, Some(body), &[]).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        let stand_in = format!("{LOCAL}{key}");
        state::with(|c| {
            let Some(m) = c.share.as_mut().filter(|m| m.id == id) else {
                return;
            };
            m.acting.remove(&key);
            match answer {
                Ok(a) => {
                    let row: Option<Invite> = serde_json::from_slice(&a.body).ok();
                    let outcome = Outcome {
                        made: a.status == 201,
                        nudged: row.as_ref().and_then(|r| r.nudged),
                        invitee: row.as_ref().and_then(|r| r.invitee.as_ref()).map(|p| {
                            p.display_name
                                .clone()
                                .or(p.email.clone())
                                .unwrap_or_default()
                        }),
                        invitee_known: reminders,
                        nudged_before,
                    };
                    if let Some(made) = row
                        && let Some(at) = m
                            .pending
                            .iter()
                            .position(|i| i.id == stand_in || i.id == made.id)
                    {
                        m.pending[at] = made;
                    }
                    m.said = Some((
                        copy::sheet_line(&outcome, &address, &who, nobody, &host),
                        false,
                    ));
                    if !again {
                        c.note = Some(format!("Invited {address}"));
                    }
                }
                Err(f) => {
                    m.pending.retain(|i| i.id != stand_in);
                    let doing = if again {
                        Doing::Reminding(who.clone())
                    } else {
                        Doing::Inviting(address.clone())
                    };
                    m.said = Some((copy::plain(&f, &doing, &host), true));
                }
            }
        });
        door::render(&shell);
        reload(&shell, &id);
    });
}

fn withdraw(shell: &Shell, invite_id: &str) {
    let Some((id, remote, _)) = current(shell) else {
        return;
    };
    let Some(api) = ops::api() else { return };
    let gone = state::with(|c| {
        let m = c.share.as_mut()?;
        let at = m.pending.iter().position(|i| i.id == invite_id)?;
        let i = m.pending.remove(at);
        m.said = None;
        Some((invite_name(&i), i))
    });
    let Some((who, gone)) = gone else { return };
    door::render(shell);
    let weak = shell.downgrade();
    let invite_id = invite_id.to_string();
    let host = host_text();
    copper_win_rt::spawn_local(async move {
        let done = cloud_call(async move { api.revoke_invite(&remote, &invite_id).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        state::with(|c| {
            if let Some(m) = c.share.as_mut().filter(|m| m.id == id) {
                match done {
                    Ok(()) => m.said = Some((format!("Withdrew the invite to {who}"), false)),
                    Err(f) => {
                        m.pending.insert(0, gone);
                        m.said = Some((
                            copy::plain(&f, &Doing::Withdrawing(who.clone()), &host),
                            true,
                        ));
                    }
                }
            }
        });
        door::render(&shell);
    });
}

fn remove(shell: &Shell, user: &str) {
    let Some((id, remote, _)) = current(shell) else {
        return;
    };
    let Some(api) = ops::api() else { return };
    let name = state::with(|c| {
        c.share.as_ref().and_then(|m| {
            m.members.iter().find(|x| x.user_id == user).map(|x| {
                x.display_name
                    .clone()
                    .filter(|n| !n.is_empty())
                    .or(x.email.clone())
                    .unwrap_or_default()
            })
        })
    })
    .unwrap_or_default();
    let weak = shell.downgrade();
    let user = user.to_string();
    let host = host_text();
    copper_win_rt::spawn_local(async move {
        let done = cloud_call(async move { api.remove_member(&remote, &user).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        if let Err(f) = done {
            state::with(|c| {
                if let Some(m) = c.share.as_mut().filter(|m| m.id == id) {
                    m.said = Some((copy::plain(&f, &Doing::Removing(name), &host), true));
                }
            });
            door::render(&shell);
        } else {
            core::refresh_soon::<Win>();
            reload(&shell, &id);
        }
    });
}
