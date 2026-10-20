//! Frame timing: a fixed ring of recent frame times and the time to first
//! frame, summarised without allocating.

/// How many recent frames the summary covers.
pub const WINDOW: usize = 240;

/// Recent frame times in milliseconds.
#[derive(Clone, Debug)]
pub struct FrameStats {
    ring: [f32; WINDOW],
    next: usize,
    len: usize,
    total: u64,
    /// Frames since the last [`FrameStats::summary`] that missed a budget.
    slow: u32,
    /// The budget a frame should fit in (one refresh interval).
    pub budget_ms: f32,
}

impl Default for FrameStats {
    fn default() -> FrameStats {
        FrameStats::new()
    }
}

/// A summary of the ring.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    pub frames: u64,
    pub p50: f32,
    pub p95: f32,
    pub max: f32,
    pub slow: u32,
}

impl FrameStats {
    pub fn new() -> FrameStats {
        FrameStats {
            ring: [0.0; WINDOW],
            next: 0,
            len: 0,
            total: 0,
            slow: 0,
            budget_ms: 1000.0 / 60.0,
        }
    }

    /// Record one frame's CPU time.
    pub fn record(&mut self, ms: f32) {
        self.ring[self.next] = ms;
        self.next = (self.next + 1) % WINDOW;
        self.len = (self.len + 1).min(WINDOW);
        self.total += 1;
        if ms > self.budget_ms {
            self.slow += 1;
        }
    }

    pub fn frames(&self) -> u64 {
        self.total
    }

    /// Percentiles of the frames in the ring; resets the slow count.
    pub fn summary(&mut self) -> Summary {
        let mut sorted = [0.0f32; WINDOW];
        let n = self.len;
        sorted[..n].copy_from_slice(&self.ring[..n]);
        let s = &mut sorted[..n];
        s.sort_unstable_by(f32::total_cmp);
        let at = |q: f32| -> f32 {
            if n == 0 {
                0.0
            } else {
                s[((n - 1) as f32 * q).round() as usize]
            }
        };
        let out = Summary {
            frames: self.total,
            p50: at(0.5),
            p95: at(0.95),
            max: if n == 0 { 0.0 } else { s[n - 1] },
            slow: self.slow,
        };
        self.slow = 0;
        out
    }
}

/// The named steps of a one-off sequence (making the window, its first
/// frame), each with the milliseconds it took — so a slow start says where
/// its time went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Phases {
    list: Vec<(&'static str, f32)>,
}

impl Phases {
    pub fn new() -> Phases {
        Phases::default()
    }

    /// Step `name` took `ms`.
    pub fn push(&mut self, name: &'static str, ms: f32) {
        self.list.push((name, ms));
    }

    pub fn list(&self) -> &[(&'static str, f32)] {
        &self.list
    }

    /// The steps' total.
    pub fn total(&self) -> f32 {
        self.list.iter().map(|(_, ms)| ms).sum()
    }

    /// One line for a log: `create 4.1 · gfx 12.0 · …`.
    pub fn line(&self) -> String {
        self.list
            .iter()
            .map(|(n, ms)| format!("{n} {ms:.1}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// A stopwatch that writes each step into [`Phases`] while it is on (off,
/// it costs a branch).
pub struct Lap {
    at: std::time::Instant,
    on: bool,
}

impl Lap {
    pub fn new(on: bool) -> Lap {
        Lap {
            at: std::time::Instant::now(),
            on,
        }
    }

    /// The step since the last mark was `name`.
    pub fn mark(&mut self, phases: &mut Phases, name: &'static str) {
        if self.on {
            let now = std::time::Instant::now();
            phases.push(name, now.duration_since(self.at).as_secs_f32() * 1000.0);
            self.at = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_list_in_order_and_sum() {
        let mut p = Phases::new();
        assert_eq!(p.line(), "");
        p.push("create", 4.04);
        p.push("gfx", 12.0);
        assert_eq!(p.line(), "create 4.0 · gfx 12.0");
        assert_eq!(p.list().len(), 2);
        assert!((p.total() - 16.04).abs() < 1e-4);
        let mut lap = Lap::new(false);
        lap.mark(&mut p, "off");
        assert_eq!(p.list().len(), 2, "an off stopwatch writes nothing");
        let mut lap = Lap::new(true);
        lap.mark(&mut p, "on");
        assert_eq!(p.list().last().map(|s| s.0), Some("on"));
    }

    #[test]
    fn percentiles() {
        let mut f = FrameStats::new();
        for i in 0..100 {
            f.record(i as f32);
        }
        let s = f.summary();
        assert_eq!(s.frames, 100);
        assert_eq!(s.p50, 50.0);
        assert_eq!(s.p95, 94.0);
        assert_eq!(s.max, 99.0);
        assert_eq!(s.slow, 100 - 17);
        assert_eq!(f.summary().slow, 0);
    }

    #[test]
    fn ring_wraps() {
        let mut f = FrameStats::new();
        for _ in 0..1000 {
            f.record(1.0);
        }
        f.record(50.0);
        let s = f.summary();
        assert_eq!(s.max, 50.0);
        assert_eq!(s.p50, 1.0);
    }
}
