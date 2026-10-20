//! The agent in the window, asked for real: `COPPER_SELFTEST=agent` with
//! `COPPER_AGENT_ASK="<question>"` asks the window's agent one question
//! against whatever router and Jev keys the world has, waits for the answer
//! (or Stop), writes what came of it to `COPPER_SELFTEST_OUT`, and quits.
//!
//! | Variable | Meaning |
//! |---|---|
//! | `COPPER_AGENT_ASK` | the question (none: only the state is written) |
//! | `COPPER_AGENT_URL` | open this page in a new tab first, and wait for it |
//! | `COPPER_AGENT_STOP_AFTER` | press Stop this many seconds after asking |
//! | `COPPER_AGENT_WAIT` | give up waiting after this many seconds (240) |
//! | `COPPER_AGENT_SETTINGS` | end on Settings › Agents this many seconds, then `agent-settings.png` |
//!
//! When the Agent sidebar holds an addon granted `agent` (a developer folder,
//! `COPPER_ADDONS_DEV_DIR`), everything goes through that page's
//! `window.copper.agent.*` and its `agent` events — the bridge end to end;
//! otherwise straight to [`super::chat`].
//!
//! Out: `agent-before.json` and `agent-state.json` (the state before and
//! after), `agent-progress.txt` (the status line once a second),
//! `agent-selftest.txt` (the summary: the tab the run ended on, the item
//! roles, the bridge's event count) and `agent-window.png`
//! (`agent-stopping.png` at Stop). Keys never appear in any of them.

use crate::platform::Shell;
use copper_core::addons::Capability;
use copper_engine::{PageView, World};
use serde_json::{Map, Value, json};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The `COPPER_SELFTEST` value that starts this lane.
const LANE: &str = "agent";

/// Start when `COPPER_SELFTEST=agent`.
pub fn maybe_start(shell: &Shell) {
    if std::env::var("COPPER_SELFTEST").ok().as_deref() != Some(LANE) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(2500)).await;
        let Some(shell) = weak.upgrade() else { return };
        let out = std::env::var_os("COPPER_SELFTEST_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("copper-agent-selftest"));
        let _ = std::fs::create_dir_all(&out);
        let summary = run(&shell, &out).await;
        let _ = std::fs::write(out.join("agent-selftest.txt"), summary);
        crate::app::quit(&shell);
    });
}

/// How the run reaches the agent.
enum Way {
    /// The slot's addon page, through `window.copper.agent`.
    Bridge(copper_engine_webview2::WebView2Page),
    /// The window's agent itself.
    Direct,
}

impl Way {
    async fn state(&self) -> Value {
        match self {
            Way::Bridge(page) => js(
                page,
                "return await window.copper.agent.state();",
                Map::new(),
            )
            .await
            .unwrap_or(Value::Null),
            Way::Direct => super::state(),
        }
    }

    async fn send(&self, question: &str) -> Result<(), String> {
        match self {
            Way::Bridge(page) => {
                let mut args = Map::new();
                args.insert("q".into(), json!(question));
                js(page, "return await window.copper.agent.send(q);", args)
                    .await
                    .map(|_| ())
            }
            Way::Direct => super::chat()
                .ok_or("no agent")?
                .send(question)
                .map_err(|e| e.to_string()),
        }
    }

    async fn stop(&self) {
        match self {
            Way::Bridge(page) => {
                let _ = js(page, "return await window.copper.agent.stop();", Map::new()).await;
            }
            Way::Direct => {
                if let Some(c) = super::chat() {
                    c.stop();
                }
            }
        }
    }
}

/// Run `body` as an async function in the addon page; a rejection is its message.
async fn js(
    page: &copper_engine_webview2::WebView2Page,
    body: &str,
    args: Map<String, Value>,
) -> Result<Value, String> {
    let wrapped = format!(
        "try {{ const v = await (async () => {{ {body} }})(); return {{ok: true, v}}; }} \
         catch (e) {{ return {{ok: false, e: String(e && (e.message || e.code) || e)}}; }}"
    );
    let answer = page
        .call_async(&wrapped, &args, &World::Page)
        .await
        .map_err(|e| e.to_string())?;
    if answer["ok"] == true {
        Ok(answer["v"].clone())
    } else {
        Err(answer["e"].as_str().unwrap_or("rejected").to_string())
    }
}

fn env_secs(key: &str) -> Option<Duration> {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(Duration::from_secs_f64)
}

fn write_json(path: PathBuf, value: &Value) {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    let _ = std::fs::write(path, text);
}

fn shot(shell: &Shell, path: PathBuf) -> String {
    match copper_shell_core::ShellView::screenshot(shell.view()) {
        Some(png) => match std::fs::write(&path, png) {
            Ok(()) => format!("SHOT {}", path.display()),
            Err(e) => format!("SHOT failed: {e}"),
        },
        None => "SHOT failed: no picture".into(),
    }
}

/// Wait until the active tab has loaded (or `limit` passes).
async fn settle(shell: &Shell, limit: Duration) {
    let start = Instant::now();
    loop {
        copper_win_rt::sleep(Duration::from_millis(200)).await;
        let loaded = shell
            .active_page()
            .is_some_and(|p| !p.is_loading() && p.url().is_some());
        if loaded || start.elapsed() > limit {
            break;
        }
    }
    copper_win_rt::sleep(Duration::from_millis(600)).await;
}

/// The bridge's way in, when the slot's addon may use the agent.
async fn bridge(shell: &Shell) -> Option<copper_engine_webview2::WebView2Page> {
    // What the slot shows: the cloud's addon, else Copper's own agent page.
    let addon = crate::addons::builtin::slot_active()?;
    if !crate::addons::glue::granted(&addon, Capability::Agent) {
        return None;
    }
    // The page is made when the slot first shows.
    crate::addons::show(shell);
    let start = Instant::now();
    let view = loop {
        if let Some(v) = crate::addons::view::live() {
            break v;
        }
        if start.elapsed() > Duration::from_secs(10) {
            return None;
        }
        copper_win_rt::sleep(Duration::from_millis(300)).await;
    };
    let page = view.page().clone();
    // The shim is there once `window.copper` is.
    let start = Instant::now();
    while js(
        &page,
        "return typeof window.copper === 'object' && !!window.copper.agent;",
        Map::new(),
    )
    .await
    .ok()
        != Some(Value::Bool(true))
    {
        if start.elapsed() > Duration::from_secs(10) {
            return None;
        }
        copper_win_rt::sleep(Duration::from_millis(300)).await;
    }
    let _ = js(
        &page,
        "window.__agentEvents = []; window.copper.on('agent', (s) => window.__agentEvents.push(s)); return true;",
        Map::new(),
    )
    .await;
    Some(page)
}

async fn run(shell: &Shell, out: &std::path::Path) -> String {
    let mut log = String::new();
    if let Ok(url) = std::env::var("COPPER_AGENT_URL")
        && !url.trim().is_empty()
    {
        crate::addons::view::open_tab(shell, url.trim());
        settle(shell, Duration::from_secs(60)).await;
        let _ = writeln!(log, "opened {}", url.trim());
    }
    let page = bridge(shell).await;
    let way = match page {
        Some(p) => {
            let _ = writeln!(log, "way: bridge (the slot's addon, window.copper.agent)");
            Way::Bridge(p)
        }
        None => {
            let _ = writeln!(log, "way: direct (no addon granted agent in the slot)");
            Way::Direct
        }
    };
    let before = way.state().await;
    write_json(out.join("agent-before.json"), &before);
    let _ = writeln!(log, "available: {}", before["available"]);

    let question = std::env::var("COPPER_AGENT_ASK").unwrap_or_default();
    if !question.trim().is_empty() {
        let asked = Instant::now();
        match way.send(&question).await {
            Ok(()) => {
                let _ = writeln!(log, "asked");
            }
            Err(e) => {
                let _ = writeln!(log, "send refused: {e}");
            }
        }
        let stop_after = env_secs("COPPER_AGENT_STOP_AFTER");
        let wait = env_secs("COPPER_AGENT_WAIT").unwrap_or(Duration::from_secs(240));
        let mut progress = String::new();
        let mut stopped = false;
        loop {
            copper_win_rt::sleep(Duration::from_millis(1000)).await;
            let state = way.state().await;
            let busy = state["busy"] == true;
            let _ = writeln!(
                progress,
                "{:>5.1}s busy={busy} status={} items={}",
                asked.elapsed().as_secs_f64(),
                state["status"],
                state["items"].as_array().map_or(0, Vec::len)
            );
            if !stopped && stop_after.is_some_and(|t| asked.elapsed() >= t) && busy {
                write_json(out.join("agent-stopping.json"), &state);
                let _ = writeln!(log, "{}", shot(shell, out.join("agent-stopping.png")));
                way.stop().await;
                stopped = true;
                let _ = writeln!(log, "stop pressed at {:.1}s", asked.elapsed().as_secs_f64());
                continue;
            }
            if !busy || asked.elapsed() > wait {
                if busy {
                    let _ = writeln!(log, "gave up waiting after {}s", wait.as_secs());
                }
                break;
            }
        }
        let _ = std::fs::write(out.join("agent-progress.txt"), progress);
        let _ = writeln!(log, "took {:.1}s", asked.elapsed().as_secs_f64());
        // The last drive events and the page's paint.
        copper_win_rt::sleep(Duration::from_millis(2000)).await;
    }

    let state = way.state().await;
    write_json(out.join("agent-state.json"), &state);
    if let Way::Bridge(page) = &way {
        let events = js(page, "return window.__agentEvents.length;", Map::new())
            .await
            .unwrap_or(Value::Null);
        let last = js(
            page,
            "const e = window.__agentEvents; return e.length ? e[e.length - 1] : null;",
            Map::new(),
        )
        .await
        .unwrap_or(Value::Null);
        let _ = writeln!(log, "agent events: {events}");
        let _ = writeln!(log, "last event equals state(): {}", last == state);
        let shows = js(
            page,
            "return ((document.body && document.body.innerText) || '').slice(0, 600);",
            Map::new(),
        )
        .await
        .unwrap_or(Value::Null);
        let shows = shows.as_str().unwrap_or("").replace('\n', " / ");
        let _ = writeln!(log, "the addon page shows: {shows}");
    }
    let roles: Vec<String> = state["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|i| {
                    format!(
                        "{}: {}",
                        i["role"].as_str().unwrap_or("?"),
                        i["text"].as_str().unwrap_or("")
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let _ = writeln!(
        log,
        "available: {} busy: {} model: {}",
        state["available"], state["busy"], state["model"]
    );
    let _ = writeln!(log, "items:\n  {}", roles.join("\n  "));
    if let Some(id) = shell.active_id()
        && let Some(tab) = shell.tab_view(&id)
    {
        let _ = writeln!(log, "tab: {} — {}", tab.url, tab.label);
    }
    let _ = writeln!(log, "{}", shot(shell, out.join("agent-window.png")));
    if let Some(hold) = env_secs("COPPER_AGENT_SETTINGS").filter(|t| !t.is_zero()) {
        crate::settings::open(shell, "agents");
        copper_win_rt::sleep(hold.max(Duration::from_millis(1500))).await;
        let _ = writeln!(log, "{}", shot(shell, out.join("agent-settings.png")));
    }
    tracing::info!(target: "copper::selftest", "agent selftest done");
    log
}
