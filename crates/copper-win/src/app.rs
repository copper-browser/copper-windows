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

/// Run the browser; the process's exit code.
pub fn run(args: Vec<String>) -> i32 {
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
    let logs = logs_dir();
    init_logging(&logs);
    log_panics();
    if let Err(e) = copper_win_rt::init() {
        sys::dialog::error("Copper could not start", &e.to_string());
        return 1;
    }
    if WebView2Engine::runtime_version().is_none() {
        sys::dialog::webview2_missing("No WebView2 Runtime is installed.");
        return 1;
    }
    let shell = build(opts.clone());
    perf::mark(perf::SESSION);
    copper_shell_core::control::install(&shell);
    let weak = shell.downgrade();
    instance::listen(move |open| {
        if let Some(s) = weak.upgrade() {
            handed(&s, &open);
        }
    });
    open_all(&shell, &open);
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
        let chrome = match Chrome::new("Copper") {
            Ok(c) => c,
            Err(e) => {
                sys::dialog::error("Copper could not open its window", &e.to_string());
                std::process::exit(1);
            }
        };
        WinView::new(chrome)
    });
    shell.view().chrome().attach(&shell);
    crate::actions::register(&shell);
    crate::features::register(&shell);
    shell.start();
    shell
}

/// The engine, in this world's folders. A runtime that cannot start at all
/// is reported and ends the run (the shell cannot do without it).
fn engine(paths: copper_engine::EnginePaths) -> WebView2Engine {
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
    let hwnd = chrome.window().hwnd();
    let maximized = sys::window::load(&place_file())
        .map(|p| sys::window::restore(hwnd, p))
        .unwrap_or(false);
    chrome.window().show_as(maximized);
}

/// What a later start handed over: open its addresses, or just come
/// forward.
fn handed(shell: &Shell, open: &[String]) {
    sys::window::bring_forward(shell.view().chrome().window().hwnd());
    open_all(shell, open);
}

/// Open the command line's addresses, the first in front.
fn open_all(shell: &Shell, open: &[String]) {
    for (i, address) in open.iter().enumerate() {
        let url = args::command_line_url(address).or_else(|| shell.destination(address));
        if let Some(url) = url {
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
    let _ = std::fs::create_dir_all(logs);
    let file = std::fs::File::create(logs.join("copper.log"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);
    let _ = match file {
        Ok(f) => builder.with_writer(std::sync::Mutex::new(f)).try_init(),
        Err(_) => builder.with_writer(std::io::stderr).try_init(),
    };
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
