//! The pages Settings opens with. General is whole; the others say and
//! set what Copper for Windows does today, in the Mac's words
//! (`SettingsIndex.swift` blurbs).

use super::Page;
use super::form::Form;
use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use copper_core::address::ENGINES;
use copper_core::settings::{Glyph, Look, TabsPosition, keys};
use std::rc::Rc;

/// Every page, in the rail's order.
pub fn all() -> Vec<Rc<dyn Page>> {
    vec![
        Rc::new(General),
        Rc::new(Privacy),
        Rc::new(Passwords),
        Rc::new(Cloud),
        Rc::new(Agents),
        Rc::new(Updates),
        Rc::new(Addons),
    ]
}

fn on(v: &str) -> bool {
    v == "on"
}

/// General: the default browser, how Copper looks, and the layout.
struct General;

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

/// Privacy: the shield, and clearing what sites keep.
struct Privacy;

impl Page for Privacy {
    fn id(&self) -> &'static str {
        "privacy"
    }
    fn title(&self) -> &'static str {
        "Privacy"
    }
    fn blurb(&self) -> &'static str {
        "Blocking ads and trackers, and clearing what sites keep."
    }
    fn glyph(&self) -> char {
        glyph::SHIELD
    }

    fn draw(&self, shell: &Shell) -> Form {
        let shield = copper_shell_core::shield::enabled::<Win>(shell);
        let paused = shell.settings().shield_paused().len();
        let mut f = Form::default();
        let sec = f.section("Shield");
        sec.switch(
            "Block ads and trackers",
            Some("On every site, unless you turn it off for one"),
            "shield",
            shield,
        );
        if paused > 0 {
            sec.value(
                "Turned off on",
                None,
                &match paused {
                    1 => "1 site".to_string(),
                    n => format!("{n} sites"),
                },
            );
        }
        f.section("What sites keep").button(
            "History, cookies and the cache",
            Some("Each cleared on its own, in History"),
            "clear",
            "Clear…",
            false,
        );
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        match action {
            "shield" => {
                copper_shell_core::shield::set_enabled::<Win>(shell, on(value));
            }
            "clear" => crate::panels::show(shell, "history"),
            _ => {}
        }
    }
}

/// Passwords: whether Copper offers to keep them and fills them.
struct Passwords;

impl Page for Passwords {
    fn id(&self) -> &'static str {
        "passwords"
    }
    fn title(&self) -> &'static str {
        "Passwords"
    }
    fn blurb(&self) -> &'static str {
        "Saved sign-ins, and whether Copper offers and fills them."
    }
    fn glyph(&self) -> char {
        glyph::KEY
    }

    fn draw(&self, shell: &Shell) -> Form {
        let (save, fill) = {
            let s = shell.settings();
            (s.saves_passwords(), s.fills_passwords())
        };
        let never = copper_shell_core::forms::never_list(shell).len();
        let mut f = Form::default();
        let sec = f.section("Signing in");
        sec.switch(
            "Offer to save passwords",
            Some("After you sign in somewhere new"),
            "save",
            save,
        )
        .switch(
            "Fill passwords",
            Some("The accounts you keep, under the box you sign in with"),
            "fill",
            fill,
        );
        if never > 0 {
            sec.value(
                "Never offered on",
                None,
                &match never {
                    1 => "1 site".to_string(),
                    n => format!("{n} sites"),
                },
            );
        }
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        let key = match action {
            "save" => keys::PASSWORDS_SAVE,
            "fill" => keys::PASSWORDS_FILL,
            _ => return,
        };
        shell.update_settings(|s| s.set(key, on(value)));
    }
}

/// Cloud: where this PC stands with Copper Cloud.
struct Cloud;

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

/// Agents: the MCP server on this PC.
struct Agents;

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

/// Updates: which Copper this is, and the next one.
struct Updates;

impl Page for Updates {
    fn id(&self) -> &'static str {
        "updates"
    }
    fn title(&self) -> &'static str {
        "Updates"
    }
    fn blurb(&self) -> &'static str {
        "Which Copper you have, and getting the next one."
    }
    fn glyph(&self) -> char {
        glyph::UPDATE
    }

    fn draw(&self, _shell: &Shell) -> Form {
        use copper_shell_core::updates;
        let st = updates::status::<Win>();
        let newer = updates::available::<Win>();
        let mut f = Form::default();
        let sec = f.section("Copper for Windows");
        sec.value("This Copper", None, env!("CARGO_PKG_VERSION"));
        let line = if st.checking {
            "Looking…".to_string()
        } else if let Some(e) = &st.error {
            format!("The last look failed: {e}")
        } else if let Some(v) = &newer {
            format!("Copper {v} is out")
        } else if let Some(when) = st.checked {
            format!(
                "Up to date — looked {}",
                copper_shell_core::format::ago::<crate::sys::calendar::WinCalendar>(
                    when,
                    copper_core::date::now_unix()
                )
            )
        } else {
            "Not looked yet".to_string()
        };
        sec.button("The newest", Some(&line), "check", "Check now", false);
        if newer.is_some() {
            sec.button(
                "Get it",
                Some("From the releases page"),
                "download",
                "Download",
                true,
            );
        }
        f
    }

    fn act(&self, shell: &Shell, action: &str, _value: &str) {
        use copper_shell_core::updates;
        match action {
            "check" => updates::check::<Win>(shell, true),
            "download" => {
                if let Err(e) = crate::sys::shell::open(&updates::release_page::<Win>()) {
                    tracing::warn!("release page: {e}");
                }
            }
            _ => {}
        }
    }
}

/// Add-ons: Chrome extensions.
struct Addons;

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
