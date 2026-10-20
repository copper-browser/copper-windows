//! Settings › Passwords › Bitwarden (`CredentialsSettings.swift`
//! `BitwardenCard`): every state of the connection in one card — no CLI
//! (with the winget line to install it), signed out (server, email, master
//! password), the two-step code asked for one step at a time, locked, and
//! open (sync, lock, the account, the vault's counts, staying unlocked,
//! auto-lock). Everything runs through Bitwarden's own `bw` CLI on the
//! forms lane ([`copper_creds::bitwarden`]).
//!
//! The master password and a code live in this card's memory only until
//! the step that needs them is done or cancelled; neither is logged, and
//! neither goes near `settings.json`.

use super::live;
use crate::platform::Shell;
use crate::settings::form::{Pill, Section, Tone};
use crate::settings::pages::redraw;
use copper_creds::bitwarden::BwState;
use copper_shell_core::forms::keys;
use copper_shell_core::forms::vault::Snapshot;
use std::cell::RefCell;

/// The CLI's own winget package.
pub const INSTALL: &str = "winget install Bitwarden.CLI";
/// Bitwarden's own server, which `bw` uses when none is configured.
const DEFAULT_SERVER: &str = "https://vault.bitwarden.com";
use crate::forms::BW_AUTOLOCK as AUTOLOCK;

mod acts;

pub use acts::act;
/// What `copper_creds` says when the account wants a two-step code.
const NEEDS_CODE: &str = "This account needs a two-step code";
// Not here, because the shared `bw` lane can't do them yet: asking
// Bitwarden to email a code ("Email me a code" / "Send again") and a new
// device's emailed verification — `bw login` is run without a terminal, so
// its own prompts for both never appear. Their failures show as said.

/// Where a sign-in is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Step {
    /// Server, email and master password.
    #[default]
    Credentials,
    /// The account wants a code: which way does it get one?
    Method,
    /// A code from method `n` (0 authenticator, 1 email, 3 YubiKey).
    Code(u8),
}

/// What is typed into the card, and where it is.
#[derive(Default)]
struct Card {
    /// `None`: what Settings remembers.
    server: Option<String>,
    email: String,
    password: String,
    code: String,
    step: Step,
    busy: bool,
    error: Option<String>,
}

thread_local! {
    static CARD: RefCell<Card> = RefCell::new(Card::default());
}

fn with<R>(f: impl FnOnce(&mut Card) -> R) -> R {
    CARD.with(|c| f(&mut c.borrow_mut()))
}

/// The pill a step is for: the step's words, or "Working…" while `bw`
/// is busy.
fn action(action: &str, label: &str, filled: bool, busy: bool) -> Pill {
    Pill {
        action: action.into(),
        label: if busy {
            "Working…".into()
        } else {
            label.into()
        },
        filled: filled && !busy,
        enabled: !busy,
    }
}

fn plain(act: &str, label: &str, enabled: bool) -> Pill {
    Pill {
        action: act.into(),
        label: label.into(),
        filled: false,
        enabled,
    }
}

/// `3 logins · 1 card`, or that there is nothing yet.
fn counts((logins, identities, cards): (usize, usize, usize)) -> String {
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
    if parts.is_empty() {
        "Nothing saved yet".into()
    } else {
        parts.join(" · ")
    }
}

/// `just now` / `3 min ago` / `not yet`, from `bw`'s ISO time.
pub fn synced(iso: Option<&str>) -> String {
    let Some(at) = iso.and_then(copper_core::date::IsoDate::parse) else {
        return "not yet".into();
    };
    let now = copper_core::date::now_unix();
    if now - (at.unix() as f64) < 45.0 {
        return "just now".into();
    }
    crate::settings::pages::updates::ago(at.unix() as f64)
}

fn code_title(method: u8) -> &'static str {
    match method {
        1 => "Enter the code Bitwarden emailed",
        3 => "Touch your YubiKey",
        _ => "Enter your authenticator app code",
    }
}

fn code_detail(method: u8, email: &str) -> String {
    match method {
        1 => format!("The code in Bitwarden's newest email to {email}"),
        3 => "With the box focused, press the key so it types its code".into(),
        _ => "The six digits your authenticator app shows for Bitwarden right now".into(),
    }
}

/// Draw the card's lines into `sec`.
pub fn draw(shell: &Shell, sec: &mut Section, snap: &Snapshot) {
    let card = CARD.with(|c| {
        let c = c.borrow();
        (
            c.server.clone(),
            c.email.clone(),
            c.password.clone(),
            c.code.clone(),
            c.step,
            c.busy,
            c.error.clone(),
        )
    });
    let (server, email, password, code, step, busy, error) = card;
    match &snap.bw_state {
        None => {
            sec.note("Bitwarden", Some("Looking for the Bitwarden CLI…"));
        }
        Some(BwState::Missing) => {
            sec.value(
                "Bitwarden",
                Some("Install the Bitwarden CLI to connect an existing vault"),
                "Not installed",
            )
            .anchor("bitwarden.install");
            sec.pills(
                INSTALL,
                Some("In a terminal, then Check again"),
                vec![
                    plain("bw.copy", "Copy", true),
                    action("bw.check", "Check again", false, busy),
                ],
            )
            .mono();
        }
        Some(BwState::Unauthenticated) => {
            sec.value(
                "Bitwarden",
                Some("Sign in to fill and save from an existing vault"),
                "Not connected",
            );
            match step {
                Step::Credentials => {
                    let remembered = shell
                        .settings()
                        .string(keys::BW_SERVER)
                        .map(str::to_string)
                        .or_else(|| snap.bw_server.clone())
                        .unwrap_or_default();
                    sec.field(
                        "Bitwarden server",
                        Some("Use bitwarden.com, EU, or a self-hosted Vaultwarden server"),
                        "bw.server",
                        &server.unwrap_or(remembered),
                        DEFAULT_SERVER,
                    )
                    .on_submit("bw.server", Some(260.0))
                    .anchor("bitwarden.server");
                    sec.field("Email", None, "bw.email", &email, "Email")
                        .on_submit("bw.signin", Some(260.0));
                    sec.secret(
                        "Master password",
                        Some("A two-step code is asked for next, if the account wants one"),
                        "bw.password",
                        &password,
                        "Password",
                    )
                    .on_submit("bw.signin", Some(260.0));
                    let ready = !email.trim().is_empty() && !password.is_empty();
                    sec.pills(
                        "Connect",
                        None,
                        vec![Pill {
                            enabled: ready && !busy,
                            ..action("bw.signin", "Sign in", true, busy)
                        }],
                    );
                }
                Step::Method => {
                    sec.pills(
                        "Two-step login",
                        Some("This account wants a code — which way does it get one?"),
                        vec![
                            plain("bw.method:0", "Authenticator app", !busy),
                            plain("bw.method:1", "Email", !busy),
                            plain("bw.method:3", "YubiKey", !busy),
                        ],
                    );
                    sec.pills(
                        "Account",
                        Some(&email),
                        vec![plain("bw.cancel", "Cancel", !busy)],
                    );
                }
                Step::Code(method) => {
                    sec.field(
                        code_title(method),
                        Some(&code_detail(method, &email)),
                        "bw.code",
                        &code,
                        "Code",
                    )
                    .on_submit("bw.continue", Some(120.0));
                    let mut pills = vec![plain("bw.cancel", "Cancel", !busy)];
                    pills.push(Pill {
                        enabled: !code.trim().is_empty() && !busy,
                        ..action("bw.continue", "Continue", true, busy)
                    });
                    sec.pills("Account", Some(&email), pills);
                }
            }
        }
        Some(BwState::Locked { email: account }) => {
            let line = account
                .as_ref()
                .map(|a| format!("Locked · {a}"))
                .unwrap_or_else(|| "Locked".into());
            sec.pills(
                "Bitwarden",
                Some(&line),
                vec![plain("bw.signout", "Sign out", !busy)],
            );
            sec.secret(
                "Master password",
                None,
                "bw.password",
                &password,
                "Password",
            )
            .on_submit("bw.unlock", Some(220.0));
            sec.pills(
                "",
                None,
                vec![Pill {
                    enabled: !password.is_empty() && !busy,
                    ..action("bw.unlock", "Unlock", true, busy)
                }],
            );
        }
        Some(BwState::Unlocked {
            email: account,
            last_sync,
        }) => {
            sec.pills(
                "Bitwarden",
                Some(&format!(
                    "Unlocked · synced {}",
                    synced(last_sync.as_deref())
                )),
                vec![
                    action("bw.sync", "Sync now", false, busy),
                    plain("bw.lock", "Lock", !busy),
                ],
            )
            .anchor("bitwarden.sync");
            sec.pills(
                "Account",
                Some(account.as_deref().unwrap_or("Signed in")),
                vec![plain("bw.signout", "Sign out", !busy)],
            );
            sec.note("Vault", Some(&counts(snap.bw_counts)));
            sec.switch(
                "Stay unlocked between launches",
                Some("The session is kept beside Bitwarden's own data in your Windows profile folder — off asks for the master password once per launch"),
                "bw.stay",
                snap.bw_stay,
            )
            .anchor("bitwarden.stay");
            let minutes = autolock(shell);
            sec.pick(
                "Auto-lock",
                Some("Lock the local Bitwarden session after inactivity"),
                "bw.autolock",
                &[
                    ("5", "5 minutes"),
                    ("15", "15 minutes"),
                    ("60", "60 minutes"),
                    ("0", "Never"),
                ],
                &minutes.to_string(),
            )
            .anchor("bitwarden.autolock");
        }
    }
    if let Some(e) = error {
        let first = e.lines().next().unwrap_or("").to_string();
        sec.pills(
            "Bitwarden couldn't do that",
            Some(&first),
            vec![plain("bw.dismiss", "Dismiss", true)],
        )
        .tone(Tone::Error);
    }
}

/// The auto-lock minutes Settings keeps (0, 5, 15 or 60; anything else is
/// never).
pub fn autolock(shell: &Shell) -> u32 {
    let m = shell.settings().number(AUTOLOCK).unwrap_or(0.0);
    [5u32, 15, 60]
        .into_iter()
        .find(|v| (*v as f64 - m).abs() < 0.5)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_counts_read_like_the_mac() {
        assert_eq!(counts((0, 0, 0)), "Nothing saved yet");
        assert_eq!(counts((3, 0, 1)), "3 logins · 1 card");
        assert_eq!(counts((1, 2, 0)), "1 login · 2 identities");
    }

    #[test]
    fn never_synced_says_so() {
        assert_eq!(synced(None), "not yet");
        assert_eq!(synced(Some("garbage")), "not yet");
    }
}
