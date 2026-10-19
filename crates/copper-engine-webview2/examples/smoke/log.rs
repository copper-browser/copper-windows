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
    /// Log to stdout and `path` (default `%TEMP%\copper-smoke.log`).
    pub fn new(path: Option<PathBuf>) -> Self {
        let path = path.unwrap_or_else(Self::default_path);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok();
        let log = Log {
            start: Instant::now(),
            file: RefCell::new(file),
        };
        log.line(&format!("--- copper smoke, pid {}", std::process::id()));
        log
    }

    /// Write a panic's message and place to the log too (the release build
    /// aborts on panic, and its stderr is usually not watched).
    pub fn capture_panics(&self) {
        let Some(file) = self.file.borrow().as_ref().and_then(|f| f.try_clone().ok()) else {
            return;
        };
        let file = std::sync::Mutex::new(file);
        let start = self.start;
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Ok(mut f) = file.lock() {
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                let _ = writeln!(f, "[{ms:8.1} ms] PANIC {info}");
            }
            previous(info);
        }));
    }

    /// Send the engine's `tracing` output (warnings, debug notes) to the
    /// same file: `RUST_LOG` filters it (default: the engine at debug,
    /// everything else at warn).
    pub fn capture_tracing(&self) {
        let Some(file) = self.file.borrow().as_ref().and_then(|f| f.try_clone().ok()) else {
            return;
        };
        let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new("copper_engine_webview2=debug,warn")
        });
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .try_init();
    }

    pub fn default_path() -> PathBuf {
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
