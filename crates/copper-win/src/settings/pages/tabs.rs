//! Tabs (`Settings.swift` 331–389, `Sections.swift` 522–530): the sidebar,
//! switching between tabs, and what happens to tabs you leave; and the
//! calendars the Calendar pin's card reads (copper-shell-core `calendar`).

use super::{not_yet, on};
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::{Form, Tone};
use copper_core::settings::{Glyph, TabsPosition};
use copper_shell_core::calendar;
use std::cell::RefCell;

thread_local! {
    /// The calendar address being typed, and why the last one was refused.
    static TYPED: RefCell<(String, Option<String>)> = RefCell::default();
}

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
        // The shell's answer, not the stored one: with nothing stored yet
        // the tabs are down the side.
        let side = shell.tabs_position() == TabsPosition::Left;
        let (letters, sleeps) = {
            let s = shell.settings();
            (s.glyph() == Glyph::Letters, s.sleeps_tabs())
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

        let sec = f.section("Calendar");
        for (i, (feed, name)) in calendar::list(shell).into_iter().enumerate() {
            // Whose it is, never the address.
            let whose = calendar::owner(&feed.url)
                .unwrap_or_else(|| copper_shell_core::shell::host_of(&feed.url));
            sec.button(
                &name,
                (whose != name).then_some(whose.as_str()),
                &format!("calendar.remove:{i}"),
                "Remove",
                false,
            );
        }
        let (typed, refused) = TYPED.with_borrow(|t| t.clone());
        sec.field(
            "Add a calendar",
            Some(refused.as_deref().unwrap_or(calendar::WHERE)),
            "calendar.typed",
            &typed,
            "Paste its secret address, then press Enter",
        )
        .on_submit("calendar.add", None)
        .tone(if refused.is_some() {
            Tone::Error
        } else {
            Tone::Plain
        })
        .anchor("tabs.calendar")
        .keywords(&["calendar", "google calendar", "ical", "meetings"])
        .footnote("Rest the pointer on a Google Calendar pin to see your next meetings. The addresses are kept on this PC only.");
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
            "calendar.typed" => TYPED.with_borrow_mut(|t| t.0 = value.to_string()),
            "calendar.add" => {
                let refused = calendar::add(shell, value).err();
                TYPED.with_borrow_mut(|t| {
                    *t = match refused {
                        Some(why) => (value.to_string(), Some(why)),
                        None => Default::default(),
                    }
                });
            }
            a => {
                let gone = a
                    .strip_prefix("calendar.remove:")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| calendar::list(shell).into_iter().nth(i));
                if let Some((feed, _)) = gone
                    && let Err(why) = calendar::remove(shell, &feed.url)
                {
                    shell.toast(&why);
                }
            }
        }
    }
}
