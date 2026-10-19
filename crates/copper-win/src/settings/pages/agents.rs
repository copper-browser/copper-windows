//! Agents: the MCP server on this PC.

use super::on;
use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use crate::settings::Page;
use crate::settings::form::Form;

/// Agents: the MCP server on this PC.
pub(super) struct Agents;

impl Page for Agents {
    fn id(&self) -> &'static str {
        "agents"
    }
    fn title(&self) -> &'static str {
        "Agents"
    }
    fn blurb(&self) -> &'static str {
        "Let MCP clients and terminal agents use this browser."
    }
    fn glyph(&self) -> char {
        glyph::AGENT
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let mut f = Form::default();
        let Some(a) = copper_shell_core::agents::get::<Win>() else {
            f.section("").note("The agent server did not start", None);
            return f;
        };
        let sec = f.section("This window");
        sec.switch(
            "Let agents drive this window",
            Some("An MCP server on this PC only; agents open their own tabs beside yours"),
            "enabled",
            a.enabled(),
        );
        if a.running() {
            sec.value(
                "Listening on",
                Some("Or `copper --mcp-stdio` from an MCP client"),
                &a.endpoint(),
            );
        }
        if let Some(t) = a.trouble() {
            sec.note("It could not start", Some(&t));
        }
        f
    }

    fn act(&self, _shell: &Shell, action: &str, value: &str) {
        if action == "enabled"
            && let Some(a) = copper_shell_core::agents::get::<Win>()
        {
            a.set_enabled(on(value));
        }
    }
}
