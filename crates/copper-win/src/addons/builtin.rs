//! Copper's own agent, the Agent sidebar's built-in face (the Mac's native
//! `AgentPane`): a web page compiled into copper.exe that fills the slot
//! whenever no cloud addon does, served as `copper-addon://copper.agent/`
//! in the slot's ordinary addon view and speaking only `window.copper`
//! (bridge v1) — `copper.agent.*` for the conversation, `page` for the
//! chip and the suggestions, `open-url` for links.
//!
//! * **What fills the slot** ([`slot_active`]): the cloud's addon (or the
//!   developer folder) when there is one; else this. A staged update
//!   doesn't change that: the agent stays until the person installs it
//!   (Settings › About, the toast).
//! * **Reserved.** The id is `copper.agent`; what marks it is [`SHA`],
//!   which no served bundle can carry (theirs are 64 hex digits), so a
//!   cloud addon that takes the id is still a cloud addon and gets none of
//!   this file's privileges.
//! * **Its own door** ([`handle`]): `webkit.messageHandlers.copperBuiltin`
//!   answers only this page, while it shows the built-in. It opens
//!   Settings › Intelligence (the bridge's `openUrl` takes only web
//!   addresses) and reads and sets `chat.json`'s `pageContext`, the
//!   page chip's switch.
//! * **Off** with `COPPER_AGENT_BUILTIN=0` (the slot shows the placeholder
//!   again), and during the addon lanes' self-tests, which test the
//!   placeholder.
//! * **Demo.** `COPPER_AGENT_UI_DEMO=1` (or `?demo=1` on the page in a
//!   debug build) plays a scripted conversation through a fake
//!   `copper.agent` inside the page, for screenshots; the real agent is
//!   the default.

#![cfg_attr(not(windows), allow(dead_code))]

use copper_core::addons::scheme::{SchemeResponse, mime_for, parse_addon_url};
use std::cell::Cell;

#[cfg(windows)]
pub use win::*;

/// The reserved id.
pub const ID: &str = "copper.agent";
/// The built-in's stand-in for a bundle hash.
pub const SHA: &str = "builtin";
/// The page's own message handler (`webkit.messageHandlers.copperBuiltin`).
pub const HANDLER: &str = "copperBuiltin";
/// `0` turns the built-in agent off: the slot shows the placeholder.
pub const ENV: &str = "COPPER_AGENT_BUILTIN";
/// `1` plays the page's scripted conversation instead of the agent.
pub const DEMO_ENV: &str = "COPPER_AGENT_UI_DEMO";

/// The page, as compiled in (`crates/copper-win/agent-ui/`).
const FILES: &[(&str, &[u8])] = &[
    (
        "manifest.json",
        include_bytes!("../../agent-ui/manifest.json"),
    ),
    ("index.html", include_bytes!("../../agent-ui/index.html")),
    ("app.css", include_bytes!("../../agent-ui/app.css")),
    ("app.js", include_bytes!("../../agent-ui/app.js")),
    ("icons.js", include_bytes!("../../agent-ui/icons.js")),
    ("markdown.js", include_bytes!("../../agent-ui/markdown.js")),
    ("demo.js", include_bytes!("../../agent-ui/demo.js")),
];

/// The Settings pages the page may open.
const SETTINGS_PAGES: &[&str] = &["intelligence", "agents", "extensions"];

thread_local! {
    /// A self-test's say over [`enabled`].
    static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether the built-in agent may fill the slot.
pub fn enabled() -> bool {
    if let Some(on) = FORCED.with(Cell::get) {
        return on;
    }
    if std::env::var(ENV).is_ok_and(|v| v.trim() == "0") {
        return false;
    }
    // The addon lanes test the placeholder and the cloud's addons.
    !matches!(
        std::env::var("COPPER_SELFTEST").ok().as_deref(),
        Some("addons" | "addons-cloud")
    )
}

/// A self-test turns the built-in on or off (`None`: as configured).
pub fn force(on: Option<bool>) {
    FORCED.with(|f| f.set(on));
}

/// A compiled-in file.
pub(crate) fn file(path: &str) -> Option<&'static [u8]> {
    FILES.iter().find(|(p, _)| *p == path).map(|(_, b)| *b)
}

/// `copper-addon://copper.agent/<path>`: the compiled-in page (the entry
/// for `/`), HTML with the CSP `<meta>` every addon page gets; anything
/// else is a 404. Same answers as the core's `serve` for a bundle.
pub fn serve(uri: &str) -> SchemeResponse {
    let not_found = || SchemeResponse {
        status: 404,
        mime: "text/plain",
        body: b"Not found".to_vec(),
        headers: Vec::new(),
    };
    let Some((id, path)) = parse_addon_url(uri) else {
        return not_found();
    };
    if id != ID {
        return not_found();
    }
    let path = if path.is_empty() {
        "index.html"
    } else {
        path.as_str()
    };
    if path == "manifest.json" {
        return not_found();
    }
    let Some(bytes) = file(path) else {
        return not_found();
    };
    let mime = mime_for(path);
    let body = if mime.starts_with("text/html") {
        copper_core::addons::csp::inject_csp_bytes(bytes)
    } else {
        bytes.to_vec()
    };
    SchemeResponse {
        status: 200,
        mime,
        body,
        headers: vec![(
            "Content-Security-Policy",
            copper_core::addons::csp::CSP.to_string(),
        )],
    }
}

/// The scripted conversation instead of the agent.
pub fn demo() -> bool {
    std::env::var(DEMO_ENV).is_ok_and(|v| v.trim() == "1")
}

#[cfg(windows)]
mod win {
    use super::{ID, SETTINGS_PAGES, SHA, demo, enabled, file};
    use crate::addons::glue::Active;
    use crate::addons::policy;
    use crate::platform::Shell;
    use copper_core::addons::manifest::Manifest;
    use copper_core::addons::slot::SlotId;
    use copper_core::addons::state::{ActiveAddon, Installed};
    use copper_core::agent::ChatConfig;
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        /// Made once: the built-in as the slot's addon.
        static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
    }

    /// What fills the slot: the cloud's addon, else the built-in agent.
    pub fn slot_active() -> Option<Active> {
        crate::addons::glue::active().or_else(active)
    }

    /// Whether `a` is the built-in agent.
    pub fn is(a: &Active) -> bool {
        a.id == ID && a.sha256 == SHA
    }

    /// The built-in as the slot's addon (when [`enabled`]).
    pub fn active() -> Option<Active> {
        if !enabled() {
            return None;
        }
        ACTIVE.with(|a| {
            let mut a = a.borrow_mut();
            if a.is_none() {
                *a = make();
            }
            a.clone()
        })
    }

    fn make() -> Option<Active> {
        let manifest = match Manifest::from_slice(file("manifest.json")?) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(error = %e, "the built-in agent's manifest doesn't read");
                return None;
            }
        };
        if let Err(e) = manifest.validate() {
            tracing::error!(error = %e, "the built-in agent's manifest is invalid");
            return None;
        }
        let dir = PathBuf::from(format!("{SHA}:{ID}"));
        let installed = Installed {
            id: ID.to_string(),
            version: manifest.version.clone(),
            sha256: SHA.to_string(),
            dir: dir.display().to_string(),
            settings: json!({}),
            installed_at: Default::default(),
            name: manifest.name.clone(),
            slot: SlotId::AgentSidebar.as_str().to_string(),
            manifest: Some(manifest.clone()),
            extra: Default::default(),
        };
        Some(Active {
            id: ID.to_string(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            sha256: SHA.to_string(),
            settings: json!({}),
            dir: dir.clone(),
            dev: false,
            core: ActiveAddon {
                slot: SlotId::AgentSidebar,
                installed,
                manifest,
                dir,
                dev: false,
            },
        })
    }

    /// `chat.json`'s `pageContext` (on unless turned off).
    fn page_context(shell: &Shell) -> bool {
        shell
            .store()
            .load::<ChatConfig>()
            .map(|l| l.value.page_context())
            .unwrap_or(true)
    }

    fn set_page_context(shell: &Shell, on: bool) {
        let mut c = shell
            .store()
            .load::<ChatConfig>()
            .map(|l| l.value)
            .unwrap_or_default();
        if c.page_context() == on && c.page_context.is_some() {
            return;
        }
        c.page_context = Some(on);
        if let Err(e) = shell.store().save(&c) {
            tracing::warn!(error = %e, "chat.json: pageContext not saved");
        }
        // The agent re-reads chat.json at every send; tell it now so the
        // page's chip and `state().pageContext` agree at once.
        crate::agents::brain::changed();
    }

    /// One message on the built-in's own door; `reply` runs JS back in the
    /// page. The caller has made sure the page shows the built-in.
    pub fn handle(shell: &Shell, body: &Value, reply: impl Fn(String)) {
        let msg: Value = match body {
            Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
            other => other.clone(),
        };
        let op = msg.get("op").and_then(Value::as_str).unwrap_or_default();
        match op {
            "hello" => {
                let hello = json!({
                    "demo": demo(),
                    "dev": cfg!(debug_assertions),
                    "pageContext": page_context(shell),
                });
                reply(format!(
                    "window.__copperBuiltin && window.__copperBuiltin.hello({})",
                    copper_core::addons::bridge::script_json(&hello)
                ));
            }
            "settings" => {
                let page = msg.get("page").and_then(Value::as_str).unwrap_or_default();
                // Addons became a section of Extensions (Copper#115).
                if page == "addons" {
                    crate::addons::open_settings(shell);
                } else if SETTINGS_PAGES.contains(&page) {
                    crate::settings::open(shell, page);
                }
            }
            "pageContext" => {
                if let Some(on) = msg.get("on").and_then(Value::as_bool) {
                    set_page_context(shell, on);
                }
                reply(format!(
                    "window.__copperBuiltin && window.__copperBuiltin.pageContext({})",
                    page_context(shell)
                ));
            }
            _ => tracing::debug!(op, "built-in agent: unknown message"),
        }
    }

    /// The built-in's page, by address (the view's checks).
    pub fn in_slot(url: &str) -> bool {
        policy::in_slot(url, ID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use copper_core::addons::manifest::Manifest;

    #[test]
    fn the_manifest_is_valid_and_grants_only_what_the_page_uses() {
        let m = Manifest::from_slice(file("manifest.json").unwrap()).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, ID);
        assert!(m.permissions.hosts.is_empty());
        let mut caps: Vec<&str> = m
            .permissions
            .capabilities()
            .iter()
            .map(|c| c.as_str())
            .collect();
        caps.sort_unstable();
        assert_eq!(caps, ["agent", "open-url", "page"]);
    }

    #[test]
    fn serves_the_entry_with_the_csp_and_nothing_else() {
        let root = serve("copper-addon://copper.agent/");
        assert_eq!(root.status, 200);
        assert!(root.mime.starts_with("text/html"));
        let html = String::from_utf8(root.body).unwrap();
        assert!(html.contains("Content-Security-Policy"));
        assert!(html.contains("app.js"));
        assert_eq!(serve("copper-addon://copper.agent/?demo=1").status, 200);
        assert_eq!(serve("copper-addon://copper.agent/app.js").status, 200);
        assert_eq!(
            serve("copper-addon://copper.agent/manifest.json").status,
            404
        );
        assert_eq!(serve("copper-addon://copper.agent/nope.js").status, 404);
        assert_eq!(serve("copper-addon://other.addon/app.js").status, 404);
        assert_eq!(serve("copper-addon://copper.agent/../x").status, 404);
    }

    #[test]
    fn every_page_file_is_compiled_in() {
        let html = std::str::from_utf8(file("index.html").unwrap()).unwrap();
        for f in ["app.css", "icons.js", "markdown.js", "app.js"] {
            assert!(html.contains(f), "index.html names {f}");
            assert!(file(f).is_some(), "{f} is compiled in");
        }
    }

    #[cfg(windows)]
    #[test]
    fn the_built_in_is_marked_by_its_hash() {
        force(Some(true));
        let a = active().expect("the built-in");
        assert!(is(&a));
        let mut other = a.clone();
        other.sha256 = "ab".repeat(32);
        assert!(!is(&other));
        force(Some(false));
        assert!(active().is_none());
        force(None);
    }
}
