//! Extensions: Chrome extensions (`ExtensionsManager.swift`), and under
//! them Copper's own addons ([`super::addons`], a page of its own until
//! Copper#115). Copper for Windows can't run Chrome extensions yet, so
//! that row says so and offers nothing to press (`ExtensionsUI.swift`
//! 4–19 does the same on an older macOS).

use super::not_yet;
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;

/// Extensions: Chrome extensions.
pub(super) struct Extensions;

impl Page for Extensions {
    fn id(&self) -> &'static str {
        "extensions"
    }
    fn title(&self) -> &'static str {
        "Extensions"
    }
    fn blurb(&self) -> &'static str {
        "Chrome extensions, and the addons your Copper Cloud assigns."
    }
    fn glyph(&self) -> char {
        glyph::ADDONS
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let mut f = Form::default();
        let sec = f.section("");
        not_yet(
            sec,
            "Chrome extensions",
            "Not yet on Windows — this version of Copper can't add or run them",
        )
        .anchor("extensions.unavailable");
        super::addons::section(&mut f);
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        super::addons::act(shell, action, value);
    }
}
