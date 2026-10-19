//! Add-ons: Chrome extensions.

use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;

/// Add-ons: Chrome extensions.
pub(super) struct Addons;

impl Page for Addons {
    fn id(&self) -> &'static str {
        "addons"
    }
    fn title(&self) -> &'static str {
        "Add-ons"
    }
    fn blurb(&self) -> &'static str {
        "Chrome extensions: add, pin, allow on sites, and remove."
    }
    fn glyph(&self) -> char {
        glyph::ADDONS
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let mut f = Form::default();
        f.section("Extensions").note(
            "None added",
            Some("Copper for Windows does not load Chrome extensions in this version"),
        );
        f
    }

    fn act(&self, _shell: &Shell, _action: &str, _value: &str) {}
}
