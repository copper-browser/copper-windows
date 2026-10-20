//! What Settings › Passwords shows that has to be asked for: how many
//! sign-ins Credential Manager keeps (or why it couldn't say), where
//! Bitwarden and 1Password stand, 1Password's accounts and vaults, and the
//! accounts, identities and cards agents may be given.
//!
//! The page draws from the copy kept here and never waits. A look is
//! started when the page is drawn and the copy is older than [`STALE`],
//! after anything the page does, and every [`TICK`] while the page is on
//! screen ([`watch`]) — so a site told "never" elsewhere, a vault locked by
//! its own timer, or a sign-in saved while Settings is open shows up
//! without a click. A vault busy with a long `bw` / `op` call (an app
//! approval waits up to two minutes) is not waited on: its last answer
//! stands until it is free.

use super::super::redraw;
use crate::platform::Shell;
use copper_creds::credential::{AutofillCard, AutofillIdentity, Credential};
use copper_creds::onepassword::Problem;
use copper_shell_core::forms::vault::Snapshot;
use copper_shell_core::forms::vault::Vault;
use copper_shell_core::forms::{self, rt};
use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A copy older than this is looked at again when the page is drawn.
const STALE: Duration = Duration::from_secs(5);
/// How often the page looks again while it is on screen.
const TICK: Duration = Duration::from_millis(1500);

/// 1Password beyond the shared snapshot: what the account and vault
/// choices need.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpInfo {
    /// `(user id, "email · host")` of every account `op` knows here.
    pub accounts: Vec<(String, String)>,
    /// Every vault read.
    pub vaults: Vec<String>,
    /// `(id, name)` of the vaults new logins may go in.
    pub writable: Vec<(String, String)>,
    /// The vault new logins go in now.
    pub save: Option<String>,
    /// What the last failure was, as a kind (the guide for the app
    /// integration hangs on it).
    pub problem: Option<Problem>,
    /// Whether the 1Password app is installed (offers its sign-in path).
    pub app: bool,
}

/// One row of agent access: an identity or a card.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// `bw:<id>` / `op:<id>`: what the allow list keeps.
    pub stable: String,
    pub title: String,
    pub detail: String,
}

/// The copy the page draws from.
#[derive(Clone, Debug, Default)]
pub struct Live {
    pub snap: Snapshot,
    /// Credential Manager's count of Copper's sign-ins, or why it couldn't
    /// be read. `None`: not asked yet.
    pub kept: Option<Result<usize, String>>,
    pub op: OpInfo,
    /// Every account agents could be given, merged across sources.
    pub accounts: Vec<Credential>,
    pub identities: Vec<Item>,
    pub cards: Vec<Item>,
    /// When the last look finished.
    fetched: Option<Instant>,
    looking: bool,
    /// Something changed during a look: look once more after it.
    again: bool,
    watching: bool,
}

impl Live {
    /// Everything the page draws from, compared between looks.
    fn fingerprint(&self) -> String {
        format!(
            "{:?}{:?}{:?}{:?}{:?}{:?}",
            self.snap, self.kept, self.op, self.accounts, self.identities, self.cards
        )
    }
}

thread_local! {
    static LIVE: RefCell<Live> = RefCell::new(Live::default());
}

/// Read the copy.
pub fn with<R>(f: impl FnOnce(&Live) -> R) -> R {
    LIVE.with(|l| f(&l.borrow()))
}

/// The shared snapshot as it is this moment (it never waits), into the
/// copy; the slower parts follow from a look.
pub fn snapshot() -> Snapshot {
    let snap = forms::vault().map(|v| v.snapshot()).unwrap_or_default();
    LIVE.with(|l| l.borrow_mut().snap = snap.clone());
    snap
}

/// Whether a look is under way.
pub fn looking() -> bool {
    LIVE.with(|l| l.borrow().looking)
}

/// Look again if the copy is stale (from `draw`).
pub fn freshen(shell: &Shell) {
    let stale = LIVE.with(|l| {
        let l = l.borrow();
        !l.looking && l.fetched.is_none_or(|t| t.elapsed() >= STALE)
    });
    if stale {
        look(shell);
    }
    watch(shell);
}

/// Look now, then draw the page again.
pub fn look(shell: &Shell) {
    let Some(v) = forms::vault() else { return };
    let busy = LIVE.with(|l| {
        let mut l = l.borrow_mut();
        l.again |= l.looking;
        std::mem::replace(&mut l.looking, true)
    });
    if busy {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let app = rt::blocking(op_app).await.flatten().is_some();
        let fetched = rt::run(async move { gather(&v).await }).await;
        let (again, moved) = LIVE.with(|l| {
            let mut l = l.borrow_mut();
            let before = l.fingerprint();
            l.looking = false;
            l.fetched = Some(Instant::now());
            if let Some(f) = fetched {
                l.snap = f.snap;
                l.kept = Some(f.kept);
                if let Some(mut op) = f.op {
                    op.app = app;
                    l.op = op;
                } else {
                    l.op.app = app;
                }
                if let Some((accounts, identities, cards)) = f.agents {
                    l.accounts = accounts;
                    l.identities = identities;
                    l.cards = cards;
                }
            }
            (std::mem::take(&mut l.again), l.fingerprint() != before)
        });
        if let Some(s) = weak.upgrade() {
            // Only a change is drawn: a box being typed in keeps its caret.
            if moved {
                redraw(&s, "passwords");
            }
            if again {
                look(&s);
            }
        }
    });
}

/// What one look found. `None` parts: that vault was busy.
struct Fetched {
    snap: Snapshot,
    kept: Result<usize, String>,
    op: Option<OpInfo>,
    agents: Option<(Vec<Credential>, Vec<Item>, Vec<Item>)>,
}

/// On the lane's runtime: everything the page shows, without waiting on a
/// vault that is mid-call.
async fn gather(v: &Arc<Vault>) -> Fetched {
    let kept = v.logins().await.map(|l| l.len());
    let free = v.bw.try_lock().is_ok() && v.op.try_lock().is_ok();
    if free {
        v.refresh().await;
    }
    let op = v.op.try_lock().ok().map(|op| {
        let (vaults, writable) = op.vaults();
        OpInfo {
            accounts: op
                .accounts()
                .iter()
                .map(|a| (a.user_id.clone(), account_label(&a.email, &a.host())))
                .collect(),
            vaults: vaults.iter().map(|v| v.name.clone()).collect(),
            writable: writable
                .iter()
                .map(|v| (v.id.clone(), v.name.clone()))
                .collect(),
            save: op.save_vault().map(|v| v.id.clone()),
            problem: op.problem().map(|p| p.problem),
            app: false,
        }
    });
    let agents = if free {
        let accounts = v.all_credentials().await;
        let identities = v.identities().await.iter().map(identity_item).collect();
        let cards = v.cards().await.iter().map(card_item).collect();
        Some((accounts, identities, cards))
    } else {
        None
    };
    Fetched {
        snap: v.snapshot(),
        kept,
        op,
        agents,
    }
}

/// `you@example.com · my.1password.com`, or whichever half there is.
fn account_label(email: &str, host: &str) -> String {
    match (email.is_empty(), host.is_empty()) {
        (false, false) => format!("{email} · {host}"),
        (false, true) => email.to_string(),
        _ => host.to_string(),
    }
}

/// The id agent access keeps for a vault item: 1Password's already says
/// where it is from; Bitwarden's gets `bw:`.
pub fn stable_id(id: &str) -> String {
    if id.starts_with("op:") {
        id.to_string()
    } else {
        format!("bw:{id}")
    }
}

/// Which vault a stable id lives in.
fn vault_name(stable: &str) -> &'static str {
    if stable.starts_with("op:") {
        "1Password"
    } else {
        "Bitwarden"
    }
}

fn identity_item(i: &AutofillIdentity) -> Item {
    let stable = stable_id(&i.id);
    let full = [&i.first_name, &i.middle_name, &i.last_name]
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| p.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let summary = [&i.email, &i.city, &i.company]
        .iter()
        .find(|p| !p.is_empty())
        .map(|p| format!("{p} · "))
        .unwrap_or_default();
    Item {
        detail: format!("{summary}Identity · {}", vault_name(&stable)),
        title: if full.is_empty() {
            i.name.clone()
        } else {
            full
        },
        stable,
    }
}

fn card_item(c: &AutofillCard) -> Item {
    let stable = stable_id(&c.id);
    let holder = if c.cardholder_name.is_empty() {
        String::new()
    } else {
        format!("{} · ", c.cardholder_name)
    };
    Item {
        detail: format!("{holder}Card · {}", vault_name(&stable)),
        title: c.name.clone(),
        stable,
    }
}

/// While the page is on screen, look again every [`TICK`] and draw it
/// when something it shows changed. Stops when Settings closes or goes to
/// another page.
fn watch(shell: &Shell) {
    if LIVE.with(|l| std::mem::replace(&mut l.borrow_mut().watching, true)) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let mut seen = None;
        loop {
            copper_win_rt::sleep(TICK).await;
            let Some(s) = weak.upgrade() else { break };
            if crate::settings::showing(&s) != Some("passwords") {
                break;
            }
            // The never list and the vault states are read without
            // waiting; anything else changes through a look.
            let now = (
                forms::never_list(&s),
                format!("{:?}", forms::vault().map(|v| v.snapshot())),
            );
            if seen.as_ref().is_some_and(|before| *before != now) {
                redraw(&s, "passwords");
            }
            seen = Some(now);
            freshen_quietly(&s);
        }
        LIVE.with(|l| l.borrow_mut().watching = false);
    });
}

/// A stale copy is looked at again; no new watch.
fn freshen_quietly(shell: &Shell) {
    let stale = LIVE.with(|l| {
        let l = l.borrow();
        !l.looking && l.fetched.is_none_or(|t| t.elapsed() >= STALE)
    });
    if stale {
        look(shell);
    }
}

/// The 1Password app, if it is installed for this Windows account or for
/// everyone: `…\1Password\app\<n>\1Password.exe`.
pub fn op_app() -> Option<std::path::PathBuf> {
    let roots = ["LOCALAPPDATA", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|r| std::path::PathBuf::from(r).join("1Password").join("app"));
    for root in roots {
        let Ok(dirs) = std::fs::read_dir(&root) else {
            continue;
        };
        let mut found: Vec<_> = dirs
            .flatten()
            .map(|d| d.path().join("1Password.exe"))
            .filter(|p| p.is_file())
            .collect();
        found.sort();
        if let Some(p) = found.pop() {
            return Some(p);
        }
    }
    None
}

/// Run `work` with the vault on the lane's runtime, then `then` on the
/// UI thread with its answer (`None` when the lane never started or the
/// runtime dropped it). A look follows, so the page shows what changed.
pub fn run<T, F>(
    shell: &Shell,
    work: impl FnOnce(Arc<Vault>) -> F + 'static,
    then: impl FnOnce(&Shell, Option<T>) + 'static,
) where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let vault = forms::vault();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let answer = match vault {
            Some(v) => rt::run(work(v)).await,
            None => None,
        };
        let Some(s) = weak.upgrade() else { return };
        then(&s, answer);
        look(&s);
    });
}
