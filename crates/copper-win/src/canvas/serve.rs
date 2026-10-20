//! `copper://canvas/…` on Windows: the page, its bridge, its tab, and join
//! links (Mac `CanvasTabs`, `CanvasJoinFlow`).
//!
//! * The page. copper-shell-core binds a canvas tab's bridge (its handler
//!   and document-start relays) inside the scheme request, and WebView2
//!   runs scripts added then only from the *next* document — so the first
//!   one could never say `ready` and edits made in it were lost. Here the
//!   first request for a page binds the bridge and answers with a one-line
//!   page that replaces itself; the canvas loads into the document after,
//!   with every script in place.
//! * The bridge's Windows half: a listener in the bridge's world, ahead of
//!   copper-shell-core's relay, takes the page's `share` to the native
//!   Share card and copies `chatRead.mentionIds` to the mentions inbox.
//! * The tab: the same canvas never in two tabs; a canvas tab opened now
//!   goes to the end of Saved; an unknown id says so in a page; the
//!   favicon is the Canvas mark.
//! * Join links (`copper://canvas/join/<token>?cloud=…&n=…`, and the web
//!   form on the linked cloud): previewed and joined, or kept until the
//!   person signs in to that cloud.

use super::{copy, inbox};
use crate::platform::{Shell, Win};
use copper_core::browser::TabId;
use copper_engine::{InjectionTime, LoadState, PageEvent, PageId, PageView, UserScript, World};
use copper_shell_core::canvas::{self as core, JoinLink, host, registry, scheme};
use copper_shell_core::control::ShellEvent;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

/// The Windows half's own handler, in the bridge's world.
const WIN_HANDLER: &str = "canvasWin";

/// The page's relay, in the bridge's world, ahead of copper-shell-core's
/// (on `window`, so before its listener on `document`, which it stops):
///
/// * `share` goes to the native card instead;
/// * `chatRead`'s mention ids are copied to the Windows half, and the
///   message goes on;
/// * everything else goes on to the `canvas` handler. (The engine gives
///   the world its way out in every document and holds what is said
///   before it is there, so nothing waits here.)
///
/// Added after copper-shell-core's relay, so where it runs that one does
/// too (`__copperWinRelay`).
const WIN_RELAY: &str = r#"(() => {
  if (location.protocol !== 'copper:' || location.host !== 'canvas') return;
  const handlers = () => (window.webkit && window.webkit.messageHandlers) || {};
  const pass = (d) => { const h = handlers().canvas; if (h) h.postMessage(d); };
  const win = (m) => { const h = handlers().canvasWin; if (h) h.postMessage(JSON.stringify(m)); };
  Object.defineProperty(globalThis, '__copperWinRelay', { value: true });
  window.addEventListener('copper-canvas-post', (e) => {
    if (location.protocol !== 'copper:' || location.host !== 'canvas') return;
    const d = e.detail;
    if (typeof d !== 'string' || d.length >= 64 * 1024 * 1024) return;
    e.stopImmediatePropagation();
    const kind = /^\{"type":"([A-Za-z]+)"/.exec(d.slice(0, 40));
    const type = kind ? kind[1] : '';
    if (type === 'share') {
      win({ type: 'share' });
      return;
    }
    if (type === 'chatRead') {
      try {
        const m = JSON.parse(d);
        if (m && Array.isArray(m.mentionIds) && m.mentionIds.length) {
          win({ type: 'chatRead', mentionIds: m.mentionIds.filter((x) => typeof x === 'string').slice(0, 200) });
        }
      } catch (_) {}
    }
    pass(d);
  }, true);
})();"#;

/// The page the first request gets: it gives way to the canvas at once.
/// (`copper-again` tells [`check_bridge`] it goes by itself.)
const AGAIN: &str = "<meta name=copper-again><script>setTimeout(() => location.replace(location.href), 150)</script>";

/// What a tab that showed a duplicate gets while it goes.
const ELSEWHERE: &str = "<body></body>";

/// What a join link shows while it is taken.
const OPENING: &str =
    "<body style='font:15px system-ui;padding:3em;opacity:.7'>Opening the canvas…</body>";

/// A passing page in the window's look (Copper's look may not be the
/// system's, which is all `prefers-color-scheme` knows).
fn passing(shell: &Shell, body: &str) -> Vec<u8> {
    let scheme = if shell.is_dark() { "dark" } else { "light" };
    format!("<!doctype html><meta charset=utf-8><meta name=color-scheme content='only {scheme}'><title>Canvas</title>{body}").into_bytes()
}

thread_local! {
    /// Pages whose bridge was bound before their document: they get the canvas.
    static PRIMED: RefCell<HashSet<PageId>> = RefCell::new(HashSet::new());
    /// Pages with the Windows half (handler, listener, events).
    static DRESSED: RefCell<HashSet<PageId>> = RefCell::new(HashSet::new());
    /// Reloads spent on a canvas page that came up without its bridge.
    static RETRIES: RefCell<HashMap<PageId, u8>> = RefCell::new(HashMap::new());
    /// Tabs whose web join link is being taken.
    static JOINING: RefCell<HashSet<TabId>> = RefCell::new(HashSet::new());
}

/// The Canvas mark as the tab's icon: a rounded square with a scribble.
const ICON: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'><rect x='1.5' y='1.5' width='13' height='13' rx='3.2' fill='none' stroke='%23808080' stroke-width='1.4'/><path d='M4.5 10.5c1.2-2.6 2.4-4 3.2-3.4.9.6-.9 3 .2 3.3 1 .3 1.9-2 3.6-3.6' fill='none' stroke='%23808080' stroke-width='1.4' stroke-linecap='round'/></svg>";

/// `canvas.html` with the Canvas mark as its icon (once).
fn page_html() -> &'static [u8] {
    static HTML: OnceLock<Vec<u8>> = OnceLock::new();
    HTML.get_or_init(|| {
        let link = format!("<link rel=\"icon\" href=\"data:image/svg+xml,{ICON}\">");
        let html = core::CANVAS_HTML;
        match html.find("<title>") {
            Some(at) => format!("{}{link}{}", &html[..at], &html[at..]),
            None => format!("{link}{html}"),
        }
        .into_bytes()
    })
}

/// Take `copper://canvas` over from copper-shell-core's handler (after
/// [`core::start`]), and follow the tabs.
pub fn start(shell: &Shell) {
    scheme::register_host::<Win>("canvas", Rc::new(serve));
    let mut events = shell.hub().subscribe();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        loop {
            let e = match events.recv().await {
                Ok(e) => e,
                Err(e) if e.to_string().contains("lagged") => continue,
                Err(_) => break,
            };
            let Some(shell) = weak.upgrade() else { break };
            match e {
                ShellEvent::TabOpened(id) => opened(&shell, &id),
                ShellEvent::TabChanged(id) => web_join(&shell, &id),
                _ => {}
            }
        }
    });
}

/// A tab opened on a canvas goes to the end of Saved.
fn opened(shell: &Shell, id: &TabId) {
    let Some(t) = shell.browser().tab(id).cloned() else {
        return;
    };
    if !t.private && !t.saved && core::id_from_url(&t.url).is_some() {
        shell.set_saved(id, true);
    }
}

/// The scheme handler.
fn serve(shell: &Shell, tab: &TabId, uri: &str) -> scheme::Answer {
    if let Some(link) = JoinLink::parse(uri) {
        let s = shell.clone();
        let t = tab.clone();
        copper_win_rt::spawn_local(async move { join(&s, link, Some(t)).await });
        return Ok((passing(shell, OPENING), "text/html"));
    }
    let Some(id) = core::id_from_url(uri) else {
        return Ok((
            copy::missing_page(uri, shell.is_dark()).into_bytes(),
            "text/html",
        ));
    };
    if registry::entry(&id).is_none() {
        return Ok((
            copy::missing_page(&id, shell.is_dark()).into_bytes(),
            "text/html",
        ));
    }
    // The same canvas already in another tab: that one comes forward.
    let private = shell.browser().tab(tab).is_some_and(|t| t.private);
    if !private && let Some(other) = other_tab(shell, &id, tab) {
        let s = shell.clone();
        let t = tab.clone();
        copper_win_rt::spawn_local(async move { give_way(&s, &t, &other) });
        return Ok((passing(shell, ELSEWHERE), "text/html"));
    }
    let Some(slot) = shell.slot(tab) else {
        host::CanvasHost::attach(shell, tab, &id);
        return Ok((page_html().to_vec(), "text/html"));
    };
    let page = slot.page.clone();
    host::CanvasHost::attach(shell, tab, &id);
    dress(shell, &page);
    if PRIMED.with(|p| p.borrow_mut().insert(page.page_id())) {
        // Bound just now: the scripts are there from the next document.
        return Ok((passing(shell, AGAIN), "text/html"));
    }
    Ok((page_html().to_vec(), "text/html"))
}

/// Another tab, not private, on canvas `id`.
fn other_tab(shell: &Shell, id: &str, not: &TabId) -> Option<TabId> {
    let b = shell.browser();
    let mut hits: Vec<TabId> = b
        .all_tabs()
        .filter(|t| &t.id != not && !t.private && core::id_from_url(&t.url).as_deref() == Some(id))
        .map(|t| t.id.clone())
        .collect();
    hits.sort_by(|x, y| x.0.cmp(&y.0));
    hits.into_iter().next()
}

/// Tab `tab` arrived at a canvas `other` shows: back where it was (or
/// closed), and `other` in front.
fn give_way(shell: &Shell, tab: &TabId, other: &TabId) {
    let in_front = shell.active_id().as_ref() == Some(tab);
    let back = shell.slot(tab).is_some_and(|s| s.page.can_go_back());
    if back {
        if let Some(s) = shell.slot(tab) {
            s.page.back();
        }
    } else {
        shell.close_force(tab);
    }
    if in_front || !back {
        shell.select(other);
    }
}

/// The Windows half of a canvas page's bridge (once per page).
fn dress(shell: &Shell, page: &copper_engine_webview2::WebView2Page) {
    if !DRESSED.with(|d| d.borrow_mut().insert(page.page_id())) {
        return;
    }
    let world = World::isolated(host::HOST_WORLD);
    page.add_message_handler(WIN_HANDLER, &world);
    let mut s = UserScript::new(WIN_RELAY);
    s.injection_time = InjectionTime::DocumentStart;
    s.frames = copper_engine::Frames::TopOnly;
    s.world = world;
    page.add_user_script(&s);
    let mut events = page.events();
    let weak = shell.downgrade();
    let mine = page.page_id();
    copper_win_rt::spawn_local(async move {
        while let Some(ev) = std::future::poll_fn(|cx| events.poll_recv(cx)).await {
            let Some(shell) = weak.upgrade() else { break };
            match ev {
                PageEvent::ScriptMessage { name, body } if name == WIN_HANDLER => {
                    let body = match body {
                        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::Null),
                        other => other,
                    };
                    message(&shell, mine, &body);
                }
                PageEvent::Load(LoadState::Finished { url: Some(url) })
                    if core::id_from_url(&url).is_some_and(|id| registry::entry(&id).is_some()) =>
                {
                    let s = shell.clone();
                    copper_win_rt::spawn_local(async move { check_bridge(&s, mine).await });
                }
                _ => {}
            }
        }
        DRESSED.with(|d| d.borrow_mut().remove(&mine));
        PRIMED.with(|p| p.borrow_mut().remove(&mine));
        RETRIES.with(|r| r.borrow_mut().remove(&mine));
    });
}

/// The page by its id, while a tab has it.
fn page_by_id(shell: &Shell, page: PageId) -> Option<copper_engine_webview2::WebView2Page> {
    shell
        .pages_map()
        .borrow()
        .values()
        .find(|s| s.page.page_id() == page)
        .map(|s| s.page.clone())
}

/// A canvas document came up: when its bridge's scripts weren't registered
/// in time for it (WebView2 registers them asynchronously, so the
/// document after the binding can still miss them), it never says `ready`
/// and its edits never reach the disk — load it again, a few times at most.
async fn check_bridge(shell: &Shell, id: PageId) {
    copper_win_rt::sleep(std::time::Duration::from_millis(60)).await;
    let Some(page) = page_by_id(shell, id) else {
        return;
    };
    let world = World::isolated(host::HOST_WORLD);
    let state = page
        .eval(
            "globalThis.__copperWinRelay === true ? 'live' : document.querySelector('meta[name=copper-again]') ? 'again' : 'bare'",
            &world,
        )
        .await;
    let state = state.ok();
    if state.as_ref().and_then(Value::as_str) == Some("again") {
        // The page that gives way to the canvas: it goes by itself.
        return;
    }
    let live = state.as_ref().and_then(Value::as_str) == Some("live");
    if live {
        RETRIES.with(|r| r.borrow_mut().remove(&id));
        tracing::debug!(target: "copper::canvas", "canvas bridge open");
        return;
    }
    let tries = RETRIES.with(|r| {
        let mut r = r.borrow_mut();
        let n = r.entry(id).or_insert(0);
        *n += 1;
        *n
    });
    if tries > 4 {
        tracing::warn!(target: "copper::canvas", "canvas page came up without its bridge");
        return;
    }
    tracing::debug!(target: "copper::canvas", tries, "canvas bridge missing, loading again");
    copper_win_rt::sleep(std::time::Duration::from_millis(250 * u64::from(tries))).await;
    if let Some(page) = page_by_id(shell, id) {
        page.reload(false);
    }
}

/// The canvas a page shows, by its tab.
fn canvas_of(shell: &Shell, page: PageId) -> Option<String> {
    let tab = shell
        .pages_map()
        .borrow()
        .iter()
        .find(|(_, s)| s.page.page_id() == page)
        .map(|(id, _)| id.clone())?;
    host::for_tab::<Win>(&tab).map(|h| h.canvas_id())
}

fn message(shell: &Shell, page: PageId, body: &Value) {
    match body["type"].as_str() {
        Some("share") => {
            if let Some(id) = canvas_of(shell, page) {
                super::share::show(shell, &id);
            }
        }
        Some("chatRead") => {
            let ids: Vec<String> = body["mentionIds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            inbox::mark_read(shell, &ids, None);
        }
        _ => {}
    }
}

// MARK: join links

fn pending_path(shell: &Shell) -> std::path::PathBuf {
    shell
        .store()
        .paths()
        .data
        .join("canvas")
        .join("pending-join.json")
}

fn keep_pending(shell: &Shell, link: &JoinLink) {
    let body = json!({"token": link.token, "cloud": link.cloud, "name": link.name});
    let _ = copper_core::store::write_atomic(
        &pending_path(shell),
        body.to_string().as_bytes(),
        true,
        false,
    );
}

fn take_pending(shell: &Shell) -> Option<JoinLink> {
    let path = pending_path(shell);
    let v: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let link = JoinLink {
        token: v["token"].as_str()?.to_string(),
        cloud: v["cloud"].as_str()?.to_string(),
        name: v["name"].as_str().map(String::from),
        copper: true,
    };
    // Only for the cloud signed in now.
    let linked = copper_shell_core::cloud::get()?.client.link()?.host();
    if JoinLink::normalize(&linked) != JoinLink::normalize(&link.cloud) {
        return None;
    }
    let _ = std::fs::remove_file(&path);
    Some(link)
}

/// Signed in: a join link kept for this cloud goes on.
pub fn resume_join(shell: &Shell) {
    if let Some(link) = take_pending(shell) {
        let s = shell.clone();
        copper_win_rt::spawn_local(async move { join(&s, link, None).await });
    }
}

/// Open the canvas a join link names (Mac `CanvasJoinFlow.join`). The tab
/// that went to the link closes either way.
pub async fn join(shell: &Shell, link: JoinLink, tab: Option<TabId>) {
    let said = join_here(shell, &link).await;
    if let Some(t) = tab
        && shell.browser().tab(&t).is_some()
    {
        shell.close_force(&t);
    }
    if let Err(text) = said {
        shell.toast(&text);
    }
}

async fn join_here(shell: &Shell, link: &JoinLink) -> Result<(), String> {
    let Some(cloud) = copper_shell_core::cloud::get() else {
        return Err("Copper Cloud is off this run".into());
    };
    let elsewhere = format!(
        "This canvas is on {}. Connect to that Copper Cloud to open it.",
        link.cloud
    );
    let Some(linked) = cloud.client.link() else {
        keep_pending(shell, link);
        crate::settings::open(shell, "cloud");
        return Err(elsewhere);
    };
    if JoinLink::normalize(&linked.host()) != JoinLink::normalize(&link.cloud) {
        crate::settings::open(shell, "cloud");
        return Err(elsewhere);
    }
    if !cloud.client.is_signed_in() {
        keep_pending(shell, link);
        crate::settings::open(shell, "cloud");
        return Err("Sign in to Copper Cloud to open this canvas.".into());
    }
    if cloud.client.supports(copper_cloud::Feature::ShareLinks) == Some(false) {
        let runs = match cloud.client.server_version() {
            Some(v) => format!("runs {v}"),
            None => "runs an older version".into(),
        };
        return Err(format!(
            "This Copper Cloud doesn't support invite links yet ({runs})."
        ));
    }
    core::join::<Win>(shell, link, None).await?;
    // Joined: an invitation to the same canvas is answered by it.
    if let Some(id) = core::active::<Win>(shell) {
        inbox::drop_canvas_invites(shell, &id);
    }
    Ok(())
}

/// A web join link on the linked cloud, reached in a tab: taken as a join.
fn web_join(shell: &Shell, id: &TabId) {
    let Some(url) = shell.browser().tab(id).map(|t| t.url.clone()) else {
        return;
    };
    let Some(link) = same_cloud_web_link(&url) else {
        return;
    };
    if !JOINING.with(|j| j.borrow_mut().insert(id.clone())) {
        return;
    }
    let s = shell.clone();
    let t = id.clone();
    copper_win_rt::spawn_local(async move {
        join(&s, link, Some(t.clone())).await;
        JOINING.with(|j| j.borrow_mut().remove(&t));
    });
}

/// `https://<linked cloud>/join/<token>`, else nothing (another cloud's
/// page loads as a page).
pub fn same_cloud_web_link(url: &str) -> Option<JoinLink> {
    let link = JoinLink::parse(url).filter(|l| !l.copper)?;
    let linked = copper_shell_core::cloud::get()?.client.link()?.host();
    (JoinLink::normalize(&linked) == JoinLink::normalize(&link.cloud)).then_some(link)
}
