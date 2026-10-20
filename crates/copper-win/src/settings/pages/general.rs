//! General (`Settings.swift` 273–326, `SettingsFork.swift` 1175–1217): the
//! default browser, how Copper looks, and moving in from another browser.
//!
//! Windows doesn't let an app make itself the default browser; "Make
//! default" opens Settings › Default apps on Copper's page, and the row
//! says Copper is the default only when Windows says so
//! ([`is_default_browser`]) — checked each time the page is drawn, and
//! every second for two minutes after the button is pressed.

use super::{count, not_yet, redraw};
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::{Form, Tone};
use copper_core::address::ENGINES;
use copper_core::settings::Look;
use copper_shell_core::forms::rt;
use std::cell::RefCell;
use std::time::Duration;

/// General: the default browser, how Copper looks, and moving in.
pub(super) struct General;

/// How many places another browser's history may bring in at once.
const HISTORY_LIMIT: usize = 50_000;

/// What the page is doing on its own.
#[derive(Default)]
struct State {
    /// "Make default" was pressed: Windows Settings is (or was) open.
    asked: bool,
    /// Reading other browsers' history.
    reading: bool,
    /// What the last history read came to.
    history: Option<(String, Tone)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// Whether Windows sends web links to Copper: the effective handler of
/// `https` is Copper's own ProgId (`CopperURL`, written by the installer).
/// `None` when Windows can't say.
fn is_default_browser() -> Option<bool> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::{
        AL_EFFECTIVE, AT_URLPROTOCOL, ApplicationAssociationRegistration,
        IApplicationAssociationRegistration,
    };
    // SAFETY: COM is initialised on the UI thread (WebView2 needs it); the
    // returned string is the caller's to free, and is freed once read.
    unsafe {
        let r: IApplicationAssociationRegistration = CoCreateInstance(
            &ApplicationAssociationRegistration,
            None,
            CLSCTX_INPROC_SERVER,
        )
        .ok()?;
        let p = r
            .QueryCurrentDefault(windows::core::w!("https"), AT_URLPROTOCOL, AL_EFFECTIVE)
            .ok()?;
        let progid = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        Some(progid? == copper_setup::registration::PROGID_URL)
    }
}

/// After "Make default": look again every second for two minutes, and
/// redraw the moment Windows says Copper is the default.
fn watch_default(shell: &Shell) {
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        for _ in 0..120 {
            copper_win_rt::sleep(Duration::from_secs(1)).await;
            if is_default_browser() == Some(true) {
                if let Some(s) = weak.upgrade() {
                    redraw(&s, "general");
                }
                return;
            }
        }
    });
}

/// Every Chromium-family browser's history into Copper's, then say what
/// came over.
fn bring_in_history(shell: &Shell) {
    if STATE.with(|s| std::mem::replace(&mut s.borrow_mut().reading, true)) {
        return;
    }
    redraw(shell, "general");
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let read = rt::blocking(|| {
            let browsers = copper_import::config_dir()
                .map(|c| copper_import::detect(&c))
                .unwrap_or_default();
            let mut places = Vec::new();
            let mut busy = Vec::new();
            for b in &browsers {
                let r = copper_import::history::read_all(&b.profile_dirs(), HISTORY_LIMIT);
                if !r.in_use.is_empty() {
                    busy.push(b.name.clone());
                }
                places.extend(r.places);
            }
            (browsers.len(), places, busy)
        })
        .await;
        let Some(shell) = weak.upgrade() else { return };
        let said = match read {
            None => (
                "Copper couldn't read the other browsers".to_string(),
                Tone::Error,
            ),
            Some((0, _, _)) => (
                "No Chrome, Edge, Brave, Vivaldi or Arc history on this PC".to_string(),
                Tone::Error,
            ),
            Some((_, places, busy)) => {
                if !places.is_empty() {
                    let mut h = shell.history_mut();
                    for p in &places {
                        p.take_into(&mut h);
                    }
                }
                if !places.is_empty() {
                    shell.history_changed();
                }
                match busy.first() {
                    Some(name) => (
                        format!("{name} is in use — close the browser and try again"),
                        Tone::Error,
                    ),
                    None => (
                        format!("{} brought in", count(places.len(), "place", "places")),
                        Tone::Good,
                    ),
                }
            }
        };
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.reading = false;
            s.history = Some(said);
        });
        redraw(&shell, "general");
    });
}

impl Page for General {
    fn id(&self) -> &'static str {
        "general"
    }
    fn title(&self) -> &'static str {
        "General"
    }
    fn blurb(&self) -> &'static str {
        "The default browser, how Copper looks, and moving in from another browser."
    }
    fn glyph(&self) -> char {
        glyph::GEAR
    }

    fn draw(&self, shell: &Shell) -> Form {
        let (look, engine) = {
            let s = shell.settings();
            (s.look(), s.search_engine().id)
        };
        let places = shell.history().len();
        let (asked, reading, history) = STATE.with(|s| {
            let s = s.borrow();
            (s.asked, s.reading, s.history.clone())
        });
        let mut f = Form::default();
        let sec = f.section("Copper on this PC");
        match is_default_browser() {
            Some(true) => {
                sec.check(
                    "Open links from other apps",
                    Some(if asked {
                        "Links now open here"
                    } else {
                        "Copper is the default browser on this PC"
                    }),
                );
            }
            _ => {
                sec.button(
                    "Open links from other apps",
                    Some(if asked {
                        "Choose Copper for web links in Windows Settings › Default apps — this says so once Windows does"
                    } else {
                        "Mail, Slack and the rest still send links elsewhere"
                    }),
                    "default-browser",
                    "Make default",
                    false,
                );
            }
        }
        sec.anchor("general.default").keywords(&["default browser"]);
        sec.choice(
            "Appearance",
            Some("Light, dark, or whatever Windows is doing — pages follow it too"),
            "look",
            &[("light", "Light"), ("dark", "Dark"), ("system", "System")],
            look.as_str(),
        )
        .anchor("general.appearance");
        not_yet(
            sec,
            "Correct spelling as you type",
            "Not yet on Windows — pages keep the engine's own spelling underline",
        )
        .anchor("general.autocorrect");

        let sec = f.section("Moving in");
        sec.button(
            "Move in from another browser",
            Some("Tabs, bookmarks and history from Chrome, Edge, Brave, Vivaldi or Arc — and passwords from a CSV they export"),
            "move-in",
            "Move in…",
            true,
        )
        .anchor("general.movein");
        let detail = match (&history, places) {
            (Some((said, _)), _) => said.clone(),
            (None, 0) => "Typed addresses complete from Chrome or Edge history".to_string(),
            (None, n) => format!(
                "{} kept — typed addresses complete from them",
                count(n, "place", "places")
            ),
        };
        sec.pills(
            "Browsing history",
            Some(&detail),
            vec![crate::settings::form::Pill {
                action: "history".into(),
                label: if reading { "Reading…" } else { "Bring in" }.into(),
                filled: false,
                enabled: !reading,
            }],
        )
        .tone(history.map(|(_, t)| t).unwrap_or_default())
        .anchor("general.history");

        let engines: Vec<(&str, &str)> = ENGINES.iter().map(|e| (e.id, e.name)).collect();
        f.section("Search")
            .pick(
                "Search with",
                Some("What the address field asks when you type words, not an address"),
                "engine",
                &engines,
                engine,
            )
            .anchor("general.search");

        let sec = f.section("Developer");
        not_yet(
            sec,
            "Let a script drive Copper",
            "Not yet on Windows — the copper command and its MCP server drive Copper instead",
        )
        .anchor("general.bench");
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        match action {
            "default-browser" => {
                STATE.with(|s| s.borrow_mut().asked = true);
                if let Err(e) = copper_setup::open_default_apps_settings() {
                    tracing::warn!("default apps: {e}");
                }
                watch_default(shell);
            }
            "look" => {
                if let Some(l) = Look::parse(value) {
                    shell.set_look(l);
                }
            }
            "move-in" => crate::settings::movein::open(shell),
            "history" => bring_in_history(shell),
            "engine" => shell.update_settings(|s| s.set_search_engine(value)),
            _ => {}
        }
    }
}
