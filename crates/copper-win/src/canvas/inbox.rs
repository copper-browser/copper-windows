//! What is waiting for the person signed in (`Canvases.invites`,
//! `CanvasChat.mentions`, `CanvasPresence`): the invitations to join a
//! canvas, the unread mentions of them in canvas chats, and who is live in
//! the canvases open here. Read on sign-in, on the cloud's `canvas` events,
//! when the card opens, and every couple of seconds for presence; a change
//! redraws the door, the card, the pills and the palette.

use super::copy::Mention;
use super::ops::{api, cloud_call};
use super::{notify, pills};
use crate::platform::{Shell, Win};
use copper_cloud::Feature;
use copper_cloud::canvas::Invite;
use copper_shell_core::canvas::{host, registry};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::Duration;

/// Who is live where, and which rooms are open and synced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Presence {
    /// Canvas id → other people in its room now.
    pub peers: BTreeMap<String, usize>,
    pub open: BTreeSet<String>,
    pub synced: BTreeSet<String>,
}

#[derive(Default)]
struct Inbox {
    /// The account the lists are for.
    account: Option<String>,
    invites: Vec<Invite>,
    /// Invite id → its `nudged_at` when last read.
    nudges: HashMap<String, Option<String>>,
    mentions: Vec<Mention>,
    /// Mentions already raised (pill, balloon).
    raised: HashSet<String>,
    presence: Presence,
    /// The registry's look last drawn.
    listed: u64,
    reading: bool,
    again: bool,
}

thread_local! {
    static INBOX: RefCell<Inbox> = RefCell::new(Inbox::default());
    static TICKING: Cell<bool> = const { Cell::new(false) };
}

fn with<R>(f: impl FnOnce(&mut Inbox) -> R) -> R {
    INBOX.with(|i| f(&mut i.borrow_mut()))
}

/// The invitations waiting, newest first.
pub fn invites() -> Vec<Invite> {
    with(|i| i.invites.clone())
}

pub fn mention_total() -> usize {
    with(|i| i.mentions.len())
}

/// Unread mentions of you in canvas `id`'s chat.
pub fn mention_count(id: &str) -> usize {
    with(|i| i.mentions.iter().filter(|m| m.canvas_id == id).count())
}

pub fn presence_now() -> Presence {
    with(|i| i.presence.clone())
}

/// Who sent an invite, as a person reads it.
pub fn sender(i: &Invite) -> String {
    i.invited_by
        .as_ref()
        .and_then(|p| {
            p.display_name
                .clone()
                .filter(|n| !n.is_empty())
                .or_else(|| p.email.clone())
        })
        .unwrap_or_default()
}

fn account() -> Option<String> {
    copper_shell_core::cloud::get()
        .and_then(|c| c.client.account())
        .map(|a| a.user_id.to_ascii_lowercase())
}

/// Start following the cloud (after canvas started).
pub fn start(shell: &Shell) {
    let Some(cloud) = copper_shell_core::cloud::get() else {
        return;
    };
    let mut status = cloud.client.watch_status();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let mut was = (
            status.borrow().signed_in,
            status.borrow().account.clone().map(|a| a.user_id),
        );
        if let Some(s) = weak.upgrade() {
            refresh_soon(&s);
        }
        while status.changed().await.is_ok() {
            let now = (
                status.borrow().signed_in,
                status.borrow().account.clone().map(|a| a.user_id),
            );
            if now == was {
                continue;
            }
            was = now;
            let Some(s) = weak.upgrade() else { break };
            refresh_soon(&s);
            if was.0 {
                super::serve::resume_join(&s);
            }
        }
    });
    let mut events = cloud.client.subscribe();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        loop {
            let again = match events.recv().await {
                Ok(e) => e.kind == "canvas" || e.kind == "open" || e.kind == "resync",
                // Lagged: something was missed, read again; closed: done.
                Err(e) if e.to_string().contains("lagged") => true,
                Err(_) => break,
            };
            if again {
                let Some(s) = weak.upgrade() else { break };
                refresh_soon(&s);
            }
        }
    });
    tick(shell);
}

/// Read invites and mentions soon (coalesced).
pub fn refresh_soon(shell: &Shell) {
    if with(|i| {
        if i.reading {
            i.again = true;
        }
        i.reading
    }) {
        return;
    }
    with(|i| i.reading = true);
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(250)).await;
        loop {
            with(|i| i.again = false);
            let Some(s) = weak.upgrade() else { return };
            read(&s).await;
            if !with(|i| i.again) {
                break;
            }
        }
        with(|i| i.reading = false);
    });
}

async fn read(shell: &Shell) {
    let me = account();
    // Another account (or none): its invites and mentions are not this one's.
    let switched = with(|i| {
        if i.account != me {
            i.account = me.clone();
            i.invites.clear();
            i.nudges.clear();
            i.mentions.clear();
            i.raised.clear();
            true
        } else {
            false
        }
    });
    if switched {
        pills::clear(shell);
        super::refresh_all(shell);
    }
    let Some(api) = api().filter(|_| registry::cloud_ready()) else {
        return;
    };
    let mentions_on = copper_shell_core::cloud::get()
        .and_then(|c| c.client.supports(Feature::Mentions))
        != Some(false);
    let a = api.clone();
    let invites = cloud_call(async move { a.my_invites().await }).await;
    let mentions = if mentions_on {
        let a = api.clone();
        Some(cloud_call(async move { a.mentions(true, 50).await }).await)
    } else {
        None
    };
    // Signed out or switched meanwhile: the old account's, which go nowhere.
    if account() != me {
        with(|i| i.again = true);
        return;
    }
    let mut fresh_invite: Option<Invite> = None;
    if let Ok(list) = invites {
        let list: Vec<Invite> = list
            .into_iter()
            .filter(|i| i.status.as_deref().is_none_or(|s| s == "pending"))
            .collect();
        fresh_invite = with(|i| {
            let before = std::mem::take(&mut i.nudges);
            i.nudges = list
                .iter()
                .map(|x| (x.id.clone(), x.nudged_at.clone()))
                .collect();
            i.invites = list.clone();
            // New since the last read (every one, the first time), or
            // reminded since.
            list.into_iter().find(|x| match before.get(&x.id) {
                None => true,
                Some(seen) => x.nudged_at.is_some() && x.nudged_at != *seen,
            })
        });
    }
    let mut fresh_mentions: Vec<Mention> = vec![];
    if let Some(Ok(rows)) = mentions {
        let list: Vec<Mention> = rows.iter().filter_map(Mention::parse).collect();
        fresh_mentions = with(|i| {
            let fresh: Vec<Mention> = list
                .iter()
                .filter(|m| !i.raised.contains(&m.id))
                .cloned()
                .collect();
            for m in &fresh {
                i.raised.insert(m.id.clone());
            }
            i.mentions = list;
            fresh
        });
    }
    pills::sync(shell, fresh_invite.as_ref());
    for m in &fresh_mentions {
        pills::mention(shell, m);
        notify::mention(shell, m);
    }
    super::refresh_all(shell);
}

/// An invitation answered: off the pill, the list and the palette at once.
pub fn drop_invite(shell: &Shell, id: &str) {
    with(|i| {
        i.invites.retain(|x| x.id != id);
        i.nudges.remove(id);
    });
    pills::sync(shell, None);
    super::refresh_all(shell);
}

/// Every invitation to canvas `canvas` goes (joined by a link).
pub fn drop_canvas_invites(shell: &Shell, canvas: &str) {
    let ids: Vec<String> = with(|i| {
        i.invites
            .iter()
            .filter(|x| x.canvas_id.as_deref() == Some(canvas))
            .map(|x| x.id.clone())
            .collect()
    });
    for id in ids {
        drop_invite(shell, &id);
    }
}

/// A canvas gone from this computer: its mentions go too.
pub fn forget_canvas(id: &str) {
    with(|i| i.mentions.retain(|m| m.canvas_id != id));
}

/// Mentions read (the canvas's chat was read, or one was opened): marked
/// on the cloud and gone from the door and the pills.
pub fn mark_read(shell: &Shell, ids: &[String], canvas: Option<&str>) {
    let ids: Vec<String> = with(|i| {
        let wanted: Vec<String> = i
            .mentions
            .iter()
            .filter(|m| ids.contains(&m.id) || canvas.is_some_and(|c| m.canvas_id == c))
            .map(|m| m.id.clone())
            .collect();
        i.mentions.retain(|m| !wanted.contains(&m.id));
        wanted
    });
    if ids.is_empty() {
        return;
    }
    pills::mentions_read(shell, &ids);
    super::refresh_all(shell);
    let Some(api) = api() else { return };
    copper_win_rt::spawn_local(async move {
        let r = cloud_call(async move { api.mark_mentions_read(&ids, None).await }).await;
        if let Err(f) = r {
            tracing::debug!("mentions read: {}", f.message);
        }
    });
}

// MARK: presence

fn read_presence() -> Presence {
    let mut p = Presence::default();
    for h in host::all::<Win>() {
        let id = h.canvas_id();
        let others = h.collaborators();
        let e = p.peers.entry(id.clone()).or_default();
        *e = (*e).max(others);
        if h.room_open() {
            p.open.insert(id.clone());
        }
        if h.room_synced() {
            p.synced.insert(id);
        }
    }
    p.peers.retain(|_, n| *n > 0);
    p
}

/// What the card's list shows of the registry, as one number.
fn listed() -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    registry::with(|r| {
        for e in &r.all {
            (
                &e.id, &e.name, &e.kind, &e.role, &e.members, &e.account, &e.owner,
            )
                .hash(&mut h);
            e.updated_at.0.hash(&mut h);
        }
        r.problem.hash(&mut h);
    });
    registry::cloud_ready().hash(&mut h);
    account().hash(&mut h);
    h.finish()
}

/// Every two seconds: presence and the list; a change redraws.
fn tick(shell: &Shell) {
    if TICKING.with(|t| t.replace(true)) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        loop {
            copper_win_rt::sleep(Duration::from_secs(2)).await;
            let Some(s) = weak.upgrade() else { break };
            let p = read_presence();
            let l = listed();
            let changed = with(|i| {
                let c = i.presence != p || i.listed != l;
                i.presence = p;
                i.listed = l;
                c
            });
            if changed {
                super::refresh_all(&s);
            }
        }
        TICKING.with(|t| t.set(false));
    });
}
