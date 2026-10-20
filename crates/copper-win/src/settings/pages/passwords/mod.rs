//! Settings › Passwords (`Settings.swift` 393–465): saved sign-ins and
//! whether Copper offers and fills them, the password managers (where new
//! passwords go, Bitwarden, 1Password), what agents may use, and passwords
//! in and out as CSV.
//!
//! On Windows the passwords Copper keeps are in Credential Manager, under
//! the Windows account — there is no separate keyring to set up or unlock,
//! so the page never asks for one; a store that fails says so in red.
//! What isn't on Windows yet (identity and card filling, Copper's own
//! passkeys) is said faded, with nothing to switch.
//!
//! Each card is its own module: [`target`], [`bitwarden`], [`onepassword`],
//! [`agents`]; what has to be asked for comes from [`live`].

mod agents;
mod bitwarden;
mod live;
mod onepassword;
mod target;

use super::{not_yet, on, redraw};
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::{Form, Pill, Section, Tone};
use crate::settings::search::Entry;
use copper_core::settings::keys;
use copper_shell_core::forms;
use std::cell::RefCell;

/// Settings › Passwords.
pub(in crate::settings) struct Passwords;

/// "Forget all" is offered from this many never-asked sites
/// (`NeverAsked.forgetAllFrom`).
const FORGET_ALL_FROM: usize = 4;

thread_local! {
    /// What the last import or export came to, and whether it worked.
    static SAID: RefCell<Option<(String, bool)>> = const { RefCell::new(None) };
}

/// `just now` under 45 seconds, else the way the rest of Copper says it.
pub(super) fn ago_short(unix: f64) -> String {
    if copper_core::date::now_unix() - unix < 45.0 {
        return "just now".into();
    }
    super::updates::ago(unix)
}

/// `1 site told to stop offering` / `N sites told to stop offering`.
fn never_summary(n: usize) -> String {
    if n == 1 {
        "1 site told to stop offering".into()
    } else {
        format!("{n} sites told to stop offering")
    }
}

/// What a Forget says (`NeverAsked.forgotten`).
fn forgotten(hosts: &[String]) -> String {
    match hosts {
        [one] => format!("{one} can offer to save again"),
        _ => "Every site can offer to save again".into(),
    }
}

/// The never list in the Mac's order: alphabetical, whatever the case.
fn ordered(mut hosts: Vec<String>) -> Vec<String> {
    hosts.sort_by_key(|h| h.to_lowercase());
    hosts
}

impl Passwords {
    /// Every card, from the copy [`live`] keeps.
    fn compose(&self, shell: &Shell) -> Form {
        let mut f = Form::default();
        self.saved(shell, &mut f);
        self.managers(shell, &mut f);
        agents::draw(shell, &mut f);
        self.transfer(&mut f);
        f
    }

    /// Saved passwords: the list, offering and filling, the never list.
    fn saved(&self, shell: &Shell, f: &mut Form) {
        let (save, fill, everything) = {
            let s = shell.settings();
            (
                s.saves_passwords(),
                s.fills_passwords(),
                s.fills_everything(),
            )
        };
        let kept = live::with(|l| l.kept.clone());
        let sec = f.section("Saved passwords");
        match kept {
            Some(Err(e)) => {
                sec.pills(
                    "Your passwords",
                    Some(&format!("Credential Manager couldn't be read: {e}")),
                    vec![
                        pill("keychain.retry", "Try again", false),
                        pill("open", "Open…", false),
                    ],
                )
                .tone(Tone::Error);
            }
            Some(Ok(n)) => {
                let detail = if n == 0 {
                    "In Windows Credential Manager — none kept yet".to_string()
                } else {
                    format!(
                        "{} in Windows Credential Manager",
                        crate::settings::count(n, "password", "passwords")
                    )
                };
                sec.button("Your passwords", Some(&detail), "open", "Open…", false);
            }
            None => {
                sec.button(
                    "Your passwords",
                    Some("In Windows Credential Manager"),
                    "open",
                    "Open…",
                    false,
                );
            }
        }
        sec.anchor("passwords.list")
            .keywords(&["credential manager", "logins", "saved"]);
        sec.switch(
            "Offer to save passwords",
            Some("Asked once per site, never again for a site you refuse"),
            "save",
            save,
        )
        .anchor("passwords.save");
        sec.switch(
            "Fill in sign-ins",
            Some("Click a sign-in box and the accounts kept for the site hang from it"),
            "fill",
            fill,
        )
        .anchor("passwords.fill");
        sec.switch(
            "Fill addresses and cards",
            Some("Click into a checkout or address form and your Bitwarden and 1Password identities and cards hang from it"),
            "everything",
            everything,
        )
        .anchor("passwords.everything");
        not_yet(
            sec,
            "Offer passkeys",
            "Not yet on Windows — Copper keeps no passkeys of its own here; a site's passkey prompt comes from Windows",
        )
        .anchor("passwords.passkeys");
        never(shell, sec);
    }

    /// Password managers: where new passwords go, then one card each.
    fn managers(&self, shell: &Shell, f: &mut Form) {
        let snap = live::snapshot();
        let mut first = true;
        let mut card = |f: &mut Form| -> usize {
            if std::mem::take(&mut first) {
                f.section("Password managers");
            } else {
                f.more();
            }
            f.sections.len() - 1
        };
        if target::shown(shell, &snap) {
            let i = card(f);
            target::draw(shell, &mut f.sections[i], &snap);
            f.sections[i].card_anchor("passwords.backend");
        }
        let i = card(f);
        bitwarden::draw(shell, &mut f.sections[i], &snap);
        f.sections[i].card_anchor("bitwarden");
        let i = card(f);
        onepassword::draw(shell, &mut f.sections[i], &snap);
        f.sections[i].card_anchor("onepassword");
    }

    /// Import and export: Move in, a CSV in, a CSV out.
    fn transfer(&self, f: &mut Form) {
        let kept = live::with(|l| matches!(l.kept, Some(Ok(n)) if n > 0));
        let sec = f.section("Import and export");
        sec.button(
            "Bring yours in",
            Some("From Chrome, Edge, Brave, Vivaldi or Arc on this PC — passwords come through each browser's own export, and nothing leaves this PC"),
            "movein",
            "Import…",
            false,
        )
        .anchor("passwords.import");
        sec.button(
            "From a CSV file",
            Some("A passwords export from Chrome, Edge, Firefox, Bitwarden, 1Password or another password manager"),
            "import",
            "Choose File…",
            false,
        )
        .anchor("passwords.csv")
        .keywords(&["import", "csv", "firefox", "bitwarden", "1password"]);
        sec.pills(
            "Export to a CSV file",
            Some("Every password Copper keeps, in plain text. Anyone with the file can read them — delete it when you're done."),
            vec![Pill {
                action: "export".into(),
                label: "Export…".into(),
                filled: false,
                enabled: kept,
            }],
        )
        .anchor("passwords.export");
        if let Some((line, worked)) = SAID.with(|s| s.borrow().clone()) {
            sec.note(
                if worked { "Done" } else { "That didn't work" },
                Some(&line),
            )
            .tone(if worked { Tone::Good } else { Tone::Error })
            .anchor("row:passwords.said");
        }
    }
}

fn pill(action: &str, label: &str, filled: bool) -> Pill {
    Pill {
        action: action.into(),
        label: label.into(),
        filled,
        enabled: true,
    }
}

/// Sites never asked (`NeverAskedLines`): only when there are any, each
/// with its own Forget, and Forget all once the list is long.
fn never(shell: &Shell, sec: &mut Section) {
    let hosts = ordered(forms::never_list(shell));
    if hosts.is_empty() {
        return;
    }
    let all = if hosts.len() >= FORGET_ALL_FROM {
        vec![pill("never.all", "Forget all", false)]
    } else {
        Vec::new()
    };
    sec.pills("Sites never asked", Some(&never_summary(hosts.len())), all)
        .anchor("passwords.never");
    for h in &hosts {
        sec.pills(h, None, vec![pill(&format!("never:{h}"), "Forget", false)])
            .anchor(&format!("row:never:{h}"));
    }
}

/// Let `hosts` be offered again, and say so.
fn forget(shell: &Shell, hosts: Vec<String>) {
    for h in &hosts {
        forms::set_never(shell, h, false);
    }
    shell.toast(&forgotten(&hosts));
}

/// What an import or export came to: on the page, and said aloud.
fn said(shell: &Shell, (line, worked): (String, bool)) {
    shell.toast(&line);
    SAID.with(|s| *s.borrow_mut() = Some((line, worked)));
    live::look(shell);
    redraw(shell, "passwords");
}

impl Page for Passwords {
    fn id(&self) -> &'static str {
        "passwords"
    }
    fn title(&self) -> &'static str {
        "Passwords"
    }
    fn blurb(&self) -> &'static str {
        "Saved sign-ins, password managers, passkeys and what agents may use."
    }
    fn glyph(&self) -> char {
        glyph::KEY
    }

    /// The page as it stands, and a fresh look when what it shows is old.
    fn draw(&self, shell: &Shell) -> Form {
        live::freshen(shell);
        self.compose(shell)
    }

    /// Search finds the page's own rows, not the sites, accounts and
    /// results listed in it — and building the index asks no vault.
    fn entries(&self, shell: &Shell) -> Vec<Entry> {
        crate::settings::index::entries_of(self.id(), self.title(), &self.compose(shell))
            .into_iter()
            .filter(|e| !e.anchor.starts_with("row:"))
            .collect()
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        if target::act(shell, action, value)
            || bitwarden::act(shell, action, value)
            || onepassword::act(shell, action, value)
            || agents::act(shell, action, value)
        {
            return;
        }
        match action {
            "save" => shell.update_settings(|s| s.set(keys::PASSWORDS_SAVE, on(value))),
            "fill" => shell.update_settings(|s| s.set(keys::PASSWORDS_FILL, on(value))),
            "everything" => shell.update_settings(|s| s.set(keys::AUTOFILL_EVERYTHING, on(value))),
            "open" => crate::settings::manager::open(shell),
            "movein" => crate::settings::movein::open(shell),
            "keychain.retry" => live::look(shell),
            "import" => crate::settings::csv::import(shell, said),
            "export" => crate::settings::csv::export(shell, said),
            "never.all" => forget(shell, ordered(forms::never_list(shell))),
            _ => {
                if let Some(host) = action.strip_prefix("never:") {
                    forget(shell, vec![host.to_string()]);
                }
            }
        }
    }

    fn left(&self) {
        SAID.with(|s| s.borrow_mut().take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_never_list_reads_like_the_mac() {
        assert_eq!(never_summary(1), "1 site told to stop offering");
        assert_eq!(never_summary(3), "3 sites told to stop offering");
        assert_eq!(
            forgotten(&["a.com".into()]),
            "a.com can offer to save again"
        );
        assert_eq!(
            forgotten(&["a.com".into(), "b.com".into()]),
            "Every site can offer to save again"
        );
        assert_eq!(
            ordered(vec!["b.com".into(), "A.com".into(), "c.com".into()]),
            vec!["A.com", "b.com", "c.com"]
        );
    }
}
