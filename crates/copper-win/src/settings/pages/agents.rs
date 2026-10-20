//! Agents: the MCP server on this PC (spec "Settings › Agents"; the Mac's
//! `AgentsSettings`, Linux `agents/settings.rs`): the listener and what it
//! announces, its port, Jev mode, the two "Copy config" rows (HTTP and
//! stdio), and the key. The server is copper-shell-core's `agents`; this
//! page reads and sets it, and redraws when it changes (`agents.rs`).
//!
//! The port is shown, not typed: the settings form has no text field yet.
//! "Use another port" moves the listener to the next free one.
//!
//! "The agent in the window" is the Ctrl+E agent's own `chat.json` (the
//! Mac's `Agent.Config`): the page in front of every question and the
//! tool-call rounds per question, which Copper Cloud may set instead.

use super::on;
use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use crate::settings::Page;
use crate::settings::form::Form;
use copper_core::agent::ChatConfig;
use copper_shell_core::{Question, ShellView};
use std::cell::Cell;
use std::time::Duration;

/// Agents: the MCP server on this PC.
pub(super) struct Agents;

type Server = copper_shell_core::agents::Agents<Win>;

thread_local! {
    /// The copy button that just copied (it says "Copied" for a moment).
    static COPIED: Cell<Option<&'static str>> = const { Cell::new(None) };
}

const COPY_HTTP: &str = "copy-http";
const COPY_STDIO: &str = "copy-stdio";

fn copy_label(action: &'static str) -> &'static str {
    if COPIED.with(Cell::get) == Some(action) {
        "Copied"
    } else {
        "Copy config"
    }
}

/// The status line (Linux `sync`).
fn status_line(a: &Server, pinned: Option<u16>) -> String {
    match (a.trouble(), a.running(), a.enabled()) {
        (Some(t), _, _) if pinned.is_none() => format!("{t} — use another port below."),
        (Some(t), _, _) => t,
        (None, true, _) => format!("Listening at {}", a.endpoint()),
        (None, false, true) => "Starting…".into(),
        (None, false, false) => "Off — no agent can reach this browser.".into(),
    }
}

/// `chat.json`: the agent in the window's own settings (`Agent.Config`).
fn chat_config(shell: &Shell) -> ChatConfig {
    shell
        .store()
        .load::<ChatConfig>()
        .map(|l| l.value)
        .unwrap_or_default()
}

/// Change `chat.json` the way the Mac writes it (`version: 2`); the agent
/// reads it again for every question, and the sidebar hears of it.
fn edit_chat(shell: &Shell, f: impl FnOnce(&mut ChatConfig)) {
    let mut c = chat_config(shell);
    f(&mut c);
    c.normalize();
    if let Err(e) = shell.store().save(&c) {
        tracing::warn!("chat.json: {e}");
    }
    crate::agents::brain::changed();
}

/// The agent in the window (Ctrl+E): the page in front of each question and
/// its tool-call budget (Mac `AgentsPaneSection`).
fn pane_section(f: &mut Form, shell: &Shell) {
    let chat = chat_config(shell);
    let sec = f.section("The agent in the window — Ctrl+E");
    sec.switch(
        "Page in front of every question",
        Some(
            "The current tab's address, title and the first 3000 characters of its text. Off, \
             it still has the tools to look.",
        ),
        "page-context",
        chat.page_context(),
    )
    .anchor("agents.context");
    let cloud = copper_mcp::intelligence::read_files(shell.store())
        .cloud
        .and_then(|c| c.agent_max_turns());
    let mine = chat.effective_max_turns();
    let detail = match cloud {
        Some(n) => format!(
            "Set by your Copper Cloud to {n}. Yours ({mine}) is back when you sign out of it."
        ),
        None => format!(
            "How many times the agent may use its tools on one question before it stops and \
             asks you to continue. Default {}.",
            ChatConfig::DEFAULT_MAX_TURNS
        ),
    };
    sec.field(
        "Tool-call rounds per question",
        Some(&detail),
        "turns",
        &cloud.unwrap_or(mine).to_string(),
        &ChatConfig::DEFAULT_MAX_TURNS.to_string(),
    )
    .on_submit("turns-done", Some(64.0))
    .mono()
    .disabled(cloud.is_some())
    .anchor("agents.turns");
}

/// The first port after `from` nothing on this PC listens on.
fn next_free_port(from: u16) -> Option<u16> {
    (from.saturating_add(1)..=from.saturating_add(50))
        .find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

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

    fn draw(&self, shell: &Shell) -> Form {
        let mut f = Form::default();
        let Some(a) = copper_shell_core::agents::get::<Win>() else {
            f.section("").note("The agent server did not start", None);
            return f;
        };
        let pinned = copper_mcp::config::port_override();

        let sec = f.section("MCP server");
        sec.switch(
            "Let agents drive this window",
            Some(
                "An MCP server on this computer only (127.0.0.1). Agents in your terminal or \
                 editor see your open tabs and act in them, in the browser you're already \
                 signed in to.",
            ),
            "enabled",
            a.enabled(),
        );
        sec.note("Status", Some(&status_line(&a, pinned)));
        sec.switch(
            "Say what the agent does",
            Some("Each tool call, in the bar over the page it works in"),
            "announces",
            a.announces(),
        );
        match pinned {
            Some(p) => {
                sec.value(
                    "Port",
                    Some(&format!("This run listens on {p}, set by SEARCH_MCP_PORT.")),
                    &p.to_string(),
                );
            }
            None => {
                let port = a.configured_port();
                sec.value(
                    "Port",
                    Some(&format!("Change it if another app already uses {port}.")),
                    &port.to_string(),
                );
                sec.button(
                    "Use another port",
                    Some("Moves the server to the next free port; set up your clients again."),
                    "port-next",
                    "Next Free Port",
                    false,
                );
            }
        }

        let sec = f.section("Jev mode — ultrafast");
        sec.switch(
            "Let the agent hand Copper a goal",
            Some(
                "Adds jev_run, jev_step, jev_observe and jev_extract. Jev picks the next click \
                 or key about every 200 ms and Copper does it in this window, until the goal \
                 is done.",
            ),
            "jev",
            a.jev(),
        );
        if a.jev() {
            let source = copper_mcp::intelligence::read_files(shell.store());
            let effective = copper_mcp::intelligence::Effective::new(&source, false);
            let line = match effective.sources.get("jevKey").map(String::as_str) {
                _ if !effective.jev_ready() => {
                    "Jev mode needs a Jev key (ts-…) in intelligence.json, the one Settings › \
                     Intelligence uses."
                }
                Some("cloud") => "From Copper Cloud.",
                _ => "Set in intelligence.json, the same key Settings › Intelligence uses.",
            };
            sec.note("Jev key", Some(line));
        }

        pane_section(&mut f, shell);

        let sec = f.section("Connect a client");
        sec.button(
            "Claude Code, phi, Cursor",
            Some(
                "Over HTTP. Paste into ~/.claude.json, ~/.pi/agent/mcp.json or the editor's \
                 MCP settings.",
            ),
            COPY_HTTP,
            copy_label(COPY_HTTP),
            false,
        );
        sec.button(
            "Claude Desktop and other stdio clients",
            Some("Runs copper.exe --mcp-stdio, a pipe to this window that starts Copper if it isn't running."),
            COPY_STDIO,
            copy_label(COPY_STDIO),
            false,
        );

        let sec = f.section("The key");
        let token = a.token();
        let shown: String = token.chars().take(8).collect();
        sec.value(
            "Bearer token",
            Some("Every request must carry it. Kept in agent.json, readable by you alone."),
            &format!("{shown}…"),
        );
        sec.button(
            "Make a new token",
            Some("A new one stops every client set up with the old one."),
            "rotate",
            "Rotate Token…",
            false,
        );
        let (calls, last) = a.stats();
        let mut note = String::new();
        if calls > 0 {
            note.push_str(&format!(
                "{calls} tool call{} this session — last: {last}. ",
                if calls == 1 { "" } else { "s" }
            ));
        }
        note.push_str(
            "Agents act as you: whatever you are signed in to, they are too. Turn the server \
             off when you don't need it.",
        );
        sec.note("", Some(&note));
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        let Some(a) = copper_shell_core::agents::get::<Win>() else {
            return;
        };
        match action {
            "enabled" => a.set_enabled(on(value)),
            "announces" => a.set_announces(on(value)),
            "jev" => a.set_jev(on(value)),
            "page-context" => edit_chat(shell, |c| c.page_context = Some(on(value))),
            "turns" => {
                if let Ok(n) = value.trim().parse::<i64>() {
                    edit_chat(shell, |c| c.max_turns = Some(n.clamp(1, 500)));
                }
            }
            "turns-done" => crate::settings::changed(shell, "agents"),
            "port-next" => match next_free_port(a.configured_port()) {
                Some(p) => {
                    a.set_port(p);
                    shell.toast(&format!("The agent server now uses port {p}"));
                }
                None => shell.toast("No free port near this one"),
            },
            COPY_HTTP | COPY_STDIO => {
                let (id, text) = if action == COPY_HTTP {
                    (COPY_HTTP, a.http_config())
                } else {
                    (COPY_STDIO, a.stdio_config())
                };
                shell.view().copy_text(&text);
                COPIED.with(|c| c.set(Some(id)));
                crate::settings::changed(shell, "agents");
                let weak = shell.downgrade();
                copper_win_rt::spawn_local(async move {
                    copper_win_rt::sleep(Duration::from_millis(1600)).await;
                    if COPIED.with(Cell::get) == Some(id) {
                        COPIED.with(|c| c.set(None));
                        if let Some(s) = weak.upgrade() {
                            crate::settings::changed(&s, "agents");
                        }
                    }
                });
            }
            "rotate" => {
                let weak = shell.downgrade();
                shell.view().ask(
                    shell,
                    Question {
                        title: "Make a new token?".into(),
                        detail: Some(
                            "Every client set up with the current token stops working until \
                             it is set up again with the new one."
                                .into(),
                        ),
                        deny: "Cancel".into(),
                        allow: "Make a New Token".into(),
                    },
                    Box::new(move |yes| {
                        if !yes {
                            return;
                        }
                        if let Some(a) = copper_shell_core::agents::get::<Win>() {
                            a.rotate_token();
                        }
                        if let Some(s) = weak.upgrade() {
                            s.toast("New token made; set your clients up again");
                            crate::settings::changed(&s, "agents");
                        }
                    }),
                );
            }
            _ => {}
        }
    }
}
