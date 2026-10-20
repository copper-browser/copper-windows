//! `copper.oauth.authorize` (CONTRACT §5.2; the Mac's
//! `Fork/Addons/AddonBridge.swift`, `oauthAuthorize`; Linux `addons/oauth.rs`).
//!
//! The sign-in runs in a new foreground **tab** of the person's normal
//! browsing profile (the current space's), so the single sign-on session
//! they already have applies — a tab rather than a second window, as the
//! Windows shell has one window. Every navigation, redirect and address
//! change of that tab is watched; the first whose address starts with the
//! addon's `redirectPrefix` is stopped (nothing listens on the loopback
//! port), the tab closes, and the bridge call resolves with that address.
//! Closing the tab is `cancelled`; running past the deadline is `timeout`.
//! The redirect is never logged (it carries the code).

use super::policy;
use crate::platform::{Shell, WinLoop};
use copper_core::browser::TabId;
use copper_engine::{LoadState, PageEvent, PageView};
use copper_shell_core::OpenTab;
use std::time::{Duration, Instant};

/// How a sign-in ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The redirect, with its code.
    Redirect(String),
    /// The person closed the tab.
    Cancelled,
    Timeout,
    /// The tab could not be made.
    Failed(String),
}

/// For the log: never the redirect itself (it carries the code).
fn outcome_name(o: &Outcome) -> &'static str {
    match o {
        Outcome::Redirect(_) => "redirect",
        Outcome::Cancelled => "cancelled",
        Outcome::Timeout => "timeout",
        Outcome::Failed(_) => "failed",
    }
}

/// How often the watch looks whether the tab is still there.
const TICK: Duration = Duration::from_millis(400);

thread_local! {
    /// Sign-in tabs open now.
    static OPEN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many sign-in tabs are open (the cloud self-test pictures one).
pub fn open_tabs() -> usize {
    OPEN.with(std::cell::Cell::get)
}

/// Open `url` in a sign-in tab for addon `name` and call `done` once with
/// how it ended. UI thread.
pub fn authorize(
    shell: &Shell,
    name: &str,
    url: &str,
    redirect_prefix: &str,
    timeout: Duration,
    done: impl FnOnce(Outcome) + 'static,
) {
    if !policy::is_web(url) {
        done(Outcome::Failed("not a web address".into()));
        return;
    }
    let tab = shell.open(
        url,
        OpenTab {
            foreground: true,
            after: shell.active_id(),
            ..Default::default()
        },
    );
    tracing::info!(addon = %name, "addon sign-in tab opened");
    let weak = shell.downgrade();
    let prefix = redirect_prefix.to_string();
    let addon = name.to_string();
    OPEN.with(|o| o.set(o.get() + 1));
    copper_win_rt::spawn_local(async move {
        let outcome = watch(&weak, &tab, &prefix, Instant::now() + timeout).await;
        OPEN.with(|o| o.set(o.get().saturating_sub(1)));
        tracing::info!(addon = %addon, outcome = outcome_name(&outcome), "addon sign-in ended");
        if let Some(s) = weak.upgrade()
            && !matches!(outcome, Outcome::Cancelled)
            && s.browser().tab(&tab).is_some()
        {
            s.close_force(&tab);
        }
        done(outcome);
    });
}

/// The address `ev` says the tab is going to, if any.
fn address(ev: &PageEvent) -> Option<&str> {
    match ev {
        PageEvent::Load(
            LoadState::Started { url: Some(u) }
            | LoadState::Redirected { url: Some(u) }
            | LoadState::Committed { url: Some(u) },
        )
        | PageEvent::Url(Some(u)) => Some(u),
        PageEvent::Load(LoadState::Failed { url, .. }) => Some(url),
        _ => None,
    }
}

async fn watch(
    weak: &crate::platform::WeakShell,
    tab: &TabId,
    prefix: &str,
    deadline: Instant,
) -> Outcome {
    loop {
        let Some(shell) = weak.upgrade() else {
            return Outcome::Cancelled;
        };
        if shell.browser().tab(tab).is_none() {
            return Outcome::Cancelled;
        }
        if Instant::now() >= deadline {
            return Outcome::Timeout;
        }
        // The tab's page (made as it opens in front; a crash makes another).
        let Some(page) = shell.slot(tab).map(|s| s.page.clone()) else {
            drop(shell);
            copper_win_rt::sleep(Duration::from_millis(50)).await;
            continue;
        };
        drop(shell);
        if let Some(u) = page.url()
            && policy::redirect_matches(&u, prefix)
        {
            page.stop();
            return Outcome::Redirect(u);
        }
        let mut events = page.events();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Outcome::Timeout;
            }
            match copper_shell_core::timeout::<WinLoop, _>(left.min(TICK), events.next()).await {
                Some(Some(ev)) => {
                    if let Some(u) = address(&ev)
                        && policy::redirect_matches(u, prefix)
                    {
                        let u = u.to_string();
                        page.stop();
                        return Outcome::Redirect(u);
                    }
                }
                // The page went (a crash, the tab closing): look again.
                Some(None) => break,
                None => {
                    let Some(shell) = weak.upgrade() else {
                        return Outcome::Cancelled;
                    };
                    let same = shell
                        .slot(tab)
                        .is_some_and(|s| s.page.page_id() == page.page_id());
                    if !same {
                        break;
                    }
                }
            }
        }
    }
}
