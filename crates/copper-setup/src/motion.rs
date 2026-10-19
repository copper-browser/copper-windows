//! The Mac app's two springs, for the installer window.
//!
//! `Design.swift`'s `Motion`: one spring for anything that moves between two
//! places (`glide`: response 0.34 s, damping 0.82) and one for anything that
//! arrives or leaves (`settle`: 0.30 s, 0.86). Same parameters as SwiftUI's
//! `Animation.spring(response:dampingFraction:)`: unit mass, stiffness
//! (2π / response)², damping 4π·ζ / response. Platform-free.

use std::f32::consts::PI;

/// A spring's character.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub response: f32,
    pub damping: f32,
}

/// Movement between two places.
pub const GLIDE: Curve = Curve {
    response: 0.34,
    damping: 0.82,
};
/// Arriving and leaving.
pub const SETTLE: Curve = Curve {
    response: 0.30,
    damping: 0.86,
};
/// Hover and press feedback: quicker than either, still a spring so a
/// pointer that sweeps across a button doesn't leave it flickering.
pub const QUICK: Curve = Curve {
    response: 0.16,
    damping: 1.0,
};

/// The progress bar's fill: slower than a glide and critically damped, so
/// the bar eases along (never past where the work is) and a quick install
/// still reads as a run from empty to full rather than a jump.
pub const FILL: Curve = Curve {
    response: 0.8,
    damping: 1.0,
};

/// A value that springs toward its target.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    value: f32,
    velocity: f32,
    target: f32,
    curve: Curve,
}

/// Close enough to call it still, in the value's own units (all the values
/// here are 0…1 fractions or dips, where 1/1000 is invisible).
const REST: f32 = 0.001;
/// Longest integration step: small enough to stay stable for these curves.
const MAX_STEP: f32 = 1.0 / 240.0;

impl Spring {
    /// At rest at `value`.
    pub const fn new(value: f32, curve: Curve) -> Self {
        Spring {
            value,
            velocity: 0.0,
            target: value,
            curve,
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Head for `target` from wherever it is now, keeping its momentum.
    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    /// Jump straight to `value` and stop there.
    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Still moving?
    pub fn is_moving(&self) -> bool {
        (self.value - self.target).abs() > REST || self.velocity.abs() > REST
    }

    /// Advance by `dt` seconds. Returns whether it is still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        if !self.is_moving() {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        let omega = 2.0 * PI / self.curve.response;
        let k = omega * omega;
        let c = 2.0 * self.curve.damping * omega;
        // Long gaps (the window was dragged, the machine stalled) shouldn't
        // turn into one huge step: cap the whole advance at a tenth of a
        // second and integrate in small pieces.
        let mut left = dt.clamp(0.0, 0.1);
        while left > 0.0 {
            let h = left.min(MAX_STEP);
            let accel = -k * (self.value - self.target) - c * self.velocity;
            self.velocity += accel * h;
            self.value += self.velocity * h;
            left -= h;
        }
        if !self.is_moving() {
            self.value = self.target;
            self.velocity = 0.0;
        }
        self.is_moving()
    }
}

/// Linear blend from `a` to `b` by `t`.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(spring: &mut Spring, seconds: f32) {
        let frames = (seconds * 60.0) as usize;
        for _ in 0..frames {
            spring.step(1.0 / 60.0);
        }
    }

    #[test]
    fn settles_on_target_within_a_second() {
        for curve in [GLIDE, SETTLE, QUICK] {
            let mut s = Spring::new(0.0, curve);
            s.set(1.0);
            assert!(s.is_moving());
            run(&mut s, 1.0);
            assert!(!s.is_moving(), "{curve:?} still moving: {s:?}");
            assert_eq!(s.value(), 1.0);
        }
    }

    #[test]
    fn underdamped_glide_overshoots_a_little() {
        let mut s = Spring::new(0.0, GLIDE);
        s.set(1.0);
        let mut peak = 0.0f32;
        for _ in 0..120 {
            s.step(1.0 / 120.0);
            peak = peak.max(s.value());
        }
        assert!(peak > 1.0 && peak < 1.05, "peak {peak}");
    }

    #[test]
    fn critically_damped_quick_never_overshoots() {
        let mut s = Spring::new(0.0, QUICK);
        s.set(1.0);
        for _ in 0..240 {
            s.step(1.0 / 240.0);
            assert!(s.value() <= 1.0 + REST, "{}", s.value());
        }
    }

    #[test]
    fn the_bar_runs_rather_than_jumps() {
        // An install that is done at once: the bar is still visibly short a
        // tenth of a second in, never passes the end, and is all but full by
        // the time the installer says "Copper is ready" (1.1 s).
        let mut s = Spring::new(0.0, FILL);
        s.set(1.0);
        run(&mut s, 0.1);
        assert!(s.value() < 0.25, "{}", s.value());
        for _ in 0..240 {
            s.step(1.0 / 240.0);
            assert!(s.value() <= 1.0 + REST, "{}", s.value());
        }
        assert!(s.value() > 0.97, "{}", s.value());
    }

    #[test]
    fn halfway_by_the_response_time() {
        // SwiftUI's response is roughly the period: by then it has arrived.
        let mut s = Spring::new(0.0, SETTLE);
        s.set(1.0);
        run(&mut s, 0.15);
        assert!(s.value() > 0.4, "{}", s.value());
    }

    #[test]
    fn huge_dt_is_stable() {
        let mut s = Spring::new(0.0, GLIDE);
        s.set(1.0);
        s.step(5.0);
        assert!(s.value().is_finite() && s.value() < 2.0);
    }

    #[test]
    fn snap_stops() {
        let mut s = Spring::new(0.0, GLIDE);
        s.set(1.0);
        s.step(0.05);
        s.snap(0.25);
        assert!(!s.is_moving());
        assert_eq!(s.value(), 0.25);
    }
}
