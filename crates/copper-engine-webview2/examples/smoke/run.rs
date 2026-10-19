//! The smoke run: engine and page, timings, then the backstage, snapshot
//! and checks (`checks.rs`).

use std::cell::Cell;
use std::future::{Future, poll_fn};
use std::path::PathBuf;
use std::rc::Rc;
use std::task::Poll;
use std::time::{Duration, Instant};

use copper_engine::{
    Backstage, Engine, EnginePaths, EventStream, JsonMap, JsonValue, LoadState, PageEvent,
    PageOptions, PageView, ProfileId, SnapshotRegion, World,
};
use copper_engine_webview2::{EngineConfig, WebView2Engine, WebView2Page, WebView2Profile};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

use crate::checks;
use crate::log::Log;
use crate::window::Window;

/// How long a run with checks may take before it gives up.
const RUN_LIMIT: Duration = Duration::from_secs(240);
/// Paint time between the load finishing and a snapshot.
pub const SETTLE: Duration = Duration::from_millis(500);
/// How long the backstage frame check may take.
const FRAME_LIMIT: Duration = Duration::from_secs(5);
/// How long one load may take.
const LOAD_LIMIT: Duration = Duration::from_secs(100);

/// Resolves two animation frames later with the visibility state: proof
/// that the page renders (rAF does not run in hidden pages).
const TWO_FRAMES: &str = "return await new Promise(r => \
    requestAnimationFrame(() => requestAnimationFrame(() => r(document.visibilityState))));";

const USAGE: &str = "usage: smoke [URL] [--backstage] [--snapshot OUT.png] [--data DIR] \
    [--log FILE] [--scheme] [--lang de-DE,fr] [--bitmap OUT.png] [--file-chooser] \
    [--input-check] [--profile-check] [--hold MS] [--exit]";

pub struct Args {
    pub url: Option<String>,
    pub backstage: bool,
    pub snapshot: Option<PathBuf>,
    pub data: Option<PathBuf>,
    pub log: Option<PathBuf>,
    pub scheme: bool,
    pub lang: Option<Vec<String>>,
    pub bitmap: Option<PathBuf>,
    pub file_chooser: bool,
    pub input_check: bool,
    pub profile_check: bool,
    pub hold: Duration,
    pub exit: bool,
}

impl Args {
    /// Whether the run checks something and then exits.
    fn exits(&self) -> bool {
        self.exit
            || self.snapshot.is_some()
            || self.scheme
            || self.lang.is_some()
            || self.bitmap.is_some()
            || self.file_chooser
            || self.input_check
            || self.profile_check
    }

    fn first_url(&self) -> String {
        if let Some(u) = &self.url {
            return u.clone();
        }
        if self.scheme {
            checks::SCHEME_HOME.into()
        } else if self.lang.is_some() {
            checks::HEADERS_URL.into()
        } else {
            "https://example.com".into()
        }
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        url: None,
        backstage: false,
        snapshot: None,
        data: None,
        log: None,
        scheme: false,
        lang: None,
        bitmap: None,
        file_chooser: false,
        input_check: false,
        profile_check: false,
        hold: Duration::ZERO,
        exit: false,
    };
    let rest: Vec<String> = std::env::args().skip(1).collect();
    let next = |name: &str, i: &mut usize| -> Result<String, String> {
        *i += 1;
        rest.get(*i).cloned().ok_or(format!("{name} needs a value"))
    };
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].clone();
        match a.as_str() {
            "--backstage" => args.backstage = true,
            "--scheme" => args.scheme = true,
            "--file-chooser" => args.file_chooser = true,
            "--input-check" => args.input_check = true,
            "--profile-check" => args.profile_check = true,
            "--exit" => args.exit = true,
            "--snapshot" => args.snapshot = Some(next(&a, &mut i)?.into()),
            "--data" => args.data = Some(next(&a, &mut i)?.into()),
            "--log" => args.log = Some(next(&a, &mut i)?.into()),
            "--bitmap" => args.bitmap = Some(next(&a, &mut i)?.into()),
            "--lang" => {
                let list = next(&a, &mut i)?;
                args.lang = Some(
                    list.split(',')
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .map(str::to_owned)
                        .collect(),
                );
            }
            "--hold" => {
                let ms: u64 = next(&a, &mut i)?
                    .parse()
                    .map_err(|e| format!("--hold: {e}"))?;
                args.hold = Duration::from_millis(ms);
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            url => args.url = Some(url.to_owned()),
        }
        i += 1;
    }
    Ok(args)
}

thread_local! {
    static EXIT: Cell<i32> = const { Cell::new(0) };
}

/// End the run with `code` (closing the window ends the message loop).
fn finish(window: &Window, code: i32) {
    EXIT.with(|e| e.set(code));
    window.close();
}

/// `fut`, or `None` once `limit` has passed.
pub async fn within<T>(fut: impl Future<Output = T>, limit: Duration) -> Option<T> {
    let mut fut = std::pin::pin!(fut);
    let mut timer = std::pin::pin!(copper_win_rt::sleep(limit));
    poll_fn(|cx| {
        if let Poll::Ready(v) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(v));
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

/// Wait for the current load to end: `Ok(url)` finished, `Err(error)`
/// failed (or timed out). Each load state on the way is logged.
pub async fn wait_load(
    log: &Log,
    events: &mut EventStream<PageEvent>,
) -> Result<Option<String>, String> {
    let wait = async {
        loop {
            match poll_fn(|cx| events.poll_recv(cx)).await {
                Some(PageEvent::Load(LoadState::Finished { url })) => return Ok(url),
                Some(PageEvent::Load(LoadState::Failed { error, .. })) => return Err(error),
                Some(PageEvent::Load(state)) => log.at(&format!("  load {state:?}")),
                Some(_) => {}
                None => return Err("the page's events ended".to_owned()),
            }
        }
    };
    within(wait, LOAD_LIMIT)
        .await
        .unwrap_or_else(|| Err("no load in time".to_owned()))
}

/// Everything a check needs.
pub struct Ctx {
    pub engine: WebView2Engine,
    pub profile: WebView2Profile,
    pub page: WebView2Page,
    pub events: EventStream<PageEvent>,
    pub window: Rc<Window>,
    pub log: Rc<Log>,
    pub hold: Duration,
}

pub fn main() -> i32 {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("smoke: {e}\n{USAGE}");
            return 64;
        }
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    if let Err(e) = copper_win_rt::init() {
        eprintln!("smoke: runtime: {e}");
        return 1;
    }
    let log = Rc::new(Log::new(args.log.clone()));
    log.capture_tracing();
    log.capture_panics();
    log.at(&format!(
        "args {:?}",
        std::env::args().skip(1).collect::<Vec<_>>()
    ));
    match WebView2Engine::runtime_version() {
        Some(v) => log.at(&format!("WebView2 Runtime {v}")),
        None => log.at("no WebView2 Runtime found"),
    }
    match start(args, &log) {
        Ok(()) => {
            copper_win_rt::run();
            EXIT.with(Cell::get)
        }
        Err(e) => {
            log.at(&format!("failed: {e}"));
            1
        }
    }
}

/// Make the engine, window and page, and start the run.
fn start(args: Args, log: &Rc<Log>) -> Result<(), String> {
    let root = args
        .data
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("copper-smoke"));
    log.at(&format!("data under {}", root.display()));
    let config = EngineConfig {
        paths: Some(EnginePaths {
            data_root: root.join("Data"),
            cache_root: root.join("Cache"),
            downloads_dir: root.join("Downloads"),
        }),
        user_data_dir: Some(root.join("WebView2")),
        ..EngineConfig::default()
    };
    let engine = WebView2Engine::new(config).map_err(|e| e.to_string())?;
    log.at("engine created");
    // What a shell does right after making the engine.
    if let Some(langs) = &args.lang {
        engine.set_preferred_languages(langs);
        log.at(&format!("preferred languages {langs:?}"));
    }
    if args.scheme {
        checks::register_scheme(&engine, log)?;
    }
    let window = Rc::new(Window::create("Copper smoke").map_err(|e| format!("window: {e}"))?);
    let id = ProfileId::new("smoke").map_err(|e| e.to_string())?;
    let profile = engine.profile(&id, false).map_err(|e| e.to_string())?;
    let page = engine.new_page(&profile, PageOptions::default());
    if args.backstage {
        engine
            .backstage()
            .hold(&page, &window.hwnd)
            .map_err(|e| format!("backstage: {e}"))?;
    } else {
        window.show_page(&page);
    }
    let events = page.events();
    page.load(&args.first_url());
    log.at("page made, load queued");

    let ctx = Ctx {
        engine,
        profile,
        page,
        events,
        window,
        log: log.clone(),
        hold: args.hold,
    };
    copper_win_rt::spawn_local(async move {
        if args.exits() {
            let window = ctx.window.clone();
            let log = ctx.log.clone();
            match within(run(ctx, args), RUN_LIMIT).await {
                Some(code) => finish(&window, code),
                None => {
                    log.at("timed out");
                    finish(&window, 2);
                }
            }
        } else {
            let window = ctx.window.clone();
            let code = run(ctx, args).await;
            if code != 0 {
                finish(&window, code);
            }
        }
    });
    Ok(())
}

/// The run; the exit status (0: everything passed).
async fn run(mut ctx: Ctx, args: Args) -> i32 {
    let log = ctx.log.clone();
    if let Err(e) = ctx.engine.ready().await {
        log.at(&format!("environment failed: {e}"));
        return 1;
    }
    let in_runtime = ctx
        .engine
        .ready_after()
        .map(|d| format!(" ({} ms creating it)", d.as_millis()));
    log.at(&format!(
        "environment ready{}",
        in_runtime.unwrap_or_default()
    ));
    if let Err(e) = ctx.page.ready().await {
        log.at(&format!("controller failed: {e}"));
        return 1;
    }
    log.at("controller ready");
    match wait_load(&log, &mut ctx.events).await {
        Ok(url) => log.at(&format!(
            "first NavigationCompleted: {}",
            url.unwrap_or_default()
        )),
        Err(e) => log.at(&format!("first NavigationCompleted: FAILED {e}")),
    }
    log.at(&format!(
        "total {:.1} ms (launch to first navigation)",
        log.elapsed().as_secs_f64() * 1000.0
    ));
    log.at(&format!("title {:?}", ctx.page.title().unwrap_or_default()));
    navigation_timing(&ctx).await;
    warm_spare(&ctx).await;

    if args.backstage {
        check_backstage(&ctx).await;
    }
    let mut ok = true;
    if args.scheme {
        ok &= checks::scheme(&mut ctx).await;
    }
    if args.lang.is_some() {
        ok &= checks::languages(&mut ctx).await;
    }
    if let Some(path) = &args.bitmap {
        ok &= checks::bitmap(&mut ctx, path).await;
    }
    if args.input_check {
        ok &= checks::input(&mut ctx).await;
    }
    if args.profile_check {
        ok &= checks::profile(&mut ctx).await;
    }
    if args.file_chooser {
        ok &= checks::file_chooser(&mut ctx).await;
    }
    if let Some(path) = &args.snapshot {
        ok &= snapshot(&ctx, path).await;
    }
    if !args.exits() {
        if args.backstage {
            ctx.engine.backstage().hand_off(&ctx.page);
            ctx.window.show_page(&ctx.page);
            log.at("handed off to the window");
        }
        return 0;
    }
    if !ctx.hold.is_zero() {
        log.at(&format!("holding {} ms", ctx.hold.as_millis()));
        copper_win_rt::sleep(ctx.hold).await;
    }
    log.at(if ok {
        "ALL CHECKS PASSED"
    } else {
        "SOME CHECK FAILED"
    });
    i32::from(!ok)
}

/// Where the first navigation's time went (W3C navigation timing, ms from
/// the navigation's start): DNS, connect, TLS, waiting for the response.
async fn navigation_timing(ctx: &Ctx) {
    const TIMING: &str = "(() => { const n = performance.getEntriesByType('navigation')[0]; \
        if (!n) return 'none'; const r = (v) => Math.round(v); \
        return `redirect ${r(n.redirectEnd)} fetchStart ${r(n.fetchStart)} \
        dns ${r(n.domainLookupStart)}-${r(n.domainLookupEnd)} \
        connect ${r(n.connectStart)}-${r(n.connectEnd)} tls ${r(n.secureConnectionStart)} \
        request ${r(n.requestStart)} response ${r(n.responseStart)}-${r(n.responseEnd)} \
        dcl ${r(n.domContentLoadedEventEnd)} load ${r(n.loadEventEnd)} \
        protocol ${n.nextHopProtocol}`; })()";
    match ctx.page.eval(TIMING, &World::Page).await {
        Ok(JsonValue::String(s)) => ctx.log.at(&format!("navigation timing: {s}")),
        Ok(other) => ctx.log.at(&format!("navigation timing: {other}")),
        Err(e) => ctx.log.at(&format!("navigation timing: {e}")),
    }
}

/// A second page right after the first load: its controller is the
/// profile's warm spare.
async fn warm_spare(ctx: &Ctx) {
    let t = Instant::now();
    let second = ctx.engine.new_page(&ctx.profile, PageOptions::default());
    match second.ready().await {
        Ok(()) => ctx.log.at(&format!(
            "second page controller ready in {:.1} ms (warm spare)",
            t.elapsed().as_secs_f64() * 1000.0
        )),
        Err(e) => ctx.log.at(&format!("second page: {e}")),
    }
}

async fn snapshot(ctx: &Ctx, path: &PathBuf) -> bool {
    copper_win_rt::sleep(SETTLE).await;
    match ctx.page.snapshot(SnapshotRegion::Visible).await {
        Ok(img) => match std::fs::write(path, &img.png) {
            Ok(()) => {
                ctx.log.at(&format!(
                    "snapshot {}x{} -> {}",
                    img.width,
                    img.height,
                    path.display()
                ));
                true
            }
            Err(e) => {
                ctx.log.at(&format!("snapshot write: {e}"));
                false
            }
        },
        Err(e) => {
            ctx.log.at(&format!("snapshot: {e}"));
            false
        }
    }
}

/// The held page renders at full rate (animation frames run, and the page
/// thinks it is visible); print the backstage's status.
async fn check_backstage(ctx: &Ctx) {
    let log = &ctx.log;
    let frames = ctx
        .page
        .call_async(TWO_FRAMES, &JsonMap::new(), &World::Page);
    match within(frames, FRAME_LIMIT).await {
        Some(Ok(JsonValue::String(state))) => {
            log.at(&format!(
                "backstage: two animation frames ran, visibilityState {state}"
            ));
        }
        Some(Ok(other)) => log.at(&format!("backstage: unexpected frame result {other}")),
        Some(Err(e)) => log.at(&format!("backstage: frame check failed: {e}")),
        None => log.at("backstage: NO animation frames within 5 s (the page is throttled)"),
    }
    let status = ctx.engine.backstage().status();
    log.at(&format!(
        "backstage status {}",
        serde_json::to_string(&status).unwrap_or_default()
    ));
}
