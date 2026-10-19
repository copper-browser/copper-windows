//! Start-up and interaction timings, measured from the process's creation
//! (as Windows recorded it, so the loader's time counts too).
//!
//! The marks the PLAN's budgets are about — window on screen, session
//! restored, first page painted — are each recorded once, logged, and
//! written as one line to `startup.log` in the logs folder when the last of
//! them lands, so a lab run reads them back without a debugger. Later
//! interactions (a tab switch, a key to its frame) are logged as they
//! happen.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Instant;

/// When `main` began: the fallback clock where the process's creation
/// time cannot be read.
static MAIN: OnceLock<Instant> = OnceLock::new();
static MARKS: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());

/// The start-up marks, in the order they normally land.
pub const FIRST_FRAME: &str = "first-frame";
pub const SESSION: &str = "session-restored";
pub const FIRST_PAINT: &str = "first-page-painted";

/// Note the start of `main` (call first thing).
pub fn mark_process_start() {
    MAIN.get_or_init(Instant::now);
}

/// Milliseconds since the process was created.
pub fn since_start_ms() -> f64 {
    #[cfg(windows)]
    if let Some(ms) = sys::since_creation_ms() {
        return ms;
    }
    MAIN.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// Record start-up mark `name` now (once; later calls are ignored) and
/// return the time it was recorded at.
pub fn mark(name: &'static str) -> f64 {
    mark_at(name, since_start_ms())
}

/// Record `name` at `ms` (when the toolkit measured it itself).
pub fn mark_at(name: &'static str, ms: f64) -> f64 {
    let Ok(mut marks) = MARKS.lock() else {
        return ms;
    };
    if let Some((_, at)) = marks.iter().find(|(n, _)| *n == name) {
        return *at;
    }
    marks.push((name, ms));
    tracing::info!(target: "copper::perf", "{name}: {ms:.1} ms from process start");
    ms
}

/// The marks so far.
pub fn marks() -> Vec<(&'static str, f64)> {
    MARKS.lock().map(|m| m.clone()).unwrap_or_default()
}

/// One line for `startup.log`: `first-frame=212.4 session-restored=230.1 …`.
pub fn line(marks: &[(&'static str, f64)]) -> String {
    marks
        .iter()
        .map(|(n, ms)| format!("{n}={ms:.1}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Append the marks to `startup.log` under `logs` (best effort).
pub fn write_report(logs: PathBuf) {
    use std::io::Write;
    let text = line(&marks());
    if text.is_empty() {
        return;
    }
    let _ = std::fs::create_dir_all(&logs);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("startup.log"))
    {
        let _ = writeln!(f, "{stamp} pid={} {text}", std::process::id());
    }
}

#[cfg(windows)]
mod sys {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    fn ticks(f: FILETIME) -> u64 {
        (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
    }

    /// Now minus the process's creation time (100 ns ticks).
    pub fn since_creation_ms() -> Option<f64> {
        let (mut created, mut exited, mut kernel, mut user) = Default::default();
        // SAFETY: queries about this process into locals.
        unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
            .ok()?;
        }
        // SAFETY: a plain clock read.
        let now = unsafe { GetSystemTimePreciseAsFileTime() };
        let d = ticks(now).checked_sub(ticks(created))?;
        Some(d as f64 / 10_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_recorded_once_and_listed_in_order() {
        let a = mark_at("test-a", 10.0);
        assert_eq!(a, 10.0);
        assert_eq!(mark_at("test-a", 99.0), 10.0, "the first one stands");
        mark_at("test-b", 20.5);
        let text = line(&marks());
        assert!(text.contains("test-a=10.0"));
        assert!(text.find("test-a").unwrap() < text.find("test-b=20.5").unwrap());
    }

    #[test]
    fn the_clock_runs() {
        mark_process_start();
        let a = since_start_ms();
        let b = since_start_ms();
        assert!(b >= a && a >= 0.0);
    }
}
