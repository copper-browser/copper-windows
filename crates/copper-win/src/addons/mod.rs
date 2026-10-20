//! Addons: Copper's slots, filled from Copper Cloud (CONTRACT; Mac
//! `Fork/Addons/`; Linux `copper-app/src/addons/`).
//!
//! A slot is a region of the window whose content an addon may replace. v1
//! has one, the **Agent sidebar** (`agent-sidebar`): the pane right of the
//! page, toggled by Ctrl+E ([`crate::chrome::pane`]). With no addon it
//! shows Copper's own agent — the built-in addon [`builtin`], compiled
//! in — or, with that turned off, Copper's placeholder; with one it shows
//! the addon's page, served from its installed bundle as
//! `copper-addon://<id>/` in a WebView2 page of its own, speaking to Copper
//! through `window.copper` (bridge v1).
//!
//! | Module | What | Mac |
//! |---|---|---|
//! | `chrome::pane` | the slot on screen: fade, width grip, placeholder | `AddonUI.swift` (`AgentSlot`) |
//! | [`view`] | the addon's page: profile, scripts, navigation rules, events | `AddonHost.swift` |
//! | [`scheme`] | `copper-addon://` from the installed bundle | `AddonHost.swift` (`AddonFiles`) |
//! | [`builtin`] | Copper's own agent, the slot's default (`agent-ui/`) | `Fork/Agent/AgentPane.swift` |
//! | [`policy`] | the pure rules (widths, addresses, payloads) | `AddonHost.swift` |
//! | [`host`] | what the bridge asks of Copper (identity, tabs, close, sign-in) | `AddonBridge.swift` |
//! | [`oauth`] | the sign-in tab for `copper.oauth.authorize` | `AddonBridge.swift` |
//! | [`updates`] | "Update available" on Settings › Updates, the toast, Install | `AddonUpdates.swift` |
//! | [`glue`] | the one door to `copper_core::addons` and `copper_cloud::addons` | — |
//! | [`selftest`] | `COPPER_SELFTEST=addons`: the bridge self-test | `AddonBench.swift` |
//! | [`selftest_cloud`] | `COPPER_SELFTEST=addons-cloud`: against a real cloud | `addons-cloud-e2e.sh` (Linux) |
//! | [`selftest_agent`] | `COPPER_SELFTEST=agent-ui`: the built-in agent's pictures | `AgentPane` bench seeds |
//!
//! Settings › Addons is `settings/pages/addons.rs`. Everything
//! platform-neutral (manifests, bundles, the on-disk store, the cloud sync,
//! the bridge's own methods) lives in the shared core; this lane is the
//! Windows side only.

/// `agents::hands` is plain state (the agents lane is Windows-only); its
/// tests run on the Linux host too.
#[cfg(all(test, not(windows)))]
#[allow(dead_code)]
#[path = "../agents/hands.rs"]
mod agents_hands;
pub mod builtin;
pub mod policy;

#[cfg(windows)]
pub mod glue;
#[cfg(windows)]
pub mod host;
#[cfg(windows)]
pub mod oauth;
#[cfg(windows)]
pub mod scheme;
#[cfg(windows)]
pub mod selftest;
#[cfg(windows)]
pub mod selftest_agent;
#[cfg(windows)]
pub mod selftest_cloud;
#[cfg(windows)]
pub mod updates;
#[cfg(windows)]
pub mod view;

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use crate::chrome::{glyph, pane};
    use crate::commands::Command;
    use crate::platform::Shell;
    use copper_win_ui::NodeId;

    /// Called once from `features::register`, right after the cloud lane
    /// (and before any page: the scheme must be known first).
    pub fn register(shell: &Shell) {
        super::scheme::install(shell);
        pane::start(shell);
        shell.view().commands().add(Command::new(
            "agent-sidebar",
            pane::NAME,
            glyph::AGENT_SIDEBAR,
            toggle,
        ));
        shell.view().commands().add(Command::new(
            "settings-addons",
            "Addons Settings",
            glyph::ADDONS,
            open_settings,
        ));
        super::updates::register(shell);
        super::glue::start(shell, changed);
        super::view::watch_shell(shell);
        super::selftest::maybe_start(shell);
        super::selftest_cloud::maybe_start(shell);
        super::selftest_agent::maybe_start(shell);
    }

    /// Ctrl+E: show the slot, or put it away.
    pub fn toggle(shell: &Shell) {
        pane::toggle(shell);
    }

    /// Show the slot (and give it the keyboard).
    pub fn show(shell: &Shell) {
        pane::show(shell);
    }

    /// Put the slot away (Ctrl+E again, `copper.ui.close()`).
    pub fn hide(shell: &Shell) {
        pane::hide(shell);
    }

    /// A click on a chrome node: `true` if it was the slot's.
    pub fn click(shell: &Shell, id: NodeId) -> bool {
        pane::click(shell, id) || crate::agents::marks::click(shell, id)
    }

    /// The addons, their state or the cloud's answer changed: bring the
    /// slot, Settings › Addons and Settings › Updates in line. UI thread.
    pub fn changed(shell: &Shell) {
        pane::sync(shell);
        crate::settings::changed(shell, "addons");
        super::updates::changed(shell);
    }

    /// Settings, on the Addons page.
    pub fn open_settings(shell: &Shell) {
        crate::settings::open(shell, "addons");
    }

    pub use super::updates::{act as updates_act, section as updates_section};
}
