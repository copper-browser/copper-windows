//! Smoke test for `copper-engine-webview2` on a real Windows machine.
//!
//! ```text
//! smoke [URL] [--backstage] [--snapshot OUT.png] [--data DIR] [--log FILE]
//!       [--scheme] [--lang de-DE,fr] [--bitmap OUT.png] [--file-chooser]
//!       [--input-check] [--profile-check] [--context-menu-check]
//!       [--download-ask-check] [--nav-filter-check] [--world-check] [--sweep LIST]
//!       [--sweep-out DIR] [--profile ID] [--prewarm] [--hold MS] [--exit]
//! ```
//!
//! Opens a plain Win32 window composed with DirectComposition, attaches one
//! page filling its client area and loads `URL` (default
//! `https://example.com`; `copper://test/` with `--scheme`,
//! `https://httpbin.org/headers` with `--lang`). Mouse, wheel, touch/pen and
//! keyboard reach the page; Ctrl+W closes the window.
//!
//! Timings — environment ready, controller ready, first navigation
//! completed, a second page's controller (the warm spare), total — go to
//! stdout and to the log (`%TEMP%\copper-smoke.log`, or `--log FILE`).
//!
//! * `--data DIR`: keep the engine's data (and the WebView2 user data
//!   folder) under `DIR` instead of `%TEMP%\copper-smoke` — a new folder
//!   measures a cold start.
//! * `--snapshot OUT.png`: once the page has loaded, write a snapshot.
//! * `--backstage`: load the page held in the backstage instead (never
//!   shown), prove it renders at full rate (two animation frames, visible
//!   state), print the backstage status, then hand it off to the window —
//!   or, with a check below, snapshot or check it from the backstage.
//! * `--scheme`: serve `copper://test/` (a page, a style sheet, a script
//!   and a `fetch`ed JSON) from a handler registered right after the
//!   engine is made; check the sub-resources loaded in a secure context,
//!   and that a handler error fails the load with the handler's text.
//! * `--lang LIST`: `set_preferred_languages(LIST)` right after the engine
//!   is made; report `navigator.languages` and the `Accept-Language` header
//!   httpbin.org echoes.
//! * `--bitmap OUT.png`: `snapshot_bitmap` (raw pixels) with `view_size`
//!   and `is_mapped`, then PNG-encoded on a worker thread into `OUT.png`.
//! * `--file-chooser`: a file input clicked through `PageView::input`:
//!   `has_file_chooser`, `cancel_file_chooser`, Escape and navigation each
//!   close it; then one more chooser waits 20 s for a pick (type a path),
//!   and a `webkitdirectory` input's chooser 20 s for a folder.
//! * `--input-check`: clicks and typing with trusted input on, off
//!   (`set_trusted_input(false)`: script input) and on again.
//! * `--profile-check`: the profile's cookie jar (set, read through the
//!   API and the page, list, delete) after the warm-spare page closed.
//! * `--context-menu-check`: app-rendered context menus — link, image,
//!   selection and field requests (CDP and real right clicks), Shift+F10,
//!   the Menu key, a reply running `selectAll`, a dropped reply, no handler.
//!   Uses real input: the window must be able to come to the front.
//! * `--download-ask-check`: Save As before a download writes — Save with
//!   the suggested name, Save to a typed path, Cancel, the close box on a
//!   `Profile::download` — then Mac-style free names (`name 2.ext`).
//! * `--nav-filter-check`: the navigation filter — refused `load`, link
//!   click, script and redirect; after a sleep; removed.
//! * `--world-check`: an isolated world's message handler hears every new
//!   document (document start and end), same-site and cross-site.
//! * `--sweep LIST`: a crash sweep — each site of `LIST` (a file of URLs,
//!   or URLs separated by commas) loaded in the page, scrolled, its first
//!   text field typed into and snapshotted into `--sweep-out DIR`
//!   (`%TEMP%\copper-sweep`); one `SWEEP PASS|FAIL` line per site.
//! * `--profile ID`: the profile the page uses (`smoke`); timing runs
//!   compare first-run costs across WebView2 profile names.
//! * `--prewarm`: no page; set the profile up and wait for its cookie store
//!   (an installer's warm-up of a fresh user data folder), then exit.
//! * `--hold MS`: keep a visual moment (a chooser open, the final page) on
//!   screen this long, for screenshots.
//!
//! With any check (or `--snapshot`, or `--exit` for timings only), the run
//! exits when done: status 0 if every check passed, 1 if one failed, 2 if
//! the run timed out.

#[cfg(windows)]
mod checks;
#[cfg(windows)]
mod download_check;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod menu_check;
#[cfg(windows)]
mod nav_check;
#[cfg(windows)]
mod os;
#[cfg(windows)]
mod png;
#[cfg(windows)]
mod run;
#[cfg(windows)]
mod sweep;
#[cfg(windows)]
mod window;
#[cfg(windows)]
mod world_check;

#[cfg(windows)]
fn main() {
    let code = run::main();
    end_process(code);
}

/// End the process with `code` without running thread-local destructors.
///
/// `copper_win_rt` keeps every unfinished task (a warm spare being made, a
/// CDP call waiting for its reply) in a thread-local, which the C runtime
/// destroys during `ExitProcess`, under the loader lock: a pending timer
/// panics there (abort, `0xC0000409`), and a WebView2 object released there
/// can deadlock the exit for good (the process stays, unkillable, with one
/// thread waiting). Until `copper_win_rt::run` drops its tasks before
/// returning, the smoke ends here, after its log is written, so its exit
/// status means something. `COPPER_SMOKE_NATURAL_EXIT=1` exits normally, to
/// show the problem.
#[cfg(windows)]
fn end_process(code: i32) -> ! {
    use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    if std::env::var_os("COPPER_SMOKE_NATURAL_EXIT").is_none() {
        unsafe {
            let _ = TerminateProcess(GetCurrentProcess(), code as u32);
        }
    }
    std::process::exit(code)
}

#[cfg(not(windows))]
fn main() {
    eprintln!("smoke runs on Windows only");
}
