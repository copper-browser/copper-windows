//! Addon updates in Copper's own update UI (CONTRACT §6.3; the Mac's
//! `Fork/Addons/AddonUpdates.swift`; Linux `addons/updates.rs`).
//!
//! A new addon or version is downloaded, verified and staged by the core
//! (`AddonSync`); here it surfaces as **"Update available"** — "<Addon
//! name> <version> for the Agent sidebar" — on Settings › About (after
//! Copper's own update, which keeps priority), as a toast once per staged
//! version, and as a palette command. **Install** unpacks it and the slot
//! reloads with it; no relaunch. Every Copper update check (the person's
//! "Check now") also asks the cloud for addons.

use super::glue;
use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::{Shell, Win};
use crate::settings::form::Form;
use std::cell::{Cell, RefCell};

/// The actions this module answers on Settings › About (the page sends
/// every action starting with `addon-` here).
const INSTALL: &str = "addon-install";
const CHECK: &str = "addon-check";

thread_local! {
    /// The staged update the toast last spoke of.
    static TOASTED: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    /// Copper's own check was running when last seen (to ask for addons
    /// with it).
    static CHECKING: Cell<bool> = const { Cell::new(false) };
    /// An Install is running.
    static INSTALLING: Cell<bool> = const { Cell::new(false) };
}

/// The palette's row for a staged update.
pub fn register(shell: &Shell) {
    shell.view().commands().add(
        Command::new(
            "addon-install",
            "Install Addon Update",
            glyph::ADDONS,
            install,
        )
        .when(|_| glue::pending().is_some()),
    );
}

/// The words of a staged update (CONTRACT §6.3).
pub fn words(p: &glue::Pending) -> String {
    format!("{} {} for the {}", p.name, p.version, p.slot_name)
}

/// The addons changed: Settings › About redraws; a newly staged version
/// is announced once.
pub fn changed(shell: &Shell) {
    crate::settings::changed(shell, "about");
    let Some(p) = glue::pending() else {
        TOASTED.with(|t| t.borrow_mut().take());
        return;
    };
    let key = (p.id.clone(), p.version.clone());
    let fresh = TOASTED.with(|t| {
        let mut t = t.borrow_mut();
        if t.as_ref() == Some(&key) {
            false
        } else {
            *t = Some(key);
            true
        }
    });
    if fresh && !glue::selftest_run() {
        shell.toast(&format!(
            "Update available: {} — Settings › About",
            words(&p)
        ));
    }
}

/// Settings › About: the addon section (after Copper's own).
pub fn section(f: &mut Form) {
    // Copper's own "Check now" asks for addons too.
    let checking = copper_shell_core::updates::status::<Win>().checking;
    if CHECKING.with(|c| c.replace(checking)) != checking && checking {
        glue::check_now();
    }
    let status = glue::status();
    if !status.cloud && glue::active().is_none() {
        return;
    }
    let sec = f.section("Addons");
    match glue::pending() {
        Some(p) => {
            let installing = INSTALLING.with(Cell::get);
            sec.button(
                "Update available",
                Some(&words(&p)),
                INSTALL,
                if installing {
                    "Installing…"
                } else {
                    "Install"
                },
                true,
            );
        }
        None => {
            let line = if status.checking {
                "Looking…".to_string()
            } else if let Some(e) = &status.error {
                format!("The last look failed: {e}")
            } else if !status.linked {
                "Addons come from your Copper Cloud. Link one in Settings › Cloud.".to_string()
            } else if let Some(when) = &status.checked {
                format!("Up to date — looked {when}")
            } else {
                "Not looked yet".to_string()
            };
            sec.button("Addons", Some(&line), CHECK, "Check now", false);
        }
    }
}

/// A Settings › About control of this module's was used.
pub fn act(shell: &Shell, action: &str) {
    match action {
        INSTALL => install(shell),
        CHECK => {
            glue::check_now();
            crate::settings::changed(shell, "about");
        }
        _ => {}
    }
}

/// Install the staged update; the slot reloads with it.
pub fn install(shell: &Shell) {
    if INSTALLING.with(|i| i.replace(true)) {
        return;
    }
    crate::settings::changed(shell, "about");
    let weak = shell.downgrade();
    glue::install_pending(move |r| {
        INSTALLING.with(|i| i.set(false));
        let Some(s) = weak.upgrade() else { return };
        match r {
            Ok(names) if !names.is_empty() => {
                s.toast(&format!("Installed {}", names.join(", ")));
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, "addon install failed");
                s.toast(&format!("The addon couldn't be installed: {e}"));
            }
        }
        super::changed(&s);
    });
}
