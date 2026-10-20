//! The browser's log files in `Data\logs`: `copper.log` for this run, and
//! the run before's kept as `copper.previous.log`, so what led up to a crash
//! is still there to read after Copper is started again.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// This run's log.
pub const LOG: &str = "copper.log";
/// The run before's.
pub const PREVIOUS_LOG: &str = "copper.previous.log";

/// Open a fresh `copper.log` in `logs` (made if missing), the last run's
/// moved aside to `copper.previous.log` first.
///
/// The last run's log is never lost: a rename keeps it whole; if something
/// holds it open without letting it be moved (a viewer that did not share
/// delete access), a copy keeps it; and if neither works this run appends
/// to it instead of starting it over.
pub fn fresh(logs: &Path) -> io::Result<File> {
    let _ = std::fs::create_dir_all(logs);
    let log = logs.join(LOG);
    let previous = logs.join(PREVIOUS_LOG);
    let kept = !log.exists()
        || std::fs::rename(&log, &previous).is_ok()
        || std::fs::copy(&log, &previous).is_ok();
    if kept {
        File::create(&log)
    } else {
        OpenOptions::new().append(true).create(true).open(&log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("copper-logs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_first_run_starts_a_log() {
        let d = dir("first");
        let mut f = fresh(&d).unwrap();
        io::Write::write_all(&mut f, b"one").unwrap();
        assert_eq!(std::fs::read_to_string(d.join(LOG)).unwrap(), "one");
        assert!(!d.join(PREVIOUS_LOG).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_last_run_is_kept_beside_the_new_one() {
        let d = dir("kept");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(LOG), "crashed run").unwrap();
        std::fs::write(d.join(PREVIOUS_LOG), "older run").unwrap();
        let mut f = fresh(&d).unwrap();
        io::Write::write_all(&mut f, b"this run").unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join(PREVIOUS_LOG)).unwrap(),
            "crashed run"
        );
        assert_eq!(std::fs::read_to_string(d.join(LOG)).unwrap(), "this run");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_restart_after_a_crash_keeps_the_crash() {
        let d = dir("restart");
        // Run 1 writes and "crashes" (the file is just left behind).
        io::Write::write_all(&mut fresh(&d).unwrap(), b"run 1 panic").unwrap();
        // Run 2 starts: run 1's log is the previous one.
        io::Write::write_all(&mut fresh(&d).unwrap(), b"run 2").unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join(PREVIOUS_LOG)).unwrap(),
            "run 1 panic"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
