//! Settings › Passwords › 1Password (`OnePasswordSettings.swift`
//! `OnePasswordCard`): three ways in — the 1Password app (approval in
//! 1Password's own window, offered when the app is on this PC), the account
//! password, or a service account token — then the vaults, where new
//! logins go, and Lock / Sign out / Sync now. Everything runs through the
//! 1Password CLI (`op`) on the forms lane ([`copper_creds::onepassword`]);
//! the card owns only what is being typed.
//!
//! The Secret Key, passwords and tokens typed here live in this card's
//! memory until the step that needs them is done; `op` keeps the account,
//! and a kept session or token sits in a file in your Windows profile
//! folder — never in `settings.json`, never in a log.

use super::live;
use crate::platform::Shell;
use crate::settings::form::{Pill, Section, Tone};
use crate::settings::pages::redraw;
use copper_creds::Source;
use copper_creds::onepassword::{Mode, OpState, Problem, Remembered};
use copper_shell_core::forms::vault::Snapshot;
use copper_shell_core::forms::{self, keys};
use std::cell::RefCell;
use std::sync::Arc;

/// The CLI's own winget package.
pub const INSTALL: &str = "winget install AgileBits.1Password.CLI";
/// Where 1Password accounts are, unless the person says otherwise.
const DEFAULT_ADDRESS: &str = "my.1password.com";
use crate::forms::OP_SAVE_VAULT as SAVE_VAULT;

mod acts;
mod views;

pub use acts::act;
use views::{locked, signed_out, unlocked};

/// What is typed into the card, and which way in is chosen.
#[derive(Default)]
struct Card {
    /// The chosen way in; `None`: the app when it is installed, else the
    /// password (or how it was connected last).
    path: Option<Mode>,
    /// The account chosen (its user id); `None`: the one in use, else the
    /// first.
    chosen: Option<String>,
    /// Adding an account `op` doesn't know yet.
    adding: bool,
    address: String,
    email: String,
    secret_key: String,
    password: String,
    token: String,
    busy: bool,
    /// The app is being asked (its approval can take a while).
    waiting: bool,
    /// When this session last read the vaults (Unix seconds).
    synced: Option<f64>,
    /// A failure the person dismissed: not shown again.
    dismissed: Option<String>,
}

thread_local! {
    static CARD: RefCell<Card> = RefCell::new(Card::default());
}

fn with<R>(f: impl FnOnce(&mut Card) -> R) -> R {
    CARD.with(|c| f(&mut c.borrow_mut()))
}

fn plain(act: &str, label: &str, enabled: bool) -> Pill {
    Pill {
        action: act.into(),
        label: label.into(),
        filled: false,
        enabled,
    }
}

/// The step's own pill, or "Working…" / "Waiting for 1Password…".
fn action(act: &str, label: &str, busy: bool, waiting: bool, enabled: bool) -> Pill {
    let label = match (busy, waiting) {
        (true, true) => "Waiting for 1Password…",
        (true, false) => "Working…",
        _ => label,
    };
    Pill {
        action: act.into(),
        label: label.into(),
        filled: !busy,
        enabled: enabled && !busy,
    }
}

/// How Copper is connected, as the account line ends.
fn summary(mode: Option<Mode>) -> &'static str {
    match mode {
        Some(Mode::App) => "through the 1Password app",
        Some(Mode::Password) => "with the account password",
        Some(Mode::Service) => "with a service account",
        None => "",
    }
}

/// `you@example.com · my.1password.com — with the account password`.
fn account_line(snap: &Snapshot) -> String {
    let head = snap.op_account.clone().unwrap_or_default();
    let how = summary(snap.op_mode);
    match (head.is_empty(), how.is_empty()) {
        (true, true) => "Signed in".into(),
        (true, false) => format!("Signed in {how}"),
        (false, true) => head,
        (false, false) => format!("{head} — {how}"),
    }
}

/// `Private, Shared — 12 logins · 1 card`.
fn vaults_line(names: &[String], (logins, identities, cards): (usize, usize, usize)) -> String {
    let mut parts = Vec::new();
    if logins > 0 {
        parts.push(crate::settings::count(logins, "login", "logins"));
    }
    if identities > 0 {
        parts.push(crate::settings::count(identities, "identity", "identities"));
    }
    if cards > 0 {
        parts.push(crate::settings::count(cards, "card", "cards"));
    }
    let what = if parts.is_empty() {
        "Nothing to fill yet".to_string()
    } else {
        parts.join(" · ")
    };
    if names.is_empty() {
        what
    } else {
        format!("{} — {what}", names.join(", "))
    }
}

/// What staying unlocked means for the way Copper is connected.
fn stay_detail(mode: Option<Mode>) -> &'static str {
    match mode {
        Some(Mode::Password) => {
            "The session is kept in a file in your Windows profile folder, beside Copper's data — off asks for the password once per launch. Takes effect from the next launch"
        }
        Some(Mode::Service) => {
            "Off, the vault stays locked at launch until you unlock it here. Takes effect from the next launch"
        }
        _ => {
            "Copper checks with 1Password at launch and opens when the app still approves it — off, it waits for you. Takes effect from the next launch"
        }
    }
}

/// The way in to show: chosen, else how it was connected, else the app
/// when it is installed.
fn path(snap: &Snapshot, app: bool) -> Mode {
    let chosen = with(|c| c.path);
    let p =
        chosen
            .or(snap.op_remembered.mode)
            .unwrap_or(if app { Mode::App } else { Mode::Password });
    if p == Mode::App && !app {
        Mode::Password
    } else {
        p
    }
}

/// Draw the card's lines into `sec`.
pub fn draw(shell: &Shell, sec: &mut Section, snap: &Snapshot) {
    let info = live::with(|l| l.op.clone());
    let (busy, waiting, dismissed, synced) =
        with(|c| (c.busy, c.waiting, c.dismissed.clone(), c.synced));
    match &snap.op_state {
        None => {
            sec.note("1Password", Some("Looking for the 1Password CLI…"));
        }
        Some(OpState::Missing) => {
            sec.value(
                "1Password",
                Some("Install the 1Password CLI to fill from your vaults"),
                "Not installed",
            )
            .anchor("onepassword.install");
            sec.pills(
                INSTALL,
                Some(if info.app {
                    "In a terminal. Then, in the 1Password app, turn on Settings › Developer › Integrate with 1Password CLI — the app then approves Copper's sign-ins"
                } else {
                    "In a terminal. Then sign in here with your account, or install the 1Password app to approve sign-ins in its own window"
                }),
                vec![plain("op.copy", "Copy", true), action("op.check", "Check again", busy, false, true)],
            )
            .mono();
        }
        Some(OpState::SignedOut) => signed_out(sec, snap, &info, busy, waiting),
        Some(OpState::Locked) => locked(sec, snap, &info, busy, waiting),
        Some(OpState::Unlocked) => unlocked(shell, sec, snap, &info, busy, synced),
    }
    let missing = snap.op_state == Some(OpState::Missing);
    if let Some(msg) = &snap.op_problem
        && info.problem != Some(Problem::IntegrationOff)
        && !missing
        && dismissed.as_ref() != Some(msg)
    {
        sec.pills(
            "1Password couldn't do that",
            Some(msg),
            vec![plain("op.dismiss", "Dismiss", true)],
        )
        .tone(Tone::Error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaults_read_like_the_mac() {
        assert_eq!(vaults_line(&[], (0, 0, 0)), "Nothing to fill yet");
        assert_eq!(
            vaults_line(&["Private".into(), "Shared".into()], (12, 0, 1)),
            "Private, Shared — 12 logins · 1 card"
        );
    }

    #[test]
    fn the_account_line_says_how() {
        let mut s = Snapshot::default();
        assert_eq!(account_line(&s), "Signed in");
        s.op_mode = Some(Mode::Service);
        assert_eq!(account_line(&s), "Signed in with a service account");
        s.op_account = Some("a@b.c · my.1password.com".into());
        assert_eq!(
            account_line(&s),
            "a@b.c · my.1password.com — with a service account"
        );
    }
}
