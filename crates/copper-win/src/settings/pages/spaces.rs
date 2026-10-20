//! Spaces (`Fork/SpacePage.swift`): which space, and how it looks — its
//! icon, its colour, its profile, its place in the row — and deleting it.
//!
//! Drawn with the plain form for now: the Mac's page has a picker of
//! chips, a name field, an icon grid and a live preview, which want the
//! panel's own-body hook. Until then a space is picked, marked, coloured,
//! given a profile, moved and deleted from here, and renamed nowhere.

use crate::chrome::glyph;
use crate::palette::{spaces, symbols};
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;
use copper_shell_core::spaces::colour::SpaceColour;

/// Spaces: Settings › Spaces.
pub(super) struct Spaces;

/// How many choices one segmented line holds before the next line.
const PER_LINE: usize = 5;

impl Page for Spaces {
    fn id(&self) -> &'static str {
        "spaces"
    }
    fn title(&self) -> &'static str {
        "Spaces"
    }
    fn blurb(&self) -> &'static str {
        "Each space keeps its own tabs, colour and icon — and, if you like, its own sign-ins."
    }
    fn glyph(&self) -> char {
        glyph::SPACES
    }

    fn draw(&self, shell: &Shell) -> Form {
        let id = spaces::editing(shell);
        let b = shell.browser();
        let all: Vec<(String, String)> = b
            .spaces()
            .map(|s| (s.id.clone(), symbols::title(s.icon.as_deref(), &s.name)))
            .collect();
        let count = all.len();
        let index = all.iter().position(|(i, _)| *i == id).unwrap_or(0);
        let Some(space) = b.space(&id).cloned() else {
            return Form::default();
        };
        let tabs = b.row(&id).len();
        let profiles = b.profiles();
        let current = b.current_space_id() == id;
        drop(b);
        let title = symbols::title(space.icon.as_deref(), &space.name);

        let mut f = Form::default();
        let pick = f.section("Your spaces");
        let options: Vec<(&str, &str)> =
            all.iter().map(|(i, t)| (i.as_str(), t.as_str())).collect();
        for (n, chunk) in options.chunks(PER_LINE).enumerate() {
            let chosen = if chunk.iter().any(|(i, _)| *i == id) {
                id.as_str()
            } else {
                ""
            };
            pick.choice(
                if n == 0 { "Showing" } else { "" },
                (n == 0).then_some("Pick one to change it; this does not switch to it"),
                "pick",
                chunk,
                chosen,
            );
        }
        pick.button(
            "New Space",
            Some("A name, a colour nobody wears, and this page to make it yours"),
            "new",
            "New Space",
            false,
        );

        let look = SpaceColour::of(&space);
        let sec = f.section(&title);
        let you = if current {
            " · the space you are in"
        } else {
            ""
        };
        sec.value(
            "Tabs",
            Some(&format!("Pins are in every space{you}")),
            &symbols::tabs(tabs),
        );
        let icon_now = symbols::symbol(space.icon.as_deref())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if space.icon.as_deref().is_some_and(|i| !i.is_empty()) {
                    "emoji".into()
                } else {
                    "none".into()
                }
            });
        let mut icons: Vec<(String, String)> = vec![("none".into(), "None".into())];
        if icon_now == "emoji"
            && let Some(e) = &space.icon
        {
            icons.push(("emoji".into(), e.clone()));
        }
        icons.extend(
            symbols::SYMBOLS
                .iter()
                .map(|(n, label, _)| ((*n).to_string(), (*label).to_string())),
        );
        // A symbol the list does not know (set elsewhere) is still offered.
        if icon_now != "none" && icon_now != "emoji" && !icons.iter().any(|(v, _)| *v == icon_now) {
            icons.insert(1, (icon_now.clone(), symbols::symbol_name(&icon_now)));
        }
        let icons: Vec<(&str, &str)> = icons
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        for (n, chunk) in icons.chunks(PER_LINE).enumerate() {
            let chosen = if chunk.iter().any(|(v, _)| *v == icon_now) {
                icon_now.as_str()
            } else {
                ""
            };
            sec.choice(
                if n == 0 { "Icon" } else { "" },
                (n == 0).then_some("A dot, a symbol or an emoji — never a letter"),
                "icon",
                chunk,
                chosen,
            );
        }
        let colours: Vec<(&str, &str)> = SpaceColour::ALL
            .iter()
            .map(|c| (c.name(), c.name()))
            .collect();
        for (n, chunk) in colours.chunks(PER_LINE).enumerate() {
            sec.choice(
                if n == 0 { "Colour" } else { "" },
                (n == 0).then_some("The column and the card wear it"),
                "colour",
                chunk,
                look.name(),
            );
        }
        let mut names: Vec<(String, String)> = vec![(String::new(), "Shared".into())];
        names.extend(profiles.iter().map(|p| (p.clone(), p.clone())));
        let names: Vec<(&str, &str)> = names
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        sec.choice(
            "Profile",
            Some("A profile keeps its own sign-ins; pages open now keep theirs until reloaded"),
            "profile",
            &names,
            space.profile.as_deref().unwrap_or(""),
        );
        let order = f.section("Order");
        if index > 0 {
            order.button("Move Left", None, "left", "Move Left", false);
        }
        if index + 1 < count {
            order.button("Move Right", None, "right", "Move Right", false);
        }
        if count > 1 {
            f.section("Delete").button(
                &format!("Delete \u{201C}{title}\u{201D}"),
                Some("Asks first, and can keep its tabs"),
                "delete",
                "Delete Space\u{2026}",
                false,
            );
        }
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        let id = spaces::editing(shell);
        match action {
            "pick" => spaces::pick(value),
            "new" => spaces::new_space(shell),
            "icon" => match value {
                "none" => shell.set_space_icon(&id, None),
                "emoji" => {}
                v => shell.set_space_icon(&id, Some(&format!("sf:{v}"))),
            },
            "colour" => {
                if let Some(c) = SpaceColour::parse(value) {
                    shell.set_space_colour(&id, c);
                }
            }
            "profile" => shell.set_space_profile(&id, (!value.is_empty()).then_some(value)),
            "left" => shell.nudge_space(&id, -1),
            "right" => shell.nudge_space(&id, 1),
            "delete" => spaces::ask_delete(shell, &id),
            _ => {}
        }
    }
}
