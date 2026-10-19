//! Passwords: whether Copper offers to keep them and fills them.

use super::on;
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;
use copper_core::settings::keys;

/// Passwords: whether Copper offers to keep them and fills them.
pub(super) struct Passwords;

impl Page for Passwords {
    fn id(&self) -> &'static str {
        "passwords"
    }
    fn title(&self) -> &'static str {
        "Passwords"
    }
    fn blurb(&self) -> &'static str {
        "Saved sign-ins, and whether Copper offers and fills them."
    }
    fn glyph(&self) -> char {
        glyph::KEY
    }

    fn draw(&self, shell: &Shell) -> Form {
        let (save, fill) = {
            let s = shell.settings();
            (s.saves_passwords(), s.fills_passwords())
        };
        let never = copper_shell_core::forms::never_list(shell).len();
        let mut f = Form::default();
        let sec = f.section("Signing in");
        sec.switch(
            "Offer to save passwords",
            Some("After you sign in somewhere new"),
            "save",
            save,
        )
        .switch(
            "Fill passwords",
            Some("The accounts you keep, under the box you sign in with"),
            "fill",
            fill,
        );
        if never > 0 {
            sec.value(
                "Never offered on",
                None,
                &match never {
                    1 => "1 site".to_string(),
                    n => format!("{n} sites"),
                },
            );
        }
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        let key = match action {
            "save" => keys::PASSWORDS_SAVE,
            "fill" => keys::PASSWORDS_FILL,
            _ => return,
        };
        shell.update_settings(|s| s.set(key, on(value)));
    }
}
