//! Smoke test for `copper-engine-webview2` on a real Windows machine.
//!
//! ```text
//! smoke [URL] [--backstage] [--snapshot OUT.png]
//! ```
//!
//! Opens a plain Win32 window composed with DirectComposition, attaches one
//! page filling its client area and loads `URL` (default
//! `https://example.com`). Mouse, wheel, touch/pen and keyboard reach the
//! page; Ctrl+W closes the window.
//!
//! Timings — environment ready, controller ready, first navigation
//! completed, total — go to stdout and to `%TEMP%\copper-smoke.log`.
//!
//! * `--snapshot OUT.png`: once the page has loaded, write a snapshot and
//!   exit (status 0, or 2 if nothing loaded within 60 s).
//! * `--backstage`: load the page held in the backstage instead (never
//!   shown), prove it renders at full rate (two animation frames, visible
//!   state), print the backstage status, then hand it off to the window —
//!   or, with `--snapshot`, snapshot it from the backstage and exit.
//!
//! Engine data lives under `%TEMP%\copper-smoke` so a run never touches a
//! real Copper profile.

#[cfg(windows)]
mod log;
#[cfg(windows)]
mod run;
#[cfg(windows)]
mod window;

#[cfg(windows)]
fn main() {
    std::process::exit(run::main());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("smoke runs on Windows only");
}
