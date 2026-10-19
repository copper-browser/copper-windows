//! Shapes Direct2D does not have ready-made: continuous ("squircle") corners
//! and soft shadows, both cached so a steady frame builds neither.
//!
//! Every rounded shape on the Mac is `RoundedRectangle(cornerRadius:style:
//! .continuous)` — a corner that eases into its curve instead of starting a
//! circle abruptly. The path here is the well-known construction of that
//! curve (a cubic into a short circular arc and a cubic out), with Apple's
//! smoothing of 0.6.
//!
//! Shadows are SwiftUI's `.shadow(radius:y:)`: a Gaussian blur of the shape
//! with σ = radius / 2, baked once per size into a bitmap through the
//! Direct2D shadow effect and then drawn like any picture.

use std::collections::HashMap;
use std::f32::consts::PI;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT,
    D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1Shadow, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_PROPERTY_TYPE_FLOAT,
    D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION, D2D1_SWEEP_DIRECTION_CLOCKWISE, ID2D1Bitmap1,
    ID2D1Factory1, ID2D1Geometry, ID2D1Image, ID2D1PathGeometry1, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::{Interface, Result};
use windows_numerics::{Matrix3x2, Vector2};

use super::gfx::Gfx;

/// Apple's continuous-corner smoothing.
const SMOOTHING: f32 = 0.6;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PathKey {
    w: u32,
    h: u32,
    r: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ShadowKey {
    w: u32,
    h: u32,
    r: u32,
    sigma: u32,
    scale: u32,
}

fn q(v: f32) -> u32 {
    (v.max(0.0) * 4.0).round() as u32
}

/// A baked shadow and how far it reaches past its shape.
#[derive(Clone)]
pub struct Baked {
    pub bitmap: ID2D1Bitmap1,
    pub margin: f32,
}

/// The caches.
pub struct Shapes {
    paths: HashMap<PathKey, ID2D1Geometry>,
    shadows: HashMap<ShadowKey, Baked>,
    black: Option<ID2D1SolidColorBrush>,
    generation: u32,
}

/// Cap on cached entries; past it the cache starts over (a resize sweep
/// makes many sizes that will never come back).
const CAP: usize = 512;

impl Default for Shapes {
    fn default() -> Shapes {
        Shapes::new()
    }
}

impl Shapes {
    pub fn new() -> Shapes {
        Shapes {
            paths: HashMap::new(),
            shadows: HashMap::new(),
            black: None,
            generation: u32::MAX,
        }
    }

    /// Forget device-bound entries after the devices were rebuilt.
    fn check(&mut self, gfx: &Gfx) {
        if self.generation != gfx.generation {
            self.generation = gfx.generation;
            self.paths.clear();
            self.shadows.clear();
            self.black = None;
        }
    }

    /// A `w`×`h` continuous-corner rectangle at the origin.
    pub fn rounded(&mut self, gfx: &Gfx, w: f32, h: f32, r: f32) -> Option<ID2D1Geometry> {
        self.check(gfx);
        let key = PathKey {
            w: q(w),
            h: q(h),
            r: q(r),
        };
        if let Some(g) = self.paths.get(&key) {
            return Some(g.clone());
        }
        if self.paths.len() > CAP {
            self.paths.clear();
        }
        let g = squircle(&gfx.d2d_factory, w, h, r).ok()?;
        let g: ID2D1Geometry = g.cast().ok()?;
        self.paths.insert(key, g.clone());
        Some(g)
    }

    /// The shadow of a `w`×`h` continuous rectangle of corner `r`, blurred
    /// by SwiftUI radius `radius`, at device `scale`.
    pub fn shadow(
        &mut self,
        gfx: &Gfx,
        w: f32,
        h: f32,
        r: f32,
        radius: f32,
        scale: f32,
    ) -> Option<Baked> {
        self.check(gfx);
        let key = ShadowKey {
            w: q(w),
            h: q(h),
            r: q(r),
            sigma: q(radius),
            scale: (scale * 100.0).round() as u32,
        };
        if let Some(b) = self.shadows.get(&key) {
            return Some(b.clone());
        }
        if self.shadows.len() > CAP {
            self.shadows.clear();
        }
        let geometry = self.rounded(gfx, w, h, r)?;
        let baked = self.bake(gfx, &geometry, w, h, radius, scale).ok()?;
        self.shadows.insert(key, baked.clone());
        Some(baked)
    }

    fn bake(
        &mut self,
        gfx: &Gfx,
        geometry: &ID2D1Geometry,
        w: f32,
        h: f32,
        radius: f32,
        scale: f32,
    ) -> Result<Baked> {
        let sigma = (radius / 2.0).max(0.01);
        let margin = (sigma * 3.0).ceil();
        let px = |v: f32| ((v + 2.0 * margin) * scale).ceil().max(1.0) as u32;
        let size = D2D_SIZE_U {
            width: px(w),
            height: px(h),
        };
        let dpi = 96.0 * scale;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let off = &gfx.offscreen;
        // SAFETY: all calls are on live objects of one D2D device, inside a
        // BeginDraw/EndDraw pair on the offscreen context, which is left with
        // no target afterwards.
        unsafe {
            let black = match &self.black {
                Some(b) => b.clone(),
                None => {
                    let b = off.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        },
                        None,
                    )?;
                    self.black = Some(b.clone());
                    b
                }
            };
            let shape = off.CreateBitmap(size, None, 0, &props)?;
            off.SetTarget(&shape);
            off.SetDpi(dpi, dpi);
            off.BeginDraw();
            off.Clear(None);
            off.SetTransform(&Matrix3x2::translation(margin, margin));
            off.FillGeometry(geometry, &black, None);
            off.SetTransform(&Matrix3x2::identity());
            off.EndDraw(None, None)?;

            let effect = off.CreateEffect(&CLSID_D2D1Shadow)?;
            effect.SetInput(0, &shape, true);
            effect.SetValue(
                D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32,
                D2D1_PROPERTY_TYPE_FLOAT,
                &sigma.to_le_bytes(),
            )?;
            let out: ID2D1Image = effect.GetOutput()?;
            let bitmap = off.CreateBitmap(size, None, 0, &props)?;
            off.SetTarget(&bitmap);
            off.BeginDraw();
            off.Clear(None);
            off.DrawImage(
                &out,
                None,
                None,
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            let done = off.EndDraw(None, None);
            off.SetTarget(None::<&ID2D1Image>);
            done?;
            Ok(Baked { bitmap, margin })
        }
    }
}

/// Where a corner's curve starts, and the parts of it (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corner {
    pub r: f32,
    /// Distance from the corner where the curve leaves the straight edge.
    pub p: f32,
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
}

/// The continuous corner for radius `r` in a `w`×`h` box.
pub fn corner(w: f32, h: f32, r: f32) -> Corner {
    let half = w.min(h) / 2.0;
    let r = r.min(half).max(0.0);
    // Shrink the smoothing when there is no room for it.
    let xi = if r > 0.0 {
        SMOOTHING.min(half / r - 1.0).max(0.0)
    } else {
        0.0
    };
    let p = (1.0 + xi) * r;
    let deg = PI / 180.0;
    let arc = 90.0 * (1.0 - xi);
    let arc_len = (arc / 2.0 * deg).sin() * r * std::f32::consts::SQRT_2;
    let alpha = (90.0 - arc) / 2.0;
    let p3p4 = r * (alpha / 2.0 * deg).tan();
    let beta = 45.0 * xi;
    let c = p3p4 * (beta * deg).cos();
    let d = c * (beta * deg).tan();
    let b = ((p - arc_len - c - d) / 3.0).max(0.0);
    let a = 2.0 * b;
    Corner { r, p, a, b, c, d }
}

fn v(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

/// Build the path of a continuous rectangle at the origin.
fn squircle(f: &ID2D1Factory1, w: f32, h: f32, r: f32) -> Result<ID2D1PathGeometry1> {
    let k = corner(w, h, r);
    let Corner { r, p, a, b, c, d } = k;
    let bez = |p1: Vector2, p2: Vector2, p3: Vector2| D2D1_BEZIER_SEGMENT {
        point1: p1,
        point2: p2,
        point3: p3,
    };
    let arc = |to: Vector2| D2D1_ARC_SEGMENT {
        point: to,
        size: D2D_SIZE_F {
            width: r,
            height: r,
        },
        rotationAngle: 0.0,
        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
        arcSize: D2D1_ARC_SIZE_SMALL,
    };
    // SAFETY: a path geometry built through its sink and closed.
    unsafe {
        let g = f.CreatePathGeometry()?;
        let s = g.Open()?;
        s.BeginFigure(v(w - p, 0.0), D2D1_FIGURE_BEGIN_FILLED);
        // Top-right.
        s.AddBezier(&bez(
            v(w - p + a, 0.0),
            v(w - p + a + b, 0.0),
            v(w - p + a + b + c, d),
        ));
        s.AddArc(&arc(v(w - d, p - a - b - c)));
        s.AddBezier(&bez(v(w, p - a - b), v(w, p - a), v(w, p)));
        s.AddLine(v(w, h - p));
        // Bottom-right.
        s.AddBezier(&bez(
            v(w, h - p + a),
            v(w, h - p + a + b),
            v(w - d, h - p + a + b + c),
        ));
        s.AddArc(&arc(v(w - p + a + b + c, h - d)));
        s.AddBezier(&bez(v(w - p + a + b, h), v(w - p + a, h), v(w - p, h)));
        s.AddLine(v(p, h));
        // Bottom-left.
        s.AddBezier(&bez(v(p - a, h), v(p - a - b, h), v(p - a - b - c, h - d)));
        s.AddArc(&arc(v(d, h - p + a + b + c)));
        s.AddBezier(&bez(
            v(0.0, h - p + a + b),
            v(0.0, h - p + a),
            v(0.0, h - p),
        ));
        s.AddLine(v(0.0, p));
        // Top-left.
        s.AddBezier(&bez(v(0.0, p - a), v(0.0, p - a - b), v(d, p - a - b - c)));
        s.AddArc(&arc(v(p - a - b - c, d)));
        s.AddBezier(&bez(v(p - a - b, 0.0), v(p - a, 0.0), v(p, 0.0)));
        s.EndFigure(D2D1_FIGURE_END_CLOSED);
        s.Close()?;
        Ok(g)
    }
}

/// A rectangle as Direct2D wants it.
pub fn rect_f(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}
