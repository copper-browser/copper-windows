//! Motion: SwiftUI's spring, its ease-out and its timing curve, as maths.
//!
//! Copper moves with two springs and one ease (Mac `Design.swift` 134–136):
//! `glide` for anything travelling between two places, `settle` for anything
//! arriving or leaving, `quick` for a hover or a press. SwiftUI describes a
//! spring by its *response* (the period of the undamped oscillation, in
//! seconds) and its *damping fraction* (ζ); with unit mass that is
//! stiffness k = (2π / response)² and damping c = 4πζ / response. The motion
//! is solved in closed form, so a frame that arrives late lands exactly where
//! the spring would have been — no integration drift, no frame-rate
//! dependence.

use std::f32::consts::PI;

/// A spring as SwiftUI's `.spring(response:dampingFraction:)` gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    /// Seconds per undamped oscillation.
    pub response: f32,
    /// ζ: 1 is critically damped, below 1 overshoots a little.
    pub damping: f32,
}

impl Spring {
    pub const fn new(response: f32, damping: f32) -> Spring {
        Spring { response, damping }
    }

    /// ω₀, the undamped angular frequency.
    fn omega(&self) -> f32 {
        2.0 * PI / self.response.max(1e-3)
    }

    /// Displacement from the target and velocity after `t` seconds, starting
    /// from displacement `x0` with velocity `v0` (units per second).
    pub fn solve(&self, x0: f32, v0: f32, t: f32) -> (f32, f32) {
        let w0 = self.omega();
        let z = self.damping.max(0.0);
        if z < 1.0 {
            // Underdamped: a decaying cosine.
            let wd = w0 * (1.0 - z * z).sqrt();
            let decay = (-z * w0 * t).exp();
            let a = x0;
            let b = (v0 + z * w0 * x0) / wd;
            let (s, c) = (wd * t).sin_cos();
            let x = decay * (a * c + b * s);
            let v = decay * ((b * wd - z * w0 * a) * c - (a * wd + z * w0 * b) * s);
            (x, v)
        } else if (z - 1.0).abs() < 1e-4 {
            // Critically damped.
            let decay = (-w0 * t).exp();
            let b = v0 + w0 * x0;
            let x = (x0 + b * t) * decay;
            let v = (b - w0 * (x0 + b * t)) * decay;
            (x, v)
        } else {
            // Overdamped: two decaying exponentials.
            let r = w0 * (z * z - 1.0).sqrt();
            let r1 = -z * w0 + r;
            let r2 = -z * w0 - r;
            let c2 = (v0 - r1 * x0) / (r2 - r1);
            let c1 = x0 - c2;
            let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
            (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
        }
    }
}

/// A cubic Bézier timing curve through (0,0) and (1,1): CSS's and SwiftUI's
/// `timingCurve(x1, y1, x2, y2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Bezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Bezier {
        Bezier { x1, y1, x2, y2 }
    }

    /// SwiftUI's `.easeOut` (the CSS `ease-out` control points).
    pub const EASE_OUT: Bezier = Bezier::new(0.0, 0.0, 0.58, 1.0);

    fn axis(a: f32, b: f32, t: f32) -> f32 {
        // B(t) = 3(1-t)²t·a + 3(1-t)t²·b + t³
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    }

    fn slope(a: f32, b: f32, t: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
    }

    /// Progress (0–1) at time fraction `x` (0–1).
    pub fn at(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.0 || x >= 1.0 {
            return x;
        }
        // Newton's method on x(t) = x, falling back to bisection.
        let mut t = x;
        for _ in 0..8 {
            let err = Self::axis(self.x1, self.x2, t) - x;
            if err.abs() < 1e-5 {
                return Self::axis(self.y1, self.y2, t);
            }
            let d = Self::slope(self.x1, self.x2, t);
            if d.abs() < 1e-6 {
                break;
            }
            t = (t - err / d).clamp(0.0, 1.0);
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        t = x;
        for _ in 0..30 {
            let v = Self::axis(self.x1, self.x2, t);
            if (v - x).abs() < 1e-5 {
                break;
            }
            if v < x {
                lo = t;
            } else {
                hi = t;
            }
            t = (lo + hi) / 2.0;
        }
        Self::axis(self.y1, self.y2, t)
    }
}

/// How a value travels to its target: on a spring, or along a curve over a
/// fixed time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    Spring(Spring),
    Curve { curve: Bezier, seconds: f32 },
}

impl Motion {
    /// A spring (see [`Spring::new`]).
    pub const fn spring(response: f32, damping: f32) -> Motion {
        Motion::Spring(Spring::new(response, damping))
    }

    /// SwiftUI's `.easeOut(duration:)`.
    pub const fn ease_out(seconds: f32) -> Motion {
        Motion::Curve {
            curve: Bezier::EASE_OUT,
            seconds,
        }
    }
}

/// One animated number: where it is, where it is going and how.
///
/// Retargeting mid-flight keeps the current position and velocity, so a
/// pill sent somewhere else halfway through a glide bends towards the new
/// place instead of jumping — the way SwiftUI's springs blend.
#[derive(Clone, Copy, Debug)]
pub struct Animated {
    from: f32,
    to: f32,
    v0: f32,
    elapsed: f32,
    motion: Motion,
    value: f32,
    velocity: f32,
    done: bool,
}

/// Close enough to the target to stop, as fractions of the distance
/// travelled (so a 0–1 opacity and a 400-DIP slide both settle cleanly):
/// a tenth of a DIP on a hundred-DIP glide, still moving under one DIP a
/// second.
const REST_DISTANCE: f32 = 0.001;
const REST_SPEED: f32 = 0.01;

impl Animated {
    /// A value at rest.
    pub fn new(value: f32) -> Animated {
        Animated {
            from: value,
            to: value,
            v0: 0.0,
            elapsed: 0.0,
            motion: Motion::ease_out(0.0),
            value,
            velocity: 0.0,
            done: true,
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Head for `to` with `motion`, from wherever the value is now.
    pub fn set(&mut self, to: f32, motion: Motion) {
        if self.done && (to - self.value).abs() < f32::EPSILON {
            self.to = to;
            return;
        }
        self.from = self.value;
        self.v0 = self.velocity;
        self.to = to;
        self.elapsed = 0.0;
        self.motion = motion;
        self.done = false;
    }

    /// Jump to `to`, no motion (Reduce Motion, or a first placement).
    pub fn snap(&mut self, to: f32) {
        *self = Animated::new(to);
    }

    /// Advance by `dt` seconds. Returns true while still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.done {
            return false;
        }
        self.elapsed += dt.max(0.0);
        match self.motion {
            Motion::Spring(s) => {
                let span = (self.to - self.from).abs().max(self.v0.abs() * 0.1);
                let (x, v) = s.solve(self.from - self.to, self.v0, self.elapsed);
                self.value = self.to + x;
                self.velocity = v;
                if x.abs() <= REST_DISTANCE * span + 1e-4 && v.abs() <= REST_SPEED * span + 1e-3 {
                    self.finish();
                }
            }
            Motion::Curve { curve, seconds } => {
                if seconds <= 0.0 || self.elapsed >= seconds {
                    self.finish();
                } else {
                    let p = curve.at(self.elapsed / seconds);
                    let prev = self.value;
                    self.value = self.from + (self.to - self.from) * p;
                    self.velocity = if dt > 0.0 {
                        (self.value - prev) / dt
                    } else {
                        0.0
                    };
                }
            }
        }
        !self.done
    }

    fn finish(&mut self) {
        self.value = self.to;
        self.velocity = 0.0;
        self.done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_starts_where_it_is_and_ends_at_target() {
        let s = Spring::new(0.34, 0.82);
        let (x, v) = s.solve(-100.0, 0.0, 0.0);
        assert!((x + 100.0).abs() < 1e-3 && v.abs() < 1e-3);
        let (x, _) = s.solve(-100.0, 0.0, 2.0);
        assert!(x.abs() < 0.01, "{x}");
    }

    #[test]
    fn glide_overshoots_a_touch_and_settle_less() {
        // ζ 0.82 overshoots by about exp(-πζ/√(1-ζ²)) ≈ 1.1 %.
        let peak = |s: Spring| {
            (0..400)
                .map(|i| s.solve(-1.0, 0.0, i as f32 / 400.0).0)
                .fold(f32::MIN, f32::max)
        };
        let glide = peak(Spring::new(0.34, 0.82));
        let settle = peak(Spring::new(0.30, 0.86));
        assert!(glide > 0.005 && glide < 0.02, "{glide}");
        assert!(settle < glide, "{settle} vs {glide}");
    }

    #[test]
    fn critical_and_overdamped_never_overshoot() {
        for z in [1.0, 1.5] {
            let s = Spring::new(0.3, z);
            for i in 0..200 {
                let (x, _) = s.solve(-1.0, 0.0, i as f32 / 100.0);
                assert!(x <= 1e-5, "ζ {z} t {i}: {x}");
            }
        }
    }

    #[test]
    fn velocity_matches_derivative() {
        let s = Spring::new(0.34, 0.82);
        let t = 0.1;
        let h = 1e-3;
        let (a, _) = s.solve(-50.0, 30.0, t - h);
        let (b, _) = s.solve(-50.0, 30.0, t + h);
        let (_, v) = s.solve(-50.0, 30.0, t);
        assert!(((b - a) / (2.0 * h) - v).abs() < 0.5, "{v}");
    }

    #[test]
    fn ease_out_is_monotonic_and_fast_early() {
        let mut last = 0.0;
        for i in 0..=100 {
            let y = Bezier::EASE_OUT.at(i as f32 / 100.0);
            assert!(y >= last - 1e-5);
            last = y;
        }
        assert!(Bezier::EASE_OUT.at(0.5) > 0.6);
        assert_eq!(Bezier::EASE_OUT.at(1.0), 1.0);
    }

    #[test]
    fn animated_settles_and_retargets_smoothly() {
        let mut a = Animated::new(0.0);
        a.set(100.0, Motion::spring(0.34, 0.82));
        let mut frames = 0;
        while a.step(1.0 / 60.0) {
            frames += 1;
            if frames == 6 {
                let before = a.value();
                a.set(0.0, Motion::spring(0.34, 0.82));
                a.step(1.0 / 60.0);
                // Still carried forward by the velocity it had.
                assert!(a.value() >= before - 1.0, "{} vs {before}", a.value());
            }
            assert!(frames < 240, "never settled");
        }
        assert_eq!(a.value(), 0.0);
    }

    #[test]
    fn curve_finishes_on_time() {
        let mut a = Animated::new(0.0);
        a.set(1.0, Motion::ease_out(0.14));
        let mut n = 0;
        while a.step(1.0 / 120.0) {
            n += 1;
        }
        assert!((16..=18).contains(&n), "{n}");
        assert_eq!(a.value(), 1.0);
    }
}
