//! Geometry and colour: the few value types every other module speaks.
//!
//! Everything the toolkit lays out is in DIPs (device-independent pixels,
//! 1/96 inch) — the Mac's points. Only the Windows backend multiplies by the
//! window's scale, and it snaps layer origins to whole physical pixels so
//! hairlines stay sharp.

use std::ops::{Add, Sub};

/// A point or a displacement, in DIPs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

/// A width and a height, in DIPs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const ZERO: Size = Size { w: 0.0, h: 0.0 };

    pub const fn new(w: f32, h: f32) -> Size {
        Size { w, h }
    }
}

/// An axis-aligned rectangle: origin at the top left, y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn from_size(s: Size) -> Rect {
        Rect::new(0.0, 0.0, s.w, s.h)
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Half-open containment: a point on the right or bottom edge belongs to
    /// the neighbour, so two rows that touch never both claim the pointer.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    pub fn offset(&self, d: Point) -> Rect {
        Rect::new(self.x + d.x, self.y + d.y, self.w, self.h)
    }

    pub fn inset(&self, i: Insets) -> Rect {
        Rect::new(
            self.x + i.left,
            self.y + i.top,
            (self.w - i.left - i.right).max(0.0),
            (self.h - i.top - i.bottom).max(0.0),
        )
    }

    pub fn outset(&self, by: f32) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            self.w + 2.0 * by,
            self.h + 2.0 * by,
        )
    }

    /// The overlap, or an empty rectangle at the origin when there is none.
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        if r <= x || b <= y {
            Rect::ZERO
        } else {
            Rect::new(x, y, r - x, b - y)
        }
    }

    /// The smallest rectangle holding both; an empty side is ignored.
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        let r = self.right().max(o.right());
        let b = self.bottom().max(o.bottom());
        Rect::new(x, y, r - x, b - y)
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        !self.intersect(o).is_empty()
    }

    /// Every edge scaled by `s` and pushed outwards to whole units: the
    /// physical pixels a DIP rectangle touches.
    pub fn scaled_out(&self, s: f32) -> Rect {
        let x = (self.x * s).floor();
        let y = (self.y * s).floor();
        let r = (self.right() * s).ceil();
        let b = (self.bottom() * s).ceil();
        Rect::new(x, y, r - x, b - y)
    }

    /// The parts of `self` not covered by `o`: at most four bands (above,
    /// below, left, right), the pieces a layer still has to draw when part of
    /// what it needs is already on its surface.
    pub fn subtract(&self, o: &Rect) -> RectList {
        let mut out = RectList::default();
        let cut = self.intersect(o);
        if cut.is_empty() {
            out.push(*self);
            return out;
        }
        if cut.y > self.y {
            out.push(Rect::new(self.x, self.y, self.w, cut.y - self.y));
        }
        if cut.bottom() < self.bottom() {
            out.push(Rect::new(
                self.x,
                cut.bottom(),
                self.w,
                self.bottom() - cut.bottom(),
            ));
        }
        if cut.x > self.x {
            out.push(Rect::new(self.x, cut.y, cut.x - self.x, cut.h));
        }
        if cut.right() < self.right() {
            out.push(Rect::new(
                cut.right(),
                cut.y,
                self.right() - cut.right(),
                cut.h,
            ));
        }
        out
    }
}

/// Up to four rectangles without allocating (see [`Rect::subtract`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct RectList {
    items: [Rect; 4],
    len: usize,
}

impl RectList {
    fn push(&mut self, r: Rect) {
        if !r.is_empty() && self.len < 4 {
            self.items[self.len] = r;
            self.len += 1;
        }
    }

    pub fn as_slice(&self) -> &[Rect] {
        &self.items[..self.len]
    }
}

/// Space inside an edge: padding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Insets {
    pub const ZERO: Insets = Insets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    pub const fn all(v: f32) -> Insets {
        Insets {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }

    /// Horizontal then vertical, like SwiftUI's `.padding(.horizontal, h)`
    /// with `.padding(.vertical, v)`.
    pub const fn hv(h: f32, v: f32) -> Insets {
        Insets {
            left: h,
            top: v,
            right: h,
            bottom: v,
        }
    }

    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Insets {
        Insets {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// A colour: straight (not premultiplied) sRGB with alpha, each 0–1 — what
/// Direct2D's `D2D1_COLOR_F` takes and what `NSColor(white:alpha:)` means.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const CLEAR: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Color = Color::rgba(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    /// A neutral of the given whiteness: the Mac palette's `NSColor(white:)`.
    pub const fn grey(white: f32) -> Color {
        Color::rgba(white, white, white, 1.0)
    }

    /// `0xRRGGBB`, opaque.
    pub const fn hex(v: u32) -> Color {
        Color::rgb(
            ((v >> 16) & 0xff) as f32 / 255.0,
            ((v >> 8) & 0xff) as f32 / 255.0,
            (v & 0xff) as f32 / 255.0,
        )
    }

    /// The same colour at `a` times its alpha: SwiftUI's `.opacity(a)`.
    pub const fn alpha(self, a: f32) -> Color {
        Color::rgba(self.r, self.g, self.b, self.a * a)
    }

    /// Straight interpolation from `self` (t = 0) to `o` (t = 1), alpha
    /// included; the colour of a hover fading in.
    pub fn mix(self, o: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        // Interpolate premultiplied so a fade from clear does not darken
        // through black on the way.
        let (pa, pb) = (self.a, o.a);
        let a = pa + (pb - pa) * t;
        if a <= 0.0 {
            return Color::CLEAR;
        }
        let ch = |x: f32, y: f32| (x * pa + (y * pb - x * pa) * t) / a;
        Color::rgba(ch(self.r, o.r), ch(self.g, o.g), ch(self.b, o.b), a)
    }

    /// `self` laid over `under`: the opaque colour a translucent token comes
    /// to on a known ground.
    pub fn over(self, under: Color) -> Color {
        let a = self.a + under.a * (1.0 - self.a);
        if a <= 0.0 {
            return Color::CLEAR;
        }
        let ch = |x: f32, y: f32| (x * self.a + y * under.a * (1.0 - self.a)) / a;
        Color::rgba(
            ch(self.r, under.r),
            ch(self.g, under.g),
            ch(self.b, under.b),
            a,
        )
    }

    /// Relative luminance (WCAG), for picking ink on an arbitrary ground.
    pub fn luminance(&self) -> f32 {
        fn lin(c: f32) -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// HSB (SwiftUI `Color(hue:saturation:brightness:)`), each 0–1.
    pub fn hsb(h: f32, s: f32, v: f32) -> Color {
        let h = (h.rem_euclid(1.0)) * 6.0;
        let i = h.floor();
        let f = h - i;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        let (r, g, b) = match i as i32 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        Color::rgb(r, g, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_union() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), Rect::new(5.0, 5.0, 5.0, 5.0));
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 15.0));
        assert!(a.intersect(&Rect::new(20.0, 0.0, 1.0, 1.0)).is_empty());
        assert_eq!(Rect::ZERO.union(&b), b);
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(!r.contains(Point::new(10.0, 5.0)));
    }

    #[test]
    fn subtract_bands() {
        let need = Rect::new(0.0, 0.0, 100.0, 300.0);
        let have = Rect::new(0.0, 100.0, 100.0, 100.0);
        let bands = need.subtract(&have);
        assert_eq!(
            bands.as_slice(),
            &[
                Rect::new(0.0, 0.0, 100.0, 100.0),
                Rect::new(0.0, 200.0, 100.0, 100.0)
            ]
        );
        assert_eq!(need.subtract(&need).as_slice().len(), 0);
        let apart = Rect::new(500.0, 0.0, 1.0, 1.0);
        assert_eq!(need.subtract(&apart).as_slice(), &[need]);
    }

    #[test]
    fn scaled_out_covers_fractional_pixels() {
        let r = Rect::new(0.5, 0.5, 1.0, 1.0).scaled_out(1.5);
        assert_eq!(r, Rect::new(0.0, 0.0, 3.0, 3.0));
    }

    #[test]
    fn mixing_from_clear_keeps_colour() {
        let ink = Color::rgba(1.0, 0.0, 0.0, 1.0);
        let half = Color::CLEAR.mix(ink, 0.5);
        assert!((half.r - 1.0).abs() < 1e-5 && (half.a - 0.5).abs() < 1e-5);
        let over = Color::BLACK.alpha(0.06).over(Color::WHITE);
        assert!((over.r - 0.94).abs() < 1e-4 && over.a == 1.0);
    }

    #[test]
    fn hsb_primaries() {
        assert_eq!(Color::hsb(0.0, 1.0, 1.0), Color::rgb(1.0, 0.0, 0.0));
        let g = Color::hsb(1.0 / 3.0, 1.0, 1.0);
        assert!((g.g - 1.0).abs() < 1e-5 && g.r.abs() < 1e-5);
    }
}
