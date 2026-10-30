//! The built-in agent's self-test: `COPPER_SELFTEST=agent-ui copper.exe`
//! in a throwaway `COPPER_WORLD`. `PASS`/`FAIL` lines and the pictures go
//! to `COPPER_SELFTEST_OUT` (as the addons lane's do). Needs no cloud and
//! no model.
//!
//! `COPPER_AGENT_UI_PHASES` picks the phases (comma-separated, in order;
//! default `placeholder,agent`):
//!
//! * `repro` — the session as it is (a copy of someone's world): window
//!   maximised, Settings open, the built-in off, Ctrl+E; a picture and the
//!   pane's laid-out nodes.
//! * `tabs` — sixty tabs: the column's list scrolls inside the column and
//!   the body, stage, page and pane stay inside the window (a long list
//!   once made them all taller than it: the "blank" sidebar).
//! * `stale` — whether the window's picture keeps up with the slot's page:
//!   the page changes, then the window (`PrintWindow`) and the page by
//!   itself (`CapturePreview`) are taken, with and without a nudge.
//! * `pending` — a cloud addon staged but not installed: the slot keeps
//!   showing Copper's agent (Settings › About offers the Install).
//! * `placeholder` — the built-in off: the placeholder, light and dark,
//!   after Settings was open, and after hiding and showing again.
//! * `agent` — the built-in on, the real `copper.agent`: the slot shows
//!   Copper's agent, the composer has the keyboard, a draft survives
//!   Ctrl+E.
//! * `demo` — with `COPPER_AGENT_UI_DEMO=1`: every state of the pane from
//!   the scripted conversation (empty, unconfigured, thinking, running, a
//!   Jev run, a finished chat with an error and code, the model menu, a
//!   long draft, 340 and 620 wide, Jump to latest, a question played out).

use super::selftest::{READING, Run};
use super::{builtin, view};
use crate::chrome::pane;
use copper_core::settings::Look;
use serde_json::Value;
use std::time::Duration;

/// The `COPPER_SELFTEST` value that starts this lane.
pub const LANE: &str = "agent-ui";

/// Start when `COPPER_SELFTEST=agent-ui`.
pub fn maybe_start(shell: &crate::platform::Shell) {
    if std::env::var("COPPER_SELFTEST").ok().as_deref() != Some(LANE) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(1500)).await;
        let Some(shell) = weak.upgrade() else { return };
        let r = Run::new(&shell);
        let phases =
            std::env::var("COPPER_AGENT_UI_PHASES").unwrap_or_else(|_| "placeholder,agent".into());
        for phase in phases.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            r.line(&format!("== {phase}"));
            match phase {
                "repro" => repro(&r).await,
                "tabs" => tabs(&r).await,
                "stale" => stale(&r).await,
                "pending" => pending(&r).await,
                "placeholder" => placeholder(&r).await,
                "agent" => agent(&r).await,
                "demo" => demo(&r).await,
                other => r.check(false, &format!("unknown phase {other}")),
            }
        }
        let failures = r.failures.borrow().len();
        r.line(&format!("agent-ui selftest: {failures} failure(s)"));
        r.flush();
        builtin::force(None);
        crate::app::quit(&shell);
    });
}

/// Two of the page's frames have been drawn (then the picture).
async fn shot(r: &Run, name: &str) {
    if pane::showing() {
        pane::nudge(&r.shell, true);
        r.sleep(250).await;
        pane::nudge(&r.shell, false);
        r.sleep(250).await;
    }
    if pane::view().is_some() {
        let drawn = r
            .js("await new Promise(f => requestAnimationFrame(() => requestAnimationFrame(f))); return true;")
            .await;
        if drawn.is_err() {
            r.line(&format!(
                "NOTE {name}: the page drew no frame ({})",
                super::selftest::short(&drawn)
            ));
        }
    }
    r.shot(name).await;
    // The slot's page by itself, from WebView2 (`CapturePreview`): what
    // the page has drawn, to hold the window's picture against.
    if std::env::var_os("COPPER_AGENT_UI_PAGE_SHOTS").is_some()
        && let Some(v) = pane::view()
    {
        let snap =
            copper_engine::PageView::snapshot(v.page(), copper_engine::SnapshotRegion::Visible)
                .await;
        match snap {
            Ok(img) => {
                let _ = std::fs::write(r.out.join(format!("{name}.page.png")), &img.png);
            }
            Err(e) => r.line(&format!("NOTE {name}: no page picture ({e})")),
        }
    }
}

async fn look(r: &Run, l: Look) {
    r.shell.set_look(l);
    r.sleep(600).await;
}

/// Ctrl+E's command, and a beat for the fade.
async fn ctrl_e(r: &Run) {
    crate::commands::run(&r.shell, "agent");
    r.sleep(450).await;
}

async fn ensure_shown(r: &Run, on: bool) {
    if pane::showing() != on {
        ctrl_e(r).await;
    }
}

fn maximize(r: &Run) {
    use windows::Win32::UI::WindowsAndMessaging::{SW_MAXIMIZE, ShowWindow};
    let hwnd = r.shell.view().chrome().window().hwnd();
    // SAFETY: our own window.
    unsafe {
        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
    }
}

/// The placeholder's words are laid out inside the pane, with room.
fn placeholder_drawn(r: &Run) -> (bool, String) {
    let dump = pane::layout_dump(&r.shell);
    let line = dump
        .lines()
        .find(|l| l.contains(&format!("{:?}", pane::EMPTY_TITLE)))
        .unwrap_or_default()
        .to_string();
    let ok = !line.is_empty()
        && !line.contains("hidden")
        && !line.contains("not-shown")
        && line
            .split('[')
            .nth(1)
            .and_then(|s| s.split(']').next())
            .and_then(|s| s.split_whitespace().nth(1))
            .and_then(|wh| wh.split('x').next())
            .and_then(|w| w.parse::<f32>().ok())
            .is_some_and(|w| w > 40.0);
    (ok, dump)
}

/// Someone's session as it is (a copy of their world): the pane that
/// showed blank with a long tab list.
async fn repro(r: &Run) {
    builtin::force(Some(false));
    pane::sync(&r.shell);
    maximize(r);
    r.sleep(6000).await;
    r.shot("repro-0-start").await;
    crate::settings::open(&r.shell, "agents");
    r.sleep(800).await;
    ensure_shown(r, true).await;
    r.sleep(1500).await;
    r.shot("repro-1-pane").await;
    let (ok, dump) = placeholder_drawn(r);
    r.line(&format!("pane layout:\n{dump}"));
    r.check(ok, "the placeholder's words are laid out in the pane");
    ensure_shown(r, false).await;
    ensure_shown(r, true).await;
    r.sleep(800).await;
    r.shot("repro-2-again").await;
    let (ok, dump) = placeholder_drawn(r);
    r.line(&format!("pane layout:\n{dump}"));
    r.check(ok, "and again after Ctrl+E twice");
    ensure_shown(r, false).await;
    builtin::force(None);
}

/// Every part of the window inside it.
fn fits(r: &Run, what: &str) {
    let Some((window, parts)) = pane::fit_report(&r.shell) else {
        r.check(false, &format!("{what}: no layout to read"));
        return;
    };
    let lines: Vec<String> = parts
        .iter()
        .map(|(n, bottom, h)| format!("{n}: bottom {bottom:.0}, height {h:.0}"))
        .collect();
    r.line(&format!("{what}: window {window:.0}; {}", lines.join("; ")));
    let over: Vec<&str> = parts
        .iter()
        .filter(|(_, bottom, _)| *bottom > window + 0.5)
        .map(|(n, _, _)| n.as_str())
        .collect();
    r.check(
        over.is_empty() && !parts.is_empty(),
        &format!("{what}: everything ends inside the window (over: {over:?})"),
    );
    let pane_h = parts.iter().find(|p| p.0 == "pane").map(|p| p.2);
    let page_h = parts
        .iter()
        .filter(|p| p.0.starts_with("slot ") && p.0 != format!("slot {}", pane::NAME))
        .map(|p| p.2)
        .fold(0.0f32, f32::max);
    if let Some(h) = pane_h {
        r.check(
            (h - page_h).abs() < 1.5,
            &format!("{what}: the pane is as tall as the page ({h:.0} vs {page_h:.0})"),
        );
    }
}

/// Sixty tabs: nothing grows past the window.
async fn tabs(r: &Run) {
    builtin::force(Some(false));
    pane::sync(&r.shell);
    for i in 0..60 {
        view::open_tab(
            &r.shell,
            &format!("data:text/html,%3Ctitle%3ETab%20{i}%3C/title%3E%3Cp%3ETab%20{i}"),
        );
        if i % 10 == 9 {
            r.sleep(200).await;
        }
    }
    r.sleep(2500).await;
    ensure_shown(r, true).await;
    r.sleep(600).await;
    fits(r, "60 tabs, pane open");
    let (ok, dump) = placeholder_drawn(r);
    r.check(ok, "60 tabs: the placeholder's words are laid out");
    let centred = dump
        .lines()
        .find(|l| l.contains(&format!("{:?}", pane::EMPTY_TITLE)))
        .and_then(|l| l.split('[').nth(1))
        .and_then(|s| s.split(',').nth(1))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|y| y.parse::<f32>().ok());
    let window = pane::fit_report(&r.shell).map(|f| f.0).unwrap_or(0.0);
    r.check(
        centred.is_some_and(|y| y > 0.0 && y < window),
        &format!("60 tabs: \"No agent here yet\" is on screen (y {centred:?}, window {window:.0})"),
    );
    r.shot("ui-tabs60-placeholder").await;
    builtin::force(Some(true));
    pane::sync(&r.shell);
    r.sleep(300).await;
    if page_ready(r).await {
        fits(r, "60 tabs, agent");
        shot(r, "ui-tabs60-agent").await;
    } else {
        r.check(false, "60 tabs: the agent page loads");
    }
    ensure_shown(r, false).await;
    fits(r, "60 tabs, pane closed");
    builtin::force(None);
}

/// Does the window's picture show the page's latest frame?
async fn stale(r: &Run) {
    builtin::force(Some(true));
    pane::sync(&r.shell);
    ensure_shown(r, true).await;
    if !page_ready(r).await {
        r.check(false, "the agent page loads");
        return;
    }
    for nudge in [false, true] {
        let mut last_window: Option<Vec<u8>> = None;
        let mut last_page: Option<Vec<u8>> = None;
        let (mut window_same, mut page_same) = (0, 0);
        for i in 0..6 {
            let _ = r
                .js(&format!(
                    "window.__agentUi.draft('Probe {i} — {}'); return true;",
                    "x".repeat(i * 7)
                ))
                .await;
            let _ = r
                .js("await new Promise(f => requestAnimationFrame(() => requestAnimationFrame(f))); return true;")
                .await;
            r.sleep(1500).await;
            if nudge {
                pane::nudge(&r.shell, true);
                r.sleep(250).await;
                pane::nudge(&r.shell, false);
                r.sleep(250).await;
            }
            let window =
                copper_shell_core::ShellView::screenshot(r.shell.view()).unwrap_or_default();
            let page = match pane::view() {
                Some(v) => copper_engine::PageView::snapshot(
                    v.page(),
                    copper_engine::SnapshotRegion::Visible,
                )
                .await
                .map(|i| i.png)
                .unwrap_or_default(),
                None => Vec::new(),
            };
            if last_window.as_ref() == Some(&window) {
                window_same += 1;
            }
            if last_page.as_ref() == Some(&page) {
                page_same += 1;
            }
            let _ = std::fs::write(
                r.out.join(format!(
                    "stale-{}-{i}.png",
                    if nudge { "nudged" } else { "plain" }
                )),
                &window,
            );
            last_window = Some(window);
            last_page = Some(page);
        }
        r.line(&format!(
            "STALE nudge={nudge}: of 5 changes of the page, the window picture missed {window_same}, the page's own picture missed {page_same}"
        ));
    }
    ensure_shown(r, false).await;
    builtin::force(None);
}

/// The shared `hello` fixture, as the addons lane stages it.
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

/// A staged cloud addon leaves Copper's agent in the slot.
async fn pending(r: &Run) {
    builtin::force(Some(true));
    match super::glue::selftest_stage(HELLO) {
        Ok(()) => r.check(true, "a cloud addon is staged (not installed)"),
        Err(e) => {
            r.check(false, &format!("stage the hello addon: {e}"));
            return;
        }
    }
    super::changed(&r.shell);
    r.sleep(300).await;
    r.check(
        super::glue::pending().is_some() && super::glue::active().is_none(),
        "it is pending, nothing installed",
    );
    view::open_tab(&r.shell, READING);
    r.sleep(1200).await;
    ensure_shown(r, true).await;
    r.check(
        pane::face() == "agent",
        &format!("the slot keeps showing Copper's agent ({})", pane::face()),
    );
    if page_ready(r).await {
        shot(r, "ui-agent-with-pending-addon").await;
    }
    crate::settings::open(&r.shell, "about");
    r.sleep(600).await;
    r.shot("ui-agent-pending-updates").await;
    r.shell.view().panels().hide(&r.shell);
    ensure_shown(r, false).await;
    builtin::force(None);
}

async fn placeholder(r: &Run) {
    builtin::force(Some(false));
    pane::sync(&r.shell);
    view::open_tab(&r.shell, READING);
    r.sleep(1500).await;
    look(r, Look::Light).await;
    ensure_shown(r, true).await;
    r.check(
        pane::face() == "placeholder",
        "with the built-in off the slot shows the placeholder",
    );
    let (ok, dump) = placeholder_drawn(r);
    r.check(ok, "the placeholder's words are laid out in the pane");
    if !ok {
        r.line(&dump);
    }
    r.shot("ui-placeholder-light").await;
    look(r, Look::Dark).await;
    r.shot("ui-placeholder-dark").await;
    look(r, Look::Light).await;
    // Hidden, Settings opened over the page, shown again.
    ensure_shown(r, false).await;
    crate::settings::open(&r.shell, "agents");
    r.sleep(800).await;
    ensure_shown(r, true).await;
    let (ok, dump) = placeholder_drawn(r);
    r.check(
        ok,
        "after Settings opened and Ctrl+E, the words are still laid out",
    );
    if !ok {
        r.line(&dump);
    }
    r.shot("ui-placeholder-settings").await;
    r.shell.view().panels().hide(&r.shell);
    r.sleep(300).await;
    ensure_shown(r, false).await;
    builtin::force(None);
}

/// The page is up and has drawn (a new `addons` profile takes a while
/// the first time).
async fn page_ready(r: &Run) -> bool {
    let start = std::time::Instant::now();
    let ok = r
        .until_js(
            Duration::from_secs(90),
            "return document.documentElement.getAttribute('data-ready') === '1';",
        )
        .await;
    r.line(&format!(
        "TIME agent page ready: {ok} after {} ms",
        start.elapsed().as_millis()
    ));
    ok
}

async fn agent(r: &Run) {
    builtin::force(Some(true));
    pane::sync(&r.shell);
    view::open_tab(&r.shell, READING);
    r.sleep(1500).await;
    look(r, Look::Light).await;
    ensure_shown(r, true).await;
    r.check(
        pane::face() == "agent",
        &format!(
            "with no cloud addon the slot shows Copper's agent ({})",
            pane::face()
        ),
    );
    let up = page_ready(r).await;
    r.check(up, "the agent page loads from copper-addon://copper.agent/");
    if !up {
        return;
    }
    let url = pane::view().and_then(|v| copper_engine::PageView::url(v.page()));
    r.check(
        url.as_deref()
            .is_some_and(|u| u.starts_with("copper-addon://copper.agent/")),
        &format!("served from the binary ({url:?})"),
    );
    let state = r.js("return await window.copper.agent.state();").await;
    r.line(&format!(
        "copper.agent.state() = {}",
        super::selftest::short(&state)
    ));
    r.check(state.is_ok(), "copper.agent.state() answers the built-in");
    let page = r.js("return await window.copper.page.current();").await;
    r.check(
        page.as_ref()
            .is_ok_and(|p| p.get("title").and_then(Value::as_str) == Some("Field notes")),
        &format!(
            "page.current() names the tab ({})",
            super::selftest::short(&page)
        ),
    );
    r.sleep(600).await;
    shot(r, "ui-agent-real-light").await;
    look(r, Look::Dark).await;
    shot(r, "ui-agent-real-dark").await;
    look(r, Look::Light).await;
    // The built-in's own door: Settings › Intelligence, and the page chip
    // writing chat.json's pageContext.
    let _ = r
        .js("document.getElementById('setup-key').click(); return true;")
        .await;
    let opened = r
        .until(Duration::from_secs(3), || {
            crate::settings::showing(&r.shell) == Some("intelligence")
        })
        .await;
    r.check(opened, "\"Use an API key\" opens Settings › Intelligence");
    r.shell.view().panels().hide(&r.shell);
    r.sleep(400).await;
    let chat = |shell: &crate::platform::Shell| {
        shell
            .store()
            .load::<copper_core::agent::ChatConfig>()
            .map(|l| l.value.page_context())
            .unwrap_or(true)
    };
    let before = chat(&r.shell);
    let _ = r
        .js("document.getElementById('chip').click(); return true;")
        .await;
    let flipped = r
        .until(Duration::from_secs(3), || chat(&r.shell) != before)
        .await;
    let chip = r
        .js("return document.getElementById('chip').classList.contains('off');")
        .await;
    r.check(
        flipped && chip.is_ok_and(|v| v == Value::Bool(before)),
        &format!(
            "the page chip turns page context off in chat.json ({before} → {})",
            chat(&r.shell)
        ),
    );
    let _ = r
        .js("document.getElementById('chip').click(); return true;")
        .await;
    let back = r
        .until(Duration::from_secs(3), || chat(&r.shell) == before)
        .await;
    r.check(back, "and on again");

    // A draft that outlives Ctrl+E.
    let _ = r
        .js("return window.__agentUi.draft('A draft that survives Ctrl+E');")
        .await;
    ctrl_e(r).await;
    r.check(!pane::showing(), "Ctrl+E puts the agent away");
    ctrl_e(r).await;
    r.sleep(500).await;
    let kept = r.js("return window.__agentUi.draft();").await;
    r.check(
        kept.as_ref()
            .is_ok_and(|v| v.as_str() == Some("A draft that survives Ctrl+E")),
        &format!(
            "the draft is still there ({})",
            super::selftest::short(&kept)
        ),
    );
    let _ = r.js("return window.__agentUi.draft('');").await;
    ensure_shown(r, false).await;
}

/// One scripted state of the pane.
async fn scene(r: &Run, name: &str) -> bool {
    let ok = r
        .js(&format!(
            "return !!(window.CopperAgentDemo && window.CopperAgentDemo.current && window.CopperAgentDemo.current.scene({name:?}));"
        ))
        .await
        .is_ok_and(|v| v == Value::Bool(true));
    r.sleep(350).await;
    ok
}

async fn demo(r: &Run) {
    builtin::force(Some(true));
    pane::sync(&r.shell);
    view::open_tab(&r.shell, READING);
    r.sleep(1500).await;
    look(r, Look::Light).await;
    ensure_shown(r, true).await;
    if !page_ready(r).await {
        r.check(false, "the agent page loads");
        return;
    }
    let on = scene(r, "empty").await;
    r.check(
        on,
        "the page plays the scripted conversation (COPPER_AGENT_UI_DEMO=1)",
    );
    if !on {
        return;
    }
    let focused = r.js("return window.__agentUi.focused();").await;
    r.check(
        focused.as_ref().is_ok_and(|v| v.as_str() == Some("input")),
        &format!(
            "the composer has the keyboard ({})",
            super::selftest::short(&focused)
        ),
    );
    shot(r, "ui-agent-empty-light").await;
    look(r, Look::Dark).await;
    shot(r, "ui-agent-empty-dark").await;
    look(r, Look::Light).await;

    scene(r, "unconfigured").await;
    shot(r, "ui-agent-unconfigured-light").await;
    look(r, Look::Dark).await;
    shot(r, "ui-agent-unconfigured-dark").await;
    look(r, Look::Light).await;

    scene(r, "thinking").await;
    let blocks = r.js("return window.__agentUi.blocks();").await;
    r.check(
        blocks.as_ref().is_ok_and(|b| {
            b.as_array().and_then(|a| a.last()).and_then(Value::as_str) == Some("thinking")
        }),
        &format!(
            "asked and nothing back: Thinking… ({})",
            super::selftest::short(&blocks)
        ),
    );
    shot(r, "ui-agent-thinking").await;

    scene(r, "running").await;
    let _ = r.js("window.__agentUi.expand(); return true;").await;
    shot(r, "ui-agent-running").await;

    scene(r, "jev").await;
    let _ = r.js("window.__agentUi.expand(); return true;").await;
    shot(r, "ui-agent-jev-running").await;

    scene(r, "jev-done").await;
    let _ = r.js("window.__agentUi.expand(); return true;").await;
    shot(r, "ui-agent-jev-done").await;

    scene(r, "chat").await;
    shot(r, "ui-agent-chat-light").await;
    look(r, Look::Dark).await;
    shot(r, "ui-agent-chat-dark").await;
    look(r, Look::Light).await;
    let _ = r.js("window.__agentUi.expand(); return true;").await;
    let _ = r.js("window.__agentUi.scrollTop(); return true;").await;
    r.sleep(300).await;
    shot(r, "ui-agent-activity-expanded").await;
    let jump = r
        .js("return !document.getElementById('jump').hidden;")
        .await;
    r.check(
        jump.is_ok_and(|v| v == Value::Bool(true)),
        "scrolled up: Jump to latest shows",
    );
    look(r, Look::Dark).await;
    shot(r, "ui-agent-activity-expanded-dark").await;
    look(r, Look::Light).await;
    let _ = r.js("window.__agentUi.expand(false); return true;").await;
    let _ = r
        .js("document.getElementById('jump').click(); return true;")
        .await;
    r.sleep(600).await;

    let _ = r.js("window.__agentUi.menu(true); return true;").await;
    shot(r, "ui-agent-model-menu").await;
    look(r, Look::Dark).await;
    r.sleep(200).await;
    let _ = r.js("window.__agentUi.menu(true); return true;").await;
    shot(r, "ui-agent-model-menu-dark").await;
    let _ = r.js("window.__agentUi.menu(false); return true;").await;
    look(r, Look::Light).await;

    let _ = r
        .js("return window.__agentUi.draft('Compare the three cheapest nonstops\\nList the departure times\\nand the baggage rules\\nfor each airline\\nthen pick one for Friday');")
        .await;
    shot(r, "ui-agent-composer-multiline").await;
    let jev = r
        .js("return window.__agentUi.draft('/jev nonstop SFO \\u2192 JFK on Friday under $300');")
        .await;
    r.check(
        jev.as_ref()
            .is_ok_and(|v| v.as_str().is_some_and(|s| s.starts_with("/jev "))),
        &format!("a /jev draft ({})", super::selftest::short(&jev)),
    );
    let pressed = r
        .js("return document.getElementById('jev').getAttribute('aria-pressed');")
        .await;
    r.check(
        pressed.is_ok_and(|v| v.as_str() == Some("true")),
        "the /jev button shows it is on",
    );
    shot(r, "ui-agent-composer-jev").await;
    let _ = r.js("return window.__agentUi.draft('');").await;

    let before = pane::width();
    pane::set_width(&r.shell, 340.0);
    r.sleep(500).await;
    let _ = r.js("window.__agentUi.expand(); return true;").await;
    shot(r, "ui-agent-narrow-340").await;
    let wide = r
        .js("const s = document.getElementById('scroller'); return s.scrollWidth <= s.clientWidth && document.documentElement.scrollWidth <= document.documentElement.clientWidth;")
        .await;
    r.check(
        wide.is_ok_and(|v| v == Value::Bool(true)),
        "340 wide: nothing scrolls sideways",
    );
    pane::set_width(&r.shell, 620.0);
    r.sleep(500).await;
    shot(r, "ui-agent-wide-620").await;
    pane::set_width(&r.shell, f64::from(before));
    r.sleep(300).await;
    let _ = r.js("window.__agentUi.expand(false); return true;").await;

    // A question played out: thinking, a Jev run, the answer.
    scene(r, "empty").await;
    let _ = r
        .js("window.__agentUi.draft('Find the release notes for version 2'); document.getElementById('input').dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true})); return true;")
        .await;
    r.sleep(400).await;
    shot(r, "ui-agent-play-thinking").await;
    r.sleep(2400).await;
    let _ = r.js("window.__agentUi.expand(); return true;").await;
    shot(r, "ui-agent-play-running").await;
    let done = r
        .until_js(
            Duration::from_secs(8),
            "return window.__agentUi.blocks().indexOf('answer') >= 0 && !window.__agentUi.state().st.busy;",
        )
        .await;
    r.check(done, "the played question ends in an answer");
    shot(r, "ui-agent-play-done").await;

    // Ctrl+E keeps the draft and gives the composer the keyboard back.
    let _ = r
        .js("return window.__agentUi.draft('Still here after Ctrl+E');")
        .await;
    ctrl_e(r).await;
    ctrl_e(r).await;
    r.sleep(600).await;
    let kept = r.js("return window.__agentUi.draft();").await;
    r.check(
        kept.as_ref()
            .is_ok_and(|v| v.as_str() == Some("Still here after Ctrl+E")),
        "Ctrl+E twice keeps the draft",
    );
    let focused = r.js("return window.__agentUi.focused();").await;
    r.check(
        focused.as_ref().is_ok_and(|v| v.as_str() == Some("input")),
        &format!(
            "and the composer has the keyboard again ({})",
            super::selftest::short(&focused)
        ),
    );
    let _ = r.js("return window.__agentUi.draft('');").await;
    ensure_shown(r, false).await;
}
