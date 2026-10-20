//! `--nav-filter-check`: the page's navigation filter
//! (`WebView2Page::set_navigation_filter`). example.com URLs with `blocked`
//! in them are refused — from `load`, a user's click, the page's own
//! script, a server redirect and after the webview was rebuilt — and must
//! leave no load events (a redirect: only the end of its load) and no
//! history entry; allowed ones load as usual; removing the filter lets
//! everything through.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use copper_engine::{EventStream, InputEvent, LoadState, PageEvent, PageView};
use copper_engine_webview2::NavigationRequest;

use crate::checks::{act, centre, pass};
use crate::run::{Ctx, wait_load, within};

const ALLOWED: &str = "https://example.com/?ok";
const REFUSED: &str = "https://example.com/?blocked-load";
const LINK_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>filter</title>
<style>body { font: 22px "Segoe UI", sans-serif; margin: 48px; }</style></head><body>
<p><a id="go" href="https://example.com/?blocked-link">a refused link</a></p></body></html>"#;
/// Navigates itself, unasked (a new document has no user gesture).
const SCRIPT_HTML: &str = r#"<!doctype html><title>script</title><p>navigating…</p>
<script>setTimeout(() => { location.href = 'https://example.com/?blocked-script'; }, 300);</script>"#;
/// A server redirect to a refused URL (needs httpbin.org).
const REDIRECT: &str =
    "https://httpbin.org/redirect-to?url=https%3A%2F%2Fexample.com%2F%3Fblocked-redirect";
/// How long to watch for events a refused navigation must not send.
const QUIET: Duration = Duration::from_secs(3);
/// How long a load may take.
const LOAD: Duration = Duration::from_secs(40);

type Asked = Rc<RefCell<Vec<NavigationRequest>>>;

pub async fn nav_filter(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    let asked: Asked = Rc::default();
    let sink = asked.clone();
    ctx.page
        .set_navigation_filter(Some(Box::new(move |r: &NavigationRequest| {
            sink.borrow_mut().push(r.clone());
            !refused(&r.url)
        })));
    let mut ok = true;

    // Allowed: loads as usual.
    ctx.page.load(ALLOWED);
    let loaded = wait(ctx).await;
    ok &= pass(
        &log,
        loaded.is_ok() && last(&asked).is_some_and(|r| r.url == ALLOWED && !r.redirected),
        &format!("nav filter: allowed load finishes ({loaded:?})"),
    );

    // Refused `load`: no events, URL and history unchanged.
    let history = ctx.page.history();
    ctx.page.load(REFUSED);
    let events = quiet(&mut ctx.events).await;
    let r = last(&asked);
    log.at(&format!("refused load: asked {r:?}; events {events:?}"));
    ok &= pass(
        &log,
        r.is_some_and(|r| r.url == REFUSED) && events.is_empty(),
        "nav filter: refused load sends no load events",
    );
    let (url, after) = (ctx.page.url(), ctx.page.history());
    log.at(&format!("url {url:?}; history {history:?} → {after:?}"));
    ok &= pass(
        &log,
        url.as_deref() == Some(ALLOWED) && urls(&after) == urls(&history),
        "nav filter: URL and history unchanged",
    );

    // A user's click on a link (from a `load_html` page, which is allowed).
    ctx.page.load_html(LINK_HTML, None);
    let _ = wait(ctx).await;
    if let Some(at) = centre(ctx, "go").await {
        let _ = act(ctx, InputEvent::click(at.0, at.1)).await;
    }
    let events = quiet(&mut ctx.events).await;
    let r = last(&asked);
    log.at(&format!("refused click: asked {r:?}; events {events:?}"));
    ok &= pass(
        &log,
        r.is_some_and(|r| r.url.ends_with("blocked-link") && r.user_initiated) && events.is_empty(),
        "nav filter: refused link click (user_initiated) sends nothing",
    );

    // The page's own script.
    ctx.page.load_html(SCRIPT_HTML, None);
    let _ = wait(ctx).await;
    let events = quiet(&mut ctx.events).await;
    let r = last(&asked);
    log.at(&format!("refused script: asked {r:?}; events {events:?}"));
    ok &= pass(
        &log,
        r.is_some_and(|r| r.url.ends_with("blocked-script") && !r.user_initiated)
            && events.is_empty(),
        "nav filter: refused script navigation (not user_initiated) sends nothing",
    );

    // A redirect to a refused URL: its load started, so it ends cancelled.
    ctx.page.load(REDIRECT);
    let end = wait(ctx).await;
    let r = last(&asked);
    log.at(&format!("refused redirect: asked {r:?}; load {end:?}"));
    if r.as_ref()
        .is_some_and(|r| r.url.ends_with("blocked-redirect"))
    {
        ok &= pass(
            &log,
            r.is_some_and(|r| r.redirected) && end.is_err(),
            "nav filter: refused redirect (redirected) ends its load (Failed)",
        );
    } else {
        log.at("SKIP nav filter: redirect (httpbin.org did not redirect)");
    }

    // The webview rebuilt: the filter still decides.
    ctx.page.load(ALLOWED);
    let _ = wait(ctx).await;
    ctx.page.detach();
    let slept = ctx.page.sleep();
    ctx.page.load("https://example.com/?blocked-after-sleep");
    let events = quiet(&mut ctx.events).await;
    let r = last(&asked);
    ctx.window.show_page(&ctx.page);
    log.at(&format!(
        "after sleep ({slept}): asked {r:?}; events {events:?}"
    ));
    ok &= pass(
        &log,
        slept
            && r.is_some_and(|r| r.url.ends_with("blocked-after-sleep"))
            && !events.iter().any(|e| {
                matches!(e, LoadState::Started { url: Some(u) } | LoadState::Committed { url: Some(u) } if refused(u))
            }),
        "nav filter: survives the webview being rebuilt",
    );

    // Removed: the same URL loads.
    ctx.page.set_navigation_filter(None);
    let before = asked.borrow().len();
    ctx.page.load(REFUSED);
    let loaded = wait(ctx).await;
    ok &= pass(
        &log,
        loaded.is_ok() && asked.borrow().len() == before,
        &format!("nav filter: None lets it through ({loaded:?})"),
    );
    ok
}

/// The entries' URLs and the current index (titles fill in late).
fn urls(h: &(Vec<copper_engine_webview2::HistoryEntry>, usize)) -> (Vec<&str>, usize) {
    (h.0.iter().map(|e| e.url.as_str()).collect(), h.1)
}

/// What the check's filter refuses: example.com URLs that say so.
fn refused(url: &str) -> bool {
    url.starts_with("https://example.com/") && url.contains("blocked")
}

fn last(asked: &Asked) -> Option<NavigationRequest> {
    asked.borrow().last().cloned()
}

/// The current load's end, or `Err` after [`LOAD`].
async fn wait(ctx: &mut Ctx) -> Result<Option<String>, String> {
    let log = ctx.log.clone();
    within(wait_load(&log, &mut ctx.events), LOAD)
        .await
        .unwrap_or_else(|| Err("timed out".into()))
}

/// The load states the page sends within [`QUIET`].
async fn quiet(events: &mut EventStream<PageEvent>) -> Vec<LoadState> {
    let mut out = Vec::new();
    let t = std::time::Instant::now();
    while t.elapsed() < QUIET {
        while let Some(ev) = events.try_next() {
            if let PageEvent::Load(state) = ev {
                out.push(state);
            }
        }
        copper_win_rt::sleep(Duration::from_millis(50)).await;
    }
    out
}
