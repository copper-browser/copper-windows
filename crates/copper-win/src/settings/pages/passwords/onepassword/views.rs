//! 1Password card's states as rows: signed out (the three ways in), locked
//! (unlock or sign out), and unlocked (vaults, where new logins go, Lock,
//! Sign out, Sync now).

use super::*;

pub(super) fn signed_out(
    sec: &mut Section,
    snap: &Snapshot,
    info: &live::OpInfo,
    busy: bool,
    waiting: bool,
) {
    sec.value(
        "1Password",
        Some("Copper reads your vaults through the 1Password CLI and keeps them in memory while unlocked"),
        "Not connected",
    );
    let mut paths = Vec::new();
    if info.app {
        paths.push(("app", "1Password app"));
    }
    paths.push(("password", "Password"));
    paths.push(("service", "Service account"));
    let way = path(snap, info.app);
    sec.choice("Sign in with", None, "op.path", &paths, way.as_str())
        .anchor("onepassword.method");
    let (adding, address, email, key, password, token) = with(|c| {
        (
            c.adding,
            c.address.clone(),
            c.email.clone(),
            c.secret_key.clone(),
            c.password.clone(),
            c.token.clone(),
        )
    });
    match way {
        Mode::App => {
            if info.accounts.len() > 1 {
                account_pick(sec, info, false);
            }
            sec.pills(
                "Unlock with 1Password",
                Some("1Password asks you to approve in its own window — no password is typed into Copper"),
                vec![action("op.app", "Unlock with 1Password", busy, waiting, true)],
            )
            .anchor("onepassword.unlock");
            if info.problem == Some(Problem::IntegrationOff) {
                guide(sec);
            }
        }
        Mode::Password if info.accounts.is_empty() || adding => {
            sec.field(
                "Sign-in address",
                None,
                "op.address",
                &address,
                DEFAULT_ADDRESS,
            )
            .on_submit("op.add", Some(240.0))
            .anchor("onepassword.account");
            sec.field("Email", None, "op.email", &email, "you@example.com")
                .on_submit("op.add", Some(240.0));
            sec.secret(
                "Secret Key",
                Some("From your Emergency Kit — A3-…"),
                "op.key",
                &key,
                "A3-XXXXXX-…",
            )
            .on_submit("op.add", Some(240.0));
            sec.secret(
                "Account password",
                None,
                "op.password",
                &password,
                "Password",
            )
            .on_submit("op.add", Some(240.0));
            let mut pills = Vec::new();
            if !info.accounts.is_empty() {
                pills.push(plain("op.cancel", "Cancel", !busy));
            }
            let ready = !email.trim().is_empty() && !key.trim().is_empty() && !password.is_empty();
            pills.push(action("op.add", "Sign in", busy, false, ready));
            sec.pills(
                "Add this account",
                Some("The Secret Key and password go to the 1Password CLI, which keeps the account on this PC"),
                pills,
            )
            .anchor("onepassword.unlock");
        }
        Mode::Password => {
            account_pick(sec, info, true);
            sec.secret(
                "Account password",
                None,
                "op.password",
                &password,
                "Password",
            )
            .on_submit("op.signin", Some(180.0));
            sec.pills(
                "",
                None,
                vec![action(
                    "op.signin",
                    "Sign in",
                    busy,
                    false,
                    !password.is_empty(),
                )],
            )
            .anchor("onepassword.unlock");
        }
        Mode::Service => {
            sec.secret(
                "Service account token",
                Some("For a PC nobody sits at. Reads the vaults the account was given; saves only where it may write"),
                "op.token",
                &token,
                "ops_…",
            )
            .on_submit("op.connect", Some(200.0))
            .anchor("onepassword.service");
            sec.pills(
                "",
                None,
                vec![action(
                    "op.connect",
                    "Connect",
                    busy,
                    false,
                    !token.trim().is_empty(),
                )],
            );
        }
    }
}

/// The accounts `op` knows, to choose one (and, signing in with a
/// password, to add another).
fn account_pick(sec: &mut Section, info: &live::OpInfo, allow_new: bool) {
    let chosen = with(|c| c.chosen.clone())
        .filter(|id| info.accounts.iter().any(|(a, _)| a == id))
        .or_else(|| info.accounts.first().map(|(a, _)| a.clone()))
        .unwrap_or_default();
    let mut options: Vec<(&str, &str)> = info
        .accounts
        .iter()
        .map(|(id, label)| (id.as_str(), label.as_str()))
        .collect();
    if allow_new {
        options.push(("+new", "Add another account…"));
    }
    sec.pick(
        "Account",
        allow_new.then_some("Already known to 1Password on this PC"),
        "op.account",
        &options,
        &chosen,
    )
    .anchor("onepassword.account");
}

/// The app integration is off: how to turn it on, or the password instead.
fn guide(sec: &mut Section) {
    sec.pills(
        "Let 1Password answer Copper",
        Some("In the 1Password app, open Settings › Developer and turn on Integrate with 1Password CLI"),
        vec![
            plain("op.use-password", "Use the password instead", true),
            Pill {
                action: "op.open-app".into(),
                label: "Open 1Password".into(),
                filled: true,
                enabled: true,
            },
        ],
    );
}

pub(super) fn locked(
    sec: &mut Section,
    snap: &Snapshot,
    info: &live::OpInfo,
    busy: bool,
    waiting: bool,
) {
    let suffix = snap
        .op_account
        .as_ref()
        .map(|a| format!(" · {a}"))
        .unwrap_or_default();
    sec.pills(
        "1Password",
        Some(&format!("Locked{suffix}")),
        vec![plain("op.signout", "Sign out", !busy)],
    )
    .anchor("onepassword.account");
    match snap.op_mode {
        Some(Mode::Password) => {
            let password = with(|c| c.password.clone());
            sec.secret(
                "Account password",
                Some("The session ended or Copper was locked"),
                "op.password",
                &password,
                "Password",
            )
            .on_submit("op.unlock", Some(180.0));
            sec.pills(
                "",
                None,
                vec![action(
                    "op.unlock",
                    "Unlock",
                    busy,
                    false,
                    !password.is_empty(),
                )],
            )
            .anchor("onepassword.unlock");
        }
        Some(Mode::Service) => {
            sec.pills(
                "Service account",
                Some("The token is kept in your Windows profile folder, readable by your Windows account only"),
                vec![action("op.unlock", "Unlock", busy, false, true)],
            )
            .anchor("onepassword.unlock");
        }
        Some(Mode::App) | None => {
            sec.pills(
                "Unlock with 1Password",
                Some("Approve in 1Password's own window"),
                vec![action(
                    "op.unlock",
                    "Unlock with 1Password",
                    busy,
                    waiting,
                    true,
                )],
            )
            .anchor("onepassword.unlock");
            if info.problem == Some(Problem::IntegrationOff) {
                guide(sec);
            }
        }
    }
}

pub(super) fn unlocked(
    shell: &Shell,
    sec: &mut Section,
    snap: &Snapshot,
    info: &live::OpInfo,
    busy: bool,
    synced: Option<f64>,
) {
    let line = match synced {
        Some(at) => format!("Unlocked · synced {}", super::super::ago_short(at)),
        None if info.vaults.is_empty() && live::looking() => {
            "Unlocked · reading your vaults…".into()
        }
        None => "Unlocked".into(),
    };
    sec.pills(
        "1Password",
        Some(&line),
        vec![
            Pill {
                filled: false,
                ..action("op.sync", "Sync now", busy, false, true)
            },
            plain("op.lock", "Lock", !busy),
        ],
    )
    .anchor("onepassword.sync");
    sec.pills(
        "Account",
        Some(&account_line(snap)),
        vec![plain("op.signout", "Sign out", !busy)],
    )
    .anchor("onepassword.account");
    sec.note("Vaults", Some(&vaults_line(&info.vaults, snap.op_counts)));
    if !info.vaults.is_empty() {
        if info.writable.is_empty() {
            sec.value(
                "New logins go to",
                Some("None of these vaults lets this account create items — saves stay in Credential Manager"),
                "Read-only",
            )
            .anchor("onepassword.vault");
        } else {
            let options: Vec<(&str, &str)> = info
                .writable
                .iter()
                .map(|(id, name)| (id.as_str(), name.as_str()))
                .collect();
            let detail = if forms::chosen_backend(shell) == Source::OnePassword {
                "A login saved from a page is created in this vault"
            } else {
                "Used when new passwords are saved to 1Password"
            };
            sec.pick(
                "New logins go to",
                Some(detail),
                "op.vault",
                &options,
                info.save.as_deref().unwrap_or(""),
            )
            .anchor("onepassword.vault");
        }
    }
    let stay = shell.settings().bool_or(keys::OP_STAY, true);
    sec.switch(
        "Stay unlocked between launches",
        Some(stay_detail(snap.op_mode)),
        "op.stay",
        stay,
    )
    .anchor("onepassword.stay");
}
