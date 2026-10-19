//! Scrolling physics for one vertical scroll container.
//!
//! Three kinds of input, each with its own feel:
//! * a notched wheel: each notch moves the target and the view chases it on
//!   a critically damped spring, so quick notches chain into one glide;
//! * a precision touchpad: deltas land as they come (the driver already
//!   supplies its own inertia), and past an end the view stretches with
//!   resistance and springs back once the fingers stop — the Mac column's
//!   elastic bounce (`Side.swift` 349, `scrollBounceBehavior(.always)`);
//! * touch: the content follows the finger, then flings with an iOS-like
//!   deceleration and bounces off the ends.
//!
//! Everything is in DIPs and seconds; [`Scroller::step`] advances it.

use crate::spring::{Animated, Motion, Spring};

/// Notched wheel: DIPs per 120-unit notch at the system's 3 lines. Edge and
/// Chrome scroll 100 DIPs per notch.
pub const NOTCH: f32 = 100.0;
/// The chase spring for notched wheels: critically damped, ~¼ s.
const CHASE: Motion = Motion::spring(0.22, 1.0);
/// Back from an overscroll: no bounce, a touch slower than a glide.
const RETURN: Spring = Spring::new(0.4, 1.0);
/// Touch fling deceleration: velocity × 0.998 per millisecond (iOS normal).
const DECEL_PER_MS: f32 = 0.998;
/// The rubber band's stiffness (Apple's 0.55).
const BAND: f32 = 0.55;
/// The fingers have stopped when no precise delta came for this long.
const SETTLE_AFTER: f32 = 0.09;
/// The indicator stays this long after the last movement, then fades.
const INDICATOR_HOLD: f32 = 0.8;
const INDICATOR_FADE: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum Mode {
    #[default]
    Idle,
    /// Chasing a wheel target.
    Chase,
    /// Precise deltas arriving (touchpad) or a finger down (touch).
    Direct,
    /// A touch fling coasting.
    Fling,
    /// Springing back inside the bounds.
    Return,
}

/// One container's scroll state.
#[derive(Clone, Debug, Default)]
pub struct Scroller {
    /// What is shown: may sit outside `0..=max` while stretched.
    offset: f32,
    /// The unclamped finger/touchpad position while stretching.
    raw: f32,
    viewport: f32,
    content: f32,
    mode: Mode,
    chase: Option<Animated>,
    velocity: f32,
    quiet: f32,
    ret_from: f32,
    ret_v: f32,
    ret_t: f32,
    /// Whether the ends stretch (the column) or stop hard.
    pub elastic: bool,
    /// Seconds since the view last moved, for the indicator.
    since_move: f32,
}

impl Scroller {
    pub fn new(elastic: bool) -> Scroller {
        Scroller {
            elastic,
            since_move: INDICATOR_HOLD + INDICATOR_FADE,
            ..Scroller::default()
        }
    }

    /// The offset to draw at.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    pub fn max(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    pub fn viewport(&self) -> f32 {
        self.viewport
    }

    pub fn content(&self) -> f32 {
        self.content
    }

    /// Whether anything is still moving.
    pub fn is_active(&self) -> bool {
        self.mode != Mode::Idle || self.indicator() > 0.0
    }

    /// New sizes after layout. Keeps the offset inside the new range.
    pub fn set_extent(&mut self, viewport: f32, content: f32) {
        self.viewport = viewport;
        self.content = content;
        if self.mode == Mode::Idle {
            let clamped = self.offset.clamp(0.0, self.max());
            self.offset = clamped;
            self.raw = clamped;
        }
    }

    /// A wheel turn: `delta` in DIPs, positive scrolls the content up (shows
    /// what is below). `precise` for touchpads (sub-notch deltas).
    pub fn wheel(&mut self, delta: f32, precise: bool) {
        if precise {
            if self.mode != Mode::Direct {
                self.raw = self.offset_unstretched();
                self.mode = Mode::Direct;
                self.chase = None;
            }
            self.raw += delta;
            self.quiet = 0.0;
            self.apply_raw();
        } else {
            let from = self.offset.clamp(0.0, self.max());
            let base = match (&self.chase, self.mode) {
                (Some(a), Mode::Chase) => a.target(),
                _ => from,
            };
            let to = (base + delta).clamp(0.0, self.max());
            let mut a = match (self.chase, self.mode) {
                (Some(a), Mode::Chase) => a,
                _ => Animated::new(from),
            };
            a.set(to, CHASE);
            self.chase = Some(a);
            self.mode = Mode::Chase;
        }
        self.since_move = 0.0;
    }

    /// A finger moved by `dy` (positive: content follows the finger down,
    /// showing what is above — so the offset decreases).
    pub fn drag(&mut self, dy: f32) {
        if self.mode != Mode::Direct {
            self.raw = self.offset_unstretched();
            self.mode = Mode::Direct;
            self.chase = None;
        }
        self.raw -= dy;
        self.quiet = f32::NEG_INFINITY; // held: never settles by itself
        self.apply_raw();
        self.since_move = 0.0;
    }

    /// The finger lifted, moving at `velocity` DIPs/s (in finger terms).
    pub fn release(&mut self, velocity: f32) {
        self.velocity = -velocity;
        if self.out_of_bounds() {
            self.start_return();
        } else if self.velocity.abs() > 20.0 {
            self.mode = Mode::Fling;
            self.raw = self.offset;
        } else {
            self.mode = Mode::Idle;
        }
    }

    /// Jump or glide to `y`.
    pub fn scroll_to(&mut self, y: f32, animated: bool) {
        let to = y.clamp(0.0, self.max());
        if animated {
            let mut a = Animated::new(self.offset);
            a.set(to, CHASE);
            self.chase = Some(a);
            self.mode = Mode::Chase;
        } else {
            self.offset = to;
            self.raw = to;
            self.mode = Mode::Idle;
            self.chase = None;
        }
        self.since_move = 0.0;
    }

    /// Bring `top..bottom` (content coordinates) into view, moving as little
    /// as possible (`ScrollViewProxy.scrollTo(anchor: nil)`).
    pub fn reveal(&mut self, top: f32, bottom: f32, animated: bool) {
        let cur = self.target_or_offset();
        let to = if top < cur {
            top
        } else if bottom > cur + self.viewport {
            bottom - self.viewport
        } else {
            return;
        };
        self.scroll_to(to, animated);
    }

    fn target_or_offset(&self) -> f32 {
        match (&self.chase, self.mode) {
            (Some(a), Mode::Chase) => a.target(),
            _ => self.offset,
        }
    }

    /// Advance by `dt` seconds; true while still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        let before = self.offset;
        match self.mode {
            Mode::Idle => {}
            Mode::Chase => {
                if let Some(a) = &mut self.chase {
                    let moving = a.step(dt);
                    self.offset = a.value();
                    self.raw = self.offset;
                    if !moving {
                        self.mode = Mode::Idle;
                        self.chase = None;
                    }
                }
            }
            Mode::Direct => {
                self.quiet += dt;
                if self.quiet > SETTLE_AFTER {
                    if self.out_of_bounds() {
                        self.start_return();
                    } else {
                        self.mode = Mode::Idle;
                    }
                }
            }
            Mode::Fling => {
                let decay = DECEL_PER_MS.powf(dt * 1000.0);
                self.raw += self.velocity * dt;
                self.velocity *= decay;
                if self.elastic && (self.raw < 0.0 || self.raw > self.max()) {
                    // Hit an end: what is left of the speed goes into the band.
                    self.offset = self.raw.clamp(0.0, self.max());
                    self.start_return_with(self.velocity * 0.5);
                } else {
                    self.offset = self.raw.clamp(0.0, self.max());
                    if self.offset != self.raw {
                        self.mode = Mode::Idle;
                        self.raw = self.offset;
                    } else if self.velocity.abs() < 5.0 {
                        self.mode = Mode::Idle;
                    }
                }
            }
            Mode::Return => {
                self.ret_t += dt;
                let end = self.ret_end();
                let (x, v) = RETURN.solve(self.ret_from - end, self.ret_v, self.ret_t);
                self.offset = end + x;
                if x.abs() < 0.1 && v.abs() < 1.0 {
                    self.offset = end;
                    self.raw = end;
                    self.mode = Mode::Idle;
                }
            }
        }
        if (self.offset - before).abs() > 1e-4 {
            self.since_move = 0.0;
        } else {
            self.since_move += dt;
        }
        self.is_active()
    }

    /// How visible the overlay indicator is (0–1).
    pub fn indicator(&self) -> f32 {
        if self.max() <= 0.0 {
            return 0.0;
        }
        if self.since_move <= INDICATOR_HOLD {
            1.0
        } else {
            (1.0 - (self.since_move - INDICATOR_HOLD) / INDICATOR_FADE).max(0.0)
        }
    }

    /// The indicator thumb as (top, height) within the viewport.
    pub fn thumb(&self) -> (f32, f32) {
        if self.content <= 0.0 || self.viewport <= 0.0 {
            return (0.0, 0.0);
        }
        let ratio = (self.viewport / self.content).min(1.0);
        let h = (self.viewport * ratio).max(24.0).min(self.viewport);
        let max = self.max().max(1.0);
        let p = (self.offset / max).clamp(0.0, 1.0);
        (p * (self.viewport - h), h)
    }

    fn offset_unstretched(&self) -> f32 {
        self.offset.clamp(0.0, self.max())
    }

    fn out_of_bounds(&self) -> bool {
        self.offset < -0.01 || self.offset > self.max() + 0.01
    }

    fn apply_raw(&mut self) {
        let max = self.max();
        if !self.elastic {
            self.raw = self.raw.clamp(0.0, max);
            self.offset = self.raw;
            return;
        }
        self.offset = if self.raw < 0.0 {
            -band(-self.raw, self.viewport)
        } else if self.raw > max {
            max + band(self.raw - max, self.viewport)
        } else {
            self.raw
        };
    }

    fn ret_end(&self) -> f32 {
        self.ret_from.clamp(0.0, self.max())
    }

    fn start_return(&mut self) {
        self.start_return_with(0.0);
    }

    fn start_return_with(&mut self, v: f32) {
        self.ret_from = self.offset;
        self.ret_v = v;
        self.ret_t = 0.0;
        self.mode = Mode::Return;
        // Spring back towards the nearest end, from where it stands.
        if v != 0.0 && !self.out_of_bounds() {
            // Coasting into an end: let the spring carry the speed past it.
            self.ret_from = self.offset;
        }
    }
}

/// Apple's rubber band: the further you pull, the less it gives.
fn band(x: f32, dim: f32) -> f32 {
    let d = dim.max(1.0);
    (1.0 - 1.0 / (x * BAND / d + 1.0)) * d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Scroller) -> usize {
        let mut n = 0;
        while s.step(1.0 / 60.0) && n < 2000 {
            n += 1;
        }
        n
    }

    fn sized() -> Scroller {
        let mut s = Scroller::new(true);
        s.set_extent(400.0, 2000.0);
        s
    }

    #[test]
    fn notches_chain_into_one_glide() {
        let mut s = sized();
        s.wheel(NOTCH, false);
        s.step(1.0 / 60.0);
        s.wheel(NOTCH, false);
        run(&mut s);
        assert!((s.offset() - 2.0 * NOTCH).abs() < 0.5, "{}", s.offset());
    }

    #[test]
    fn wheel_stops_at_the_ends() {
        let mut s = sized();
        s.wheel(-NOTCH, false);
        run(&mut s);
        assert_eq!(s.offset(), 0.0);
        s.wheel(10_000.0, false);
        run(&mut s);
        assert_eq!(s.offset(), 1600.0);
    }

    #[test]
    fn touchpad_stretches_then_springs_back() {
        let mut s = sized();
        for _ in 0..10 {
            s.wheel(-30.0, true);
        }
        assert!(s.offset() < 0.0 && s.offset() > -300.0, "{}", s.offset());
        run(&mut s);
        assert!(s.offset().abs() < 0.2, "{}", s.offset());
    }

    #[test]
    fn rubber_band_resists() {
        assert!(band(100.0, 400.0) < 100.0);
        assert!(band(10_000.0, 400.0) < 400.0);
        assert!(band(0.0, 400.0).abs() < 1e-6);
    }

    #[test]
    fn fling_coasts_and_stops() {
        let mut s = sized();
        s.drag(-50.0);
        s.release(-1500.0);
        let start = s.offset();
        run(&mut s);
        assert!(s.offset() > start + 200.0, "{} vs {start}", s.offset());
        assert!(s.offset() <= s.max() + 0.2);
    }

    #[test]
    fn hard_ends_never_stretch() {
        let mut s = Scroller::new(false);
        s.set_extent(400.0, 2000.0);
        s.wheel(-500.0, true);
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn reveal_moves_as_little_as_it_can() {
        let mut s = sized();
        s.reveal(500.0, 537.0, false);
        assert_eq!(s.offset(), 137.0);
        s.reveal(200.0, 237.0, false);
        assert_eq!(s.offset(), 137.0);
        s.reveal(100.0, 137.0, false);
        assert_eq!(s.offset(), 100.0);
    }

    #[test]
    fn indicator_fades_after_a_hold() {
        let mut s = sized();
        s.wheel(10.0, true);
        assert_eq!(s.indicator(), 1.0);
        run(&mut s);
        assert_eq!(s.indicator(), 0.0);
        let (top, h) = s.thumb();
        assert!(h >= 24.0 && top >= 0.0);
    }

    #[test]
    fn shrinking_content_clamps() {
        let mut s = sized();
        s.scroll_to(1600.0, false);
        s.set_extent(400.0, 800.0);
        assert_eq!(s.offset(), 400.0);
    }
}
