//! What the bridge asks of Copper itself (the Mac's
//! `Fork/Addons/AddonBridge.swift`, its `BridgeHost` side; Linux
//! `addons/host.rs`).
//!
//! The core's `dispatch` runs on the cloud's tokio runtime and does the
//! platform-neutral methods itself (`info`, `settings`, `storage.*`,
//! `fetch`, capability checks). For the rest it calls back into this host,
//! which is `Send + Sync`: every call that touches the window hops to the
//! UI thread (`copper_win_rt::post`) and awaits the answer there, so
//! nothing ever blocks the UI thread.
//!
//! | Bridge | Here |
//! |---|---|
//! | `identity()` | the Copper Cloud account (`null` when not signed in) |
//! | `openUrl(url)` | a new foreground tab |
//! | `page.current()` | the active tab's `{url, title}` (`null` for private tabs) |
//! | `ui.close()` | hides the slot |
//! | `oauth.authorize(…)` | [`super::oauth`] |
//! | `agent.*` | the window's agent ([`crate::agents::brain`]) |

use super::glue::Active;
use super::oauth::Outcome;
use crate::platform::Shell;
use copper_core::addons::bridge::{AgentCall, BridgeError, agent_unavailable_state};
use serde_json::Value;
use std::time::Duration;

/// The host for one addon's calls.
#[derive(Debug, Clone)]
pub struct Host {
    pub addon_id: String,
    pub addon_name: String,
}

impl Host {
    pub fn new(active: &Active) -> Host {
        Host {
            addon_id: active.id.clone(),
            addon_name: active.name.clone(),
        }
    }

    /// `copper.openUrl(url)`: `false` if the window is gone.
    pub async fn open_url(&self, url: String) -> bool {
        tracing::debug!(addon = %self.addon_id, "addon opens a tab");
        on_main(move |s| super::view::open_tab(s, &url))
            .await
            .is_some()
    }

    /// `copper.page.current()`.
    pub async fn page_current(&self) -> Value {
        on_main(super::view::current_page)
            .await
            .unwrap_or(Value::Null)
    }

    /// `copper.ui.close()`.
    pub async fn close_slot(&self) {
        on_main(super::hide).await;
    }

    /// `copper.agent.state()`: the window's agent, read on the UI thread.
    pub async fn agent_state(&self) -> Value {
        on_main(|_| crate::agents::brain::state())
            .await
            .unwrap_or_else(agent_unavailable_state)
    }

    /// `copper.agent.setModel/send/stop/clear`, on the UI thread.
    pub async fn agent_call(&self, call: AgentCall) -> Result<Value, BridgeError> {
        let what = match &call {
            AgentCall::SetModel(_) => "setModel",
            AgentCall::Send(_) => "send",
            AgentCall::Stop => "stop",
            AgentCall::Clear => "clear",
        };
        tracing::debug!(addon = %self.addon_id, what, "addon asks the agent");
        on_main(move |_| crate::agents::brain::call(call))
            .await
            .unwrap_or_else(|| Err(BridgeError::internal("the window has closed")))
    }

    /// `copper.oauth.authorize(…)`. The core has already checked the
    /// capability, the host of `url` and the form of `redirect_prefix`.
    pub async fn oauth_authorize(
        &self,
        url: String,
        redirect_prefix: String,
        timeout: Duration,
    ) -> Outcome {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let name = self.addon_name.clone();
        copper_win_rt::post(move || {
            let Some(s) = crate::platform::current() else {
                let _ = tx.send(Outcome::Failed("the window has closed".into()));
                return;
            };
            super::oauth::authorize(&s, &name, &url, &redirect_prefix, timeout, move |o| {
                let _ = tx.send(o);
            });
        });
        rx.await.unwrap_or(Outcome::Cancelled)
    }
}

/// Run `f` with the shell on the UI thread; `None` if the window is gone.
/// Callable from any thread.
pub async fn on_main<T: Send + 'static>(f: impl FnOnce(&Shell) -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    copper_win_rt::post(move || {
        if let Some(s) = crate::platform::current() {
            let _ = tx.send(f(&s));
        }
    });
    rx.await.ok()
}
