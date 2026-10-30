//! Passwords in and out as CSV (`PasswordCSV.swift`): the only way another
//! browser's or password manager's passwords come into Copper for Windows.
//!
//! In: the person exports a CSV from the other browser (or Bitwarden,
//! 1Password, Firefox…) and picks the file in Windows' Open dialog; it is
//! read in place, never changed, checked before anything is saved, and its
//! logins go to Credential Manager keeping when each was last used
//! ([`copper_shell_core::forms::csv::import_file`]). Copper never reads
//! another browser's `Login Data` or its DPAPI key.
//!
//! Out: only after a yes to a plain-text warning, to a file the person
//! names in Windows' Save dialog ([`copper_shell_core::forms::csv::export_file`]).
//! Neither direction logs a row, a username or a password, and none of it
//! goes near `settings.json`.

use super::dialog::{self, Ask};
use crate::platform::Shell;
use copper_shell_core::forms::csv;
use copper_shell_core::{Question, ShellView};
use std::path::PathBuf;

pub use csv::Said;

/// Where Windows keeps Copper's passwords, as the sentences name it.
const STORE: &str = "Credential Manager";

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
    csv::import_file(path, STORE).await
}

/// Warn, ask where, and write every password Copper keeps there as CSV
/// (`name,url,username,password,note`); `done` hears how it went.
pub fn export(shell: &Shell, done: impl FnOnce(&Shell, Said) + 'static) {
    let weak = shell.downgrade();
    shell.view().ask(
        shell,
        Question {
            title: csv::EXPORT_TITLE.into(),
            detail: Some(csv::EXPORT_DETAIL.into()),
            deny: "Cancel".into(),
            allow: csv::EXPORT_ALLOW.into(),
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
                name: csv::EXPORT_NAME.into(),
            },
        )
        .await
        else {
            return;
        };
        let said = csv::export_file(path, STORE).await;
        if let Some(s) = weak.upgrade() {
            done(&s, said);
        }
    });
}
