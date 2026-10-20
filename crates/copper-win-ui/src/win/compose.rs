//! Layers to DirectComposition visuals.
//!
//! Each layer node owns a visual whose content is one composition surface
//! (its frame plus a bleed for shadows and the focus ring), and a second
//! visual inside it that holds the layers nested in it, clipped to the
//! layer's frame when the layer clips. A page's slot gets one more visual,
//! beneath the nested layers, for the engine to host its content in.
//!
//! Moving, fading or scaling a layer only changes its visual — nothing is
//! redrawn — and each property is written only when it changed, so a
//! steady frame makes no DirectComposition calls at all.

use std::collections::HashMap;

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::ID2D1DeviceContext;
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionAnimation, IDCompositionRotateTransform, IDCompositionTarget,
    IDCompositionTransform3D, IDCompositionVirtualSurface, IDCompositionVisual,
    IDCompositionVisual3,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
};
use windows::core::{Interface, Result};
use windows_numerics::{Matrix3x2, Matrix4x4};

use super::gfx::Gfx;
use super::paint::{Frame, Painter, bleed};
use super::text::Fonts;
use crate::geom::{Point, Rect};
use crate::node::SPIN_PERIOD;
use crate::theme::Theme;
use crate::tree::{NodeId, Tree};

/// `AddVisual(v, insertAbove, None)`: with no reference visual, `FALSE`
/// puts the new visual above all its siblings and `TRUE` below them all
/// (measured on Windows 11: the reverse of what the parameter's name
/// suggests).
const STACK_TOP: bool = false;
const STACK_BOTTOM: bool = true;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Applied {
    x: f32,
    y: f32,
    opacity: f32,
    scale: f32,
    anchor: (f32, f32),
    clip: Option<(f32, f32, f32, f32)>,
    kids_clip: Option<(f32, f32)>,
    kids_at: f32,
    /// Turning ([`crate::node::Flags::spin`], shown) about `center`.
    spin: bool,
    center: (f32, f32),
}

struct Layer {
    visual: IDCompositionVisual3,
    kids: IDCompositionVisual3,
    slot: Option<IDCompositionVisual3>,
    /// A spinning layer's turn: its rotation and the endless animation
    /// driving the angle.
    spin: Option<(IDCompositionRotateTransform, IDCompositionAnimation)>,
    surface: Option<IDCompositionVirtualSurface>,
    size: (u32, u32),
    bleed_px: u32,
    parent: Option<NodeId>,
    applied: Option<Applied>,
    /// Needs drawing whatever the tree says (new, resized, device rebuilt).
    fresh: bool,
    /// The frame it was last drawn in, so it is drawn once per frame.
    drawn: u64,
}

/// One window's visual tree.
pub struct Scene {
    hwnd: HWND,
    target: Option<IDCompositionTarget>,
    layers: HashMap<NodeId, Layer>,
    structure_rev: u32,
    generation: u32,
    scale: f32,
    frame: u64,
    scratch: Vec<NodeId>,
}

fn new_visual(gfx: &Gfx) -> Result<IDCompositionVisual3> {
    // SAFETY: visual creation on the live device.
    unsafe { gfx.dcomp.CreateVisual()?.cast() }
}

fn px(v: f32, scale: f32) -> f32 {
    (v * scale).round()
}

impl Scene {
    pub fn new(hwnd: HWND) -> Scene {
        Scene {
            hwnd,
            target: None,
            layers: HashMap::new(),
            structure_rev: u32::MAX,
            generation: u32::MAX,
            scale: 1.0,
            frame: 0,
            scratch: Vec::new(),
        }
    }

    /// Every surface must be drawn at a new scale.
    pub fn set_scale(&mut self, scale: f32) {
        if scale != self.scale {
            self.scale = scale;
            for l in self.layers.values_mut() {
                l.fresh = true;
                l.applied = None;
            }
        }
    }

    /// Forget everything device-bound (after device loss).
    pub fn reset(&mut self) {
        self.layers.clear();
        self.target = None;
        self.generation = u32::MAX;
        self.structure_rev = u32::MAX;
    }

    /// The visual a slot's page is hosted in.
    pub fn slot_visual(&self, id: NodeId) -> Option<IDCompositionVisual> {
        self.layers.get(&id)?.slot.as_ref()?.cast().ok()
    }

    /// Rebuild the visual tree if layers came, went or moved in the tree.
    /// Slots whose visual is new are pushed to `new_slots`.
    pub fn sync_structure(
        &mut self,
        gfx: &Gfx,
        tree: &Tree,
        new_slots: &mut Vec<NodeId>,
    ) -> Result<()> {
        if self.generation != gfx.generation {
            self.layers.clear();
            self.target = None;
            self.generation = gfx.generation;
            self.structure_rev = u32::MAX;
        }
        if self.target.is_none() {
            // SAFETY: a target for our own window, below any child windows.
            self.target = Some(unsafe { gfx.dcomp.CreateTargetForHwnd(self.hwnd, true)? });
        }
        if self.structure_rev == tree.structure_rev() {
            return Ok(());
        }
        self.structure_rev = tree.structure_rev();

        let mut order = std::mem::take(&mut self.scratch);
        order.clear();
        let root = tree.root();
        tree.walk(|id, n| {
            if n.flags.layer || Some(id) == root {
                order.push(id);
            }
        });
        self.layers
            .retain(|id, _| tree.contains(*id) && order.contains(id));
        for &id in &order {
            let is_slot = tree.get(id).is_some_and(|n| n.is_slot());
            let spins = tree.get(id).is_some_and(|n| n.flags.spin);
            let parent = tree.parent(id).and_then(|p| tree.layer_of(p));
            if let Some(l) = self.layers.get_mut(&id) {
                l.parent = parent;
                // SAFETY: detaching children of our own visual.
                unsafe { l.kids.RemoveAllVisuals()? };
                if let Some(s) = &l.slot {
                    // SAFETY: re-adding our own slot visual at the bottom
                    // (see `STACK_BOTTOM`).
                    unsafe {
                        l.kids
                            .AddVisual(s, STACK_BOTTOM, None::<&IDCompositionVisual>)?
                    };
                }
                continue;
            }
            let visual = new_visual(gfx)?;
            let kids = new_visual(gfx)?;
            // SAFETY: building our own visual tree.
            unsafe { visual.AddVisual(&kids, true, None::<&IDCompositionVisual>)? };
            let slot = if is_slot {
                let s = new_visual(gfx)?;
                // SAFETY: as above.
                unsafe { kids.AddVisual(&s, STACK_BOTTOM, None::<&IDCompositionVisual>)? };
                new_slots.push(id);
                Some(s)
            } else {
                None
            };
            let spin = if spins { Some(turn(gfx)?) } else { None };
            self.layers.insert(
                id,
                Layer {
                    visual,
                    kids,
                    slot,
                    spin,
                    surface: None,
                    size: (0, 0),
                    bleed_px: 0,
                    parent,
                    applied: None,
                    fresh: true,
                    drawn: 0,
                },
            );
        }
        for &id in &order {
            let Some(parent) = self.layers.get(&id).and_then(|l| l.parent) else {
                continue;
            };
            let (Some(child), Some(p)) = (self.layers.get(&id), self.layers.get(&parent)) else {
                continue;
            };
            // SAFETY: each visual is added to exactly one parent, on top,
            // in document order, so later layers sit above earlier ones.
            unsafe {
                p.kids
                    .AddVisual(&child.visual, STACK_TOP, None::<&IDCompositionVisual>)?
            };
        }
        if let (Some(t), Some(r)) = (&self.target, root.and_then(|r| self.layers.get(&r))) {
            // SAFETY: rooting our tree in our target.
            unsafe { t.SetRoot(&r.visual)? };
        }
        self.scratch = order;
        Ok(())
    }

    /// Bring every layer's position, opacity, scale and clip up to date.
    pub fn sync_visuals(&mut self, tree: &Tree) -> Result<()> {
        let s = self.scale;
        let ids: &mut Vec<NodeId> = &mut self.scratch;
        ids.clear();
        ids.extend(self.layers.keys().copied());
        for &id in ids.iter() {
            let Some(n) = tree.get(id) else {
                continue;
            };
            let w = tree.window_rect(id);
            let b = bleed(n);
            let bleed_px = (b * s).ceil();
            let origin = (px(w.x, s), px(w.y, s));
            let Some(l) = self.layers.get(&id) else {
                continue;
            };
            let parent_origin = l
                .parent
                .map(|p| {
                    let pw = tree.window_rect(p);
                    (px(pw.x, s), px(pw.y, s))
                })
                .unwrap_or((0.0, 0.0));
            // Opacity of this node and of the plain nodes between it and its
            // parent layer (a parent layer's own opacity is inherited by the
            // visual tree).
            let mut opacity = n.transform.opacity;
            if n.flags.reveal || n.flags.conceal {
                opacity *= tree.shown_t(id);
            }
            let mut clip: Option<Rect> = None;
            let mut cur = tree.parent(id);
            while let Some(c) = cur {
                if Some(c) == l.parent {
                    break;
                }
                if let Some(cn) = tree.get(c) {
                    opacity *= cn.transform.opacity;
                    if cn.flags.clip {
                        let cr = tree.window_rect(c);
                        clip = Some(clip.map_or(cr, |k| k.intersect(&cr)));
                    }
                }
                cur = tree.parent(c);
            }
            let shown = tree.is_shown(id);
            let vx = origin.0 - bleed_px - parent_origin.0;
            let vy = origin.1 - bleed_px - parent_origin.1;
            let a = Applied {
                x: vx,
                y: vy,
                opacity: if shown { opacity.clamp(0.0, 1.0) } else { 0.0 },
                scale: n.transform.scale,
                anchor: (
                    bleed_px + n.transform.anchor.x * w.w * s,
                    bleed_px + n.transform.anchor.y * w.h * s,
                ),
                clip: clip.map(|c| {
                    let x0 = px(c.x, s) - (origin.0 - bleed_px);
                    let y0 = px(c.y, s) - (origin.1 - bleed_px);
                    (x0, y0, x0 + px(c.w, s), y0 + px(c.h, s))
                }),
                kids_clip: n.flags.clip.then(|| (px(w.w, s), px(w.h, s))),
                kids_at: bleed_px,
                spin: n.flags.spin && shown && opacity > 0.0,
                center: (bleed_px + w.w * s / 2.0, bleed_px + w.h * s / 2.0),
            };
            if l.applied == Some(a) {
                continue;
            }
            let before = l.applied;
            apply(l, a, before)?;
            if let Some(l) = self.layers.get_mut(&id) {
                l.applied = Some(a);
            }
        }
        Ok(())
    }

    /// Draw the layers in `dirty`, and any that must be drawn anyway.
    /// Returns how many were drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gfx: &Gfx,
        tree: &Tree,
        dirty: &[NodeId],
        painter: &mut Painter,
        fonts: &mut Fonts,
        theme: &Theme,
        active: bool,
    ) -> Result<u32> {
        self.frame += 1;
        let mut count = 0;
        for &id in dirty {
            count += self.draw_one(gfx, tree, id, painter, fonts, theme, active)?;
        }
        let ids: &mut Vec<NodeId> = &mut self.scratch;
        ids.clear();
        ids.extend(
            self.layers
                .iter()
                .filter(|(_, l)| l.fresh)
                .map(|(id, _)| *id),
        );
        let fresh = std::mem::take(&mut self.scratch);
        for &id in &fresh {
            count += self.draw_one(gfx, tree, id, painter, fonts, theme, active)?;
        }
        self.scratch = fresh;
        Ok(count)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_one(
        &mut self,
        gfx: &Gfx,
        tree: &Tree,
        id: NodeId,
        painter: &mut Painter,
        fonts: &mut Fonts,
        theme: &Theme,
        active: bool,
    ) -> Result<u32> {
        let s = self.scale;
        let frame = self.frame;
        let Some(n) = tree.get(id) else {
            return Ok(0);
        };
        let Some(l) = self.layers.get_mut(&id) else {
            return Ok(0);
        };
        if l.drawn == frame {
            return Ok(0);
        }
        l.drawn = frame;
        l.fresh = false;
        let bleed_px = (bleed(n) * s).ceil() as u32;
        let w = (n.frame.w * s).round().max(0.0) as u32 + 2 * bleed_px;
        let h = (n.frame.h * s).round().max(0.0) as u32 + 2 * bleed_px;
        if w == 0 || h == 0 || n.style.hidden {
            if l.surface.is_some() {
                // SAFETY: clearing our own visual's content.
                unsafe { l.visual.SetContent(None::<&windows::core::IUnknown>)? };
                l.surface = None;
                l.size = (0, 0);
            }
            return Ok(0);
        }
        if l.bleed_px != bleed_px {
            l.bleed_px = bleed_px;
            l.applied = None;
        }
        match &l.surface {
            Some(sf) if l.size != (w, h) => {
                // SAFETY: resizing our own surface.
                unsafe { sf.Resize(w, h)? };
                l.size = (w, h);
            }
            Some(_) => {}
            None => {
                // SAFETY: surface creation on the live device.
                let sf = unsafe {
                    gfx.dcomp.CreateVirtualSurface(
                        w,
                        h,
                        DXGI_FORMAT_B8G8R8A8_UNORM,
                        DXGI_ALPHA_MODE_PREMULTIPLIED,
                    )?
                };
                // SAFETY: as above.
                unsafe { l.visual.SetContent(&sf)? };
                l.surface = Some(sf);
                l.size = (w, h);
            }
        }
        let Some(surface) = l.surface.clone() else {
            return Ok(0);
        };
        let mut offset = POINT::default();
        // SAFETY: BeginDraw/EndDraw are paired; the context is used only
        // in between.
        unsafe {
            let dc: ID2D1DeviceContext = surface.BeginDraw(None, &mut offset)?;
            dc.SetDpi(96.0 * s, 96.0 * s);
            dc.SetTransform(&Matrix3x2::translation(
                offset.x as f32 / s,
                offset.y as f32 / s,
            ));
            let mut f = Frame {
                dc: &dc,
                gfx,
                theme,
                fonts,
                scale: s,
                active,
            };
            let b = bleed_px as f32 / s;
            painter.paint_layer(&mut f, tree, id, Point::new(b, b));
            surface.EndDraw()?;
        }
        Ok(1)
    }
}

fn apply(l: &Layer, a: Applied, before: Option<Applied>) -> Result<()> {
    // SAFETY: setters on our own visuals.
    unsafe {
        if before.is_none_or(|b| b.x != a.x || b.y != a.y) {
            l.visual.SetOffsetX2(a.x)?;
            l.visual.SetOffsetY2(a.y)?;
        }
        if before.is_none_or(|b| b.opacity != a.opacity) {
            l.visual.SetOpacity2(a.opacity)?;
            l.visual.SetVisible(a.opacity > 0.0)?;
        }
        match &l.spin {
            // Turning while shown; a hidden spinner costs the compositor
            // nothing.
            Some((rotate, angle)) if a.spin => {
                if before.is_none_or(|b| !b.spin || b.center != a.center) {
                    rotate.SetCenterX2(a.center.0)?;
                    rotate.SetCenterY2(a.center.1)?;
                    rotate.SetAngle(angle)?;
                    let t: IDCompositionTransform3D = rotate.cast()?;
                    l.visual.SetTransform(&t)?;
                }
            }
            _ => {
                if before.is_none_or(|b| b.spin || b.scale != a.scale || b.anchor != a.anchor) {
                    let m = scale_about(a.scale, a.anchor.0, a.anchor.1);
                    l.visual.SetTransform2(&m)?;
                }
            }
        }
        if before.is_none_or(|b| b.clip != a.clip) {
            match a.clip {
                Some((x0, y0, x1, y1)) => l.visual.SetClip2(&D2D_RECT_F {
                    left: x0,
                    top: y0,
                    right: x1,
                    bottom: y1,
                })?,
                None => l.visual.SetClip(
                    None::<&windows::Win32::Graphics::DirectComposition::IDCompositionClip>,
                )?,
            }
        }
        if before.is_none_or(|b| b.kids_at != a.kids_at) {
            l.kids.SetOffsetX2(a.kids_at)?;
            l.kids.SetOffsetY2(a.kids_at)?;
        }
        if before.is_none_or(|b| b.kids_clip != a.kids_clip) {
            match a.kids_clip {
                Some((w, h)) => l.kids.SetClip2(&D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: w,
                    bottom: h,
                })?,
                None => l.kids.SetClip(
                    None::<&windows::Win32::Graphics::DirectComposition::IDCompositionClip>,
                )?,
            }
        }
    }
    Ok(())
}

/// A rotation whose angle runs 0→360° once every [`SPIN_PERIOD`], for
/// ever (`.linear(duration:).repeatForever(autoreverses: false)`).
fn turn(gfx: &Gfx) -> Result<(IDCompositionRotateTransform, IDCompositionAnimation)> {
    let period = f64::from(SPIN_PERIOD);
    // SAFETY: creating and scripting objects on the live device.
    unsafe {
        let rotate = gfx.dcomp.CreateRotateTransform()?;
        let angle = gfx.dcomp.CreateAnimation()?;
        angle.AddCubic(0.0, 0.0, 360.0 / SPIN_PERIOD, 0.0, 0.0)?;
        angle.AddRepeat(period, period)?;
        Ok((rotate, angle))
    }
}

/// A 2-D scale by `s` about (`ax`, `ay`), as the 4×4 matrix visuals take.
fn scale_about(s: f32, ax: f32, ay: f32) -> Matrix4x4 {
    Matrix4x4 {
        M11: s,
        M12: 0.0,
        M13: 0.0,
        M14: 0.0,
        M21: 0.0,
        M22: s,
        M23: 0.0,
        M24: 0.0,
        M31: 0.0,
        M32: 0.0,
        M33: 1.0,
        M34: 0.0,
        M41: ax * (1.0 - s),
        M42: ay * (1.0 - s),
        M43: 0.0,
        M44: 1.0,
    }
}
