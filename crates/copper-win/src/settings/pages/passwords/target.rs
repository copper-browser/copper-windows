//! "Save new passwords to: Credential Manager | Bitwarden | 1Password"
//! (`OnePasswordSettings.swift` `SaveTargetCard`): one chooser for every
//! place a new password can go, at the head of Password managers, drawn
//! once a password manager is connected. A vault that is set up but locked
//! stays offered; its saves go to Credential Manager until it opens, and
//! the line says so. The choice changes where saves go only — filling
//! always uses every source.

use crate::platform::Shell;
use crate::settings::form::Section;
use copper_creds::Source;
use copper_creds::offer::save_target;
use copper_shell_core::forms;
use copper_shell_core::forms::vault::Snapshot;

/// Credential Manager always; a manager once it is connected (locked or
/// open), or while it is the chosen one.
pub fn choices(snap: &Snapshot, chosen: Source) -> Vec<(&'static str, &'static str)> {
    let mut list = vec![("keychain", "Credential Manager")];
    if snap.bw_locked() || snap.bw_unlocked() || chosen == Source::Bitwarden {
        list.push(("bitwarden", "Bitwarden"));
    }
    if snap.op_locked() || snap.op_unlocked() || chosen == Source::OnePassword {
        list.push(("onepassword", "1Password"));
    }
    list
}

/// What happens to a save now, in one sentence.
pub fn detail(snap: &Snapshot, chosen: Source) -> String {
    if chosen == Source::Keychain {
        return "Offers to save go to Credential Manager; filling uses every source".into();
    }
    let name = forms::source_title(chosen);
    let effective = save_target(
        chosen,
        snap.bw_unlocked(),
        snap.op_unlocked(),
        snap.op_save_vault.is_some(),
    );
    if effective == chosen {
        if chosen == Source::OnePassword
            && let Some(vault) = &snap.op_save_vault
        {
            return format!(
                "New logins go to 1Password's {vault} vault; filling still uses every source"
            );
        }
        return format!("Offers to save go to {name}; filling still uses every source");
    }
    if chosen == Source::OnePassword && snap.op_unlocked() {
        return "1Password has no vault this account may add to — saves go to Credential Manager"
            .into();
    }
    let locked = match chosen {
        Source::Bitwarden => snap.bw_locked(),
        Source::OnePassword => snap.op_locked(),
        Source::Keychain => false,
    };
    if locked {
        format!("{name} is locked — saves go to Credential Manager until it opens")
    } else {
        format!("{name} isn't connected — saves go to Credential Manager")
    }
}

/// Whether the card is drawn at all: only with more than one choice.
pub fn shown(shell: &Shell, snap: &Snapshot) -> bool {
    choices(snap, forms::chosen_backend(shell)).len() > 1
}

/// The chooser's one line.
pub fn draw(shell: &Shell, sec: &mut Section, snap: &Snapshot) {
    let chosen = forms::chosen_backend(shell);
    let value = match chosen {
        Source::Keychain => "keychain",
        Source::Bitwarden => "bitwarden",
        Source::OnePassword => "onepassword",
    };
    sec.choice(
        "Save new passwords to",
        Some(&detail(snap, chosen)),
        "target",
        &choices(snap, chosen),
        value,
    )
    .anchor("passwords.backend");
}

/// `target`: where new passwords go.
pub fn act(shell: &Shell, action: &str, value: &str) -> bool {
    if action != "target" {
        return false;
    }
    let source = match value {
        "bitwarden" => Source::Bitwarden,
        "onepassword" => Source::OnePassword,
        _ => Source::Keychain,
    };
    forms::set_backend(shell, source);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use copper_creds::bitwarden::BwState;
    use copper_creds::onepassword::OpState;

    #[test]
    fn credential_manager_is_always_there_and_managers_once_connected() {
        let mut s = Snapshot::default();
        assert_eq!(choices(&s, Source::Keychain).len(), 1);
        s.bw_state = Some(BwState::Locked { email: None });
        assert_eq!(
            choices(&s, Source::Keychain),
            vec![
                ("keychain", "Credential Manager"),
                ("bitwarden", "Bitwarden")
            ]
        );
        assert_eq!(choices(&Snapshot::default(), Source::OnePassword).len(), 2);
    }

    #[test]
    fn a_locked_or_absent_manager_falls_back_out_loud() {
        let mut s = Snapshot::default();
        assert_eq!(
            detail(&s, Source::Bitwarden),
            "Bitwarden isn't connected — saves go to Credential Manager"
        );
        s.bw_state = Some(BwState::Locked { email: None });
        assert_eq!(
            detail(&s, Source::Bitwarden),
            "Bitwarden is locked — saves go to Credential Manager until it opens"
        );
        s.op_state = Some(OpState::Unlocked);
        assert_eq!(
            detail(&s, Source::OnePassword),
            "1Password has no vault this account may add to — saves go to Credential Manager"
        );
        s.op_save_vault = Some("Private".into());
        assert_eq!(
            detail(&s, Source::OnePassword),
            "New logins go to 1Password's Private vault; filling still uses every source"
        );
    }
}
