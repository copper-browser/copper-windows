//! Timing log: stdout and `%TEMP%\copper-smoke.log`.

use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Log {
    start: Instant,
    file: RefCell<Option<std::fs::File>>,
}

impl Log {
    pub fn new() -> Self {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(Self::path())
            .ok();
        let log = Log {
            start: Instant::now(),
            file: RefCell::new(file),
        };
        log.line(&format!("--- copper smoke, pid {}", std::process::id()));
        log
    }

    pub fn path() -> PathBuf {
        std::env::temp_dir().join("copper-smoke.log")
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// A message stamped with milliseconds since the run started.
    pub fn at(&self, what: &str) {
        let ms = self.elapsed().as_secs_f64() * 1000.0;
        self.line(&format!("[{ms:8.1} ms] {what}"));
    }

    pub fn line(&self, text: &str) {
        println!("{text}");
        if let Some(f) = &mut *self.file.borrow_mut() {
            let _ = writeln!(f, "{text}");
        }
    }
}
