//! Tabs (`Settings.swift` 331–389, `Sections.swift` 522–530): the sidebar,
//! switching between tabs, and what happens to tabs you leave.

use super::{not_yet, on};
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;
use copper_core::settings::{Glyph, TabsPosition};

/// Tabs: the sidebar, switching, and tabs you leave.
pub(super) struct Tabs;

impl Page for Tabs {
    fn id(&self) -> &'static str {
        "tabs"
    }
    fn title(&self) -> &'static str {
        "Tabs"
    }
    fn blurb(&self) -> &'static str {
        "The sidebar, switching between tabs, and what happens to tabs you leave."
    }
    fn glyph(&self) -> char {
        glyph::TABS
    }

    fn draw(&self, shell: &Shell) -> Form {
        let (side, letters, sleeps) = {
            let s = shell.settings();
            (
                s.tabs_position() == TabsPosition::Left,
                s.glyph() == Glyph::Letters,
                s.sleeps_tabs(),
            )
        };
        let mut f = Form::default();
        f.section("Layout")
            .switch(
                "Tabs in a sidebar",
                Some("Down the left instead of across the top. Drag its edge to resize."),
                "sidebar",
                side,
            )
            .anchor("tabs.sidebar")
            .choice(
                "Tabs show",
                Some("Beside the title, and on a pinned square"),
                "glyph",
                &[("letters", "Letters"), ("icons", "Site icons")],
                if letters { "letters" } else { "icons" },
            )
            .anchor("tabs.glyph");

        let sec = f.section("Switching");
        not_yet(
            sec,
            "Ctrl+Tab switches to",
            "The next tab in the row — most recent order is not yet on Windows",
        )
        .anchor("tabs.switching");
        not_yet(
            sec,
            "Swipe between spaces",
            "Not yet on Windows — Ctrl+Alt+← and → move between spaces",
        )
        .anchor("tabs.swipe");

        let sec = f.section("Tabs you leave");
        sec.switch(
            "Sleep tabs you aren't using",
            Some("Only when more pages are open than this PC keeps easily, or memory runs short: the ones you left longest go first, and come back where you left them. Pinned tabs, sound, calls and anything typed stay awake."),
            "sleep",
            sleeps,
        )
        .anchor("tabs.sleep");
        not_yet(
            sec,
            "Archive Today after",
            "Not yet on Windows — tabs stay until you close them",
        )
        .anchor("tabs.archive");
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        match action {
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
            _ => {}
        }
    }
}
