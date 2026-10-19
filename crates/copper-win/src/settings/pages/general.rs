//! General: the default browser, how Copper looks, and the layout.

use super::on;
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;
use copper_core::address::ENGINES;
use copper_core::settings::{Glyph, Look, TabsPosition};

/// General: the default browser, how Copper looks, and the layout.
pub(super) struct General;

impl Page for General {
    fn id(&self) -> &'static str {
        "general"
    }
    fn title(&self) -> &'static str {
        "General"
    }
    fn blurb(&self) -> &'static str {
        "The default browser, how Copper looks, and where the tabs go."
    }
    fn glyph(&self) -> char {
        glyph::GEAR
    }

    fn draw(&self, shell: &Shell) -> Form {
        let (look, side, letters, engine, sleeps) = {
            let s = shell.settings();
            (
                s.look(),
                s.tabs_position() == TabsPosition::Left,
                s.glyph() == Glyph::Letters,
                s.search_engine().id,
                s.sleeps_tabs(),
            )
        };
        let mut f = Form::default();
        f.section("Copper on this PC")
            .button(
                "Open links from other apps",
                Some("Windows lets you choose the browser under Settings › Default apps"),
                "default-browser",
                "Choose…",
                true,
            )
            .choice(
                "Appearance",
                Some("Light, dark, or whatever Windows is doing — pages follow it too"),
                "look",
                &[("light", "Light"), ("dark", "Dark"), ("system", "System")],
                look.as_str(),
            );
        f.section("Tabs")
            .switch(
                "Tabs in a sidebar",
                Some("Down the left instead of across the top"),
                "sidebar",
                side,
            )
            .choice(
                "Tabs show",
                Some("Beside the title, and on a pinned square"),
                "glyph",
                &[("letters", "Letters"), ("icons", "Icons")],
                if letters { "letters" } else { "icons" },
            )
            .switch(
                "Let tabs sleep",
                Some("A tab you have not looked at for a while gives back its memory"),
                "sleep",
                sleeps,
            );
        let engines: Vec<(&str, &str)> = ENGINES.iter().map(|e| (e.id, e.name)).collect();
        f.section("Search").choice(
            "Search with",
            Some("What the field asks when you type words, not an address"),
            "engine",
            &engines,
            engine,
        );
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        match action {
            "default-browser" => {
                if let Err(e) = crate::sys::shell::open("ms-settings:defaultapps") {
                    tracing::warn!("default apps: {e}");
                }
            }
            "look" => {
                if let Some(l) = Look::parse(value) {
                    shell.set_look(l);
                }
            }
            "sidebar" => shell.set_tabs_position(if on(value) {
                TabsPosition::Left
            } else {
                TabsPosition::Top
            }),
            "glyph" => {
                if let Some(g) = Glyph::parse(value) {
                    shell.update_settings(|s| s.set_glyph(g));
                    shell.view().sync(shell);
                }
            }
            "sleep" => shell.update_settings(|s| s.set_sleeps_tabs(on(value))),
            "engine" => shell.update_settings(|s| s.set_search_engine(value)),
            _ => {}
        }
    }
}
