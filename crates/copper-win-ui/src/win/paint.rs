//! Drawing one layer's nodes with Direct2D.
//!
//! A layer's surface covers its frame plus a bleed for shadows and the
//! focus ring. Nodes are painted in document order — shadows, fill, stroke,
//! edge line, content, children (clipped when asked), the scroll
//! indicator, the focus ring — and nodes that are layers of their own are
//! skipped (they have their own surfaces, composited above).
//!
//! The painter keeps one solid brush and recolours it, and caches shapes
//! and shadows ([`super::shape`]) and text layouts ([`super::text`]); a
//! frame that only redraws a hover allocates nothing.

use std::collections::HashMap;
use std::mem::ManuallyDrop;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_NONE,
    D2D1_BITMAP_PROPERTIES1, D2D1_BRUSH_PROPERTIES, D2D1_BUFFER_PRECISION_8BPC_UNORM,
    D2D1_COLOR_INTERPOLATION_MODE_STRAIGHT, D2D1_COLOR_SPACE_SRGB, D2D1_DRAW_TEXT_OPTIONS,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_EXTEND_MODE_CLAMP,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1Bitmap1,
    ID2D1DeviceContext, ID2D1Geometry, ID2D1GradientStopCollection, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::IDWriteTextLayout;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCARETWIDTH, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows_numerics::{Matrix3x2, Vector2};

use super::gfx::Gfx;
use super::shape::{Shapes, rect_f};
use super::text::Fonts;
use crate::geom::{Color, Point, Rect};
use crate::node::{Content, Edge, Node};
use crate::theme::{Swatch, Theme, Token};
use crate::tokens::{Shadow, focus, scroll};
use crate::tree::{NodeId, Tree};

/// A picture the app registered (a favicon), kept as pixels so it can be
/// re-uploaded after the device is lost.
struct Image {
    /// Premultiplied BGRA, `w * 4` bytes per row.
    pixels: Vec<u8>,
    w: u32,
    h: u32,
    bitmap: Option<(u32, ID2D1Bitmap1)>,
}

/// Everything a layer is painted with besides the tree.
pub struct Frame<'a> {
    pub dc: &'a ID2D1DeviceContext,
    pub gfx: &'a Gfx,
    pub theme: &'a Theme,
    pub fonts: &'a mut Fonts,
    /// Physical pixels per DIP.
    pub scale: f32,
    /// The window is the foreground one (the caret only shows then).
    pub active: bool,
}

/// The painter and its caches.
pub struct Painter {
    brush: Option<(u32, ID2D1SolidColorBrush)>,
    pub shapes: Shapes,
    images: HashMap<u32, Image>,
    rects: Vec<Rect>,
    caret_pixels: u32,
}

impl Default for Painter {
    fn default() -> Painter {
        Painter::new()
    }
}

fn color_f(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

fn v2(p: Point) -> Vector2 {
    Vector2 { X: p.x, Y: p.y }
}

fn d2r(r: Rect) -> D2D_RECT_F {
    rect_f(r.x, r.y, r.w, r.h)
}

/// The colour of a resting swatch mixed toward hover and press.
fn mixed(
    theme: &Theme,
    rest: Swatch,
    hover: Option<Swatch>,
    ht: f32,
    press: Option<Swatch>,
    pt: f32,
) -> Color {
    let mut c = theme.color(rest);
    if let Some(h) = hover
        && ht > 0.0
    {
        c = c.mix(theme.color(h), ht);
    }
    if let Some(p) = press
        && pt > 0.0
    {
        c = c.mix(theme.color(p), pt);
    }
    c
}

impl Painter {
    pub fn new() -> Painter {
        let mut caret = focus::CARET_PIXELS;
        // SAFETY: SPI_GETCARETWIDTH writes a DWORD.
        unsafe {
            let _ = SystemParametersInfoW(
                SPI_GETCARETWIDTH,
                0,
                Some((&mut caret as *mut u32).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }
        Painter {
            brush: None,
            shapes: Shapes::new(),
            images: HashMap::new(),
            rects: Vec::with_capacity(8),
            caret_pixels: caret.clamp(1, 20),
        }
    }

    /// Register picture `id`: premultiplied BGRA rows of `w` pixels.
    pub fn set_image(&mut self, id: u32, pixels: Vec<u8>, w: u32, h: u32) {
        self.images.insert(
            id,
            Image {
                pixels,
                w,
                h,
                bitmap: None,
            },
        );
    }

    pub fn remove_image(&mut self, id: u32) {
        self.images.remove(&id);
    }

    fn brush(&mut self, f: &Frame, c: Color) -> Option<ID2D1SolidColorBrush> {
        let fresh = matches!(&self.brush, Some((g, _)) if *g == f.gfx.generation);
        if !fresh {
            // SAFETY: brush creation on a live context.
            let b = unsafe { f.dc.CreateSolidColorBrush(&color_f(c), None).ok()? };
            self.brush = Some((f.gfx.generation, b));
        }
        let b = &self.brush.as_ref()?.1;
        // SAFETY: recolouring our own brush.
        unsafe { b.SetColor(&color_f(c)) };
        Some(b.clone())
    }

    fn bitmap(&mut self, f: &Frame, id: u32) -> Option<ID2D1Bitmap1> {
        let img = self.images.get_mut(&id)?;
        if let Some((g, b)) = &img.bitmap
            && *g == f.gfx.generation
        {
            return Some(b.clone());
        }
        let dpi = 96.0;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: ManuallyDrop::new(None),
        };
        // SAFETY: the pixel buffer holds h rows of w*4 bytes.
        let b = unsafe {
            f.dc.CreateBitmap(
                D2D_SIZE_U {
                    width: img.w,
                    height: img.h,
                },
                Some(img.pixels.as_ptr().cast()),
                img.w * 4,
                &props,
            )
            .ok()?
        };
        img.bitmap = Some((f.gfx.generation, b.clone()));
        Some(b)
    }

    /// Paint layer `layer` into a surface whose DIP origin is `origin`
    /// in the layer's own coordinates (the bleed, positive).
    pub fn paint_layer(&mut self, f: &mut Frame, tree: &Tree, layer: NodeId, origin: Point) {
        // SAFETY: state setters on the live drawing context.
        unsafe {
            f.dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            f.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            f.dc.Clear(None);
        }
        self.node(f, tree, layer, origin, true);
    }

    fn node(&mut self, f: &mut Frame, tree: &Tree, id: NodeId, at: Point, is_root: bool) {
        let Some(n) = tree.get(id) else {
            return;
        };
        if n.style.hidden || (!is_root && n.flags.layer) {
            return;
        }
        let mut opacity = if is_root { 1.0 } else { n.transform.opacity };
        // A layer's own reveal / conceal is its visual's opacity.
        if !is_root && (n.flags.reveal || n.flags.conceal) {
            opacity *= tree.shown_t(id);
        }
        let faded = opacity < 0.999;
        if faded {
            if opacity <= 0.001 {
                return;
            }
            self.push_layer(f, None, Matrix3x2::identity(), opacity);
        }
        let r = Rect::new(at.x, at.y, n.frame.w, n.frame.h);
        self.background(f, n, r);
        self.content(f, id, n, r);

        let children = tree.children(id);
        if !children.is_empty() {
            let clip = n.flags.clip;
            if clip {
                self.push_clip(f, r, n.look.radius);
            }
            let mut origin = at;
            if let Some(s) = &n.scroll {
                origin.y -= s.offset();
            }
            for &c in children {
                let Some(cn) = tree.get(c) else {
                    continue;
                };
                let p = Point::new(
                    origin.x + cn.frame.x + cn.transform.translate.x,
                    origin.y + cn.frame.y + cn.transform.translate.y,
                );
                self.node(f, tree, c, p, false);
            }
            if clip {
                self.pop_clip(f, n.look.radius);
            }
        }
        if let Some(s) = &n.scroll {
            let a = s.indicator();
            if a > 0.0 {
                let (top, h) = s.thumb();
                let knob = Rect::new(
                    r.right() - scroll::INSET - scroll::KNOB,
                    r.y + top + scroll::INSET,
                    scroll::KNOB,
                    (h - 2.0 * scroll::INSET).max(scroll::KNOB),
                );
                let ink = f.theme.token(Token::Ink).alpha(scroll::ALPHA * a);
                self.fill_shape(f, knob, scroll::KNOB / 2.0, ink);
            }
        }
        if n.state.focused && n.state.focus_visible {
            let o = focus::RING_OUTSET;
            let ring = Rect::new(r.x - o, r.y - o, r.w + 2.0 * o, r.h + 2.0 * o);
            let radius = if n.look.radius > 0.0 {
                n.look.radius + o
            } else {
                0.0
            };
            let ink = f.theme.token(Token::Focus);
            self.stroke_shape(f, ring, radius, focus::RING_WIDTH, ink);
        }
        if faded {
            // SAFETY: matches the push above.
            unsafe { f.dc.PopLayer() };
        }
    }

    fn background(&mut self, f: &mut Frame, n: &Node, r: Rect) {
        if !n.look.paints() || r.is_empty() {
            return;
        }
        let (rest, hover, ht, press, pt) = n.fill_now();
        let fill = mixed(f.theme, rest, hover, ht, press, pt);
        let radius = n.look.radius;
        // Shadows are cast by the filled shape; nothing to cast when clear.
        if fill.a > 0.0 {
            let ink = f.theme.color(n.look.shadow_ink);
            if let Some(s) = n.look.shadow2 {
                self.shadow(f, r, radius, s, ink, fill.a);
            }
            if let Some(s) = n.look.shadow {
                self.shadow(f, r, radius, s, ink, fill.a);
            }
            let gradient = (n.look.fill == Swatch::Token(Token::ColumnGround))
                .then(|| f.theme.space.and_then(|t| t.grounds(f.theme.dark)))
                .flatten();
            match gradient {
                Some(stops) if radius <= 0.0 => self.fill_gradient(f, r, &stops, fill.a),
                _ => self.fill_shape(f, r, radius, fill),
            }
        }
        if n.look.stroke_width > 0.0 {
            let c = f.theme.color(n.look.stroke);
            if c.a > 0.0 {
                let w = n.look.stroke_width;
                let inner = Rect::new(r.x + w / 2.0, r.y + w / 2.0, r.w - w, r.h - w);
                self.stroke_shape(f, inner, (radius - w / 2.0).max(0.0), w, c);
            }
        }
        if let Some((edge, s)) = n.look.edge {
            let px = f.scale.round().max(1.0) / f.scale;
            let line = match edge {
                Edge::Top => Rect::new(r.x, r.y, r.w, px),
                Edge::Bottom => Rect::new(r.x, r.bottom() - px, r.w, px),
                Edge::Left => Rect::new(r.x, r.y, px, r.h),
                Edge::Right => Rect::new(r.right() - px, r.y, px, r.h),
            };
            let c = f.theme.color(s);
            self.fill_shape(f, line, 0.0, c);
        }
    }

    fn shadow(
        &mut self,
        f: &mut Frame,
        r: Rect,
        radius: f32,
        s: Shadow,
        ink: Color,
        fill_alpha: f32,
    ) {
        let Some(baked) = self
            .shapes
            .shadow(f.gfx, r.w, r.h, radius, s.radius, f.scale)
        else {
            return;
        };
        let m = baked.margin;
        let dest = rect_f(r.x - m, r.y - m + s.y, r.w + 2.0 * m, r.h + 2.0 * m);
        let Some(brush) = self.brush(f, ink.alpha(s.alpha * fill_alpha)) else {
            return;
        };
        // SAFETY: FillOpacityMask needs aliased mode; restored right after.
        unsafe {
            f.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            f.dc.FillOpacityMask(&baked.bitmap, &brush, Some(&dest), None);
            f.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    /// A space's gradient over the window's ground (`ThemeBackdrop`): its
    /// stops evenly along a run from the top-leading corner down to the
    /// bottom, leaning as far right as a column is wide for its height, so
    /// the column shows the whole run the way the Mac's does; past the end
    /// the last stop holds.
    fn fill_gradient(&mut self, f: &mut Frame, r: Rect, stops: &[Color], alpha: f32) {
        let n = stops.len();
        if n < 2 || r.is_empty() {
            return;
        }
        let gs: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .enumerate()
            .map(|(i, c)| D2D1_GRADIENT_STOP {
                position: i as f32 / (n - 1) as f32,
                color: color_f(*c),
            })
            .collect();
        // SAFETY: brush creation and drawing on the live context.
        unsafe {
            let Ok(collection) = f.dc.CreateGradientStopCollection(
                &gs,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_COLOR_SPACE_SRGB,
                D2D1_BUFFER_PRECISION_8BPC_UNORM,
                D2D1_EXTEND_MODE_CLAMP,
                D2D1_COLOR_INTERPOLATION_MODE_STRAIGHT,
            ) else {
                return;
            };
            let Ok(collection) =
                windows_core::Interface::cast::<ID2D1GradientStopCollection>(&collection)
            else {
                return;
            };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 { X: r.x, Y: r.y },
                endPoint: Vector2 {
                    X: r.x + r.h * 0.21,
                    Y: r.bottom(),
                },
            };
            let bp = D2D1_BRUSH_PROPERTIES {
                opacity: alpha,
                transform: Matrix3x2::identity(),
            };
            let Ok(brush) =
                f.dc.CreateLinearGradientBrush(&props, Some(&bp), &collection)
            else {
                return;
            };
            f.dc.FillRectangle(&d2r(r), &brush);
        }
    }

    fn fill_shape(&mut self, f: &mut Frame, r: Rect, radius: f32, c: Color) {
        if c.a <= 0.0 || r.is_empty() {
            return;
        }
        let Some(brush) = self.brush(f, c) else {
            return;
        };
        if radius <= 0.0 {
            // SAFETY: drawing on the live context.
            unsafe { f.dc.FillRectangle(&d2r(r), &brush) };
            return;
        }
        let Some(g) = self.shapes.rounded(f.gfx, r.w, r.h, radius) else {
            return;
        };
        self.with_offset(f, r.origin(), |dc| {
            // SAFETY: drawing on the live context.
            unsafe { dc.FillGeometry(&g, &brush, None) };
        });
    }

    fn stroke_shape(&mut self, f: &mut Frame, r: Rect, radius: f32, width: f32, c: Color) {
        if c.a <= 0.0 || r.is_empty() {
            return;
        }
        let Some(brush) = self.brush(f, c) else {
            return;
        };
        if radius <= 0.0 {
            // SAFETY: drawing on the live context.
            unsafe { f.dc.DrawRectangle(&d2r(r), &brush, width, None) };
            return;
        }
        let Some(g) = self.shapes.rounded(f.gfx, r.w, r.h, radius) else {
            return;
        };
        self.with_offset(f, r.origin(), |dc| {
            // SAFETY: drawing on the live context.
            unsafe { dc.DrawGeometry(&g, &brush, width, None) };
        });
    }

    /// Run `draw` with the drawing origin moved to `p`.
    fn with_offset(&self, f: &Frame, p: Point, draw: impl FnOnce(&ID2D1DeviceContext)) {
        let mut before = Matrix3x2::identity();
        // SAFETY: transform get/set on the live context.
        unsafe {
            f.dc.GetTransform(&mut before);
            f.dc.SetTransform(&(Matrix3x2::translation(p.x, p.y) * before));
        }
        draw(f.dc);
        // SAFETY: as above.
        unsafe { f.dc.SetTransform(&before) };
    }

    fn push_layer(&self, f: &Frame, mask: Option<ID2D1Geometry>, mask_at: Matrix3x2, opacity: f32) {
        let params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: f32::MIN,
                top: f32::MIN,
                right: f32::MAX,
                bottom: f32::MAX,
            },
            geometricMask: ManuallyDrop::new(mask),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: mask_at,
            opacity,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        // SAFETY: the context AddRefs what it keeps; our references are
        // released when `params` fields are dropped below.
        unsafe { f.dc.PushLayer(&params, None) };
        let D2D1_LAYER_PARAMETERS1 {
            geometricMask,
            opacityBrush,
            ..
        } = params;
        drop(ManuallyDrop::into_inner(geometricMask));
        drop(ManuallyDrop::into_inner(opacityBrush));
    }

    fn push_clip(&mut self, f: &mut Frame, r: Rect, radius: f32) {
        if radius > 0.0
            && let Some(g) = self.shapes.rounded(f.gfx, r.w, r.h, radius)
        {
            // The world transform applies to a layer's mask as well, so the
            // mask's own transform only places it (applying the world
            // transform here too would shift it by the surface's atlas
            // offset and clip away whatever it misses).
            self.push_layer(f, Some(g), Matrix3x2::translation(r.x, r.y), 1.0);
            return;
        }
        // SAFETY: balanced by pop_clip.
        unsafe {
            f.dc.PushAxisAlignedClip(&d2r(r), D2D1_ANTIALIAS_MODE_ALIASED)
        };
    }

    fn pop_clip(&mut self, f: &mut Frame, radius: f32) {
        // SAFETY: matches push_clip, which pushed a layer exactly when the
        // radius is positive and the shape could be built (it is cached, so
        // the second lookup agrees with the first).
        unsafe {
            if radius > 0.0 {
                f.dc.PopLayer();
            } else {
                f.dc.PopAxisAlignedClip();
            }
        }
    }

    fn content(&mut self, f: &mut Frame, id: NodeId, n: &Node, r: Rect) {
        match &n.content {
            Content::Text(t) => {
                let Some(layout) = f.fonts.text_layout(id, t, r.w, r.h) else {
                    return;
                };
                let (ink, hover, ht) = n.ink_now();
                let c = mixed(f.theme, ink, hover, ht, None, 0.0);
                self.text(f, &layout, r.origin(), c, false);
            }
            Content::Field(_) => self.field(f, id, n, r),
            Content::Image(img) => {
                if let Some(b) = self.bitmap(f, *img) {
                    // SAFETY: drawing on the live context.
                    unsafe {
                        f.dc.DrawBitmap(
                            &b,
                            Some(&d2r(r)),
                            1.0,
                            D2D1_INTERPOLATION_MODE_LINEAR,
                            None,
                            None,
                        );
                    }
                }
            }
            Content::Ring(ring) => {
                let (ink, hover, ht) = n.ink_now();
                let c = mixed(f.theme, ink, hover, ht, None, 0.0);
                self.ring(f, r, *ring, c);
            }
            Content::None | Content::Slot => {}
        }
    }

    /// An open ring inscribed in `r` ([`Content::Ring`]).
    fn ring(&mut self, f: &mut Frame, r: Rect, ring: crate::node::Ring, c: Color) {
        if c.a <= 0.0 || r.is_empty() {
            return;
        }
        let d = r.w.min(r.h);
        let Some(g) = self.shapes.arc(f.gfx, d, ring.width, ring.trim) else {
            return;
        };
        let Some(style) = self.shapes.round_caps(f.gfx) else {
            return;
        };
        let Some(brush) = self.brush(f, c) else {
            return;
        };
        let at = Point::new(r.x + (r.w - d) / 2.0, r.y + (r.h - d) / 2.0);
        self.with_offset(f, at, |dc| {
            // SAFETY: drawing on the live context.
            unsafe { dc.DrawGeometry(&g, &brush, ring.width, &style) };
        });
    }

    fn text(&mut self, f: &mut Frame, layout: &IDWriteTextLayout, at: Point, c: Color, clip: bool) {
        if c.a <= 0.0 {
            return;
        }
        let Some(brush) = self.brush(f, c) else {
            return;
        };
        let mut options = D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT;
        if clip {
            options = D2D1_DRAW_TEXT_OPTIONS(options.0 | D2D1_DRAW_TEXT_OPTIONS_CLIP.0);
        }
        // SAFETY: drawing on the live context.
        unsafe { f.dc.DrawTextLayout(v2(at), layout, &brush, options) };
    }

    fn field(&mut self, f: &mut Frame, id: NodeId, n: &Node, r: Rect) {
        let Some(field) = n.field_ref() else {
            return;
        };
        let focused = n.state.focused;
        let composing = field.edit.composition().is_some_and(|c| !c.text.is_empty());
        let show_caret = focused && f.active && field.caret_on;
        let px = 1.0 / f.scale;
        let caret_w = self.caret_pixels as f32 * px;
        let scale = f.scale;
        let snap = |x: f32| (x * scale).round() / scale;
        // SAFETY: balanced below.
        unsafe {
            f.dc.PushAxisAlignedClip(&d2r(r), D2D1_ANTIALIAS_MODE_ALIASED)
        };
        if field.edit.text().is_empty() && !composing {
            if let Some(l) = f.fonts.placeholder_layout(id, field, r.w, r.h) {
                let c = f.theme.color(field.placeholder_ink);
                self.text(f, &l, r.origin(), c, true);
            }
            if show_caret {
                let lh = f.fonts.line_height(field.size, field.weight);
                let rtl = crate::edit::first_strong_rtl(&field.placeholder);
                let x = if rtl { r.right() - caret_w } else { r.x };
                let caret = Rect::new(snap(x), r.y + (r.h - lh) / 2.0, caret_w, lh);
                let ink = f.theme.color(n.look.ink);
                self.fill_shape(f, caret, 0.0, ink);
            }
        } else if let Some(fl) = f.fonts.field_layout(id, field, r.w, r.h) {
            let origin = Point::new(r.x - field.scroll_x, r.y);
            let edit = &field.edit;
            if edit.has_selection() {
                let sel = edit.selection();
                let comp_len = edit.composition().map_or(0, |c| c.text.len());
                let shift = |b: usize| {
                    if b >= edit.caret() && comp_len > 0 {
                        b + comp_len
                    } else {
                        b
                    }
                };
                let a = f.fonts.field_utf16(id, field.shown_byte(shift(sel.start)));
                let b = f.fonts.field_utf16(id, field.shown_byte(shift(sel.end)));
                let mut rects = std::mem::take(&mut self.rects);
                fl.range_rects(a..b, &mut rects);
                let c = if focused {
                    f.theme.token(Token::Selection)
                } else {
                    f.theme.token(Token::Hover)
                };
                for q in &rects {
                    self.fill_shape(f, q.offset(origin), 0.0, c);
                }
                self.rects = rects;
            }
            let ink = f.theme.color(n.look.ink);
            self.text(f, &fl.layout, origin, ink, false);
            if let Some(comp) = edit.composition() {
                let start = edit.caret();
                let mut rects = std::mem::take(&mut self.rects);
                let clauses = comp.clauses.len().max(1);
                for i in 0..clauses {
                    let (range, target) = match comp.clauses.get(i) {
                        Some(cl) => (cl.range.clone(), cl.target),
                        None => (0..comp.text.len(), false),
                    };
                    let a = f
                        .fonts
                        .field_utf16(id, field.shown_byte(start + range.start));
                    let b = f.fonts.field_utf16(id, field.shown_byte(start + range.end));
                    fl.range_rects(a..b, &mut rects);
                    let thick = if target { 2.0 * px.max(0.5) } else { px };
                    for q in &rects {
                        // Leave a pixel between clauses, as IMEs draw them.
                        let line = Rect::new(
                            q.x + px,
                            q.bottom() - thick - px,
                            (q.w - 2.0 * px).max(px),
                            thick,
                        );
                        self.fill_shape(f, line.offset(origin), 0.0, ink);
                    }
                }
                self.rects = rects;
            }
            if field.shows_ghost()
                && let Some(g) = f.fonts.ghost_layout(id, field, r.h)
            {
                // Right after the text: where the caret stands at its end.
                let (x, _, _) = fl.caret();
                let c = f.theme.color(field.placeholder_ink);
                self.text(f, &g, Point::new(origin.x + x, origin.y), c, true);
            }
            if show_caret && !edit.has_selection() {
                let (x, top, h) = fl.caret();
                let x = snap(origin.x + x).min(r.right() - caret_w);
                let caret = Rect::new(x, origin.y + top, caret_w, h);
                self.fill_shape(f, caret, 0.0, ink);
            }
        }
        // SAFETY: matches the push above.
        unsafe { f.dc.PopAxisAlignedClip() };
    }
}

/// How far a layer's shadows and focus ring reach past its frame, in DIPs.
pub fn bleed(n: &Node) -> f32 {
    if n.flags.clip && !n.flags.focusable {
        return 0.0;
    }
    let mut b: f32 = 0.0;
    if let Some(s) = n.look.shadow {
        b = b.max(s.reach());
    }
    if let Some(s) = n.look.shadow2 {
        b = b.max(s.reach());
    }
    if n.flags.focusable {
        b = b.max(focus::RING_OUTSET + focus::RING_WIDTH);
    }
    b.ceil()
}
