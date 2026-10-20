//! Removing Copper for the current user, with no window: the steps both the
//! uninstall window and silent mode run.
//!
//! Shortcuts, registry and the install folder always go. Browsing data
//! (`%LOCALAPPDATA%\Copper`: history, passwords, cookies, cache, WebView2's
//! profile) goes only when asked. A running copper.exe can't delete itself,
//! so when the uninstaller *is* the installed exe it empties the folder
//! around itself and leaves the last step — the exe and the folder — to a
//! short-lived `cmd.exe` started as it exits ([`finish`]).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::UninstallOptions;
use crate::files::{is_inside, only_warmed_up};
use crate::install::{desktop_shortcut, install_dir, start_menu_shortcut};
use crate::log::log;
use crate::registration::APP_NAME;
use crate::sys::{process, shell};

/// How long a running Copper gets to close before setup ends it.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// `%LOCALAPPDATA%\Copper` (CONTRACT §4): everything Copper keeps.
pub fn data_dir() -> anyhow::Result<PathBuf> {
    Ok(shell::local_app_data()?.join(APP_NAME))
}

/// What is left to do once the process is about to exit.
#[derive(Debug, Default)]
#[must_use = "call finish() as the process exits, or the exe is left behind"]
pub struct Pending {
    /// The running exe and its folder, to delete after exit.
    self_delete: Option<(PathBuf, PathBuf)>,
}

/// Remove a folder, retrying briefly: WebView2's helper processes take a
/// moment to let go of their files after the browser closes.
fn remove_dir_patiently(dir: &Path) -> anyhow::Result<()> {
    let mut last = None;
    for attempt in 0..10 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(150 * (attempt + 1)));
    }
    Err(last.map_or_else(
        || anyhow::anyhow!("couldn't remove {}", dir.display()),
        Into::into,
    ))
}

/// Uninstall. `report` hears progress as a fraction 0…1. Every step is
/// attempted even if an earlier one failed; the first failure is returned
/// beside what is still to do at exit.
pub fn run(
    options: &UninstallOptions,
    mut report: impl FnMut(f32),
) -> (Pending, anyhow::Result<()>) {
    log!("uninstall options {options:?}");
    let _com = shell::Com::init();
    let mut first_error: Option<anyhow::Error> = None;
    let mut note = |r: anyhow::Result<()>, what: &str| {
        if let Err(e) = r {
            log!("uninstall: {what}: {e:#}");
            first_error.get_or_insert(e.context(what.to_owned()));
        }
    };

    report(0.05);
    process::close_running_copper(&install_dir(), CLOSE_TIMEOUT, || {});

    report(0.25);
    for lnk in [start_menu_shortcut(), desktop_shortcut()]
        .into_iter()
        .flatten()
    {
        if lnk.exists() {
            note(
                std::fs::remove_file(&lnk).map_err(Into::into),
                "remove shortcut",
            );
        }
    }

    report(0.40);
    note(crate::install::unregister(), "remove registration");

    report(0.55);
    if options.delete_data {
        note(
            data_dir().and_then(|d| remove_dir_patiently(&d)),
            "delete browsing data",
        );
    } else if let Ok(d) = data_dir()
        && only_warmed_up(&d)
    {
        // Never opened: what is there is the installer's warm-up, not the
        // user's data.
        note(remove_dir_patiently(&d), "remove the warm-up's data");
    }

    report(0.80);
    let dir = install_dir();
    let me = std::env::current_exe().ok();
    let mut pending = Pending::default();
    match me.filter(|me| is_inside(&me.to_string_lossy(), &dir.to_string_lossy())) {
        Some(me) => {
            // Everything but ourselves now; ourselves at exit.
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.as_path() == me.as_path() {
                        continue;
                    }
                    let r = if path.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                    if let Err(e) = r {
                        log!("couldn't remove {}: {e}", path.display());
                    }
                }
            }
            pending.self_delete = Some((me, dir));
        }
        None => note(remove_dir_patiently(&dir), "remove Copper's folder"),
    }

    report(1.0);
    (pending, first_error.map_or(Ok(()), Err))
}

/// The last step, as the process exits: hand the running exe and its
/// folder to a `cmd.exe` that deletes them once this process is gone.
pub fn finish(pending: Pending) {
    if let Some((exe, dir)) = pending.self_delete {
        process::delete_after_exit(&exe, &dir);
    }
}
