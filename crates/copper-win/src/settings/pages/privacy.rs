//! Privacy (`Settings.swift` 486–515, `PrivacySettings.swift`): blocking
//! ads and trackers, site permissions, and clearing what sites keep.
//!
//! The shield's switch says what is really happening: when its block list
//! couldn't be built, the card says why and nothing claims to be blocked.
//! History and cookies are cleared only after a yes in the window's
//! question bar; the cache clears at once. Cookies and the cache are
//! cleared in every profile's jar, the default one too; saved passwords
//! are never touched.

use super::{on, redraw};
use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use crate::settings::Page;
use crate::settings::form::{Form, Tone};
use copper_engine::{Engine, Profile, WebsiteDataKinds};
use copper_shell_core::{Question, ShellView, shield};

/// Privacy: the shield, permissions, and clearing what sites keep.
pub(super) struct Privacy;

/// The site in front, as the shield keys it (no `www.`).
fn site_here(shell: &Shell) -> Option<String> {
    let url = shell.browser().active().map(|t| t.url.clone())?;
    copper_shell_core::util::site_key(&url)
}

/// History syncs to Copper Cloud now: clearing it here clears it there.
fn history_syncs() -> bool {
    copper_shell_core::cloud::get().is_some_and(|c| c.sync.active(copper_cloud::Domain::History))
}

/// Ask in the window's question bar; `then` runs on yes only.
fn confirm(
    shell: &Shell,
    title: &str,
    detail: String,
    yes: &str,
    then: impl FnOnce(&Shell) + 'static,
) {
    let weak = shell.downgrade();
    shell.view().ask(
        shell,
        Question {
            title: title.into(),
            detail: Some(detail),
            deny: "Cancel".into(),
            allow: yes.into(),
        },
        Box::new(move |allowed| {
            if allowed && let Some(s) = weak.upgrade() {
                then(&s);
            }
        }),
    );
}

/// Every address, here and — while history syncs — on Copper Cloud.
fn clear_history(shell: &Shell) {
    shell.history_mut().clear();
    shell.history_changed();
    copper_shell_core::cloud::history_cleared();
    shell.toast("History cleared");
}

/// Clear `kinds` from every profile's jar (the default one first), then
/// say `done`.
fn clear_jars(shell: &Shell, kinds: WebsiteDataKinds, done: &'static str) {
    let mut names: Vec<Option<String>> = vec![None];
    names.extend(shell.browser().profiles().into_iter().map(Some));
    let engine = shell.engine().clone();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let mut failed = false;
        for name in names {
            let id = copper_shell_core::shell::pages::profile_id(name.as_deref());
            match engine.profile(&id, false) {
                Ok(p) => {
                    if let Err(e) = p.clear_website_data(None, kinds).await {
                        tracing::warn!("clearing {}: {e}", id.as_str());
                        failed = true;
                    }
                }
                Err(e) => {
                    tracing::warn!("profile {}: {e}", id.as_str());
                    failed = true;
                }
            }
        }
        if let Some(s) = weak.upgrade() {
            s.toast(if failed {
                "Some of it couldn't be cleared — try again"
            } else {
                done
            });
        }
    });
}

impl Page for Privacy {
    fn id(&self) -> &'static str {
        "privacy"
    }
    fn title(&self) -> &'static str {
        "Privacy"
    }
    fn blurb(&self) -> &'static str {
        "Blocking ads and trackers, site permissions, and clearing what sites keep."
    }
    fn glyph(&self) -> char {
        glyph::SHIELD
    }

    fn draw(&self, shell: &Shell) -> Form {
        let enabled = shield::enabled::<Win>(shell);
        let trouble = shield::trouble::<Win>();
        let here = site_here(shell);
        let mut f = Form::default();
        let sec = f.section("Blocking");
        sec.switch(
            "Block ads and trackers",
            Some("Third parties whose only job is to watch"),
            "shield",
            enabled,
        )
        .anchor("privacy.shield")
        .keywords(&["ad blocker", "tracking", "shield"]);
        match (&trouble, enabled) {
            (Some(why), true) => {
                sec.button(
                    why,
                    Some(
                        "Nothing is being blocked until this clears — try again, or restart Copper",
                    ),
                    "retry",
                    "Try again",
                    false,
                )
                .tone(Tone::Error);
            }
            (None, true) => {
                if let Some(site) = &here {
                    let paused = shield::paused_on::<Win>(shell, site);
                    sec.switch(
                        &format!("Block on {site}"),
                        Some("Turn off here if the site breaks — the page reloads"),
                        "here",
                        !paused,
                    )
                    .anchor("privacy.here");
                }
            }
            _ => {}
        }

        let sec = f.section("Permissions");
        super::not_yet(
            sec,
            "Camera and microphone",
            "Each site asks every time — Copper for Windows keeps no choices to forget",
        )
        .anchor("privacy.capture");

        let sec = f.section("Clear browsing data");
        sec.button(
            "History",
            Some("Every address you have been to"),
            "history",
            "Clear…",
            false,
        )
        .anchor("privacy.history")
        .button(
            "Cookies and sign-ins",
            Some("Signs you out of every site, in every space"),
            "cookies",
            "Sign out of everything…",
            false,
        )
        .anchor("privacy.cookies")
        .button(
            "Cache",
            Some("Only what was fetched to draw pages"),
            "cache",
            "Clear",
            false,
        )
        .anchor("privacy.cache");
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        match action {
            "shield" => shield::set_enabled::<Win>(shell, on(value)),
            "retry" => {
                shield::retry::<Win>(shell);
                let weak = shell.downgrade();
                copper_win_rt::spawn_local(async move {
                    copper_win_rt::sleep(std::time::Duration::from_millis(600)).await;
                    if let Some(s) = weak.upgrade() {
                        redraw(&s, "privacy");
                    }
                });
            }
            "here" => {
                if let Some(site) = site_here(shell) {
                    shield::set_paused::<Win>(shell, &site, !on(value));
                }
            }
            "history" => {
                let detail = if history_syncs() {
                    "Every address you have been to goes from this PC and from Copper Cloud, for every computer signed in as you. This can't be undone."
                } else {
                    "Every address you have been to goes from this PC. Tabs, bookmarks and passwords stay. This can't be undone."
                };
                confirm(
                    shell,
                    "Clear all history?",
                    detail.into(),
                    "Clear History",
                    clear_history,
                );
            }
            "cookies" => {
                let detail = if shell.browser().profiles().is_empty() {
                    "Cookies and site data are deleted, so every site asks you to sign in again. Saved passwords stay."
                } else {
                    "Cookies and site data are deleted — in every space and profile, so every site asks you to sign in again. Saved passwords stay."
                };
                confirm(
                    shell,
                    "Sign out of every site?",
                    detail.into(),
                    "Sign Out of Everything",
                    |s| clear_jars(s, WebsiteDataKinds::ALL, "Signed out of every site"),
                );
            }
            "cache" => clear_jars(shell, WebsiteDataKinds::CACHES, "Cache cleared"),
            _ => {}
        }
    }
}
