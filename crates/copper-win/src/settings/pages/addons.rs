//! Addons: what Copper Cloud put in Copper's slots (CONTRACT §6.3; the
//! Mac's `Fork/Addons/AddonsSettings.swift`; Linux `addons/settings.rs`).
//! Installed and staged addons — name, version, slot, source cloud — each
//! with "Use in Agent sidebar" and "Remove"; "Check for addon updates";
//! and, with no cloud linked, the way to link one. Copper's own agent (the
//! built-in addon, `addons::builtin`) is never listed: it is what the slot
//! shows when no addon here does.

use crate::addons::glue;
use crate::chrome::{glyph, pane};
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;

/// Addons: Copper's cloud-assigned addons.
pub(super) struct Addons;

impl Page for Addons {
    fn id(&self) -> &'static str {
        "addons"
    }
    fn title(&self) -> &'static str {
        "Addons"
    }
    fn blurb(&self) -> &'static str {
        "Panels your Copper Cloud adds to Copper, like the Agent sidebar."
    }
    fn glyph(&self) -> char {
        glyph::ADDONS
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let mut f = Form::default();
        let status = glue::status();
        let list = glue::list();

        if !status.linked && list.iter().all(|a| !a.dev) {
            f.section("").button(
                "Addons come from your Copper Cloud",
                Some("Link one in Settings › Cloud."),
                "cloud",
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
                        "install",
                        "Update",
                        true,
                    );
                }
                (Some(_), false) => {
                    sec.button(
                        &title,
                        Some(&format!("{detail}. Ready to install.")),
                        "install",
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
            f.section("Updates").button(
                "Check for addon updates",
                Some(&line),
                "check",
                "Check now",
                false,
            );
        }

        f.section("Agent sidebar").button(
            pane::NAME,
            Some(&format!(
                "Ctrl+E. {} px wide; drag its left edge to change it.",
                pane::width().round()
            )),
            "toggle",
            if pane::showing() { "Hide" } else { "Show" },
            false,
        );
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        match action {
            "cloud" => crate::settings::open(shell, "cloud"),
            "check" => {
                glue::check_now();
                crate::settings::changed(shell, "addons");
            }
            "install" => crate::addons::updates::install(shell),
            "toggle" => {
                crate::addons::toggle(shell);
                crate::settings::changed(shell, "addons");
            }
            a => {
                if let Some(id) = a.strip_prefix("use:") {
                    glue::set_enabled(id, super::on(value));
                } else if let Some(id) = a.strip_prefix("remove:") {
                    glue::remove(id);
                }
            }
        }
    }
}
