//! The smoke run: engine and page, timings, then the backstage and
//! snapshot modes.

use std::cell::Cell;
use std::future::{Future, poll_fn};
use std::path::PathBuf;
use std::rc::Rc;
use std::task::Poll;
use std::time::Duration;

use copper_engine::{
    Backstage, Engine, EnginePaths, JsonMap, JsonValue, LoadState, PageEvent, PageOptions,
    PageView, ProfileId, SnapshotRegion, World,
};
use copper_engine_webview2::{EngineConfig, WebView2Engine, WebView2Page};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

use crate::log::Log;
use crate::window::Window;

/// How long `--snapshot` waits for the whole run before giving up.
const RUN_LIMIT: Duration = Duration::from_secs(60);
/// Paint time between the load finishing and the snapshot.
const SETTLE: Duration = Duration::from_millis(500);
/// How long the backstage frame check may take.
const FRAME_LIMIT: Duration = Duration::from_secs(5);

/// Resolves two animation frames later with the visibility state: proof
/// that the page renders (rAF does not run in hidden pages).
const TWO_FRAMES: &str = "return await new Promise(r => \
    requestAnimationFrame(() => requestAnimationFrame(() => r(document.visibilityState))));";

struct Args {
    url: String,
    backstage: bool,
    snapshot: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        url: "https://example.com".into(),
        backstage: false,
        snapshot: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--backstage" => args.backstage = true,
            "--snapshot" => {
                args.snapshot = Some(it.next().ok_or("--snapshot needs a file name")?.into());
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            url => args.url = url.to_owned(),
        }
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
async fn within<T>(fut: impl Future<Output = T>, limit: Duration) -> Option<T> {
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

pub fn main() -> i32 {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("smoke: {e}\nusage: smoke [URL] [--backstage] [--snapshot OUT.png]");
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
    let log = Rc::new(Log::new());
    log.at(&format!(
        "url {}{}{}",
        args.url,
        if args.backstage { ", backstage" } else { "" },
        args.snapshot
            .as_ref()
            .map(|p| format!(", snapshot to {}", p.display()))
            .unwrap_or_default()
    ));
    match WebView2Engine::runtime_version() {
        Some(v) => log.at(&format!("WebView2 Runtime {v}")),
        None => log.at("no WebView2 Runtime found"),
    }
    match start(&args, &log) {
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
fn start(args: &Args, log: &Rc<Log>) -> Result<(), String> {
    let root = std::env::temp_dir().join("copper-smoke");
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
    log.at("engine created (browser process starting)");
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
    page.load(&args.url);
    log.at("page made, load queued");

    let snapshot = args.snapshot.clone();
    let backstage = args.backstage;
    let log = log.clone();
    copper_win_rt::spawn_local(async move {
        let run = run(
            &engine,
            &page,
            events,
            &window,
            &log,
            backstage,
            snapshot.clone(),
        );
        if snapshot.is_some() {
            if within(run, RUN_LIMIT).await.is_none() {
                log.at("timed out");
                finish(&window, 2);
            }
        } else {
            run.await;
        }
    });
    Ok(())
}

async fn run(
    engine: &WebView2Engine,
    page: &WebView2Page,
    mut events: copper_engine::EventStream<PageEvent>,
    window: &Window,
    log: &Log,
    backstage: bool,
    snapshot: Option<PathBuf>,
) {
    if let Err(e) = engine.ready().await {
        log.at(&format!("environment failed: {e}"));
        return finish(window, 1);
    }
    let in_runtime = engine
        .ready_after()
        .map(|d| format!(" ({} ms creating it)", d.as_millis()));
    log.at(&format!(
        "environment ready{}",
        in_runtime.unwrap_or_default()
    ));
    if let Err(e) = page.ready().await {
        log.at(&format!("controller failed: {e}"));
        return finish(window, 1);
    }
    log.at("controller ready");
    loop {
        match poll_fn(|cx| events.poll_recv(cx)).await {
            Some(PageEvent::Load(LoadState::Finished { url })) => {
                log.at(&format!(
                    "first NavigationCompleted: {}",
                    url.unwrap_or_default()
                ));
                break;
            }
            Some(PageEvent::Load(LoadState::Failed { url, error, .. })) => {
                log.at(&format!("first NavigationCompleted: FAILED {url}: {error}"));
                break;
            }
            Some(_) => {}
            None => return finish(window, 1),
        }
    }
    log.at(&format!(
        "total {:.1} ms (launch to first navigation)",
        log.elapsed().as_secs_f64() * 1000.0
    ));
    log.at(&format!("title {:?}", page.title().unwrap_or_default()));

    if backstage {
        check_backstage(engine, page, log).await;
    }
    match snapshot {
        Some(path) => {
            copper_win_rt::sleep(SETTLE).await;
            let code = match page.snapshot(SnapshotRegion::Visible).await {
                Ok(img) => match std::fs::write(&path, &img.png) {
                    Ok(()) => {
                        log.at(&format!(
                            "snapshot {}x{} -> {}",
                            img.width,
                            img.height,
                            path.display()
                        ));
                        0
                    }
                    Err(e) => {
                        log.at(&format!("snapshot write: {e}"));
                        1
                    }
                },
                Err(e) => {
                    log.at(&format!("snapshot: {e}"));
                    1
                }
            };
            finish(window, code);
        }
        None if backstage => {
            engine.backstage().hand_off(page);
            window.show_page(page);
            log.at("handed off to the window");
        }
        None => {}
    }
}

/// The held page renders at full rate (animation frames run, and the page
/// thinks it is visible); print the backstage's status.
async fn check_backstage(engine: &WebView2Engine, page: &WebView2Page, log: &Log) {
    let frames = page.call_async(TWO_FRAMES, &JsonMap::new(), &World::Page);
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
    let status = engine.backstage().status();
    log.at(&format!(
        "backstage status {}",
        serde_json::to_string(&status).unwrap_or_default()
    ));
}
