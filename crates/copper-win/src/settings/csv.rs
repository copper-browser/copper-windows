//! Passwords in and out as CSV (`PasswordCSV.swift`): the only way another
//! browser's or password manager's passwords come into Copper for Windows.
//!
//! In: the person exports a CSV from the other browser (or Bitwarden,
//! 1Password, Firefox…) and picks the file in Windows' Open dialog; it is
//! read in place, never changed, checked before anything is saved
//! ([`copper_import::passwords_csv::read_file`]), and its logins go to
//! Credential Manager keeping when each was last used
//! ([`copper_shell_core::forms::vault::Vault::save_imported`]). Copper never
//! reads another browser's `Login Data` or its DPAPI key.
//!
//! Out: only after a yes to a plain-text warning, to a file the person
//! names in Windows' Save dialog. Neither direction logs a row, a username
//! or a password, and none of it goes near `settings.json`.

use super::dialog::{self, Ask};
use crate::platform::Shell;
use copper_creds::credential::CredentialId;
use copper_import::ImportedLogin;
use copper_import::passwords_csv::{self, Outcome};
use copper_shell_core::forms::{self, rt};
use copper_shell_core::{Question, ShellView};
use std::path::PathBuf;

/// What came of an import or an export, in one sentence, and whether it
/// worked.
pub type Said = (String, bool);

/// Words for a vault that isn't there (the forms lane never started).
const NO_VAULT: &str =
    "Copper's passwords aren't available right now — restart Copper and try again";

/// Ask for a passwords CSV, bring its logins in, and tell `done` what came
/// of it. Nothing happens (and `done` isn't called) when the person
/// cancels the dialog.
pub fn import(shell: &Shell, done: impl FnOnce(&Shell, Said) + 'static) {
    import_reading(shell, |_, _| {}, done);
}

/// [`import`], telling `reading` the file's name once one is chosen and
/// is being read (the Passwords panel's `Reading <file>…`).
pub fn import_reading(
    shell: &Shell,
    reading: impl FnOnce(&Shell, &str) + 'static,
    done: impl FnOnce(&Shell, Said) + 'static,
) {
    let Some(owner) = dialog::owner(shell) else {
        return;
    };
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let Some(path) = dialog::ask(
            owner,
            Ask::OpenCsv {
                title: "A passwords export — from Chrome, Edge, Firefox or a password manager"
                    .into(),
            },
        )
        .await
        else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "the file".into());
        if let Some(s) = weak.upgrade() {
            reading(&s, &name);
        }
        let said = import_file(path).await;
        if let Some(s) = weak.upgrade() {
            done(&s, said);
        }
    });
}

/// Read the CSV at `path` and keep what it holds in Credential Manager.
pub async fn import_file(path: PathBuf) -> Said {
    let read = rt::blocking(move || passwords_csv::read_file(&path))
        .await
        .unwrap_or_else(|| Err(copper_import::ImportError::Unreadable("the file".into())));
    let read = match read {
        Ok(r) => r,
        Err(e) => {
            let refused = Outcome {
                refused: Some(e.to_string()),
                ..Outcome::default()
            };
            return (refused.sentence(), false);
        }
    };
    let total = read.logins.len();
    let kept = if total == 0 {
        Ok(0)
    } else {
        let Some(v) = forms::vault() else {
            return (NO_VAULT.into(), false);
        };
        let logins: Vec<(String, String, String, Option<f64>)> = read
            .logins
            .into_iter()
            .map(|l| (l.host, l.user, l.password, l.used))
            .collect();
        rt::run(async move { v.save_imported(logins).await })
            .await
            .unwrap_or_else(|| Err(NO_VAULT.into()))
    };
    match kept {
        Ok(n) => {
            let outcome = Outcome {
                kept: n,
                skipped: read.skipped,
                codes: read.codes,
                refused: None,
            };
            let mut line = outcome.sentence();
            if n < total {
                line += &format!(" — {} couldn't be kept in Credential Manager", total - n);
            }
            (line, n > 0 || total == 0)
        }
        Err(e) => {
            tracing::warn!("importing passwords: {e}");
            (format!("Credential Manager refused them: {e}"), false)
        }
    }
}

/// Warn, ask where, and write every password Copper keeps there as CSV
/// (`name,url,username,password,note`); `done` hears how it went.
pub fn export(shell: &Shell, done: impl FnOnce(&Shell, Said) + 'static) {
    let weak = shell.downgrade();
    shell.view().ask(
        shell,
        Question {
            title: "Export every password in plain text?".into(),
            detail: Some(
                "The file holds every password Copper keeps, readable by anyone who opens it. Keep it somewhere safe and delete it when you're done."
                    .into(),
            ),
            deny: "Cancel".into(),
            allow: "Export…".into(),
        },
        Box::new(move |yes| {
            if let (true, Some(s)) = (yes, weak.upgrade()) {
                export_to_chosen(&s, done);
            }
        }),
    );
}

/// Ask where, then write.
fn export_to_chosen(shell: &Shell, done: impl FnOnce(&Shell, Said) + 'static) {
    let Some(owner) = dialog::owner(shell) else {
        return;
    };
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let Some(path) = dialog::ask(
            owner,
            Ask::SaveCsv {
                title: "Export passwords".into(),
                name: "Copper Passwords.csv".into(),
            },
        )
        .await
        else {
            return;
        };
        let said = export_file(path).await;
        if let Some(s) = weak.upgrade() {
            done(&s, said);
        }
    });
}

/// Every login with its password, read one at a time, as CSV at `path`.
async fn export_file(path: PathBuf) -> Said {
    let Some(v) = forms::vault() else {
        return (NO_VAULT.into(), false);
    };
    let collected = rt::run(async move {
        let logins = v.logins().await?;
        let mut out = Vec::with_capacity(logins.len());
        let mut missed = 0usize;
        for l in logins {
            let id = CredentialId::Keychain {
                host: l.host.clone(),
                user: l.user.clone(),
            };
            match v.secret(&id).await {
                Ok(password) => out.push(ImportedLogin {
                    host: l.host,
                    user: l.user,
                    password,
                    used: l.used,
                }),
                Err(_) => missed += 1,
            }
        }
        Ok::<_, String>((out, missed))
    })
    .await
    .unwrap_or_else(|| Err(NO_VAULT.into()));
    let (logins, missed) = match collected {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("exporting passwords: {e}");
            return (format!("Credential Manager couldn't be read: {e}"), false);
        }
    };
    let n = logins.len();
    if n == 0 && missed == 0 {
        return ("No passwords to export yet".into(), false);
    }
    let text = passwords_csv::csv(&logins);
    drop(logins);
    let name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let wrote = rt::blocking(move || std::fs::write(&path, text.as_bytes()))
        .await
        .unwrap_or_else(|| Err(std::io::Error::other("the write never finished")));
    match wrote {
        Ok(()) => (export_sentence(n, missed, &name), true),
        Err(e) => {
            tracing::warn!("exporting passwords: writing the file failed: {e}");
            (
                format!("Couldn't write {name} — choose another folder"),
                false,
            )
        }
    }
}

/// What an export came to (`PasswordCSV.exportSentence`), with Windows'
/// store named.
fn export_sentence(written: usize, refused: usize, file: &str) -> String {
    let head = format!(
        "{} exported to {file}",
        super::count(written, "password", "passwords")
    );
    if refused == 0 {
        return head;
    }
    format!("{head} — {refused} Credential Manager wouldn't give up")
}

#[cfg(test)]
mod tests {
    use super::export_sentence;

    #[test]
    fn export_says_what_was_written_and_refused() {
        assert_eq!(
            export_sentence(1, 0, "a.csv"),
            "1 password exported to a.csv"
        );
        assert_eq!(
            export_sentence(3, 2, "a.csv"),
            "3 passwords exported to a.csv — 2 Credential Manager wouldn't give up"
        );
    }
}
