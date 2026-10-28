//! The browser's start and end (`docs/APP.md`).
//!
//! In order: the taskbar identity; the command line; one browser per world
//! (a second start hands over and leaves); the loop; WebView2 present or a
//! clear message; then the shell — the engine first, so the browser
//! process starts while the window is made — the core commands and the
//! lanes, the session restored and the window shown; then the loop runs
//! until Quit, which saves everything.

use crate::args;
use crate::chrome::Chrome;
use crate::instance::{self, Role};
use crate::perf;
use crate::platform::Shell;
use crate::sys;
use crate::view::WinView;
use copper_engine_webview2::{EngineConfig, WebView2Engine};
use copper_shell_core::OpenTab;
use copper_shell_core::options::Options;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// The taskbar's identity for every Copper window (and the installer's
/// shortcuts).
const APP_ID: &str = "Copper.Browser";

/// The installer's warm-up: `copper --prewarm` ([`prewarm`]).
pub const PREWARM: &str = "--prewarm";

/// How long the warm-up keeps WebView2's browser process up. Its first
/// run in a new user data folder goes on well past the first blank page;
/// cut short (gone ~0.7 s in, once the page is loaded) the first start
/// still took ~6 s to paint, kept up 30 s ~1 s (measured on the lab PC).
const PREWARM_STAY: Duration = Duration::from_secs(30);

/// The custom schemes the browser registers before its first page, in its
/// order: the canvas's (copper-shell-core `canvas::scheme`) and the addons'
/// (`addons::scheme::install`).
const PREWARM_SCHEMES: [&str; 2] = ["copper", copper_core::addons::scheme::SCHEME];

/// What the warm-up asks the cookie store about: any web address makes
/// Chromium open the store (nothing is sent anywhere).
const PREWARM_COOKIE_URL: &str = "https://example.com/";

/// Run the browser; the process's exit code.
pub fn run(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == PREWARM) {
        return prewarm();
    }
    set_app_id();
    let launch = args::split(&args);
    let opts = match Options::parse(launch.options.clone()) {
        Ok(o) => o,
        Err(e) => {
            crate::cli::attach_console();
            eprintln!("copper: {e}\n\n{}", crate::cli::usage());
            return 2;
        }
    };
    let mut open: Vec<String> = opts
        .open
        .iter()
        .map(|a| args::command_line_url(a).unwrap_or_else(|| a.to_string()))
        .collect();
    open.extend(launch.single.clone());
    let _guard = if opts.scripted() {
        None
    } else {
        match instance::claim(&open) {
            Role::Handed => return 0,
            Role::First(g) => Some(g),
        }
    };
    perf::step("claimed");
    // This start opens the window: the GPU device and the font system
    // are made on their own thread meanwhile.
    copper_win_ui::win::prewarm();
    let logs = logs_dir();
    init_logging(&logs);
    log_panics();
    perf::step("log open");
    if let Err(e) = copper_win_rt::init() {
        sys::dialog::error("Copper could not start", &e.to_string());
        return 1;
    }
    if WebView2Engine::runtime_version().is_none() {
        sys::dialog::webview2_missing("No WebView2 Runtime is installed.");
        return 1;
    }
    perf::step("webview2 found");
    let shell = build(opts.clone());
    perf::mark(perf::SESSION);
    copper_shell_core::control::install(&shell);
    let weak = shell.downgrade();
    instance::listen(move |open| {
        if let Some(s) = weak.upgrade() {
            handed(&s, &open);
        }
    });
    shell.view().apply_download_ask(&shell);
    open_all(&shell, &open);
    if crate::chrome::welcome::wanted(&shell, !open.is_empty()) {
        crate::chrome::welcome::show(&shell);
    }
    if let Some(path) = opts.screenshot.clone() {
        screenshot_then_quit(&shell, path);
    }
    watch_first_paint(&shell, logs);
    let code = copper_win_rt::run();
    // The shell (and every page) lives as long as the loop runs; `quit`
    // already wrote everything down.
    drop(shell);
    end(code)
}

/// `copper --prewarm`, which the installer starts, detached, once Copper
/// is in place: make the WebView2 environment and the main profile's first
/// webview exactly as the browser will (the same folders, languages and
/// `copper://` scheme, so a browser started meanwhile shares its browser
/// process), load a blank page, ask the profile for a cookie (so Chromium
/// makes the profile's cookie store now — on a new user data folder that
/// took ~10 s on the lab PC, and the first page the user opens waits for
/// it), keep the browser process up for [`PREWARM_STAY`] and leave — no
/// window, no single-instance claim, no `copper.log` (its own goes to
/// `logs\prewarm.log`). The user's first start then finds the user data
/// folder made and set up, and the runtime's files in memory.
fn prewarm() -> i32 {
    use copper_engine::{Engine, PageOptions, PageView, Profile, ProfileId};
    use copper_shell_core::Platform;
    use std::cell::Cell;
    init_prewarm_log(&logs_dir());
    if copper_win_rt::init().is_err() || WebView2Engine::runtime_version().is_none() {
        // Nothing to warm: the browser explains a missing runtime itself.
        return 0;
    }
    let store = copper_core::store::Store::from_env();
    let loaded = copper_shell_core::session::load(&store);
    let engine = engine(copper_shell_core::session::engine_paths(
        &store,
        &loaded.settings,
    ));
    engine.set_preferred_languages(&crate::platform::Win::preferred_languages());
    // Every scheme the browser registers, in its order (the canvas's, then
    // the addons'): WebView2 shares one browser process only between
    // environments made with the same options, and a browser started while
    // this one still runs gets no webview at all otherwise
    // (ERROR_INVALID_STATE, 0x8007139F, until the warm-up ends).
    let mut scheme = Ok(());
    for name in PREWARM_SCHEMES {
        let r = engine.register_scheme(
            name,
            Box::new(|_| Err("not served while warming up".into())),
        );
        scheme = scheme.and(r);
    }
    let started = std::time::Instant::now();
    let Some(profile) = ProfileId::new("default")
        .ok()
        .and_then(|id| engine.profile(&id, false).ok())
    else {
        return 1;
    };
    let page = engine.new_page(&profile, PageOptions::default());
    let mut events = page.events();
    page.load("about:blank");
    // The cookie store, asked for alongside the page (it does not need one).
    let cookies_ms: Rc<Cell<Option<f64>>> = Rc::default();
    let asked = profile.cookies(PREWARM_COOKIE_URL);
    let noted = cookies_ms.clone();
    copper_win_rt::spawn_local(async move {
        let outcome = match asked.await {
            Ok(_) => "ready".to_owned(),
            Err(e) => format!("failed ({e})"),
        };
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        noted.set(Some(ms));
        tracing::info!(target: "copper::prewarm", "cookie store {outcome} in {ms:.0} ms");
    });
    copper_win_rt::spawn_local(async move {
        use copper_engine::{LoadState, PageEvent};
        let mut ready_ms = None;
        let outcome = loop {
            copper_win_rt::sleep(Duration::from_millis(50)).await;
            let mut end = None;
            while let Some(e) = events.try_next() {
                match e {
                    PageEvent::Load(LoadState::Finished { .. }) => end = Some("ready".to_owned()),
                    PageEvent::Load(LoadState::Failed { error, .. }) => {
                        end = Some(format!("failed ({error})"));
                    }
                    _ => {}
                }
            }
            if let Some(end) = end
                && ready_ms.is_none()
            {
                ready_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
                tracing::info!(
                    target: "copper::prewarm",
                    "page {end} in {:.0} ms (scheme: {})",
                    ready_ms.unwrap_or_default(),
                    if scheme.is_ok() { "same" } else { "missing" },
                );
            }
            if started.elapsed() > PREWARM_STAY {
                let cookies = if cookies_ms.get().is_some() {
                    ""
                } else {
                    ", the cookie store never answered"
                };
                break match ready_ms {
                    Some(_) => format!("done{cookies}"),
                    None => format!(
                        "done, the page never loaded (url {:?}, loading {}, progress {:.2}){cookies}",
                        page.url(),
                        page.is_loading(),
                        page.progress()
                    ),
                };
            }
        };
        tracing::info!(
            target: "copper::prewarm",
            "prewarm {outcome} after {:.0} ms",
            started.elapsed().as_secs_f64() * 1000.0,
        );
        drop(page);
        copper_win_rt::quit(0);
    });
    let code = copper_win_rt::run();
    drop(profile);
    drop(engine);
    end(code)
}

/// End the process now, without running thread-local destructors: the
/// loop's leftover tasks (a pending `sleep`) would otherwise be dropped
/// after the loop's timer table and abort the exit (copper-win-rt).
/// Everything worth keeping is on disk by now.
fn end(code: i32) -> i32 {
    use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    // SAFETY: terminating our own process.
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), code as u32);
    }
    code
}

/// The shell, its window and everything it runs, started.
fn build(opts: Options) -> Shell {
    let shell = Shell::new(opts, engine, |_| {
        perf::step("engine made");
        let chrome = match Chrome::new("Copper") {
            Ok(c) => c,
            Err(e) => {
                sys::dialog::error("Copper could not open its window", &e.to_string());
                std::process::exit(1);
            }
        };
        perf::step("window built");
        WinView::new(chrome)
    });
    shell.view().chrome().attach(&shell);
    crate::actions::register(&shell);
    perf::step("actions");
    crate::features::register(&shell);
    perf::step("features");
    // Every scheme and the languages are set: the browser process can
    // start while the session is restored and the window shown.
    shell.engine().start();
    WinView::listen_downloads(&shell);
    shell.start();
    crate::signins::take_in(&shell);
    crate::freeze::start(&shell);
    shell
}

/// The engine, in this world's folders. A runtime that cannot start at all
/// is reported and ends the run (the shell cannot do without it).
fn engine(paths: copper_engine::EnginePaths) -> WebView2Engine {
    perf::step("session loaded");
    let user_data_dir = paths.data_root.parent().map(|root| root.join("WebView2"));
    let config = EngineConfig {
        paths: Some(paths),
        user_data_dir,
        // The shell draws its own page for a failed load.
        builtin_error_page: false,
        ..EngineConfig::default()
    };
    match WebView2Engine::new(config) {
        Ok(e) => e,
        Err(e) => {
            sys::dialog::webview2_missing(&e.to_string());
            std::process::exit(1);
        }
    }
}

/// `ShellView::present`: put the window where it was last time and show
/// it.
pub fn show_window(_shell: &Shell, chrome: &Rc<Chrome>) {
    perf::step("shell started");
    let hwnd = chrome.window().hwnd();
    let maximized = sys::window::load(&place_file())
        .map(|p| sys::window::restore(hwnd, p))
        .unwrap_or(false);
    chrome.window().show_as(maximized);
    perf::step("shown");
    tracing::debug!(target: "copper::perf", "start-up: {}", perf::steps_line());
}

/// What a later start handed over: open its addresses, or just come
/// forward.
fn handed(shell: &Shell, open: &[String]) {
    sys::window::bring_forward(shell.view().chrome().window().hwnd());
    open_all(shell, open);
}

/// Open the command line's addresses, the first in front.
///
/// As on the Mac (`Browser.arrive` → `open`): a page gets a tab of its own
/// even if the restored session has it open already, but a canvas that
/// already has a tab is that tab (`CanvasTabs.already`), brought forward —
/// never a second tab that would have to give way.
fn open_all(shell: &Shell, open: &[String]) {
    for (i, address) in open.iter().enumerate() {
        if crate::cloud::open_link(shell, address) {
            continue;
        }
        let url = args::command_line_url(address).or_else(|| shell.destination(address));
        if let Some(url) = url {
            if let Some(tab) = canvas_tab(shell, &url) {
                if i == 0 {
                    shell.select(&tab);
                }
                continue;
            }
            shell.open(
                &url,
                OpenTab {
                    foreground: i == 0,
                    ..Default::default()
                },
            );
        }
    }
}

/// The tab already showing the canvas `url` is for, if it is one.
fn canvas_tab(shell: &Shell, url: &str) -> Option<copper_core::browser::TabId> {
    let id = copper_shell_core::canvas::id_from_url(url)?;
    copper_shell_core::canvas::tab_showing::<crate::platform::Win>(shell, &id)
}

/// Quit: remember the window's place, save the session, history and
/// settings, stop the agents' server, close the window and end the loop.
pub fn quit(shell: &Shell) {
    let chrome = shell.view().chrome().clone();
    if let Some(place) = sys::window::place(chrome.window().hwnd()) {
        sys::window::save(&place_file(), place);
    }
    shell.shutdown();
    copper_shell_core::agents::shutdown::<crate::platform::Win>();
    chrome.edit(|ui, _| ui.close());
    // The window's `Closed` ends the loop; if it never comes, this does.
    copper_win_rt::spawn_local(async {
        copper_win_rt::sleep(Duration::from_secs(2)).await;
        copper_win_rt::quit(0);
    });
}

/// `%LOCALAPPDATA%\Copper[-world]\Data`.
fn data_dir() -> PathBuf {
    copper_core::store::Paths::from_env().data
}

fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Where the window's place is remembered (Windows only: the Mac keeps it
/// in its own defaults).
fn place_file() -> PathBuf {
    data_dir().join("window-place")
}

/// Log to `copper.log` in the logs folder (`COPPER_LOG` filters, as on
/// Linux).
fn init_logging(logs: &std::path::Path) {
    let filter = tracing_subscriber::EnvFilter::try_from_env("COPPER_LOG").unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "warn,copper=info,copper::perf=info,copper_shell_core=info,copper_engine_webview2=info",
        )
    });
    // Only the window that won the claim gets here (a launch that hands its
    // address over has returned already). The last run's log is kept beside
    // the new one, so a crash is still readable after the restart.
    let file = crate::logs::fresh(logs);
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);
    let _ = match file {
        Ok(f) => builder.with_writer(std::sync::Mutex::new(f)).try_init(),
        Err(_) => builder.with_writer(std::io::stderr).try_init(),
    };
}

/// The warm-up's log, `logs\prewarm.log` (the last warm-up's; the
/// browser's own `copper.log` is left alone).
fn init_prewarm_log(logs: &std::path::Path) {
    let _ = std::fs::create_dir_all(logs);
    if let Ok(f) = std::fs::File::create(logs.join("prewarm.log")) {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(
                "warn,copper::prewarm=info,copper_engine_webview2=info",
            ))
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(f))
            .try_init();
    }
}

/// Write a panic to the log (where it is found after the fact: the
/// browser has no console) before the default report runs.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(target: "copper::panic", "{info}");
        default(info);
    }));
}

/// `SetCurrentProcessExplicitAppUserModelID`: before any window, so the
/// taskbar groups and pins Copper by its own identity.
fn set_app_id() {
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    let id = sys::wide(APP_ID);
    // SAFETY: a NUL-terminated string that outlives the call.
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(windows::core::PCWSTR(id.as_ptr()));
    }
}

/// `--screenshot <png>`: once the window is drawn and the page loaded (or
/// ten seconds pass), save the window and quit.
fn screenshot_then_quit(shell: &Shell, path: PathBuf) {
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        settle(&weak, Duration::from_secs(10)).await;
        let Some(s) = weak.upgrade() else { return };
        let saved = copper_shell_core::ShellView::screenshot(s.view())
            .ok_or_else(|| "no picture".to_string())
            .and_then(|png| std::fs::write(&path, png).map_err(|e| e.to_string()));
        match saved {
            Ok(()) => tracing::info!("saved {}", path.display()),
            Err(e) => tracing::error!("screenshot failed: {e}"),
        }
        quit(&s);
    });
}

/// Wait until the selected page has loaded (or `limit` passes), then a beat
/// more for its last paint.
async fn settle(weak: &crate::platform::WeakShell, limit: Duration) {
    use copper_engine::PageView;
    let start = std::time::Instant::now();
    loop {
        copper_win_rt::sleep(Duration::from_millis(100)).await;
        let Some(s) = weak.upgrade() else { return };
        let loaded = match s.active_page() {
            Some(p) => !p.is_loading() && p.url().is_some(),
            None => true,
        };
        if loaded || start.elapsed() > limit {
            break;
        }
    }
    copper_win_rt::sleep(Duration::from_millis(400)).await;
}

/// Note when the first page has painted (its first load finished), and
/// write the start-up line.
fn watch_first_paint(shell: &Shell, logs: PathBuf) {
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        use copper_engine::PageView;
        let start = std::time::Instant::now();
        loop {
            copper_win_rt::sleep(Duration::from_millis(16)).await;
            let Some(s) = weak.upgrade() else { return };
            let painted = match s.active_page() {
                Some(p) => p.url().is_some() && !p.is_loading(),
                None => s.active_id().is_none(),
            };
            if painted || start.elapsed() > Duration::from_secs(30) {
                break;
            }
        }
        perf::mark(perf::FIRST_PAINT);
        perf::write_report(logs);
    });
}
