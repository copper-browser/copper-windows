//! Extensions: Chrome extensions (`ExtensionsManager.swift`). Copper's own
//! addons are Settings › Addons, a different thing. Copper for Windows
//! can't run Chrome extensions yet, so the page says so and offers nothing
//! to press (`ExtensionsUI.swift` 4–19 does the same on an older macOS).

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
        "Chrome extensions: add, pin, allow on sites, and remove."
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
        f
    }

    fn act(&self, _shell: &Shell, _action: &str, _value: &str) {}
}
