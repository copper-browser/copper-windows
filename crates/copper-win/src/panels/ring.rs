//! The download door's ring (`DownloadRing`), drawn into pixels: the
//! toolkit has no arcs, so the ring is rasterised here — anti-aliased,
//! premultiplied BGRA at the window's scale — and shown as a picture.
//!
//! A ring is drawn from layers, bottom first: a whole circle (the hairline
//! track, the failed door's orange), an arc from twelve o'clock clockwise
//! with round caps (the fraction done), or dashes (a download of unknown
//! length, turned by `phase`).

use crate::picture::Picture;
use copper_win_ui::Color;
use std::f32::consts::TAU;

/// How a layer's stroke runs round the circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stroke {
    /// All the way round.
    Whole,
    /// From twelve o'clock clockwise, this fraction of the way, round-capped.
    Arc(f32),
    /// `dash` on, `gap` off (DIPs along the circle), turned `phase` of a
    /// full turn clockwise.
    Dashed { dash: f32, gap: f32, phase: f32 },
}

/// One stroke of the ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub stroke: Stroke,
    /// The line's width in DIPs.
    pub width: f32,
    pub color: Color,
}

/// A ring of `diameter` DIPs (the stroke centred on it) in a square
/// picture `box_size` DIPs wide, at `scale` pixels per DIP.
pub fn draw(box_size: f32, diameter: f32, scale: f32, layers: &[Layer]) -> Picture {
    let px = (box_size * scale).round().max(1.0) as u32;
    let mut bgra = vec![0u8; (px * px * 4) as usize];
    let c = px as f32 / 2.0;
    let radius = diameter / 2.0 * scale;
    for y in 0..px {
        for x in 0..px {
            let dx = x as f32 + 0.5 - c;
            let dy = y as f32 + 0.5 - c;
            let r = (dx * dx + dy * dy).sqrt();
            // 0 at twelve o'clock, growing clockwise.
            let theta = dx.atan2(-dy).rem_euclid(TAU);
            let mut out = [0f32; 4];
            for l in layers {
                let cov = coverage(l, r, theta, radius, scale, dx, dy);
                if cov <= 0.0 {
                    continue;
                }
                let a = l.color.a * cov;
                let src = [l.color.b * a, l.color.g * a, l.color.r * a, a];
                for i in 0..4 {
                    out[i] = src[i] + out[i] * (1.0 - a);
                }
            }
            let at = ((y * px + x) * 4) as usize;
            for i in 0..4 {
                bgra[at + i] = (out[i].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    Picture { w: px, h: px, bgra }
}

/// How much of the pixel at (`dx`, `dy`) from the centre — `r` out at
/// angle `theta` — layer `l` covers, 0–1.
fn coverage(l: &Layer, r: f32, theta: f32, radius: f32, scale: f32, dx: f32, dy: f32) -> f32 {
    let half = l.width * scale / 2.0;
    let band = (half - (r - radius).abs() + 0.5).clamp(0.0, 1.0);
    match l.stroke {
        Stroke::Whole => band,
        Stroke::Arc(f) => {
            let f = f.clamp(0.0, 1.0);
            if f <= 0.0 {
                return 0.0;
            }
            let end = f * TAU;
            if theta <= end {
                return band;
            }
            // The round caps: discs at each end of the arc.
            let cap = |angle: f32| {
                let (cx, cy) = (radius * angle.sin(), -radius * angle.cos());
                let d = ((dx - cx).powi(2) + (dy - cy).powi(2)).sqrt();
                (half - d + 0.5).clamp(0.0, 1.0)
            };
            cap(0.0).max(cap(end))
        }
        Stroke::Dashed { dash, gap, phase } => {
            let period = (dash + gap) * scale;
            if period <= 0.0 || band <= 0.0 {
                return band;
            }
            let along = (theta / TAU - phase).rem_euclid(1.0) * TAU * radius;
            let at = along.rem_euclid(period);
            let on = dash * scale;
            // Where the pixel is from its dash's start (just before it, at
            // the end of a period); soft ends a pixel wide.
            let d = if at > period - 0.5 { at - period } else { at };
            band * (d + 0.5).min(on - d + 0.5).clamp(0.0, 1.0)
        }
    }
}

/// The fraction rounded to what a door can show (1/120 of a turn), so the
/// ring is redrawn only when it would look different.
pub fn step(fraction: f32) -> u32 {
    (fraction.clamp(0.0, 1.0) * 120.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    fn alpha(p: &Picture, x: u32, y: u32) -> u8 {
        p.bgra[((y * p.w + x) * 4 + 3) as usize]
    }

    #[test]
    fn a_whole_ring_is_on_its_circle_and_nowhere_else() {
        let p = draw(
            26.0,
            23.0,
            1.0,
            &[Layer {
                stroke: Stroke::Whole,
                width: 1.5,
                color: INK,
            }],
        );
        assert_eq!((p.w, p.h), (26, 26));
        // On the circle at three o'clock (13 + 11.5).
        assert!(alpha(&p, 24, 13) > 200);
        // The centre and the corner stay clear.
        assert_eq!(alpha(&p, 13, 13), 0);
        assert_eq!(alpha(&p, 0, 0), 0);
    }

    #[test]
    fn an_arc_runs_clockwise_from_twelve() {
        let p = draw(
            52.0,
            46.0,
            1.0,
            &[Layer {
                stroke: Stroke::Arc(0.25),
                width: 2.0,
                color: INK,
            }],
        );
        let c = 26;
        // A little past twelve and before three: drawn.
        assert!(alpha(&p, c + 8, 4) > 100);
        // Six and nine o'clock: not.
        assert_eq!(alpha(&p, c, 48), 0);
        assert_eq!(alpha(&p, 3, c), 0);
    }

    #[test]
    fn dashes_leave_gaps() {
        let p = draw(
            52.0,
            46.0,
            1.0,
            &[Layer {
                stroke: Stroke::Dashed {
                    dash: 2.0,
                    gap: 2.0,
                    phase: 0.0,
                },
                width: 1.2,
                color: INK,
            }],
        );
        let ring: Vec<u8> = (0..52)
            .map(|x| alpha(&p, x, 3))
            .filter(|a| *a > 0)
            .collect();
        assert!(!ring.is_empty());
        let covered = p.bgra.chunks(4).filter(|c| c[3] > 128).count();
        let whole = draw(
            52.0,
            46.0,
            1.0,
            &[Layer {
                stroke: Stroke::Whole,
                width: 1.2,
                color: INK,
            }],
        );
        let all = whole.bgra.chunks(4).filter(|c| c[3] > 128).count();
        assert!(covered < all * 3 / 4, "{covered} of {all}");
    }

    #[test]
    fn it_scales_with_the_window() {
        let p = draw(26.0, 23.0, 1.5, &[]);
        assert_eq!(p.w, 39);
    }

    #[test]
    fn steps() {
        assert_eq!(step(0.0), 0);
        assert_eq!(step(0.5), 60);
        assert_eq!(step(2.0), 120);
    }
}
