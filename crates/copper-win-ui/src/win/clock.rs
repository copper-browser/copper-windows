//! The frame clock: a thread that waits on `DwmFlush` and posts one frame
//! message to the window per composition, while there is something to
//! animate.
//!
//! `DwmFlush` blocks until the desktop compositor has presented, so frames
//! land just after a vblank with a whole refresh interval to be ready in —
//! the same rhythm DWM itself keeps. The UI thread never blocks on it, and
//! never pumps messages for it: the clock only posts, and at most one frame
//! message is ever queued.
//!
//! A change on an idle window does not wait for the clock: the UI thread
//! posts the frame message itself ([`Clock::post_now`]), behind the work
//! already queued, so the frame is drawn as soon as that work is done and
//! shows at the very next composition ([`crate::pace`] decides when).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DWM_TIMING_INFO, DwmFlush, DwmGetCompositionTimingInfo};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::pace::Vsync;

/// The message the clock posts.
pub const WM_FRAME: u32 = WM_APP + 0x0C21;

/// Posted when the hardware device a window waited for is ready (it drew
/// its first frames on WARP); `WPARAM` is how long it took, in ms.
pub const WM_GFX_READY: u32 = WM_APP + 0x0C22;

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

    /// Post the frame message now, from the UI thread, unless one is
    /// already queued: it is handled once the messages before it are, so a
    /// burst of work becomes one frame. `false` if it could not be posted.
    pub fn post_now(&self) -> bool {
        if self.shared.queued.swap(true, Ordering::AcqRel) {
            return true;
        }
        let hwnd = HWND(self.shared.hwnd.load(Ordering::Acquire) as *mut _);
        // SAFETY: posting to our own window.
        let posted = unsafe { PostMessageW(Some(hwnd), WM_FRAME, WPARAM(0), LPARAM(0)) }.is_ok();
        if !posted {
            self.shared.queued.store(false, Ordering::Release);
        }
        posted
    }
}

/// `QueryPerformanceCounter` now.
pub fn qpc() -> i64 {
    let mut t = 0i64;
    // SAFETY: writes one i64.
    unsafe {
        let _ = QueryPerformanceCounter(&mut t);
    }
    t
}

/// `QueryPerformanceCounter` ticks per second.
pub fn qpc_frequency() -> i64 {
    let mut f = 0i64;
    // SAFETY: writes one i64.
    unsafe {
        let _ = QueryPerformanceFrequency(&mut f);
    }
    f.max(1)
}

/// The compositor's blank and refresh period, or `None` while composition
/// is paused (minimised, display off) or the call fails.
pub fn vsync() -> Option<Vsync> {
    let mut info = DWM_TIMING_INFO {
        cbSize: std::mem::size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a sized DWM_TIMING_INFO; the window argument must
    // be null on Windows 8.1 and later (timing is the desktop's).
    unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut info).ok()? };
    let (vblank, period) = (info.qpcVBlank, info.qpcRefreshPeriod);
    (period > 0).then_some(Vsync {
        vblank: vblank as i64,
        period: period as i64,
    })
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
