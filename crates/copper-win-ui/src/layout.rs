//! A small flexbox: rows, columns and stacks, enough for browser chrome.
//!
//! Each node is measured (its natural size for the room it is offered) and
//! then arranged (given a frame, which places its children). Leaves with
//! content — text, a glyph, the field — are measured by the backend through
//! [`Measure`], so this module needs no fonts and is tested on any host.
//!
//! The rules, in CSS terms: `flex-direction` row or column, or a stack that
//! lays every child over the same box; `width`/`height` auto, fixed or
//! filling the parent; `flex-grow` and `flex-shrink`; `gap`; `padding`;
//! `align-items` with `align-self`; `justify-content`; and absolute children
//! placed from their parent's padding box. Frames are local to the parent.

use crate::geom::{Insets, Rect, Size};
use crate::tree::{NodeId, Tree};

/// How children are laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dir {
    Row,
    #[default]
    Column,
    /// Every child over the same box, each aligned on both axes.
    Stack,
}

/// One dimension of a node's size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Dim {
    /// As big as the content.
    #[default]
    Auto,
    /// Exactly this many DIPs.
    Px(f32),
    /// The whole of what the parent offers on this axis.
    Fill,
}

/// Cross-axis placement (and a stack's placement on both axes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

/// Main-axis distribution of what is left over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    /// Leftover split evenly between children.
    Between,
}

/// A child taken out of the flow and placed from its parent's padding box.
/// Edges left `None` are free: the size then comes from the node's own
/// `width`/`height` or its content.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Abs {
    pub left: Option<f32>,
    pub top: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
}

impl Abs {
    /// Pinned to all four edges: an overlay over the whole parent.
    pub const FILL: Abs = Abs {
        left: Some(0.0),
        top: Some(0.0),
        right: Some(0.0),
        bottom: Some(0.0),
    };

    pub const fn at(left: f32, top: f32) -> Abs {
        Abs {
            left: Some(left),
            top: Some(top),
            right: None,
            bottom: None,
        }
    }
}

/// Layout properties of one node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub dir: Dir,
    pub width: Dim,
    pub height: Dim,
    pub min_w: f32,
    pub max_w: f32,
    pub min_h: f32,
    pub max_h: f32,
    pub padding: Insets,
    pub gap: f32,
    /// How this node's children sit on the cross axis.
    pub align: Align,
    pub justify: Justify,
    /// This node's own cross-axis placement, overriding the parent's `align`.
    pub align_self: Option<Align>,
    pub grow: f32,
    pub shrink: f32,
    pub absolute: Option<Abs>,
    /// Out of layout and paint altogether (`display: none`).
    pub hidden: bool,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            dir: Dir::Column,
            width: Dim::Auto,
            height: Dim::Auto,
            min_w: 0.0,
            max_w: f32::INFINITY,
            min_h: 0.0,
            max_h: f32::INFINITY,
            padding: Insets::ZERO,
            gap: 0.0,
            align: Align::Stretch,
            justify: Justify::Start,
            align_self: None,
            grow: 0.0,
            shrink: 0.0,
            absolute: None,
            hidden: false,
        }
    }
}

/// Natural size of a leaf's content for the room offered (`f32::INFINITY`
/// where unconstrained). Implemented by the backend with DirectWrite.
pub trait Measure {
    fn measure(&mut self, tree: &Tree, id: NodeId, room: Size) -> Size;
}

/// A measurer for leaves that have no content of their own.
pub struct NoContent;

impl Measure for NoContent {
    fn measure(&mut self, _: &Tree, _: NodeId, _: Size) -> Size {
        Size::ZERO
    }
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi.max(lo))
}

/// Lay out the subtree at `root` into `frame` (local to its parent).
pub fn layout(tree: &mut Tree, root: NodeId, frame: Rect, m: &mut dyn Measure) {
    tree.begin_layout_pass();
    arrange(tree, root, frame, m);
}

/// The size `id` wants given `room` on each axis.
pub fn measure(tree: &mut Tree, id: NodeId, room: Size, m: &mut dyn Measure) -> Size {
    if let Some(hit) = tree.cached_measure(id, room) {
        return hit;
    }
    let size = measure_uncached(tree, id, room, m);
    tree.store_measure(id, room, size);
    size
}

fn measure_uncached(tree: &mut Tree, id: NodeId, room: Size, m: &mut dyn Measure) -> Size {
    let Some(node) = tree.get(id) else {
        return Size::ZERO;
    };
    let s = node.style;
    if s.hidden {
        return Size::ZERO;
    }
    let fixed_w = match s.width {
        Dim::Px(v) => Some(v),
        Dim::Fill if room.w.is_finite() => Some(room.w),
        _ => None,
    };
    let fixed_h = match s.height {
        Dim::Px(v) => Some(v),
        Dim::Fill if room.h.is_finite() => Some(room.h),
        _ => None,
    };
    if let (Some(w), Some(h)) = (fixed_w, fixed_h) {
        return Size::new(clamp(w, s.min_w, s.max_w), clamp(h, s.min_h, s.max_h));
    }
    let inner_room = Size::new(
        fixed_w.unwrap_or(room.w) - s.padding.horizontal(),
        fixed_h.unwrap_or(room.h) - s.padding.vertical(),
    );
    let children: Vec<NodeId> = flow_children(tree, id);
    let content = if children.is_empty() {
        m.measure(
            tree,
            id,
            Size::new(inner_room.w.max(0.0), inner_room.h.max(0.0)),
        )
    } else {
        let mut main = 0.0f32;
        let mut cross = 0.0f32;
        let mut n = 0;
        // A row gives what shrinks only the width its fixed parts leave,
        // as arranging will: text that wraps is measured at the width it
        // gets, so the row is as tall as it ends up.
        let shrink_room = if s.dir == Dir::Row && inner_room.w.is_finite() {
            let mut fixed = s.gap * children.len().saturating_sub(1) as f32;
            for c in &children {
                if tree.get(*c).is_some_and(|n| n.style.shrink == 0.0) {
                    fixed += measure(tree, *c, inner_room, m).w;
                }
            }
            Some(Size::new((inner_room.w - fixed).max(0.0), inner_room.h))
        } else {
            None
        };
        for c in &children {
            let room = match shrink_room {
                Some(r) if tree.get(*c).is_some_and(|n| n.style.shrink > 0.0) => r,
                _ => inner_room,
            };
            let cs = measure(tree, *c, room, m);
            n += 1;
            match s.dir {
                Dir::Row => {
                    main += cs.w;
                    cross = cross.max(cs.h);
                }
                Dir::Column => {
                    main += cs.h;
                    cross = cross.max(cs.w);
                }
                Dir::Stack => {
                    main = main.max(cs.w);
                    cross = cross.max(cs.h);
                }
            }
        }
        let gaps = if s.dir == Dir::Stack {
            0.0
        } else {
            s.gap * (n.max(1) - 1) as f32
        };
        match s.dir {
            Dir::Row | Dir::Stack => Size::new(main + gaps, cross),
            Dir::Column => Size::new(cross, main + gaps),
        }
    };
    let w = fixed_w.unwrap_or(content.w + s.padding.horizontal());
    let h = fixed_h.unwrap_or(content.h + s.padding.vertical());
    Size::new(clamp(w, s.min_w, s.max_w), clamp(h, s.min_h, s.max_h))
}

/// Children that take part in the flow (not hidden, not absolute).
fn flow_children(tree: &Tree, id: NodeId) -> Vec<NodeId> {
    tree.children(id)
        .iter()
        .copied()
        .filter(|c| {
            tree.get(*c)
                .is_some_and(|n| !n.style.hidden && n.style.absolute.is_none())
        })
        .collect()
}

fn arrange(tree: &mut Tree, id: NodeId, frame: Rect, m: &mut dyn Measure) {
    tree.set_frame(id, frame);
    let Some(node) = tree.get(id) else {
        return;
    };
    let s = node.style;
    let inner = Rect::from_size(frame.size()).inset(s.padding);
    let kids = flow_children(tree, id);

    if node.scroll.is_some() {
        // A scroll container's content is as tall as it wants to be: stack
        // it from the top, full width, never squeezed into the viewport.
        let mut y = inner.y;
        for c in kids {
            let cs = measure(tree, c, Size::new(inner.w, f32::INFINITY), m);
            arrange(tree, c, Rect::new(inner.x, y, inner.w, cs.h), m);
            y += cs.h + s.gap;
        }
    } else {
        arrange_flow(tree, &s, inner, kids, m);
    }
    arrange_absolute(tree, id, frame, &s, m);
}

/// Rows, columns and stacks.
fn arrange_flow(tree: &mut Tree, s: &Style, inner: Rect, kids: Vec<NodeId>, m: &mut dyn Measure) {
    match s.dir {
        Dir::Stack => {
            for c in kids {
                let cs = measure(tree, c, inner.size(), m);
                let cst = tree.get(c).map(|n| n.style).unwrap_or_default();
                let al = cst.align_self.unwrap_or(s.align);
                let w = if al == Align::Stretch && cst.width == Dim::Auto {
                    inner.w
                } else {
                    cs.w
                };
                let h = if al == Align::Stretch && cst.height == Dim::Auto {
                    inner.h
                } else {
                    cs.h
                };
                let x = place(inner.x, inner.w, w, al);
                let y = place(inner.y, inner.h, h, al);
                arrange(tree, c, Rect::new(x, y, w, h), m);
            }
        }
        Dir::Row | Dir::Column => {
            let row = s.dir == Dir::Row;
            let main_room = if row { inner.w } else { inner.h };
            let cross_room = if row { inner.h } else { inner.w };
            let n = kids.len();
            // Base sizes.
            let mut base: Vec<f32> = Vec::with_capacity(n);
            let mut cross: Vec<f32> = Vec::with_capacity(n);
            let mut styles = Vec::with_capacity(n);
            for c in &kids {
                let cst = tree.get(*c).map(|n| n.style).unwrap_or_default();
                let room = if row {
                    Size::new(f32::INFINITY, cross_room)
                } else {
                    Size::new(cross_room, f32::INFINITY)
                };
                let mut cs = measure(tree, *c, room, m);
                // A filling child takes the main room in full before flex.
                if row && cst.width == Dim::Fill {
                    cs.w = main_room;
                }
                if !row && cst.height == Dim::Fill {
                    cs.h = main_room;
                }
                base.push(if row { cs.w } else { cs.h });
                cross.push(if row { cs.h } else { cs.w });
                styles.push(cst);
            }
            let gaps = s.gap * n.saturating_sub(1) as f32;
            let natural = base.clone();
            let used: f32 = base.iter().sum::<f32>() + gaps;
            let free = main_room - used;
            if free > 0.0 {
                let grow: f32 = styles.iter().map(|st| st.grow).sum();
                if grow > 0.0 {
                    for (b, st) in base.iter_mut().zip(&styles) {
                        *b += free * st.grow / grow;
                    }
                }
            } else if free < 0.0 {
                let weight: f32 = base.iter().zip(&styles).map(|(b, st)| b * st.shrink).sum();
                if weight > 0.0 {
                    for (b, st) in base.iter_mut().zip(&styles) {
                        let lo = if row { st.min_w } else { st.min_h };
                        *b = (*b + free * (*b * st.shrink) / weight).max(lo);
                    }
                }
            }
            // Clamp to each child's own min/max on the main axis.
            for (b, st) in base.iter_mut().zip(&styles) {
                *b = if row {
                    clamp(*b, st.min_w, st.max_w)
                } else {
                    clamp(*b, st.min_h, st.max_h)
                };
            }
            // In a row, a child that shrank wraps at the width it got: it is
            // as tall as it measures there, not at its natural width.
            if row {
                for (i, c) in kids.iter().enumerate() {
                    if base[i] < natural[i] {
                        cross[i] = measure(tree, *c, Size::new(base[i], cross_room), m).h;
                    }
                }
            }
            let used: f32 = base.iter().sum::<f32>() + gaps;
            let left = (main_room - used).max(0.0);
            let (mut pos, between) = match s.justify {
                Justify::Start => (0.0, 0.0),
                Justify::Center => (left / 2.0, 0.0),
                Justify::End => (left, 0.0),
                Justify::Between if n > 1 => (0.0, left / (n - 1) as f32),
                Justify::Between => (0.0, 0.0),
            };
            for (i, c) in kids.iter().enumerate() {
                let st = &styles[i];
                let al = st.align_self.unwrap_or(s.align);
                let cdim = if row { st.height } else { st.width };
                let mut csz = if al == Align::Stretch && cdim == Dim::Auto || cdim == Dim::Fill {
                    cross_room
                } else {
                    cross[i]
                };
                csz = if row {
                    clamp(csz, st.min_h, st.max_h)
                } else {
                    clamp(csz, st.min_w, st.max_w)
                };
                let cpos = place(0.0, cross_room, csz, al);
                let r = if row {
                    Rect::new(inner.x + pos, inner.y + cpos, base[i], csz)
                } else {
                    Rect::new(inner.x + cpos, inner.y + pos, csz, base[i])
                };
                arrange(tree, *c, r, m);
                pos += base[i] + s.gap + between;
            }
        }
    }
}

/// Children taken out of the flow, placed from the padding box.
fn arrange_absolute(tree: &mut Tree, id: NodeId, frame: Rect, s: &Style, m: &mut dyn Measure) {
    let abs: Vec<NodeId> = tree
        .children(id)
        .iter()
        .copied()
        .filter(|c| {
            tree.get(*c)
                .is_some_and(|n| !n.style.hidden && n.style.absolute.is_some())
        })
        .collect();
    let pad = Rect::from_size(frame.size()).inset(s.padding);
    for c in abs {
        let st = tree.get(c).map(|n| n.style).unwrap_or_default();
        let a = st.absolute.unwrap_or_default();
        let room = Size::new(
            match (a.left, a.right) {
                (Some(l), Some(r)) => (pad.w - l - r).max(0.0),
                _ => pad.w,
            },
            match (a.top, a.bottom) {
                (Some(t), Some(b)) => (pad.h - t - b).max(0.0),
                _ => pad.h,
            },
        );
        let cs = measure(tree, c, room, m);
        let w = match (a.left, a.right, st.width) {
            (Some(_), Some(_), Dim::Auto) => room.w,
            _ => cs.w,
        };
        let h = match (a.top, a.bottom, st.height) {
            (Some(_), Some(_), Dim::Auto) => room.h,
            _ => cs.h,
        };
        let x = match (a.left, a.right) {
            (Some(l), _) => pad.x + l,
            (None, Some(r)) => pad.right() - r - w,
            (None, None) => pad.x,
        };
        let y = match (a.top, a.bottom) {
            (Some(t), _) => pad.y + t,
            (None, Some(b)) => pad.bottom() - b - h,
            (None, None) => pad.y,
        };
        arrange(tree, c, Rect::new(x, y, w, h), m);
    }
}

fn place(start: f32, room: f32, size: f32, al: Align) -> f32 {
    match al {
        Align::Start | Align::Stretch => start,
        Align::Center => start + (room - size) / 2.0,
        Align::End => start + room - size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    struct Fixed;
    impl Measure for Fixed {
        fn measure(&mut self, tree: &Tree, id: NodeId, room: Size) -> Size {
            // A "text" leaf: 8 DIPs per char, 18 high, never wider than room.
            let n = tree
                .get(id)
                .and_then(|n| n.content_text())
                .map_or(0, |t| t.chars().count());
            Size::new((n as f32 * 8.0).min(room.w), if n > 0 { 18.0 } else { 0.0 })
        }
    }

    fn frame(t: &Tree, id: NodeId) -> Rect {
        t.get(id).unwrap().frame
    }

    #[test]
    fn row_with_grow_and_fixed() {
        let mut t = Tree::new();
        let root = t.add_root(Node::row().padding(Insets::hv(10.0, 0.0)).gap(10.0));
        let mark = t.add(root, Node::new().size(16.0, 16.0));
        let title = t.add(
            root,
            Node::text("Office Commun", 15.0).grow(1.0).shrink(1.0),
        );
        let close = t.add(root, Node::new().size(15.0, 15.0));
        layout(&mut t, root, Rect::new(0.0, 0.0, 216.0, 37.0), &mut Fixed);
        assert_eq!(frame(&t, mark), Rect::new(10.0, 0.0, 16.0, 16.0));
        // 216 - 20 padding - 16 - 15 - 20 gaps = 145 for the title.
        assert_eq!(frame(&t, title).w, 145.0);
        assert_eq!(frame(&t, close).x, 216.0 - 10.0 - 15.0);
    }

    #[test]
    fn centred_cross_axis() {
        let mut t = Tree::new();
        let root = t.add_root(Node::row().align(Align::Center).height(37.0));
        let mark = t.add(root, Node::new().size(16.0, 16.0));
        layout(&mut t, root, Rect::new(0.0, 0.0, 100.0, 37.0), &mut Fixed);
        assert_eq!(frame(&t, mark).y, 10.5);
    }

    #[test]
    fn a_row_measures_wrapping_text_at_the_width_it_gets() {
        // A "text" leaf that wraps: 8 DIPs per char, 18 per line.
        struct Wrapping;
        impl Measure for Wrapping {
            fn measure(&mut self, tree: &Tree, id: NodeId, room: Size) -> Size {
                let n = tree
                    .get(id)
                    .and_then(|n| n.content_text())
                    .map_or(0, |t| t.chars().count()) as f32;
                let w = (n * 8.0).min(room.w.max(8.0));
                let lines = (n * 8.0 / w).ceil().max(1.0);
                Size::new(w, if n > 0.0 { 18.0 * lines } else { 0.0 })
            }
        }
        let mut t = Tree::new();
        let root = t.add_root(Node::column());
        let row = t.add(root, Node::row().gap(10.0).align(Align::Center));
        // 40 chars = 320 wide: one line at the row's 330, two at the 290
        // the switch leaves it.
        let words = t.add(row, Node::text(&"x".repeat(40), 13.0).shrink(1.0));
        let switch = t.add(row, Node::new().size(30.0, 18.0));
        layout(
            &mut t,
            root,
            Rect::new(0.0, 0.0, 330.0, 400.0),
            &mut Wrapping,
        );
        assert_eq!(frame(&t, switch).w, 30.0);
        assert_eq!(frame(&t, words).w, 290.0);
        assert_eq!(frame(&t, words).h, 36.0);
        assert_eq!(frame(&t, row).h, 36.0);
    }

    #[test]
    fn shrinks_text_before_fixed_parts() {
        let mut t = Tree::new();
        let root = t.add_root(Node::row());
        let long = t.add(root, Node::text(&"x".repeat(40), 15.0).shrink(1.0));
        let fixed = t.add(root, Node::new().size(30.0, 10.0));
        layout(&mut t, root, Rect::new(0.0, 0.0, 200.0, 20.0), &mut Fixed);
        assert_eq!(frame(&t, fixed).w, 30.0);
        assert_eq!(frame(&t, long).w, 170.0);
    }

    #[test]
    fn column_auto_height_and_gap() {
        let mut t = Tree::new();
        let root = t.add_root(Node::column());
        let list = t.add(root, Node::column().gap(4.0));
        for _ in 0..3 {
            t.add(list, Node::new().height(37.0));
        }
        layout(&mut t, root, Rect::new(0.0, 0.0, 232.0, 600.0), &mut Fixed);
        assert_eq!(frame(&t, list).h, 3.0 * 37.0 + 2.0 * 4.0);
        assert_eq!(frame(&t, list).w, 232.0);
    }

    #[test]
    fn justify_end_and_between() {
        let mut t = Tree::new();
        let root = t.add_root(Node::row().justify(Justify::Between));
        let a = t.add(root, Node::new().size(10.0, 10.0));
        let b = t.add(root, Node::new().size(10.0, 10.0));
        layout(&mut t, root, Rect::new(0.0, 0.0, 100.0, 10.0), &mut Fixed);
        assert_eq!(frame(&t, a).x, 0.0);
        assert_eq!(frame(&t, b).x, 90.0);
    }

    #[test]
    fn absolute_children() {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let over = t.add(root, Node::new().absolute(Abs::FILL));
        let corner = t.add(
            root,
            Node::new().size(46.0, 34.0).absolute(Abs {
                right: Some(0.0),
                top: Some(0.0),
                ..Abs::default()
            }),
        );
        layout(&mut t, root, Rect::new(0.0, 0.0, 800.0, 600.0), &mut Fixed);
        assert_eq!(frame(&t, over), Rect::new(0.0, 0.0, 800.0, 600.0));
        assert_eq!(frame(&t, corner), Rect::new(754.0, 0.0, 46.0, 34.0));
    }

    #[test]
    fn hidden_nodes_take_no_room() {
        let mut t = Tree::new();
        let root = t.add_root(Node::column().gap(4.0));
        t.add(root, Node::new().height(10.0));
        t.add(root, Node::new().height(10.0).hidden(true));
        let last = t.add(root, Node::new().height(10.0));
        layout(&mut t, root, Rect::new(0.0, 0.0, 10.0, 100.0), &mut Fixed);
        assert_eq!(frame(&t, last).y, 14.0);
    }

    #[test]
    fn min_and_max_clamp() {
        let mut t = Tree::new();
        let root = t.add_root(Node::row());
        let side = t.add(root, Node::new().width(500.0).max_width(440.0));
        let stage = t.add(root, Node::new().grow(1.0));
        layout(&mut t, root, Rect::new(0.0, 0.0, 1000.0, 10.0), &mut Fixed);
        assert_eq!(frame(&t, side).w, 440.0);
        assert_eq!(frame(&t, stage).x, 440.0);
        assert_eq!(frame(&t, stage).w, 560.0);
    }
}
