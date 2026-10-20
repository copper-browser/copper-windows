//! Frame pacing: when a frame asked for may be drawn at once, and when it
//! waits for the frame clock.
//!
//! The compositor shows the newest committed state at each vertical blank.
//! A change that arrives while the window is idle (a key, a click, a panel
//! opening) is drawn and committed straight away, so it shows at the very
//! next blank instead of the one after (waiting for the clock's
//! `DwmFlush` costs up to a whole refresh before drawing even starts).
//!
//! Otherwise at most one frame is drawn per refresh interval: once
//! something was committed since the last blank, later requests in the
//! same interval go to the clock, which draws once after the next blank —
//! an animation keeps the clock's one-frame-per-composition rhythm, and
//! work arriving on its own (a page's title, a timer) never draws faster
//! than the display shows. The one exception is a key press or a click
//! still waiting for its frame: the user's input is drawn at once even in
//! the middle of an animation (one extra frame per press, so never a busy
//! loop), or it would wait for the blank after next. All times are
//! `QueryPerformanceCounter` ticks.

/// The compositor's rhythm: a vertical blank (past or future) and the
/// refresh period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vsync {
    pub vblank: i64,
    pub period: i64,
}

impl Vsync {
    /// The latest blank at or before `now`.
    pub fn latest(&self, now: i64) -> i64 {
        if self.period <= 0 {
            return self.vblank.min(now);
        }
        let d = now - self.vblank;
        // Floor division, so a blank reported in the future steps back.
        self.vblank + d.div_euclid(self.period) * self.period
    }
}

/// Whether a frame wanted at `now` may be drawn at once: a key press or
/// click waits for it (`input_waiting`), or nothing was committed since
/// the latest blank. Without the compositor's rhythm (it is paused:
/// minimised, display off) a frame is drawn at once when the last commit
/// is at least `fallback_period` old.
pub fn draw_now(
    input_waiting: bool,
    last_commit: Option<i64>,
    vsync: Option<Vsync>,
    now: i64,
    fallback_period: i64,
) -> bool {
    if input_waiting {
        return true;
    }
    let Some(last) = last_commit else {
        return true;
    };
    match vsync {
        Some(v) if v.period > 0 => last < v.latest(now),
        _ => now - last >= fallback_period,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: i64 = 16_667;

    #[test]
    fn the_latest_blank_is_at_or_before_now() {
        let v = Vsync {
            vblank: 100_000,
            period: P,
        };
        assert_eq!(v.latest(100_000), 100_000);
        assert_eq!(v.latest(100_000 + P - 1), 100_000);
        assert_eq!(v.latest(100_000 + P), 100_000 + P);
        // A blank reported ahead of now (some builds give the next one).
        assert_eq!(v.latest(100_000 - 1), 100_000 - P);
        assert_eq!(v.latest(100_000 - P), 100_000 - P);
    }

    #[test]
    fn one_frame_at_once_per_interval() {
        let v = Some(Vsync {
            vblank: 0,
            period: P,
        });
        // Idle since long ago: draw at once.
        assert!(draw_now(false, None, v, 5_000, P));
        assert!(draw_now(false, Some(-3 * P), v, 5_000, P));
        // Committed earlier in this interval: the clock draws the next one.
        assert!(!draw_now(false, Some(1_000), v, 5_000, P));
        assert!(!draw_now(false, Some(0), v, 5_000, P));
        // A blank passed since that commit: at once again.
        assert!(draw_now(false, Some(P - 10), v, P + 10, P));
        // A key press doesn't wait for the next interval.
        assert!(draw_now(true, Some(1_000), v, 5_000, P));
    }

    #[test]
    fn without_the_rhythm_a_refresh_of_rest_draws_at_once() {
        assert!(draw_now(false, Some(0), None, P, P));
        assert!(!draw_now(false, Some(0), None, P - 1, P));
        let paused = Some(Vsync {
            vblank: 0,
            period: 0,
        });
        assert!(!draw_now(false, Some(0), paused, 10, P));
        assert!(draw_now(false, Some(0), paused, P + 1, P));
    }
}
