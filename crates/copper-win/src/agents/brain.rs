//! The agent in the window, Windows half (Mac: `Fork/Agent/Agent.swift` as
//! the ⌘E pane and `AddonBridge` reach it): one
//! [`copper_shell_core::chat::Chat`] for the window, reached through the
//! addon bridge's `copper.agent.*`.
//!
//! * It lives on the UI thread and runs there: the router requests go out on
//!   the cloud lane's tokio runtime and the tools run in-process through the
//!   agents lane's `Mcp` — with or without Settings › Agents' loopback
//!   listener.
//! * Every change is pushed to the slot's page as the `agent` event, at most
//!   ten a second and only when the state really changed, and only to an
//!   addon granted the `agent` capability (Mac `AddonHost.agentChanged`).
//! * Copper Cloud's keys arriving (or going), the model chosen in Settings
//!   and `chat.json`'s page-context switch change the state too: the event
//!   follows those files while a page listens.
//! * Nothing secret is kept: the conversation is in memory only; the model
//!   choice goes to `intelligence.json` as the Mac keeps it.
//!
//! With the listener off nobody runs the backstage sweep the server keeps,
//! so a tab the agent held while the person looked elsewhere is let go here.

use crate::platform::{Shell, Win};
use copper_core::addons::Capability;
use copper_core::addons::bridge::{AgentCall, BridgeError, agent_unavailable_state};
use copper_shell_core::chat::{Chat, ShellChat};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::time::Duration;

#[path = "brain_selftest.rs"]
mod selftest;

/// The agent for this window.
pub type WinChat = Chat<ShellChat<Win>>;

/// The `agent` event waits this long after a change, so a burst of changes
/// is one event (≤ 10 a second).
const EMIT_GAP: Duration = Duration::from_millis(100);
/// How often held tabs are let go while the loopback server is off.
const SWEEP_EVERY: Duration = Duration::from_secs(15);
/// How often the files the state reads are looked at.
const FILES_EVERY: Duration = Duration::from_secs(1);

thread_local! {
    static CHAT: RefCell<Option<WinChat>> = const { RefCell::new(None) };
    /// An `agent` event is on its way.
    static DUE: Cell<bool> = const { Cell::new(false) };
    /// The last `agent` event's JSON, so an unchanged state is not sent again.
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Make the window's agent (once, from the agents lane, after its server).
pub fn register(shell: &Shell) {
    let chat = Chat::new(ShellChat::new(shell.downgrade()));
    let follower = chat.clone();
    copper_shell_core::agents::on_drive(move |event| follower.drive(event));
    chat.on_change(changed);
    CHAT.with(|c| *c.borrow_mut() = Some(chat));
    follow_files(shell);
    sweep_while_unlistened();
    selftest::maybe_start(shell);
}

/// The window's agent, once registered.
pub fn chat() -> Option<WinChat> {
    CHAT.with(|c| c.borrow().clone())
}

/// `copper.agent.state()`.
pub fn state() -> Value {
    chat().map_or_else(agent_unavailable_state, |c| c.state())
}

/// `copper.agent.setModel/send/stop/clear` (Mac `AddonBridge.call`).
pub fn call(call: AgentCall) -> Result<Value, BridgeError> {
    let chat =
        chat().ok_or_else(|| BridgeError::unavailable("The window's agent is not running"))?;
    match call {
        AgentCall::SetModel(id) => chat.set_model(&id).map_err(BridgeError::bad_request)?,
        AgentCall::Send(text) => {
            chat.send(&text)
                .map_err(|e| BridgeError::bad_request(e.to_string()))?;
            tracing::info!(target: "copper::agent", "question asked");
        }
        AgentCall::Stop => chat.stop(),
        AgentCall::Clear => chat.clear(),
    }
    Ok(Value::Bool(true))
}

/// Something changed: the `agent` event, a beat later.
pub fn changed() {
    if DUE.with(|d| d.replace(true)) {
        return;
    }
    copper_win_rt::spawn_local(async {
        copper_win_rt::sleep(EMIT_GAP).await;
        DUE.with(|d| d.set(false));
        emit();
    });
}

/// Send the state to the slot's page if it may hear it and it changed.
fn emit() {
    let Some(view) = crate::addons::view::live() else {
        return;
    };
    if !crate::addons::glue::granted(&view.addon(), Capability::Agent) {
        return;
    }
    let state = state();
    let text = state.to_string();
    if LAST.with(|l| *l.borrow() == text) {
        return;
    }
    LAST.with(|l| *l.borrow_mut() = text);
    view.emit("agent", state);
}

/// What the state reads from disk: the keys (this PC's and Copper Cloud's)
/// and `chat.json`, which Settings and the sidebar change. While a page may
/// hear the `agent` event, a change to any of them is one (Mac: the agent's
/// changes merged with Intelligence's).
fn follow_files(shell: &Shell) {
    use copper_core::DataFile;
    let files = [
        DataFile::Intelligence,
        DataFile::CloudIntelligence,
        DataFile::Chat,
    ]
    .map(|f| shell.store().path(f));
    copper_win_rt::spawn_local(async move {
        let mut seen = None;
        loop {
            copper_win_rt::sleep(FILES_EVERY).await;
            let listening = crate::addons::view::live()
                .is_some_and(|v| crate::addons::glue::granted(&v.addon(), Capability::Agent));
            if !listening {
                seen = None;
                continue;
            }
            let now: Vec<Option<(std::time::SystemTime, u64)>> = files
                .iter()
                .map(|p| {
                    let m = std::fs::metadata(p).ok()?;
                    Some((m.modified().ok()?, m.len()))
                })
                .collect();
            if seen.as_ref().is_some_and(|s| *s != now) {
                changed();
            }
            seen = Some(now);
        }
    });
}

/// Let go of backstage holds nobody asked for in a while, while the
/// loopback server (whose loop otherwise does it) is off.
fn sweep_while_unlistened() {
    copper_win_rt::spawn_local(async {
        loop {
            copper_win_rt::sleep(SWEEP_EVERY).await;
            if let Some(a) = copper_shell_core::agents::get::<Win>()
                && !a.running()
            {
                a.mcp().sweep();
            }
        }
    });
}
