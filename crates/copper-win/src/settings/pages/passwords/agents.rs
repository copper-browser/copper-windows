//! Settings › Passwords › Agent access (`CredentialsSettings.swift`
//! `AgentAccessCard`): which saved accounts, identities and cards Copper's
//! agents may use. Nothing is shared unless shared here — every account at
//! once, one at a time, or by the vault itself (a Bitwarden or 1Password
//! item marked for agents). A vault's own deny can't be overridden.
//! Sharing means the agent can have Copper sign in or fill with it; it
//! never receives the password.
//!
//! This is Passwords' card, not Settings › Agents. What is kept is the
//! stable id only (`agent.credentials.allowed`), in the shape the shared
//! forms lane checks ([`forms::agent_allowed`]).

use super::live::{self, Item};
use crate::platform::Shell;
use crate::settings::form::{Form, Section};
use copper_creds::{AgentHint, Credential, Source};
use copper_shell_core::forms::{self, keys};
use std::cell::RefCell;

/// The most account rows drawn at once (the Mac's 400-pt window); a search
/// finds the rest.
const SHOWN: usize = 10;

thread_local! {
    /// What the search box says.
    static HUNT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Whether `c` matches the search `needle` (lowercased).
fn matches(c: &Credential, needle: &str) -> bool {
    needle.is_empty()
        || c.name.to_lowercase().contains(needle)
        || c.host.to_lowercase().contains(needle)
        || c.user.to_lowercase().contains(needle)
}

fn item_matches(i: &Item, needle: &str) -> bool {
    needle.is_empty()
        || i.title.to_lowercase().contains(needle)
        || i.detail.to_lowercase().contains(needle)
}

/// The site after the item's own name when it has one (a Credential
/// Manager login is named by its username, which the detail says).
fn title(c: &Credential) -> String {
    if c.name.is_empty() || c.name == c.user || c.name == c.host {
        c.host.clone()
    } else {
        format!("{} · {}", c.name, c.host)
    }
}

fn source_name(s: Source) -> &'static str {
    match s {
        Source::Keychain => "Credential Manager",
        Source::Bitwarden => "Bitwarden",
        Source::OnePassword => "1Password",
    }
}

/// `user · Bitwarden`, with the vault's own say when it has one.
fn detail(c: &Credential) -> String {
    let user = if c.user.is_empty() {
        "No username"
    } else {
        &c.user
    };
    let mut line = format!("{user} · {}", source_name(c.source));
    match c.agent_hint {
        AgentHint::Allow if c.source == Source::OnePassword => {
            if c.folder
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case("agents"))
            {
                line += " · Agents vault";
            } else {
                line += " · copper-agent tag";
            }
        }
        AgentHint::Allow => line += " · Agents folder",
        AgentHint::Deny => line += &format!(" · denied in {}", source_name(c.source)),
        AgentHint::None => {}
    }
    line
}

/// Draw the card as its own section.
pub fn draw(shell: &Shell, f: &mut Form) {
    let share_all = shell.settings().bool_or(keys::SHARE_ALL, false);
    let hunt = HUNT.with(|h| h.borrow().clone());
    let needle = hunt.trim().to_lowercase();
    let (accounts, identities, cards) =
        live::with(|l| (l.accounts.clone(), l.identities.clone(), l.cards.clone()));
    let sec = f.section("Agent access").card_anchor("passwords.agents");
    sec.switch(
        "Share every saved account with agents",
        Some("Agents can use saved sign-ins in Copper without receiving the password"),
        "agent.all",
        share_all,
    )
    .anchor("passwords.agents");
    sec.field("", None, "agent.hunt", &hunt, "Search saved accounts")
        .on_submit("agent.hunt", None);
    let found: Vec<&Credential> = accounts.iter().filter(|c| matches(c, &needle)).collect();
    if found.is_empty() {
        sec.note(
            "",
            Some(if accounts.is_empty() {
                "Nothing kept yet — sign in somewhere or connect Bitwarden or 1Password."
            } else {
                "Nothing matches."
            }),
        );
    }
    for c in found.iter().take(SHOWN) {
        let denied = c.agent_hint == AgentHint::Deny;
        let on = !denied && forms::agent_allowed(shell, c);
        row(
            sec,
            &title(c),
            &detail(c),
            &c.id.to_string(),
            on,
            share_all || denied,
        );
    }
    if found.len() > SHOWN {
        let more = found.len() - SHOWN;
        sec.note(
            "",
            Some(&format!(
                "{} — search to find one",
                crate::settings::count(more, "more account", "more accounts")
            )),
        );
    }
    items(
        shell,
        sec,
        "Identities",
        "No identities kept yet.",
        &identities,
        &needle,
        share_all,
    );
    items(
        shell,
        sec,
        "Cards",
        "No cards kept yet.",
        &cards,
        &needle,
        share_all,
    );
}

/// One account, identity or card with its switch.
fn row(sec: &mut Section, title: &str, detail: &str, stable: &str, on: bool, off: bool) {
    sec.switch(title, Some(detail), &format!("agent:{stable}"), on)
        .disabled(off)
        .anchor(&format!("row:agent:{stable}"));
}

/// Identities or cards, under a heading line.
fn items(
    shell: &Shell,
    sec: &mut Section,
    heading: &str,
    empty: &str,
    list: &[Item],
    needle: &str,
    share_all: bool,
) {
    let found: Vec<&Item> = list.iter().filter(|i| item_matches(i, needle)).collect();
    sec.note(heading, found.is_empty().then_some(empty))
        .anchor(&format!("row:agent:{heading}"));
    for i in found {
        let on = forms::agent_allowed_id(shell, &i.stable);
        row(sec, &i.title, &i.detail, &i.stable, on, share_all);
    }
}

/// Allow or stop allowing one stable id (`agent.credentials.allowed`).
fn set_allowed(shell: &Shell, stable: &str, on: bool) {
    let mut list: Vec<String> = shell
        .settings()
        .get(keys::ALLOWED)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    list.retain(|x| x != stable);
    if on {
        list.push(stable.to_string());
    }
    list.sort();
    list.dedup();
    let value = serde_json::Value::Array(list.into_iter().map(Into::into).collect());
    shell.update_settings(|s| s.set(keys::ALLOWED, value));
}

/// The card's actions; `false` if `action` isn't one.
pub fn act(shell: &Shell, action: &str, value: &str) -> bool {
    match action {
        "agent.all" => {
            let on = crate::settings::pages::on(value);
            shell.update_settings(|s| s.set(keys::SHARE_ALL, on));
        }
        // The list narrows as the search is typed; the box keeps the
        // keyboard through the redraw.
        "agent.hunt" => {
            HUNT.with(|h| *h.borrow_mut() = value.to_string());
            // After the field's own event is done with the box it
            // replaces.
            let weak = shell.downgrade();
            copper_win_rt::spawn_local(async move {
                if let Some(s) = weak.upgrade() {
                    crate::settings::pages::redraw(&s, "passwords");
                }
            });
        }
        _ => match action.strip_prefix("agent:") {
            Some(stable) => set_allowed(shell, stable, crate::settings::pages::on(value)),
            None => return false,
        },
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use copper_creds::CredentialId;

    fn login(host: &str, user: &str) -> Credential {
        Credential {
            id: CredentialId::Keychain {
                host: host.into(),
                user: user.into(),
            },
            source: Source::Keychain,
            host: host.into(),
            user: user.into(),
            sites: vec![],
            name: user.into(),
            has_totp: false,
            used: None,
            folder: None,
            agent_hint: AgentHint::None,
        }
    }

    #[test]
    fn a_login_reads_as_its_site_and_user() {
        let c = login("example.com", "me");
        assert_eq!(title(&c), "example.com");
        assert_eq!(detail(&c), "me · Credential Manager");
        assert!(matches(&c, "exa"));
        assert!(!matches(&c, "zzz"));
    }

    #[test]
    fn a_vault_deny_says_so() {
        let mut c = login("example.com", "");
        c.source = Source::Bitwarden;
        c.agent_hint = AgentHint::Deny;
        assert_eq!(detail(&c), "No username · Bitwarden · denied in Bitwarden");
    }
}
