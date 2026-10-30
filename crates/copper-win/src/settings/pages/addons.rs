//! Settings › Extensions › Addons: what Copper Cloud put in Copper's slots
//! (CONTRACT §6.3; the Mac's `Fork/Addons/AddonUI.swift` `AddonsPage`, a
//! page of its own until Copper#115; Linux `addons/settings.rs`).
//! Installed and staged addons — name, version, slot, source cloud — each
//! with "Use in Agent sidebar" and "Remove"; "Check for addon updates";
//! and, with no cloud linked, the way to link one. Copper's own agent (the
//! built-in addon, `addons::builtin`) is never listed: it is what the slot
//! shows when no addon here does.

use crate::addons::glue;
use crate::chrome::pane;
use crate::platform::Shell;
use crate::settings::form::Form;

/// The Addons sections, on `f`.
pub(super) fn section(f: &mut Form) {
    let status = glue::status();
    let list = glue::list();
    let first = f.sections.len();

    if !status.linked && list.iter().all(|a| !a.dev) {
        f.section("Addons").button(
            "Addons come from a Copper Cloud",
            Some("Web apps an admin assigns, like the Agent sidebar's"),
            "addons.cloud",
            "Open Cloud",
            true,
        );
    }

    for a in &list {
        let sec = f.section(&a.name);
        let mut detail = a.slot_name.clone();
        if a.dev {
            detail.push_str(" · from a developer folder (COPPER_ADDONS_DEV_DIR)");
        } else if !a.source.is_empty() {
            detail.push_str(&format!(" · from {}", a.source));
        }
        let title = format!("Version {}", a.version);
        match (&a.pending, a.installed) {
            (Some(v), true) => {
                sec.button(
                    &title,
                    Some(&format!("{detail}. Version {v} is ready.")),
                    "addons.install",
                    "Update",
                    true,
                );
            }
            (Some(_), false) => {
                sec.button(
                    &title,
                    Some(&format!("{detail}. Ready to install.")),
                    "addons.install",
                    "Install",
                    true,
                );
            }
            _ => {
                sec.note(&title, Some(&detail));
            }
        }
        if a.dev {
            continue;
        }
        sec.switch(
            "Use in Agent sidebar",
            Some(if a.enabled {
                "Ctrl+E shows it, right of the page."
            } else {
                "Off here: the Agent sidebar shows Copper's own agent."
            }),
            &format!("use:{}", a.id),
            a.enabled,
        );
        sec.button(
            "Remove",
            Some("From this Copper, until your cloud changes it."),
            &format!("remove:{}", a.id),
            "Remove",
            false,
        );
    }

    if status.cloud {
        let line = if status.checking {
            "Looking…".to_string()
        } else if let Some(e) = &status.error {
            format!("The last look failed: {e}")
        } else if let Some(when) = &status.checked {
            format!("Looked {when}")
        } else if !status.linked {
            "Not linked to a Copper Cloud".to_string()
        } else {
            "Not looked yet".to_string()
        };
        let sec = if f.sections.len() == first {
            f.section("Addons")
        } else {
            f.more()
        };
        sec.button(
            "Check for addon updates",
            Some(&line),
            "addons.check",
            "Check now",
            false,
        )
        .anchor("addons.check");
    }
    if let Some(sec) = f.sections.get_mut(first) {
        sec.card_anchor("addons.installed");
    }

    f.section("Agent sidebar").button(
        pane::NAME,
        Some(&format!(
            "Ctrl+E. {} px wide; drag its left edge to change it.",
            pane::width().round()
        )),
        "addons.toggle",
        if pane::showing() { "Hide" } else { "Show" },
        false,
    );
}

/// A control in the sections was used; `false` if `action` isn't one.
pub(super) fn act(shell: &Shell, action: &str, value: &str) -> bool {
    match action {
        "addons.cloud" => crate::settings::open(shell, "cloud"),
        "addons.check" => {
            glue::check_now();
            crate::settings::changed(shell, "extensions");
        }
        "addons.install" => crate::addons::updates::install(shell),
        "addons.toggle" => {
            crate::addons::toggle(shell);
            crate::settings::changed(shell, "extensions");
        }
        a => {
            if let Some(id) = a.strip_prefix("use:") {
                glue::set_enabled(id, super::on(value));
            } else if let Some(id) = a.strip_prefix("remove:") {
                glue::remove(id);
            } else {
                return false;
            }
        }
    }
    true
}
