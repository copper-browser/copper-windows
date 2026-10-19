//! Cloud: where this PC stands with Copper Cloud.

use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;

/// Cloud: where this PC stands with Copper Cloud.
pub(super) struct Cloud;

impl Page for Cloud {
    fn id(&self) -> &'static str {
        "cloud"
    }
    fn title(&self) -> &'static str {
        "Cloud"
    }
    fn blurb(&self) -> &'static str {
        "Connect, sign in, then choose what follows you between computers."
    }
    fn glyph(&self) -> char {
        glyph::CLOUD
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let (state, detail) = match copper_shell_core::cloud::get() {
            None => ("Off", "This build of Copper has no cloud".to_string()),
            Some(c) if !c.client.is_linked() => (
                "Not connected",
                "Run `copper cloud link <code>` with the code from your administrator".to_string(),
            ),
            Some(c) if !c.client.is_signed_in() => (
                "Connected",
                "Run `copper cloud login` to sign in".to_string(),
            ),
            Some(c) => {
                let who = c.client.account().map(|a| a.email).unwrap_or_default();
                let on = c.sync.is_on();
                (
                    if on { "Syncing" } else { "Signed in" },
                    format!("As {who} on {}", c.client.device_name()),
                )
            }
        };
        let mut f = Form::default();
        f.section("Copper Cloud")
            .value("Status", Some(&detail), state);
        f
    }

    fn act(&self, _shell: &Shell, _action: &str, _value: &str) {}
}
