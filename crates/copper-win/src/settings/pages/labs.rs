//! Labs (`Trails/Flights.swift` 101–153): previews of what Copper is
//! trying next. Trails needs the sidebar's trail drawing, which Copper for
//! Windows doesn't have yet, so both rows say so and stay off.

use super::not_yet;
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;

/// Labs: previews.
pub(super) struct Labs;

impl Page for Labs {
    fn id(&self) -> &'static str {
        "labs"
    }
    fn title(&self) -> &'static str {
        "Labs"
    }
    fn blurb(&self) -> &'static str {
        "Previews of what Copper is trying next. Each is off until you turn it on."
    }
    fn glyph(&self) -> char {
        glyph::LABS
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let mut f = Form::default();
        let sec = f.section("Previews");
        not_yet(
            sec,
            "Trails — tabs by what you were doing",
            "Today's tabs gather into lines of intent. Not yet on Windows — it stays off",
        )
        .anchor("labs.trails");
        not_yet(
            sec,
            "Tide",
            "How long a trail is left alone before it drifts under Earlier — once Trails is here",
        )
        .anchor("labs.tide");
        sec.footnote("Turning a preview off puts everything back exactly as it was.");
        f
    }

    fn act(&self, _shell: &Shell, _action: &str, _value: &str) {}
}
