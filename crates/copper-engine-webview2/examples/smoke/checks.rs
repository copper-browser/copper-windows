//! The smoke checks: custom scheme, preferred languages, raw snapshot,
//! script/trusted input, the file chooser and the profile's cookie jar.
//! Each logs what it saw and returns whether it passed.

use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use copper_engine::{
    Backstage, Cookie, Engine, InputEvent, JsonValue, PageView, Profile, SchemeBody, SchemeRequest,
    SchemeResponse, Trust, World,
};
use copper_engine_webview2::WebView2Engine;

use crate::log::Log;
use crate::png;
use crate::run::{Ctx, SETTLE, wait_load};

/// The `--scheme` page.
pub const SCHEME_HOME: &str = "copper://test/";
/// Echoes the request's headers as JSON.
pub const HEADERS_URL: &str = "https://httpbin.org/headers";

const HOME_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8">
<title>copper scheme</title><link rel="stylesheet" href="style.css"></head>
<body><h1>Served by copper://</h1>
<p>Sub-resource script: <b id="r">pending</b></p>
<p>fetch(data.json): <b id="f">pending</b></p>
<p>Secure context: <b id="s">?</b> &middot; crypto.randomUUID: <b id="u">?</b></p>
<p>Origin: <b id="o">?</b></p>
<script src="app.js"></script></body></html>"#;

const STYLE_CSS: &str = r#"body { font: 22px "Segoe UI", sans-serif; margin: 48px;
  background: #fbf3ea; color: #3b2a1a; }
h1 { color: #b05a1e; } b { color: #1e6bb0; }"#;

const APP_JS: &str = r#"const $ = (id) => document.getElementById(id);
$('r').textContent = 'loaded';
$('s').textContent = String(isSecureContext);
$('u').textContent = typeof crypto.randomUUID === 'function' ? crypto.randomUUID().slice(0, 8) : 'missing';
$('o').textContent = location.origin;
window.__smoke = { script: true };
fetch('data.json')
  .then((r) => r.json())
  .then((j) => { $('f').textContent = j.message; window.__smoke.fetched = j.message; })
  .catch((e) => { $('f').textContent = 'failed: ' + e; window.__smoke.fetched = 'failed: ' + e; });"#;

const DATA_JSON: &str = r#"{"message":"hello from data.json"}"#;

/// What `copper://test/<path>` serves.
fn serve(uri: &str) -> SchemeResponse {
    let path = uri
        .strip_prefix("copper://test/")
        .unwrap_or(uri)
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let (data, mime) = match path {
        "" | "index.html" => (HOME_HTML, "text/html; charset=utf-8"),
        "style.css" => (STYLE_CSS, "text/css"),
        "app.js" => (APP_JS, "text/javascript"),
        "data.json" => (DATA_JSON, "application/json"),
        _ => return Err(format!("copper smoke: nothing at {uri}")),
    };
    Ok(SchemeBody {
        data: data.as_bytes().to_vec(),
        mime: mime.to_owned(),
    })
}

/// Register `copper:` twice right after the engine is made: the second
/// handler must replace the first.
pub fn register_scheme(engine: &WebView2Engine, log: &Rc<Log>) -> Result<(), String> {
    let first = |_: &SchemeRequest| -> SchemeResponse {
        Ok(SchemeBody {
            data: b"the first handler (should have been replaced)".to_vec(),
            mime: "text/plain".into(),
        })
    };
    engine
        .register_scheme("copper", Box::new(first))
        .map_err(|e| format!("register_scheme: {e}"))?;
    let l = log.clone();
    engine
        .register_scheme(
            "Copper",
            Box::new(move |req: &SchemeRequest| {
                l.at(&format!(
                    "scheme request {} (page {:?})",
                    req.uri,
                    req.page.map(|p| p.0)
                ));
                serve(&req.uri)
            }),
        )
        .map_err(|e| format!("register_scheme: {e}"))?;
    log.at("registered copper: (twice)");
    Ok(())
}

/// Evaluate `expr` in the page world.
pub async fn js(ctx: &Ctx, expr: &str) -> Result<JsonValue, String> {
    ctx.page
        .eval(expr, &World::Page)
        .await
        .map_err(|e| e.to_string())
}

/// Poll `f` every 50 ms for up to `limit`.
pub async fn until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    loop {
        if f() {
            return true;
        }
        if t.elapsed() >= limit {
            return false;
        }
        copper_win_rt::sleep(Duration::from_millis(50)).await;
    }
}

/// Poll page expression `expr` until it equals `want`.
pub async fn until_js(ctx: &Ctx, expr: &str, want: &JsonValue, limit: Duration) -> bool {
    let t = Instant::now();
    loop {
        if js(ctx, expr).await.ok().as_ref() == Some(want) {
            return true;
        }
        if t.elapsed() >= limit {
            return false;
        }
        copper_win_rt::sleep(Duration::from_millis(50)).await;
    }
}

/// A page-world expression that `JSON.stringify`s, parsed back.
pub async fn js_json(ctx: &Ctx, expr: &str) -> Result<JsonValue, String> {
    match js(ctx, expr).await? {
        JsonValue::String(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        other => Ok(other),
    }
}

pub fn pass(log: &Log, ok: bool, what: &str) -> bool {
    log.at(&format!("{} {what}", if ok { "PASS" } else { "FAIL" }));
    ok
}

/// Keep a visual moment on screen for `--hold`.
pub async fn hold(ctx: &Ctx, what: &str) {
    if !ctx.hold.is_zero() {
        ctx.log.at(&format!(
            "{what}: holding {} ms (screenshot now)",
            ctx.hold.as_millis()
        ));
        copper_win_rt::sleep(ctx.hold).await;
    }
}

// -- --scheme -------------------------------------------------------------------

pub async fn scheme(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    if ctx.page.url().as_deref() != Some(SCHEME_HOME) {
        ctx.page.load(SCHEME_HOME);
        if let Err(e) = wait_load(&ctx.log, &mut ctx.events).await {
            return pass(&log, false, &format!("scheme: {SCHEME_HOME} failed: {e}"));
        }
    }
    let settled = until_js(
        ctx,
        "!!(window.__smoke && window.__smoke.fetched)",
        &JsonValue::Bool(true),
        Duration::from_secs(5),
    )
    .await;
    let summary = js_json(
        ctx,
        "JSON.stringify({title: document.title, script: document.getElementById('r').textContent, \
         fetched: window.__smoke && window.__smoke.fetched, secure: isSecureContext, \
         origin: location.origin, uuid: typeof crypto.randomUUID, \
         color: getComputedStyle(document.body).backgroundColor})",
    )
    .await;
    log.at(&format!("scheme page: settled {settled}, {summary:?}"));
    let s = summary.unwrap_or_default();
    let mut ok = pass(
        &log,
        s["title"] == "copper scheme" && s["script"] == "loaded",
        "scheme: page and sub-resource script served (second handler replaced the first)",
    );
    ok &= pass(
        &log,
        s["fetched"] == "hello from data.json",
        "scheme: fetch() of a copper: URL",
    );
    ok &= pass(
        &log,
        s["secure"] == true && s["uuid"] == "function",
        "scheme: secure context",
    );
    ok &= pass(
        &log,
        s["color"] == "rgb(251, 243, 234)",
        "scheme: style sheet applied",
    );

    // A handler error fails the load with the handler's text.
    ctx.page.load("copper://test/missing");
    let failed = wait_load(&ctx.log, &mut ctx.events).await;
    log.at(&format!("copper://test/missing: {failed:?}"));
    ok &= pass(
        &log,
        matches!(&failed, Err(e) if e.contains("copper smoke: nothing at copper://test/missing")),
        "scheme: handler error fails the load with its text",
    );

    // Late registration: allowed, with a warning (it is not part of the
    // running environment).
    let late = ctx
        .engine
        .register_scheme("copperlate", Box::new(|r: &SchemeRequest| serve(&r.uri)));
    log.at(&format!("late register_scheme(copperlate): {late:?}"));

    ctx.page.load(SCHEME_HOME);
    let back = wait_load(&ctx.log, &mut ctx.events).await;
    log.at(&format!("back to {SCHEME_HOME}: {back:?}"));
    hold(ctx, "scheme page").await;
    ok
}

// -- --lang ---------------------------------------------------------------------

pub async fn languages(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    if ctx.page.url().as_deref() != Some(HEADERS_URL) {
        ctx.page.load(HEADERS_URL);
        if let Err(e) = wait_load(&ctx.log, &mut ctx.events).await {
            return pass(
                &log,
                false,
                &format!("languages: {HEADERS_URL} failed: {e}"),
            );
        }
    }
    let seen = js_json(
        ctx,
        "JSON.stringify({languages: navigator.languages, language: navigator.language, \
         intl: Intl.DateTimeFormat().resolvedOptions().locale, \
         body: document.body ? document.body.innerText : ''})",
    )
    .await;
    let seen = match seen {
        Ok(v) => v,
        Err(e) => return pass(&log, false, &format!("languages: {e}")),
    };
    let header = seen["body"]
        .as_str()
        .and_then(|b| serde_json::from_str::<JsonValue>(b).ok())
        .and_then(|j| j["headers"]["Accept-Language"].as_str().map(str::to_owned));
    log.at(&format!(
        "navigator.languages {} navigator.language {} Intl locale {}",
        seen["languages"], seen["language"], seen["intl"]
    ));
    log.at(&format!("Accept-Language (httpbin echo): {header:?}"));
    let langs: Vec<String> = seen["languages"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let first = langs.first().cloned().unwrap_or_default();
    let mut ok = pass(
        &log,
        first == "de-DE" && langs.iter().any(|l| l == "fr"),
        "languages: navigator.languages",
    );
    ok &= pass(
        &log,
        header
            .as_deref()
            .is_some_and(|h| h.starts_with("de-DE") && h.contains("fr")),
        "languages: Accept-Language",
    );
    hold(ctx, "languages").await;
    ok
}

// -- --bitmap -------------------------------------------------------------------

pub async fn bitmap(ctx: &mut Ctx, path: &Path) -> bool {
    let log = ctx.log.clone();
    copper_win_rt::sleep(SETTLE).await;
    let (vw, vh) = ctx.page.view_size();
    log.at(&format!(
        "view_size {vw}x{vh} (logical px), is_mapped {}",
        ctx.page.is_mapped()
    ));
    let t = Instant::now();
    let bitmap = match ctx.page.snapshot_bitmap().await {
        Ok(b) => b,
        Err(e) => return pass(&log, false, &format!("bitmap: {e}")),
    };
    let took = t.elapsed();
    let translucent = (0..bitmap.height)
        .filter_map(|y| bitmap.row(y))
        .flat_map(|row| row.as_chunks::<4>().0)
        .filter(|px| px[3] != 255)
        .count();
    let scale = if vw > 0 {
        f64::from(bitmap.width) / f64::from(vw)
    } else {
        0.0
    };
    log.at(&format!(
        "snapshot_bitmap {}x{} stride {} ({} bytes) in {:.1} ms; display scale {scale:.2}; \
         {translucent} pixels not opaque",
        bitmap.width,
        bitmap.height,
        bitmap.stride,
        bitmap.rgba.len(),
        took.as_secs_f64() * 1000.0
    ));
    let mut ok = pass(
        &log,
        bitmap.rgba.len() == bitmap.stride * bitmap.height as usize
            && bitmap.stride >= bitmap.width as usize * 4
            && bitmap.width > 0
            && vw > 0
            && (scale - 1.0).abs() < 0.05,
        "bitmap: device-pixel RGBA of the view's size",
    );
    let main_thread = std::thread::current().id();
    match png::encode(bitmap, path.to_owned()).await {
        Ok(e) => {
            log.at(&format!(
                "encoded {} bytes on thread {:?} in {:.1} ms -> {} (UI thread {main_thread:?})",
                e.bytes,
                e.thread,
                e.took.as_secs_f64() * 1000.0,
                path.display()
            ));
        }
        Err(e) => ok &= pass(&log, false, &format!("bitmap: encode: {e}")),
    }
    ok
}

// -- --input-check ----------------------------------------------------------------

const INPUT_HTML: &str = r#"<!doctype html><title>input check</title>
<style>body{font:20px "Segoe UI",sans-serif;margin:40px} button,input{font-size:20px;padding:10px 20px}</style>
<p><button id="b">Click me</button> <input id="t" placeholder="type here"></p>
<pre id="out"></pre>
<script>
window.ic = { clicks: [], keys: [], inputs: [] };
const show = () => { document.getElementById('out').textContent = JSON.stringify(ic); };
document.getElementById('b').addEventListener('click', (e) => { ic.clicks.push(e.isTrusted); show(); });
document.getElementById('t').addEventListener('keydown', (e) => { ic.keys.push(e.isTrusted); show(); });
document.getElementById('t').addEventListener('input', (e) => { ic.inputs.push(e.isTrusted); show(); });
</script>"#;

/// The centre of the element with `id`, in viewport CSS px.
pub async fn centre(ctx: &Ctx, id: &str) -> Option<(f64, f64)> {
    let r = js_json(
        ctx,
        &format!(
            "(() => {{ const r = document.getElementById('{id}').getBoundingClientRect(); \
             return JSON.stringify([r.left + r.width / 2, r.top + r.height / 2]); }})()"
        ),
    )
    .await
    .ok()?;
    Some((r[0].as_f64()?, r[1].as_f64()?))
}

pub async fn act(ctx: &Ctx, ev: InputEvent) -> Result<Trust, String> {
    ctx.page.input(ev).await.map_err(|e| e.to_string())
}

pub async fn input(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    ctx.page.load_html(INPUT_HTML, None);
    if let Err(e) = wait_load(&ctx.log, &mut ctx.events).await {
        return pass(&log, false, &format!("input: page failed: {e}"));
    }
    let (Some(b), Some(t)) = (centre(ctx, "b").await, centre(ctx, "t").await) else {
        return pass(&log, false, "input: no button/field");
    };
    let mut ok = true;
    for (round, trusted) in [(1, true), (2, false), (3, true)] {
        ctx.page.set_trusted_input(trusted);
        log.at(&format!(
            "round {round}: set_trusted_input({trusted}) -> trusted_input {:?}",
            ctx.page.trusted_input()
        ));
        let click = act(ctx, InputEvent::click(b.0, b.1)).await;
        let focus = act(ctx, InputEvent::click(t.0, t.1)).await;
        let typed = act(ctx, InputEvent::Type(format!("{round}x"))).await;
        let state = js_json(
            ctx,
            "JSON.stringify({ic: window.ic, value: document.getElementById('t').value})",
        )
        .await
        .unwrap_or_default();
        log.at(&format!(
            "round {round}: click {click:?}, focus click {focus:?}, type {typed:?}; page {state}"
        ));
        let want = if trusted {
            Trust::Trusted
        } else {
            Trust::Script
        };
        let clicks = state["ic"]["clicks"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        ok &= pass(
            &log,
            click.as_ref().ok() == Some(&want)
                && typed.as_ref().ok() == Some(&want)
                && clicks.last() == Some(&JsonValue::Bool(trusted)),
            &format!("input round {round}: {want:?} and isTrusted {trusted}"),
        );
    }
    let value = js(ctx, "document.getElementById('t').value").await;
    ok &= pass(
        &log,
        value.as_ref().ok() == Some(&JsonValue::String("1x2x3x".into())),
        &format!("input: field value {value:?}"),
    );
    ok
}

// -- --file-chooser -----------------------------------------------------------------

const FILE_HTML: &str = r#"<!doctype html><title>file chooser</title>
<style>body{font:20px "Segoe UI",sans-serif;margin:40px} input{font-size:20px;width:640px;height:80px}</style>
<p>Copper smoke: file chooser (multiple, accept="image/*,.txt")</p>
<input id="f" type="file" multiple accept="image/*,.txt">
<p>Folder (webkitdirectory)</p>
<input id="d" type="file" webkitdirectory>
<p id="s">idle</p>
<script>
window.fc = { cancels: 0, changes: 0, files: [], esc: 0, dchanges: 0, dfiles: [] };
const f = document.getElementById('f');
const d = document.getElementById('d');
const s = document.getElementById('s');
f.addEventListener('cancel', (e) => { fc.cancels++; s.textContent = 'cancel #' + fc.cancels + ' trusted ' + e.isTrusted; });
f.addEventListener('change', () => { fc.changes++; fc.files = [...f.files].map((x) => x.name); s.textContent = 'chose ' + fc.files.join(', '); });
d.addEventListener('change', () => { fc.dchanges++; fc.dfiles = [...d.files].map((x) => x.webkitRelativePath); s.textContent = 'folder ' + fc.dfiles.join(', '); });
document.addEventListener('keydown', (e) => { if (e.key === 'Escape') fc.esc++; });
</script>"#;

/// A held page off screen: its chooser is swallowed (counted as a blocked
/// popup, the page hears `cancel`), and no dialog shows.
async fn chooser_backstaged(ctx: &Ctx, at: (f64, f64)) -> bool {
    let log = &ctx.log;
    let before = ctx.engine.backstage().status().popups_blocked;
    let clicked = act(ctx, InputEvent::click(at.0, at.1)).await;
    let heard = until_js(
        ctx,
        "fc.cancels",
        &JsonValue::from(1),
        Duration::from_secs(3),
    )
    .await;
    let after = ctx.engine.backstage().status().popups_blocked;
    log.at(&format!(
        "backstaged click {clicked:?}: has_file_chooser {}, popups_blocked {before} -> {after}, cancel heard {heard}",
        ctx.page.has_file_chooser()
    ));
    pass(
        log,
        clicked.is_ok() && !ctx.page.has_file_chooser() && after == before + 1 && heard,
        "file chooser: swallowed while held off screen",
    )
}

/// Click the file input and wait for the chooser; how long it took.
async fn open_chooser(ctx: &Ctx, at: (f64, f64)) -> Option<Duration> {
    let t = Instant::now();
    if let Err(e) = act(ctx, InputEvent::click(at.0, at.1)).await {
        ctx.log.at(&format!("click on the file input: {e}"));
        return None;
    }
    let page = ctx.page.clone();
    until(Duration::from_secs(3), || page.has_file_chooser())
        .await
        .then(|| t.elapsed())
}

pub async fn file_chooser(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    ctx.page.load_html(FILE_HTML, None);
    if let Err(e) = wait_load(&ctx.log, &mut ctx.events).await {
        return pass(&log, false, &format!("file chooser: page failed: {e}"));
    }
    let Some(at) = centre(ctx, "f").await else {
        return pass(&log, false, "file chooser: no input");
    };
    if ctx.engine.backstage().status().hosted > 0 {
        return chooser_backstaged(ctx, at).await;
    }
    let mut ok = true;

    // Open, then cancel from the API.
    log.at(&format!(
        "has_file_chooser before: {}",
        ctx.page.has_file_chooser()
    ));
    let opened = open_chooser(ctx, at).await;
    log.at(&format!(
        "click -> has_file_chooser {} ({opened:?})",
        ctx.page.has_file_chooser()
    ));
    ok &= pass(&log, opened.is_some(), "file chooser: click opens it");
    hold(ctx, "file chooser open").await;
    let cancelled = ctx.page.cancel_file_chooser();
    log.at(&format!(
        "cancel_file_chooser() -> {cancelled}; has_file_chooser {}",
        ctx.page.has_file_chooser()
    ));
    let heard = until_js(
        ctx,
        "fc.cancels",
        &JsonValue::from(1),
        Duration::from_secs(3),
    )
    .await;
    ok &= pass(
        &log,
        cancelled && !ctx.page.has_file_chooser() && heard,
        "file chooser: cancel_file_chooser closes it, the page hears cancel",
    );
    ok &= pass(
        &log,
        !ctx.page.cancel_file_chooser(),
        "file chooser: a second cancel finds none",
    );

    // Escape from an agent closes it; with none open it is a plain key.
    let opened = open_chooser(ctx, at).await;
    let esc = act(ctx, InputEvent::key("Escape")).await;
    log.at(&format!(
        "reopened {opened:?}; Escape -> {esc:?}; has_file_chooser {}",
        ctx.page.has_file_chooser()
    ));
    let heard = until_js(
        ctx,
        "fc.cancels",
        &JsonValue::from(2),
        Duration::from_secs(3),
    )
    .await;
    ok &= pass(
        &log,
        opened.is_some() && esc == Ok(Trust::Trusted) && !ctx.page.has_file_chooser() && heard,
        "file chooser: agent Escape closes it",
    );
    let esc = act(ctx, InputEvent::key("Escape")).await;
    let keys = js(ctx, "fc.esc").await;
    ok &= pass(
        &log,
        esc.is_ok() && keys == Ok(JsonValue::from(1)),
        &format!("file chooser: Escape without a chooser reaches the page ({keys:?})"),
    );

    // Navigating closes it.
    let opened = open_chooser(ctx, at).await;
    ctx.page.load_html(FILE_HTML, None);
    let loaded = wait_load(&ctx.log, &mut ctx.events).await;
    log.at(&format!(
        "reopened {opened:?}; navigated {loaded:?}; has_file_chooser {}",
        ctx.page.has_file_chooser()
    ));
    ok &= pass(
        &log,
        opened.is_some() && !ctx.page.has_file_chooser(),
        "file chooser: navigating closes it",
    );

    // A person picks a file (the lab script types a path into the dialog).
    let opened = open_chooser(ctx, at).await;
    log.at(&format!(
        "waiting 20 s for a pick (opened {opened:?}); type a path and press Enter"
    ));
    let picked = until_js(
        ctx,
        "fc.changes",
        &JsonValue::from(1),
        Duration::from_secs(20),
    )
    .await;
    let files = js(ctx, "JSON.stringify(fc.files)").await;
    log.at(&format!(
        "pick: changed {picked}, files {files:?}, has_file_chooser {}",
        ctx.page.has_file_chooser()
    ));
    if !picked {
        ctx.page.cancel_file_chooser();
        log.at("no pick (informational)");
    }

    // A person picks a folder for the `webkitdirectory` input.
    let Some(at) = centre(ctx, "d").await else {
        return pass(&log, false, "file chooser: no folder input");
    };
    let opened = open_chooser(ctx, at).await;
    log.at(&format!(
        "waiting 20 s for a folder pick (opened {opened:?}); type a folder and press Enter"
    ));
    let picked = until_js(
        ctx,
        "fc.dchanges",
        &JsonValue::from(1),
        Duration::from_secs(20),
    )
    .await;
    let files = js(ctx, "JSON.stringify(fc.dfiles)").await;
    log.at(&format!(
        "folder pick: changed {picked}, files {files:?}, has_file_chooser {}",
        ctx.page.has_file_chooser()
    ));
    if !picked {
        ctx.page.cancel_file_chooser();
        log.at("no folder pick (informational)");
    }
    ok
}

// -- --profile-check ------------------------------------------------------------

/// The cookie jar after the warm-spare page (often the profile's first
/// webview) has closed: set, read (API and page), list, delete.
pub async fn profile(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    let Some(url) = ctx.page.url() else {
        return pass(&log, false, "profile: the page has no URL");
    };
    let Some(host) = url
        .split("://")
        .nth(1)
        .and_then(|r| r.split(['/', ':']).next())
        .filter(|h| !h.is_empty())
        .map(str::to_owned)
    else {
        return pass(&log, false, &format!("profile: no host in {url}"));
    };
    let value = format!(
        "v{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis())
    );
    let mut cookie = Cookie::new("copper_smoke", value.clone(), host.clone());
    cookie.secure = url.starts_with("https:");
    let mut ok = true;

    let set = ctx.profile.set_cookie(&cookie).await;
    log.at(&format!(
        "set_cookie copper_smoke={value} for {host}: {set:?}"
    ));
    ok &= pass(&log, set.is_ok(), "profile: set_cookie");

    let read = ctx.profile.cookies(&url).await;
    let found = read.as_ref().is_ok_and(|c| {
        c.iter()
            .any(|c| c.name == "copper_smoke" && c.value == value)
    });
    log.at(&format!("cookies({url}): {read:?}"));
    ok &= pass(&log, found, "profile: cookies(url) returns it");

    let page_sees = js(ctx, "document.cookie").await;
    let seen = page_sees
        .as_ref()
        .ok()
        .and_then(JsonValue::as_str)
        .is_some_and(|c| c.contains(&format!("copper_smoke={value}")));
    log.at(&format!("document.cookie: {page_sees:?}"));
    ok &= pass(&log, seen, "profile: the page sees it");

    let all = ctx.profile.all_cookies().await;
    ok &= pass(
        &log,
        all.as_ref()
            .is_ok_and(|c| c.iter().any(|c| c.name == "copper_smoke")),
        &format!(
            "profile: all_cookies lists it ({})",
            all.as_ref()
                .map_or_else(|e| e.to_string(), |c| c.len().to_string())
        ),
    );

    // Deleting the cookie as given, and as read back (which may differ in
    // its domain form); the jar must not list it once the call resolves.
    let deleted = ctx.profile.delete_cookie(&cookie).await;
    let after = ctx.profile.cookies(&url).await;
    let gone = after
        .as_ref()
        .is_ok_and(|c| !c.iter().any(|c| c.name == "copper_smoke"));
    log.at(&format!(
        "delete_cookie: {deleted:?}; right after: {after:?}"
    ));
    if !gone {
        let t = Instant::now();
        let profile = ctx.profile.clone();
        let mut later = false;
        while t.elapsed() < Duration::from_secs(3) {
            copper_win_rt::sleep(Duration::from_millis(50)).await;
            if profile
                .cookies(&url)
                .await
                .is_ok_and(|c| !c.iter().any(|c| c.name == "copper_smoke"))
            {
                later = true;
                break;
            }
        }
        log.at(&format!(
            "  gone later: {later} after {:.0} ms",
            t.elapsed().as_secs_f64() * 1000.0
        ));
    }
    ok &= pass(
        &log,
        deleted.is_ok() && gone,
        "profile: delete_cookie removes it",
    );
    ok
}
