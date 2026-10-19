//! Privacy: the shield, and clearing what sites keep.

use super::on;
use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use crate::settings::Page;
use crate::settings::form::Form;

/// Privacy: the shield, and clearing what sites keep.
pub(super) struct Privacy;

impl Page for Privacy {
    fn id(&self) -> &'static str {
        "privacy"
    }
    fn title(&self) -> &'static str {
        "Privacy"
    }
    fn blurb(&self) -> &'static str {
        "Blocking ads and trackers, and clearing what sites keep."
    }
    fn glyph(&self) -> char {
        glyph::SHIELD
    }

    fn draw(&self, shell: &Shell) -> Form {
        let shield = copper_shell_core::shield::enabled::<Win>(shell);
        let paused = shell.settings().shield_paused().len();
        let mut f = Form::default();
        let sec = f.section("Shield");
        sec.switch(
            "Block ads and trackers",
            Some("On every site, unless you turn it off for one"),
            "shield",
            shield,
        );
        if paused > 0 {
            sec.value(
                "Turned off on",
                None,
                &match paused {
                    1 => "1 site".to_string(),
                    n => format!("{n} sites"),
                },
            );
        }
        f.section("What sites keep").button(
            "History, cookies and the cache",
            Some("Each cleared on its own, in History"),
            "clear",
            "Clear…",
            false,
        );
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        match action {
            "shield" => {
                copper_shell_core::shield::set_enabled::<Win>(shell, on(value));
            }
            "clear" => crate::panels::show(shell, "history"),
            _ => {}
        }
    }
}
