//! The addons lane against a real Copper Cloud: `COPPER_SELFTEST=addons-cloud
//! copper.exe` in a throwaway `COPPER_WORLD` that is already linked and
//! signed in (`copper cloud pair <code>` first). Nothing is staged by hand:
//! the addon arrives through the app's own sync, over the network. The
//! Windows twin of Linux's `addons/selftest_cloud.rs`; results go to
//! `selftest.txt` and PNGs in `COPPER_SELFTEST_OUT`.
//!
//! 1. Within 90 s the cloud sync stages `COPPER_E2E_EXPECT_ID` at
//!    `COPPER_E2E_EXPECT_VERSION`; Settings › Updates says "Update
//!    available"; its **Install** runs.
//! 2. Ctrl+E shows the addon page; `copper.info()` reports the version and
//!    `copper.identity()` the signed-in cloud account.
//! 3. `COPPER_E2E_PANE_SCRIPT`, if set: a file holding the body of an async
//!    function run in the addon page, which drives the addon's own UI and
//!    returns `{ok, steps}`; it may `await shot(name)` and `await
//!    sleep(ms)`. Limit `COPPER_E2E_PANE_TIMEOUT` seconds (default 120).
//! 4. `COPPER_E2E_UPDATE_TO`, if set: the lane writes `update-wanted` in
//!    the output folder and waits for the file `COPPER_E2E_UPDATE_READY`
//!    (the harness publishes that version, then makes the file); then
//!    Check now, "Update available", **Install**, and the page reports the
//!    new version — the same process and the same view, no relaunch.
//!    `COPPER_E2E_UPDATE_PANE_SCRIPT` then runs like step 3.
//! 5. Settings › Addons lists it.

use super::selftest::{READING, Run, short};
use super::{glue, view};
use crate::chrome::pane;
use crate::platform::Shell;
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The `COPPER_SELFTEST` value that starts this lane.
const LANE: &str = "addons-cloud";
/// How long the cloud sync may take to stage a version.
const STAGE_LIMIT: Duration = Duration::from_secs(90);

/// Start when `COPPER_SELFTEST=addons-cloud`.
pub fn maybe_start(shell: &Shell) {
    if std::env::var("COPPER_SELFTEST").ok().as_deref() != Some(LANE) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(1500)).await;
        let Some(shell) = weak.upgrade() else { return };
        let r = Run::new(&shell);
        run(&r).await;
        let failures = r.failures.borrow().len();
        r.line(&format!("addons cloud e2e: {failures} failure(s)"));
        r.flush();
        crate::app::quit(&shell);
    });
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn secs(name: &str, default: u64) -> Duration {
    Duration::from_secs(env(name).and_then(|v| v.parse().ok()).unwrap_or(default))
}

/// A file name part from a name a script asked for.
fn clean(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(48)
        .collect();
    if s.is_empty() { "shot".into() } else { s }
}

async fn run(r: &Run) {
    let shell = &r.shell;
    let (Some(id), Some(version)) = (
        env("COPPER_E2E_EXPECT_ID"),
        env("COPPER_E2E_EXPECT_VERSION"),
    ) else {
        r.check(
            false,
            "COPPER_E2E_EXPECT_ID and COPPER_E2E_EXPECT_VERSION are set",
        );
        return;
    };

    // 0. The world is linked and signed in.
    let linked = r
        .until(Duration::from_secs(20), || glue::status().linked)
        .await;
    r.check(
        linked,
        &format!("the world is linked and signed in ({:?})", glue::status()),
    );
    if !linked {
        return;
    }
    view::open_tab(shell, READING);
    r.sleep(1500).await;

    // 1. The cloud sync stages the expected version, on its own.
    let started = Instant::now();
    let ok = staged(r, &id, &version).await;
    r.check(
        ok,
        &format!(
            "the cloud sync staged {id} {version} ({:.1} s; {:?}; pending {:?})",
            started.elapsed().as_secs_f32(),
            glue::status(),
            glue::pending()
        ),
    );
    if !ok || !install_from_updates(r, &id, &version, "cloud-update-available").await {
        return;
    }

    // 2. Ctrl+E, and the page.
    if pane::showing() {
        pane::hide(shell);
        r.sleep(300).await;
    }
    crate::commands::run(shell, "agent");
    r.sleep(300).await;
    r.check(pane::showing(), "Ctrl+E shows the slot");
    r.check(pane::face() == "addon", "the slot shows the addon");
    let loaded = r
        .until_js(
            Duration::from_secs(40),
            &format!(
                "return location.href.startsWith({}) && document.readyState === 'complete' && !!window.copper;",
                json!(format!("copper-addon://{id}/"))
            ),
        )
        .await;
    r.check(loaded, &format!("the page loads from copper-addon://{id}/"));
    let info = r.js("return await copper.info();").await;
    r.check(
        info.as_ref().is_ok_and(|i| {
            i["platform"] == json!("windows")
                && i["slot"] == json!("agent-sidebar")
                && i["addon"]["id"] == json!(id)
                && i["addon"]["version"] == json!(version)
        }),
        &format!("info() reports {id} {version} {}", short(&info)),
    );
    let identity = r.js("return await copper.identity();").await;
    let email = env("COPPER_E2E_EXPECT_EMAIL").unwrap_or_default();
    r.check(
        identity.as_ref().is_ok_and(|i| {
            i["email"].is_string() && (email.is_empty() || i["email"] == json!(email))
        }),
        &format!(
            "identity() is the cloud account ({})",
            identity
                .as_ref()
                .map(|i| if i["email"].is_string() {
                    "an account".to_string()
                } else {
                    i.to_string()
                })
                .unwrap_or_else(|e| e.clone())
        ),
    );
    r.shot("cloud-pane").await;

    // 3. The addon's own flow.
    if let Some(script) = env("COPPER_E2E_PANE_SCRIPT") {
        pane_script(r, &script, "cloud-pane-script").await;
    }

    // 4. A newer version, while this one runs.
    if let Some(next) = env("COPPER_E2E_UPDATE_TO") {
        update(r, &id, &next).await;
    }

    // 5. Settings › Addons.
    let want = env("COPPER_E2E_UPDATE_TO").unwrap_or(version);
    let listed = glue::list();
    r.check(
        listed
            .iter()
            .any(|l| l.id == id && l.version == want && l.installed && l.enabled),
        &format!("Settings › Addons lists {id} {want}, installed and on"),
    );
    crate::settings::open(shell, "addons");
    r.shot("cloud-settings-addons").await;
    shell.set_look(copper_core::settings::Look::Dark);
    r.shot("cloud-settings-addons-dark").await;
    shell.set_look(copper_core::settings::Look::Light);
    copper_shell_core::ShellView::hide_panel(shell.view());
    r.sleep(300).await;
}

/// Wait for the sync to stage `id` at `version`.
async fn staged(r: &Run, id: &str, version: &str) -> bool {
    r.until(STAGE_LIMIT, || {
        glue::pending().is_some_and(|p| p.id == id && p.version == version)
    })
    .await
}

/// Settings › Updates: screenshot its "Update available", press Install,
/// and wait for `id` `version` to run.
async fn install_from_updates(r: &Run, id: &str, version: &str, shot: &str) -> bool {
    let shell = &r.shell;
    let Some(p) = glue::pending() else {
        r.check(false, "an update is pending");
        return false;
    };
    crate::settings::open(shell, "updates");
    r.check(
        p.version == version,
        &format!(
            "Settings › Updates says “Update available — {}”",
            super::updates::words(&p)
        ),
    );
    r.shot(shot).await;
    super::updates::act(shell, "addon-install");
    let installed = r
        .until(Duration::from_secs(30), || {
            glue::active().is_some_and(|a| a.id == id && a.version == version)
        })
        .await;
    r.check(
        installed,
        &format!("Install puts {id} {version} in the slot (no relaunch)"),
    );
    r.check(
        glue::pending().is_none(),
        "nothing is pending after Install",
    );
    copper_shell_core::ShellView::hide_panel(shell.view());
    r.sleep(300).await;
    installed
}

/// Run the script at `path` in the addon page and wait for its answer.
async fn pane_script(r: &Run, path: &str, shot: &str) {
    let body = match std::fs::read_to_string(path) {
        Ok(b) => b,
        Err(e) => {
            r.check(false, &format!("read the pane script {path}: {e}"));
            return;
        }
    };
    let start = format!(
        "const e = window.__copperE2E = {{done: false, result: null, want: [], taken: []}};\n\
         const sleep = ms => new Promise(ok => setTimeout(ok, ms));\n\
         const shot = name => new Promise(ok => {{\n\
           name = String(name); e.want.push(name); const t0 = Date.now();\n\
           const t = setInterval(() => {{ if (e.taken.includes(name) || Date.now() - t0 > 20000) {{ clearInterval(t); ok(); }} }}, 100);\n\
         }});\n\
         (async () => {{\n{body}\n}})().then(\n\
           v => {{ e.result = v === undefined ? null : v; e.done = true; }},\n\
           err => {{ e.result = {{ok: false, error: String((err && err.stack) || err)}}; e.done = true; }});\n\
         return true;"
    );
    if let Err(e) = r.js(&start).await {
        r.check(false, &format!("start the pane script: {e}"));
        return;
    }
    let limit = secs("COPPER_E2E_PANE_TIMEOUT", 120);
    let began = Instant::now();
    let mut oauth_shot = false;
    let result = loop {
        if began.elapsed() > limit {
            break None;
        }
        // The sign-in tab, if the script opened one (it may close at once).
        if !oauth_shot && super::oauth::open_tabs() > 0 {
            oauth_shot = true;
            r.shot(&format!("{shot}-oauth-tab")).await;
        }
        let state = r
            .js("const e = window.__copperE2E; if (!e) return null;\
                 return {done: e.done, want: e.want.filter(n => !e.taken.includes(n)), result: e.done ? e.result : null};")
            .await;
        let Ok(state) = state else {
            r.sleep(300).await;
            continue;
        };
        if state.is_null() {
            r.check(false, "the pane script is running (the page reloaded?)");
            return;
        }
        for name in state["want"].as_array().into_iter().flatten() {
            let name = name.as_str().unwrap_or("shot").to_string();
            r.shot(&format!("cloud-pane-{}", clean(&name))).await;
            let _ = r
                .js(&format!(
                    "window.__copperE2E.taken.push({}); return true;",
                    json!(name)
                ))
                .await;
        }
        if state["done"] == json!(true) {
            break Some(state["result"].clone());
        }
        r.sleep(250).await;
    };
    let Some(result) = result else {
        r.check(
            false,
            &format!("the pane script finished within {} s", limit.as_secs()),
        );
        r.shot(shot).await;
        return;
    };
    for step in result["steps"].as_array().into_iter().flatten() {
        match step.as_str() {
            Some(s) => r.line(&format!("  step: {s}")),
            None => r.line(&format!("  step: {step}")),
        }
    }
    let why = result
        .get("error")
        .map(|e| format!(" — {e}"))
        .unwrap_or_default();
    r.check(
        result["ok"] == json!(true),
        &format!(
            "the pane script ({}) passed in {:.1} s{why}",
            PathBuf::from(path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
            began.elapsed().as_secs_f32()
        ),
    );
    r.shot(shot).await;
}

/// Step 4: a newer version arrives while the old one runs.
async fn update(r: &Run, id: &str, next: &str) {
    let shell = &r.shell;
    let Some(ready) = env("COPPER_E2E_UPDATE_READY").map(PathBuf::from) else {
        r.check(
            false,
            "COPPER_E2E_UPDATE_READY is set with COPPER_E2E_UPDATE_TO",
        );
        return;
    };
    let _ = std::fs::write(r.out.join("update-wanted"), next);
    let limit = secs("COPPER_E2E_UPDATE_WAIT", 600);
    let came = r.until(limit, || ready.exists()).await;
    r.check(came, &format!("the harness published {id} {next}"));
    if !came {
        return;
    }
    // Let a check the cloud's own event started finish, in an earlier
    // second, so Check now is seen to run one of its own.
    let _ = r
        .until(Duration::from_secs(30), || !glue::status().checking)
        .await;
    r.sleep(1100).await;
    let early = glue::pending().is_some_and(|p| p.version == next);
    let before = glue::checked_at();
    crate::settings::open(shell, "updates");
    r.sleep(300).await;
    super::updates::act(shell, "addon-check");
    let checked = r
        .until(Duration::from_secs(30), || {
            glue::checked_at() > before && !glue::status().checking
        })
        .await;
    r.check(
        checked || early,
        &format!(
            "Settings › Updates › Check now runs a cloud check{}",
            if early {
                " (the cloud's event stream had already staged the version)"
            } else {
                ""
            }
        ),
    );
    copper_shell_core::ShellView::hide_panel(shell.view());
    r.sleep(300).await;
    let started = Instant::now();
    let ok = staged(r, id, next).await;
    r.check(
        ok,
        &format!(
            "the cloud sync staged {id} {next} ({:.1} s)",
            started.elapsed().as_secs_f32()
        ),
    );
    if !ok {
        return;
    }
    let before = pane::view().map(|v| std::rc::Rc::as_ptr(&v) as usize);
    if !install_from_updates(r, id, next, "cloud-update2-available").await {
        return;
    }
    if !pane::showing() {
        pane::show(shell);
    }
    let after = pane::view().map(|v| std::rc::Rc::as_ptr(&v) as usize);
    r.check(
        before.is_some() && before == after,
        "the same addon view reloads in place",
    );
    let updated = r
        .until_js(
            Duration::from_secs(40),
            &format!(
                "if (!window.copper || document.readyState !== 'complete') return false;\
                 const i = await copper.info(); return i.addon.version === {};",
                json!(next)
            ),
        )
        .await;
    let info = r.js("return await copper.info();").await;
    r.check(
        updated,
        &format!(
            "the page reports {id} {next} without a relaunch {}",
            short(&info)
        ),
    );
    r.shot("cloud-pane-updated").await;
    if let Some(script) = env("COPPER_E2E_UPDATE_PANE_SCRIPT") {
        pane_script(r, &script, "cloud-pane-updated-script").await;
    }
}
