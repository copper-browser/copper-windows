//! The addons lane's self-test: `COPPER_SELFTEST=addons copper.exe` in a
//! throwaway `COPPER_WORLD`. Results (`PASS`/`FAIL` lines, then a count)
//! go to `selftest.txt`, screenshots beside it, in `COPPER_SELFTEST_OUT`
//! (default `%TEMP%\copper-addons-selftest`). Needs no cloud. The Windows
//! twin of Linux's `addons/selftest.rs` (Mac `AddonsBench.swift`); the
//! fixture is the shared `qa/addons/hello`, built in.
//!
//! 1. Nothing installed: Ctrl+E shows Copper's placeholder — light, dark.
//! 2. The hello bundle is packed in the served format, verified and staged:
//!    Settings › Updates says "Update available"; Install puts it in the
//!    slot without a relaunch.
//! 3. Through the page's own `window.copper`: `info` (`platform:
//!    "windows"`), `settings`, `storage.*`, `fetch` to an unlisted host
//!    (`denied`), `openUrl`, `page.current` and the `page` event,
//!    `agent.state`, `identity`, only the slot's own files, no navigation
//!    away, `oauth.authorize` (redirect, then a closed tab = `cancelled`),
//!    the `theme` event, `ui.close`, `visibility`, kept while hidden.
//! 4. "Use in Agent sidebar" off and on; Remove puts the placeholder back.

use super::{glue, policy, view};
use crate::chrome::pane;
use crate::platform::Shell;
use copper_core::settings::Look;
use copper_engine::{PageView, World};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const HELLO: &[(&str, &[u8])] = &[
    (
        "manifest.json",
        include_bytes!(
            "../../../../shared/copper-linux/crates/copper-app/qa/addons/hello/manifest.json"
        ),
    ),
    (
        "index.html",
        include_bytes!(
            "../../../../shared/copper-linux/crates/copper-app/qa/addons/hello/index.html"
        ),
    ),
    (
        "app.js",
        include_bytes!("../../../../shared/copper-linux/crates/copper-app/qa/addons/hello/app.js"),
    ),
    (
        "app.css",
        include_bytes!("../../../../shared/copper-linux/crates/copper-app/qa/addons/hello/app.css"),
    ),
];

/// A page for the stage while the slot is open (no network needed).
pub(super) const READING: &str = "data:text/html;charset=utf-8,%3C!doctype%20html%3E%3Ctitle%3EField%20notes%3C/title%3E\
%3Cmeta%20name=color-scheme%20content=%22light%20dark%22%3E\
%3Cbody%20style=%22font:17px/1.6%20Georgia,serif;max-width:34em;margin:9vh%20auto;padding:0%202em%22%3E\
%3Ch1%20style=%22font-weight:500%22%3EField%20notes%3C/h1%3E\
%3Cp%3EThe%20Agent%20sidebar%20sits%20to%20the%20right%20of%20the%20page,%20full%20height.%20\
Ctrl+E%20shows%20it%20or%20puts%20it%20away;%20its%20leading%20edge%20can%20be%20dragged%20wider.%3C/p%3E\
%3Cp%3EWhat%20it%20shows%20comes%20from%20your%20Copper%20Cloud:%20a%20small%20web%20app%20with%20its%20own%20\
storage%20and%20settings,%20updated%20without%20a%20new%20Copper.%3C/p%3E";

/// Start when `COPPER_SELFTEST=addons`.
pub fn maybe_start(shell: &Shell) {
    if !glue::selftest_run() {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(1500)).await;
        let Some(shell) = weak.upgrade() else { return };
        let r = Run::new(&shell);
        run(&r).await;
        let failures = r.failures.borrow().len();
        r.line(&format!("addons selftest: {failures} failure(s)"));
        r.flush();
        crate::app::quit(&shell);
    });
}

pub(super) struct Run {
    pub(super) shell: Shell,
    pub(super) out: PathBuf,
    log: RefCell<String>,
    pub(super) failures: RefCell<Vec<String>>,
}

impl Run {
    pub(super) fn new(shell: &Shell) -> Run {
        let out = std::env::var_os("COPPER_SELFTEST_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("copper-addons-selftest"));
        let _ = std::fs::create_dir_all(&out);
        Run {
            shell: shell.clone(),
            out,
            log: RefCell::default(),
            failures: RefCell::default(),
        }
    }

    pub(super) fn line(&self, s: &str) {
        tracing::info!(target: "copper::selftest", "{s}");
        let _ = writeln!(self.log.borrow_mut(), "{s}");
        self.flush();
    }

    pub(super) fn flush(&self) {
        let _ = std::fs::write(self.out.join("selftest.txt"), self.log.borrow().as_bytes());
    }

    pub(super) fn check(&self, ok: bool, what: &str) {
        self.line(&format!("{} {what}", if ok { "PASS" } else { "FAIL" }));
        if !ok {
            self.failures.borrow_mut().push(what.into());
        }
    }

    pub(super) async fn sleep(&self, ms: u64) {
        copper_win_rt::sleep(Duration::from_millis(ms)).await;
    }

    pub(super) async fn shot(&self, name: &str) {
        self.sleep(900).await;
        let path = self.out.join(format!("{name}.png"));
        match copper_shell_core::ShellView::screenshot(self.shell.view()) {
            Some(png) => match std::fs::write(&path, png) {
                Ok(()) => self.line(&format!("SHOT {}", path.display())),
                Err(e) => self.check(false, &format!("screenshot {name}: {e}")),
            },
            None => self.check(false, &format!("screenshot {name}: no picture")),
        }
    }

    async fn look(&self, look: Look) {
        self.shell.set_look(look);
        self.sleep(500).await;
    }

    /// `body` as an async function in the addon's page world.
    pub(super) async fn js(&self, body: &str) -> Result<Value, String> {
        let Some(v) = pane::view() else {
            return Err("no addon view".into());
        };
        let fut = v.page().call_async(body, &Map::new(), &World::Page);
        match copper_shell_core::timeout::<crate::platform::WinLoop, _>(
            Duration::from_secs(20),
            fut,
        )
        .await
        {
            Some(Ok(v)) => Ok(v),
            Some(Err(e)) => Err(e.to_string()),
            None => Err("timed out".into()),
        }
    }

    pub(super) async fn until(&self, limit: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < limit {
            if cond() {
                return true;
            }
            self.sleep(100).await;
        }
        cond()
    }

    pub(super) async fn until_js(&self, limit: Duration, body: &str) -> bool {
        let start = Instant::now();
        while start.elapsed() < limit {
            if self.js(body).await.is_ok_and(|v| v == Value::Bool(true)) {
                return true;
            }
            self.sleep(200).await;
        }
        false
    }
}

/// One JSON-RPC message to the agent server, as an MCP client sends it;
/// the session it named and the answer.
async fn mcp(
    token: &str,
    session: Option<&str>,
    body: Value,
) -> Result<(Option<String>, Value), String> {
    let (status, session, v) = http(
        "POST",
        &[("Authorization", format!("Bearer {token}"))],
        session,
        Some(body),
    )
    .await?;
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    Ok((session, v))
}

/// A raw request to the agent server's endpoint: status, session, body.
async fn http(
    method: &'static str,
    extra: &[(&'static str, String)],
    session: Option<&str>,
    body: Option<Value>,
) -> Result<(u16, Option<String>, Value), String> {
    let port = copper_mcp::config::port_override().ok_or("no SEARCH_MCP_PORT")?;
    let mut headers = vec![("Content-Type", "application/json".to_string())];
    headers.extend(extra.iter().cloned());
    if let Some(s) = session {
        headers.push(("Mcp-Session-Id", s.to_string()));
    }
    let raw = body.map(|b| b.to_string()).unwrap_or_default();
    copper_shell_core::main_loop::spawn_blocking(move || {
        let r = copper_mcp::client::request(
            port,
            method,
            copper_mcp::config::PATH,
            &headers,
            raw.as_bytes(),
            Duration::from_secs(30),
        )
        .map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
        Ok((r.status, r.header("mcp-session-id").map(str::to_string), v))
    })
    .await
    .ok_or_else(|| "the request's thread went away".to_string())?
}

fn init(name: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": { "name": name, "version": "1" } }
    })
}

/// The listener's doors (spec checks 12, 14, 26): no or a wrong bearer is
/// 401, a foreign Origin 403, GET 405; after Stop another caller still
/// works; a rotated token shuts the old one out and lets the new one in.
async fn server_doors(r: &Run, a: &copper_shell_core::agents::Agents<crate::platform::Win>) {
    let token = a.token();
    let bearer = |t: &str| ("Authorization", format!("Bearer {t}"));
    let code = |res: Result<(u16, Option<String>, Value), String>| {
        res.map(|(s, _, _)| s.to_string()).unwrap_or_else(|e| e)
    };
    let none = code(http("POST", &[], None, Some(init("x"))).await);
    let wrong = code(http("POST", &[bearer("0000")], None, Some(init("x"))).await);
    r.check(
        none == "401" && wrong == "401",
        &format!("no bearer → {none}, a wrong one → {wrong} (401)"),
    );
    let evil = code(
        http(
            "POST",
            &[bearer(&token), ("Origin", "https://evil.example".into())],
            None,
            Some(init("x")),
        )
        .await,
    );
    r.check(evil == "403", &format!("a foreign Origin → {evil} (403)"));
    let deceptive = code(
        http(
            "POST",
            &[
                bearer(&token),
                ("Origin", "http://localhost.evil.example".into()),
            ],
            None,
            Some(init("x")),
        )
        .await,
    );
    // Upstream copper-mcp matches origins by substring: reported, not ours.
    r.line(&format!(
        "{} Origin http://localhost.evil.example → {deceptive} (want 403; copper-mcp transport.rs origin_allowed)",
        if deceptive == "403" { "PASS" } else { "KNOWN" }
    ));
    let local = code(
        http(
            "POST",
            &[bearer(&token), ("Origin", "http://127.0.0.1:9".into())],
            None,
            Some(init("x")),
        )
        .await,
    );
    r.check(
        local == "200",
        &format!("a loopback Origin → {local} (200)"),
    );
    let get = code(http("GET", &[bearer(&token)], None, None).await);
    r.check(get == "405", &format!("GET /mcp → {get} (405)"));

    // Another caller still works while the stopped one is refused.
    match mcp(&token, None, init("Cursor")).await {
        Ok((Some(s), _)) => {
            let v = mcp(
                &token,
                Some(&s),
                json!({
                    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                    "params": { "name": "browser_tabs", "arguments": { "action": "list" } }
                }),
            )
            .await;
            let (text, err) = v.as_ref().map(|(_, v)| tool_text(v)).unwrap_or_default();
            r.check(
                !err && !text.starts_with("Stopped"),
                &format!(
                    "another caller still works after Stop ({})",
                    text.chars().take(40).collect::<String>()
                ),
            );
        }
        other => r.check(false, &format!("a second MCP session ({other:?})")),
    }

    // Rotate.
    a.rotate_token();
    let fresh = a.token();
    let old = code(http("POST", &[bearer(&token)], None, Some(init("x"))).await);
    let new = code(http("POST", &[bearer(&fresh)], None, Some(init("x"))).await);
    r.check(
        old == "401" && new == "200" && fresh != token && fresh.len() == 64,
        &format!("Rotate: the old token → {old}, the new one → {new} (64 hex, changed)"),
    );
}

/// A tool call's text and whether it was an error.
fn tool_text(v: &Value) -> (String, bool) {
    let text = v["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    (text, v["result"]["isError"].as_bool().unwrap_or(true))
}

fn call(id: u64, tool: &str, args: Value) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {
            "name": tool, "arguments": args,
            "_meta": { "copper/agent": { "label": "Claude Code", "thread": "Self-test" } }
        }
    })
}

/// The marks: a real MCP client's calls put its square on its tab, the
/// bar and ring over the page while it works there, the away pill while it
/// works behind; a second agent wears another colour; the pointer tucks the
/// bar; Stop turns its next call away.
async fn marks_test(r: &Run, a: &copper_shell_core::agents::Agents<crate::platform::Win>) {
    use crate::agents::marks;
    let shell = &r.shell;
    let token = a.token();
    let session = match mcp(&token, None, init("Claude Code")).await {
        Ok((Some(s), v)) if v["result"].is_object() => s,
        other => {
            r.check(false, &format!("MCP initialize over HTTP ({other:?})"));
            return;
        }
    };
    let s = Some(session.as_str());
    let home = shell.active_id();
    let opened = mcp(
        &token,
        s,
        call(
            2,
            "browser_tabs",
            json!({ "action": "new", "url": "https://example.com/?from=agent" }),
        ),
    )
    .await;
    let tab = shell
        .browser()
        .all_tabs()
        .into_iter()
        .find(|t| t.url.contains("from=agent"))
        .map(|t| t.id.clone());
    let Some(tab) = tab else {
        r.check(
            false,
            &format!("browser_tabs new opened a tab ({opened:?})"),
        );
        return;
    };
    let hands = marks::on(&tab);
    r.check(
        hands.len() == 1 && hands[0].agent == "Claude Code" && shell.active_id() == home,
        &format!(
            "a background MCP call marks its own tab, not the user's ({:?})",
            hands
                .iter()
                .map(|h| (&h.agent, &h.initials))
                .collect::<Vec<_>>()
        ),
    );
    let (bar, _, _, rows) = marks::shown(shell);
    r.check(
        rows == 1 && bar == 0,
        &format!("the square is on its row, no bar over the user's page (rows {rows}, bar {bar})"),
    );
    r.shot("win-marks-badge").await;

    // A long call behind the user's tab: the away pill.
    let (token2, sess2) = (token.clone(), session.clone());
    copper_win_rt::spawn_local(async move {
        let _ = mcp(
            &token2,
            Some(&sess2),
            call(3, "browser_wait_for", json!({ "time": 3 })),
        )
        .await;
    });
    let away = r
        .until(Duration::from_secs(2), || marks::shown(shell).2)
        .await;
    r.check(
        away,
        "an agent busy only behind the user's tab shows the top-right pill",
    );
    r.shot("win-marks-away").await;
    r.sleep(2600).await;

    // On screen: the bar and the ring while it works.
    shell.select(&tab);
    r.sleep(400).await;
    let (token3, sess3) = (token.clone(), session.clone());
    copper_win_rt::spawn_local(async move {
        let _ = mcp(
            &token3,
            Some(&sess3),
            call(4, "browser_wait_for", json!({ "time": 4 })),
        )
        .await;
    });
    let busy = r
        .until(Duration::from_secs(2), || {
            let (bar, ring, away, _) = marks::shown(shell);
            bar == 1 && ring && !away
        })
        .await;
    r.check(
        busy,
        "on screen: one capsule top centre and the ring while it works",
    );
    // A second agent on the same tab wears another colour.
    let phi = copper_mcp::Who {
        key: "selftest-phi".into(),
        agent: "phi".into(),
        thread: "Second agent".into(),
        seed: "selftest-phi".into(),
        ..Default::default()
    };
    marks::event(&copper_mcp::drive::DriveEvent::CallBegan {
        ticket: 9_000_001,
        who: phi.clone(),
        tool: "browser_snapshot".into(),
        title: "Read the page".into(),
        detail: None,
        tab: Some(tab.clone()),
        behind: false,
    });
    let hands = marks::on(&tab);
    r.check(
        hands.len() == 2 && hands[0].color != hands[1].color && marks::shown(shell).0 == 2,
        &format!(
            "two agents on one tab: two capsules, two colours ({:?})",
            hands
                .iter()
                .map(|h| (&h.agent, h.color))
                .collect::<Vec<_>>()
        ),
    );
    r.shot("win-marks-busy").await;
    r.look(Look::Dark).await;
    r.shot("win-marks-busy-dark").await;
    r.look(Look::Light).await;
    marks::event(&copper_mcp::drive::DriveEvent::CallEnded {
        ticket: 9_000_001,
        who: phi,
        tool: "browser_snapshot".into(),
        outcome: Default::default(),
        tab: Some(tab.clone()),
    });
    r.sleep(3500).await;
    let (bar, ring, _, _) = marks::shown(shell);
    r.check(
        bar == 2 && !ring,
        &format!("after the calls: the capsules linger, the ring goes (bar {bar}, ring {ring})"),
    );

    // The pointer near the bar tucks it in; away, it comes back.
    if let Some((x, y)) = marks::capsule_point(shell) {
        point_at(shell, x, y);
        let tucked = r.until(Duration::from_secs(1), marks::tucked).await;
        r.check(tucked, "the pointer near the bar tucks it in");
        r.shot("win-marks-tucked").await;
        point_at(shell, x, y + 300);
        let back = r.until(Duration::from_secs(2), || !marks::tucked()).await;
        r.check(back, "and it comes back once the pointer has left");
    } else {
        r.check(false, "the bar has a capsule to point at");
    }

    // Stop: its next call is turned away.
    let key = marks::on(&tab)
        .into_iter()
        .find(|h| h.agent == "Claude Code")
        .map(|h| h.key);
    if let Some(key) = key {
        marks::stop(shell, &key);
        let next = mcp(&token, s, call(5, "browser_snapshot", json!({}))).await;
        let (text, err) = next.as_ref().map(|(_, v)| tool_text(v)).unwrap_or_default();
        r.check(
            err && text.starts_with("Stopped by the user in Copper"),
            &format!(
                "Stop turns its next call away ({})",
                text.chars().take(60).collect::<String>()
            ),
        );
        r.shot("win-marks-stopped").await;
    } else {
        r.check(false, "Claude Code's hand is on its tab");
    }
    if let Some(h) = home {
        shell.select(&h);
    }
    r.sleep(300).await;
}

/// The real pointer to (x, y) client pixels.
fn point_at(shell: &Shell, x: i32, y: i32) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    let hwnd = shell.view().chrome().window().hwnd();
    let mut at = POINT { x, y };
    // SAFETY: our own window; a plain cursor move.
    unsafe {
        if ClientToScreen(hwnd, &mut at).as_bool() {
            let _ = SetCursorPos(at.x, at.y);
        }
    }
}

/// Drag the grip `dx` client pixels with posted mouse messages (the path a
/// real drag takes); the window's scale, if it ran.
async fn drag_grip(shell: &Shell, dx: i32) -> Option<f32> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEINPUT, SendInput,
    };
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    // Real input, as a person drags: posted messages race the real
    // pointer's own moves (whose button is up), which end a drag.
    let (x, y) = pane::grip_point(shell)?;
    let chrome = shell.view().chrome().clone();
    let scale = chrome.read(|ui, _| ui.scale())?;
    let hwnd = chrome.window().hwnd();
    let mut at = POINT { x, y };
    // SAFETY: our own window; a plain point.
    if !unsafe { ClientToScreen(hwnd, &mut at) }.as_bool() {
        return None;
    }
    let button = |flags: MOUSE_EVENT_FLAGS| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    // SAFETY: plain cursor moves and button input.
    unsafe {
        SetCursorPos(at.x, at.y).ok()?;
    }
    copper_win_rt::sleep(Duration::from_millis(120)).await;
    // SAFETY: as above.
    unsafe {
        SendInput(
            &[button(MOUSEEVENTF_LEFTDOWN)],
            std::mem::size_of::<INPUT>() as i32,
        );
    }
    copper_win_rt::sleep(Duration::from_millis(80)).await;
    for step in 1..=4 {
        // SAFETY: as above.
        unsafe {
            let _ = SetCursorPos(at.x + dx * step / 4, at.y);
        }
        copper_win_rt::sleep(Duration::from_millis(50)).await;
    }
    // SAFETY: as above.
    unsafe {
        SendInput(
            &[button(MOUSEEVENTF_LEFTUP)],
            std::mem::size_of::<INPUT>() as i32,
        );
        let _ = SetCursorPos(at.x + dx, at.y + 200);
    }
    copper_win_rt::sleep(Duration::from_millis(150)).await;
    Some(scale)
}

pub(super) fn short(v: &Result<Value, String>) -> String {
    let s = match v {
        Ok(v) => v.to_string(),
        Err(e) => format!("error: {e}"),
    };
    if s.chars().count() > 200 {
        format!("{}…", s.chars().take(200).collect::<String>())
    } else {
        s
    }
}

fn active_url(shell: &Shell) -> String {
    shell
        .active_id()
        .and_then(|id| shell.tab_view(&id))
        .map(|t| t.url)
        .unwrap_or_default()
}

fn tabs(shell: &Shell) -> usize {
    shell.browser().all_tabs().count()
}

async fn run(r: &Run) {
    let shell = &r.shell;
    r.check(
        policy::clamp_width(Some(100.0)) == policy::MIN_WIDTH
            && policy::clamp_width(Some(9000.0)) == policy::MAX_WIDTH,
        "pane width is clamped to 340…620",
    );
    r.check(
        pane::width() == policy::clamp_width(shell.settings().number(policy::WIDTH_KEY)),
        &format!(
            "pane width comes from {} ({})",
            policy::WIDTH_KEY,
            pane::width()
        ),
    );
    view::open_tab(shell, READING);
    r.sleep(1500).await;

    // 1. The placeholder.
    r.check(
        glue::active().is_none(),
        "a fresh world has no addon in the slot",
    );
    r.check(!pane::showing(), "the slot starts hidden");
    r.look(Look::Light).await;
    crate::commands::run(shell, "agent");
    r.check(pane::showing(), "Ctrl+E's command shows the slot");
    r.check(
        pane::face() == "placeholder",
        "with no addon the slot shows Copper's placeholder",
    );
    r.shot("win-placeholder-light").await;
    r.look(Look::Dark).await;
    r.shot("win-placeholder-dark").await;
    r.look(Look::Light).await;
    let before = pane::width();
    pane::set_width(shell, f64::from(before) + 120.0);
    r.sleep(300).await;
    r.check(
        (pane::width() - (before + 120.0).min(policy::MAX_WIDTH)).abs() < 0.5
            && shell.settings().number(policy::WIDTH_KEY).is_some(),
        &format!("the width changes and is kept ({})", pane::width()),
    );
    r.shot("win-placeholder-wide").await;
    pane::set_width(shell, f64::from(before));
    r.sleep(300).await;
    // The grip, dragged 80 px left with the window's own mouse messages.
    let dragged = drag_grip(shell, -80).await;
    r.sleep(300).await;
    let kept = shell.settings().number(policy::WIDTH_KEY).unwrap_or(0.0) as f32;
    r.check(
        dragged.is_some_and(|scale| (pane::width() - (before + 80.0 / scale)).abs() < 2.0)
            && (kept - pane::width()).abs() < 0.5,
        &format!(
            "dragging the grip left widens the pane and keeps it ({before} → {}, kept {kept})",
            pane::width()
        ),
    );
    r.shot("win-placeholder-dragged").await;
    pane::set_width(shell, f64::from(before));
    crate::commands::run(shell, "agent");
    r.sleep(400).await;
    r.check(!pane::showing(), "Ctrl+E's command again hides the slot");

    // 2. Stage the hello bundle as a cloud update, then Install.
    match glue::selftest_stage(HELLO) {
        Ok(()) => r.check(true, "the hello bundle is verified and staged"),
        Err(e) => r.check(false, &format!("stage the hello bundle: {e}")),
    }
    super::changed(shell);
    let pending = glue::pending();
    r.check(
        pending
            .as_ref()
            .is_some_and(|p| p.id == "example.hello" && p.version == "1.0.0"),
        &format!("pending update is listed ({pending:?})"),
    );
    crate::settings::open(shell, "updates");
    r.shot("win-update-available").await;
    crate::settings::open(shell, "addons");
    r.shot("win-settings-addons-pending").await;
    super::updates::install(shell);
    let installed = r
        .until(Duration::from_secs(10), || {
            glue::active().is_some_and(|a| a.id == "example.hello")
        })
        .await;
    r.check(
        installed,
        "Install puts the addon in the slot (no relaunch)",
    );
    r.check(
        glue::pending().is_none(),
        "nothing is pending after Install",
    );
    copper_shell_core::ShellView::hide_panel(shell.view());
    r.sleep(300).await;

    // 3. The bridge, from the page.
    let started = Instant::now();
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    super::show(shell);
    r.check(pane::face() == "addon", "the slot shows the addon");
    let ready = r
        .until_js(
            Duration::from_secs(30),
            "return !!window.copper && window.__helloDone === true;",
        )
        .await;
    r.line(&format!(
        "TIME addon page ready in {} ms",
        started.elapsed().as_millis()
    ));
    r.check(ready, "the addon page loaded and ran every call");
    // Where the first show's time goes: the page's own clock, then one
    // bridge round trip each way.
    let split = r
        .js("const n = performance.getEntriesByType('navigation')[0] || {}; \
             const t = async f => { const s = performance.now(); await f(); return Math.round((performance.now() - s) * 10) / 10; }; \
             const r = e => [e.name.split('/').pop() || e.name, Math.round(e.startTime), Math.round(e.responseStart || 0), Math.round(e.responseEnd)]; \
             return { origin: performance.timeOrigin, doc: r(n), resources: performance.getEntriesByType('resource').map(r), \
                      domContentLoaded: Math.round(n.domContentLoadedEventEnd || 0), \
                      info: await t(() => copper.info()), storage: await t(() => copper.storage.get('count')), \
                      denied: await t(() => copper.fetch('https://denied.example.org/').catch(() => 0)), \
                      fetch: await t(() => copper.fetch('https://example.com/', { timeoutMs: 8000 }).catch(() => 0)) };")
        .await;
    let before_nav = split
        .as_ref()
        .ok()
        .and_then(|v| v.get("origin"))
        .and_then(Value::as_f64)
        .map(|o| (o - epoch).round());
    r.line(&format!(
        "TIME addon page: show→navigation start {before_nav:?} ms; navigation→DOMContentLoaded and bridge round trips, ms {}",
        split.as_ref().map(Value::to_string).unwrap_or_else(|e| e.clone())
    ));
    if !ready {
        let why = r
            .js("return {head: document.head && document.head.outerHTML.slice(0, 600), \
                 entries: performance.getEntries().map(e => e.name + ' ' + (e.responseStatus || '')), \
                 copper: typeof window.copper, done: window.__helloDone, results: window.__helloResults};")
            .await;
        r.line(&format!("  page: {}", short(&why)));
    }
    let url = pane::view()
        .and_then(|v| v.page().url())
        .unwrap_or_default();
    r.check(
        url.starts_with("copper-addon://example.hello/"),
        &format!("the page is served from copper-addon:// ({url})"),
    );
    let info = r.js("return await copper.info();").await;
    r.check(
        info.as_ref().is_ok_and(|i| {
            i["bridge"] == json!(1)
                && i["platform"] == json!("windows")
                && i["slot"] == json!("agent-sidebar")
                && i["addon"]["id"] == json!("example.hello")
                && !i["capabilities"]
                    .as_array()
                    .is_some_and(|c| c.contains(&json!("agent")))
        }),
        &format!("info() {}", short(&info)),
    );
    let settings = r.js("return await copper.settings();").await;
    r.check(
        settings
            .as_ref()
            .is_ok_and(|s| s["greeting"] == json!("Hello from the addon’s settings")),
        &format!("settings() {}", short(&settings)),
    );
    let storage = r
        .js("await copper.storage.set('selftest', {n: 42, s: 'ü'});\
             const got = await copper.storage.get('selftest');\
             const keys = await copper.storage.keys();\
             await copper.storage.remove('selftest');\
             const gone = await copper.storage.get('selftest');\
             return {got, keys, gone};")
        .await;
    r.check(
        storage.as_ref().is_ok_and(|s| {
            s["got"] == json!({"n": 42, "s": "ü"})
                && s["keys"]
                    .as_array()
                    .is_some_and(|k| k.contains(&json!("selftest")))
                && s["gone"].is_null()
        }),
        &format!("storage set/get/keys/remove {}", short(&storage)),
    );
    let denied = r
        .js(
            "try { await copper.fetch('https://denied.example.org/'); return 'resolved'; }\
             catch (e) { return e.code; }",
        )
        .await;
    r.check(
        denied.as_ref().is_ok_and(|d| d == "denied"),
        &format!("fetch to an unlisted host is denied ({})", short(&denied)),
    );
    let before = tabs(shell);
    let opened = r
        .js("return await copper.openUrl('https://example.com/?from=hello-addon');")
        .await;
    r.sleep(600).await;
    let now = active_url(shell);
    r.check(
        opened.is_ok() && now.starts_with("https://example.com/") && tabs(shell) == before + 1,
        &format!("openUrl() opens a new foreground tab ({now})"),
    );
    let page = r.js("return await copper.page.current();").await;
    r.check(
        page.as_ref().is_ok_and(|p| {
            p["url"]
                .as_str()
                .is_some_and(|u| u.starts_with("https://example.com/"))
        }),
        &format!("page.current() {}", short(&page)),
    );
    let page_event = r
        .until_js(
            Duration::from_secs(10),
            "return window.__helloEvents.some(e => e.name === 'page' && e.payload \
             && String(e.payload.url).startsWith('https://example.com/'));",
        )
        .await;
    r.check(page_event, "the page event follows the active tab");
    let agent = r.js("return await copper.agent.state();").await;
    r.check(
        agent.as_ref().is_ok_and(|a| a["available"] == json!(false)),
        &format!("agent.state() says unavailable {}", short(&agent)),
    );
    let identity = r.js("return await copper.identity();").await;
    r.check(
        identity.as_ref().is_ok_and(Value::is_null),
        &format!("identity() is null without a cloud {}", short(&identity)),
    );
    let files = r
        .js("const load = src => new Promise(ok => { const i = new Image(); \
               i.onload = () => ok('loaded'); i.onerror = () => ok('refused'); i.src = src; });\
             return {missing: await load('nope.png'), other: await load('copper-addon://example.other/icon.png'), \
                     up: await load('copper-addon://example.hello/..%2Fmanifest.json')};")
        .await;
    r.check(
        files.as_ref().is_ok_and(|f| {
            f["missing"] == json!("refused")
                && f["other"] == json!("refused")
                && f["up"] == json!("refused")
        }),
        &format!(
            "copper-addon:// answers only the slot's own files {}",
            short(&files)
        ),
    );
    let before = tabs(shell);
    let popup = r
        .js("const w = window.open('https://example.com/popup'); return w === null;")
        .await;
    r.sleep(600).await;
    r.check(
        popup.as_ref().is_ok_and(|v| v == &json!(true))
            && !active_url(shell).contains("/popup")
            && tabs(shell) == before,
        &format!("window.open is refused {}", short(&popup)),
    );
    let link = r
        .js(
            "const a = document.createElement('a'); a.href = 'https://example.com/?from=link'; \
             a.target = '_blank'; a.textContent = 'link'; document.body.appendChild(a); a.click(); \
             a.remove(); return true;",
        )
        .await;
    r.sleep(800).await;
    let now = active_url(shell);
    r.check(
        link.is_ok() && now.contains("from=link") && tabs(shell) == before + 1,
        &format!("a web link click opens in a new foreground tab ({now})"),
    );
    let tabs_before = tabs(shell);
    let _ = r
        .js("setTimeout(() => { location.href = 'file:///C:/Windows/win.ini'; }, 0); return true;")
        .await;
    r.sleep(1000).await;
    let url = pane::view()
        .and_then(|v| v.page().url())
        .unwrap_or_default();
    r.check(
        url.starts_with("copper-addon://example.hello/") && tabs(shell) == tabs_before,
        &format!("a navigation away from the addon is refused ({url})"),
    );
    // A script's web navigation: the engine's filter vetoes it before it
    // starts (no tab, no reload).
    let refused = super::view::refused();
    let _ = r
        .js("setTimeout(() => { location.href = 'https://example.com/?from=script'; }, 0); return true;")
        .await;
    r.sleep(1000).await;
    let url = pane::view()
        .and_then(|v| v.page().url())
        .unwrap_or_default();
    // Within a person's activation (the click above) WebView2 calls it
    // user-initiated: it becomes a tab, as that click's link would.
    let tabbed = tabs(shell) > tabs_before;
    r.check(
        url.starts_with("copper-addon://example.hello/") && super::view::refused() == refused + 1,
        &format!(
            "a script's web navigation is vetoed by the navigation filter ({url}; vetoes {}; {})",
            super::view::refused() - refused,
            if tabbed {
                "opened as a tab: user-initiated"
            } else {
                "refused"
            }
        ),
    );
    let still = r
        .until_js(
            Duration::from_secs(20),
            "return window.__helloDone === true;",
        )
        .await;
    r.check(still, "the addon page is (still or again) its own");

    // OAuth: the first address with the redirect prefix ends the sign-in.
    let tabs_before = tabs(shell);
    let oauth = r
        .js("return await copper.oauth.authorize({url: 'https://example.com/authorize?code=abc&state=s', \
             redirectPrefix: 'https://example.com/authorize', timeoutMs: 20000});")
        .await;
    r.sleep(400).await;
    r.check(
        oauth
            .as_ref()
            .is_ok_and(|o| o["url"] == json!("https://example.com/authorize?code=abc&state=s")),
        &format!(
            "oauth.authorize() resolves with the redirect {}",
            short(&oauth)
        ),
    );
    r.check(tabs(shell) == tabs_before, "the sign-in tab closes itself");
    let _ = r
        .js("window.__oauth = 'pending'; copper.oauth.authorize({url: 'https://example.com/?signin', \
             redirectPrefix: 'http://127.0.0.1:8765/callback', timeoutMs: 60000}) \
             .then(() => { window.__oauth = 'resolved'; }, e => { window.__oauth = e.code; }); return true;")
        .await;
    let opened = r
        .until(Duration::from_secs(5), || {
            active_url(shell).starts_with("https://example.com/?signin")
        })
        .await;
    r.check(opened, "oauth.authorize() opens a foreground sign-in tab");
    r.shot("win-oauth-tab").await;
    if let Some(id) = shell.active_id() {
        shell.close(&id);
    }
    let cancelled = r
        .until_js(
            Duration::from_secs(5),
            "return window.__oauth === 'cancelled';",
        )
        .await;
    r.check(cancelled, "closing the sign-in tab is `cancelled`");

    // Theme, ui.close and visibility.
    r.shot("win-open-light").await;
    r.look(Look::Dark).await;
    let themed = r
        .until_js(
            Duration::from_secs(5),
            "return window.__helloEvents.some(e => e.name === 'theme' && e.payload && e.payload.dark === true);",
        )
        .await;
    r.check(themed, "the theme event follows the window to dark");
    r.shot("win-open-dark").await;
    let closed = r.js("await copper.ui.close(); return true;").await;
    let hidden = r.until(Duration::from_secs(3), || !pane::showing()).await;
    r.check(closed.is_ok() && hidden, "ui.close() hides the slot");
    r.shot("win-closed-dark").await;
    r.look(Look::Light).await;
    r.shot("win-closed-light").await;
    super::show(shell);
    let vis = r
        .until_js(
            Duration::from_secs(5),
            "const v = window.__helloEvents.filter(e => e.name === 'visibility');\
             return v.some(e => e.payload.visible === false) && v.some(e => e.payload.visible === true);",
        )
        .await;
    r.check(vis, "visibility events arrive (hidden, then shown again)");
    let kept = r
        .js("return window.__helloDone === true && document.querySelectorAll('#results li').length > 5;")
        .await;
    r.check(
        kept.as_ref().is_ok_and(|v| v == &json!(true)),
        "the page was kept while hidden (no reload)",
    );
    crate::settings::open(shell, "addons");
    r.shot("win-settings-addons").await;
    r.look(Look::Dark).await;
    r.shot("win-settings-addons-dark").await;
    // With SEARCH_MCP_PORT set (a port of its own), the server listens
    // for the Settings › Agents pictures.
    let serve = copper_mcp::config::port_override().is_some();
    let server = copper_shell_core::agents::get::<crate::platform::Win>();
    if serve && let Some(a) = &server {
        a.set_enabled(true);
        let up = r.until(Duration::from_secs(5), || a.running()).await;
        r.check(up, &format!("the agent server listens ({})", a.endpoint()));
    }
    crate::settings::open(shell, "agents");
    r.shot("win-settings-agents-dark").await;
    r.look(Look::Light).await;
    r.shot("win-settings-agents-light").await;
    copper_shell_core::ShellView::hide_panel(shell.view());
    r.sleep(300).await;
    if serve && let Some(a) = &server {
        marks_test(r, a).await;
        server_doors(r, a).await;
        a.set_enabled(false);
        let cleared = r
            .until(Duration::from_secs(3), || {
                let (bar, ring, away, rows) = crate::agents::marks::shown(shell);
                bar == 0 && !ring && !away && rows == 0
            })
            .await;
        r.check(cleared, "turning the server off clears every mark");
    }

    // "Use in Agent sidebar" off and on again.
    glue::set_enabled("example.hello", false);
    let off = r
        .until(Duration::from_secs(5), || {
            glue::active().is_none() && pane::face() == "placeholder"
        })
        .await;
    r.check(off, "turning the addon off here shows the placeholder");
    glue::set_enabled("example.hello", true);
    let on = r
        .until(Duration::from_secs(5), || {
            glue::active().is_some() && pane::face() == "addon"
        })
        .await;
    let ran = on
        && r.until_js(
            Duration::from_secs(20),
            "return window.__helloDone === true;",
        )
        .await;
    r.check(ran, "turning it on again brings the addon back");

    // Remove puts the placeholder back.
    glue::remove("example.hello");
    let back = r
        .until(Duration::from_secs(5), || {
            glue::active().is_none() && pane::face() == "placeholder"
        })
        .await;
    r.check(back, "Remove puts the placeholder back");
}
