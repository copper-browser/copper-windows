//! The frame clock: a thread that waits on `DwmFlush` and posts one frame
//! message to the window per composition, while there is something to
//! animate.
//!
//! `DwmFlush` blocks until the desktop compositor has presented, so frames
//! land just after a vblank with a whole refresh interval to be ready in —
//! the same rhythm DWM itself keeps. The UI thread never blocks on it, and
//! never pumps messages for it: the clock only posts, and at most one frame
//! message is ever queued.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// The message the clock posts.
pub const WM_FRAME: u32 = WM_APP + 0x0C21;

/// A refresh interval to fall back on when DWM returns at once (the window
/// is minimised, the display is off, or composition is paused).
const FALLBACK: Duration = Duration::from_millis(16);

struct Shared {
    hwnd: AtomicIsize,
    wanted: AtomicBool,
    queued: AtomicBool,
    quit: AtomicBool,
}

/// The clock and its thread.
pub struct Clock {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Clock {
    /// Start a clock that posts [`WM_FRAME`] to `hwnd`.
    pub fn start(hwnd: HWND) -> Clock {
        let shared = Arc::new(Shared {
            hwnd: AtomicIsize::new(hwnd.0 as isize),
            wanted: AtomicBool::new(false),
            queued: AtomicBool::new(false),
            quit: AtomicBool::new(false),
        });
        let s = shared.clone();
        let thread = thread::Builder::new()
            .name("copper-frame-clock".into())
            .spawn(move || run(&s))
            .ok();
        Clock { shared, thread }
    }

    /// Ask for frames until [`Clock::rest`].
    pub fn request(&self) {
        if !self.shared.wanted.swap(true, Ordering::AcqRel)
            && let Some(t) = &self.thread
        {
            t.thread().unpark();
        }
    }

    /// No more frames are needed.
    pub fn rest(&self) {
        self.shared.wanted.store(false, Ordering::Release);
    }

    /// The posted frame message was handled; the next may be posted.
    pub fn handled(&self) {
        self.shared.queued.store(false, Ordering::Release);
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

fn run(s: &Shared) {
    let mut last = Instant::now();
    loop {
        if s.quit.load(Ordering::Acquire) {
            return;
        }
        if !s.wanted.load(Ordering::Acquire) {
            thread::park_timeout(Duration::from_millis(500));
            continue;
        }
        let start = Instant::now();
        // SAFETY: no arguments; returns when DWM has composed a frame.
        let flushed = unsafe { DwmFlush().is_ok() };
        if !flushed || start.elapsed() < Duration::from_millis(1) {
            let since = last.elapsed();
            if since < FALLBACK {
                thread::sleep(FALLBACK - since);
            }
        }
        last = Instant::now();
        if !s.queued.swap(true, Ordering::AcqRel) {
            let hwnd = HWND(s.hwnd.load(Ordering::Acquire) as *mut _);
            // SAFETY: posting to a window owned by another thread is allowed;
            // a destroyed window makes the call fail harmlessly.
            let posted = unsafe { PostMessageW(Some(hwnd), WM_FRAME, WPARAM(0), LPARAM(0)) };
            if posted.is_err() {
                s.queued.store(false, Ordering::Release);
            }
        }
    }
}
