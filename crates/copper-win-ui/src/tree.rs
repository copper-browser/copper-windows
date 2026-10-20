//! The retained node tree: storage, change tracking, hit testing, focus,
//! pointer states and animation.
//!
//! Nodes live in a slab and are named by generation-checked [`NodeId`]s, so a
//! stale id held by the app reads as "gone" rather than as another node.
//!
//! The tree knows which *layers* need repainting. A layer is a node with
//! `flags.layer` (and always the root): it gets its own composition visual
//! and surface, and every non-layer node beneath it paints into it. Changing
//! a node marks its layer; moving, fading or scaling a layer marks only its
//! visual, which the compositor applies without drawing anything. A layer
//! always composites above what its parent layer paints, so content that
//! must sit over a layer (the row titles over the sliding pill) is a layer
//! of its own, after it in document order.

use crate::geom::{Point, Rect, Size};
use crate::layout::{self, Measure};
use crate::node::{Content, Cursor, Node};
use crate::scroll::Scroller;
use crate::spring::{Animated, Motion};
use crate::tokens::motion;

/// A node's name. Stale ids (of removed nodes) resolve to nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

impl NodeId {
    /// A stable small number for this node (UIA runtime ids, logging).
    pub fn raw(&self) -> u64 {
        (u64::from(self.generation) << 32) | u64::from(self.index)
    }
}

/// An animatable property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    /// `transform.translate.x`, DIPs.
    X,
    /// `transform.translate.y`, DIPs.
    Y,
    Opacity,
    Scale,
    /// `style.width` in DIPs (relayouts every frame; use sparingly).
    Width,
    /// `style.height` in DIPs.
    Height,
    /// The eased hover state (0–1).
    Hover,
    /// The eased press state (0–1).
    Press,
    /// How far through a refusal shake (0–1): it sways `translate.x`
    /// (see [`Tree::shake`]).
    Shake,
}

#[derive(Clone, Copy, Debug)]
struct PropAnim {
    id: NodeId,
    prop: Prop,
    value: Animated,
}

#[derive(Clone, Copy, Debug, Default)]
struct MeasureCache {
    pass: u32,
    room: Size,
    out: Size,
}

#[derive(Debug, Default)]
struct Slot {
    node: Option<Node>,
    generation: u32,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    measure: [MeasureCache; 2],
    queued_paint: bool,
}

/// Where a shake `travel` (0–1) through puts its node: three
/// there-and-backs, tapering to nothing, so it settles rather than stopping
/// mid-swing (`Design.swift` 246–250).
pub fn shake_offset(travel: f32) -> f32 {
    let decay = 1.0 - travel;
    (travel * std::f32::consts::PI * 6.0).sin() * motion::SHAKE_TRAVEL * decay
}

/// What lies under a point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pick {
    /// The innermost pointer target (`flags.hit`).
    pub target: Option<NodeId>,
    /// The point is on a window-drag region with no target above it.
    pub drag: bool,
    /// The cursor to show.
    pub cursor: Cursor,
}

/// The tree.
#[derive(Debug, Default)]
pub struct Tree {
    slots: Vec<Slot>,
    free: Vec<u32>,
    root: Option<NodeId>,
    pass: u32,
    layout_dirty: bool,
    last_size: Size,
    dirty_layers: Vec<NodeId>,
    visuals_dirty: bool,
    /// Some layer was ever made with [`Flags::conceal`](crate::node::Flags)
    /// (hover then re-reads the visuals too).
    concealed_layers: bool,
    structure_rev: u32,
    hover_chain: Vec<NodeId>,
    hovered: Option<NodeId>,
    pressed: Option<NodeId>,
    focused: Option<NodeId>,
    anims: Vec<PropAnim>,
    scrolling: Vec<NodeId>,
    a11y_rev: u32,
    /// Jump instead of animating (Windows' "Animation effects" off).
    pub reduce_motion: bool,
}

impl Tree {
    pub fn new() -> Tree {
        Tree::default()
    }

    // — structure —

    fn alloc(&mut self, mut node: Node, parent: Option<NodeId>) -> NodeId {
        if node.flags.conceal && node.flags.layer {
            self.concealed_layers = true;
        }
        if node.scroll.is_some() {
            node.flags.layer = true;
        }
        let index = match self.free.pop() {
            Some(i) => i,
            None => {
                self.slots.push(Slot::default());
                (self.slots.len() - 1) as u32
            }
        };
        let slot = &mut self.slots[index as usize];
        slot.generation = slot.generation.wrapping_add(1);
        slot.node = Some(node);
        slot.parent = parent;
        slot.children.clear();
        slot.measure = Default::default();
        slot.queued_paint = false;
        NodeId {
            index,
            generation: slot.generation,
        }
    }

    fn slot(&self, id: NodeId) -> Option<&Slot> {
        self.slots
            .get(id.index as usize)
            .filter(|s| s.generation == id.generation && s.node.is_some())
    }

    fn slot_mut(&mut self, id: NodeId) -> Option<&mut Slot> {
        self.slots
            .get_mut(id.index as usize)
            .filter(|s| s.generation == id.generation && s.node.is_some())
    }

    /// Set the root (always a layer). Replaces any previous tree.
    pub fn add_root(&mut self, mut node: Node) -> NodeId {
        if let Some(old) = self.root {
            self.remove(old);
        }
        node.flags.layer = true;
        let id = self.alloc(node, None);
        self.root = Some(id);
        self.structure_changed();
        self.mark_paint(id);
        id
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Append `node` under `parent`.
    pub fn add(&mut self, parent: NodeId, node: Node) -> NodeId {
        let at = self.children(parent).len();
        self.insert(parent, at, node)
    }

    /// Insert `node` under `parent` at `index`.
    pub fn insert(&mut self, parent: NodeId, index: usize, node: Node) -> NodeId {
        let layer = node.flags.layer || node.scroll.is_some();
        let id = self.alloc(node, Some(parent));
        if let Some(p) = self.slot_mut(parent) {
            let i = index.min(p.children.len());
            p.children.insert(i, id);
        }
        if layer {
            self.structure_changed();
            self.mark_paint(id);
        }
        self.mark_paint(parent);
        self.layout_dirty = true;
        id
    }

    /// Remove `id` and everything under it.
    pub fn remove(&mut self, id: NodeId) {
        let Some(slot) = self.slot(id) else {
            return;
        };
        let parent = slot.parent;
        if let Some(p) = parent {
            self.mark_paint(p);
            if let Some(ps) = self.slot_mut(p) {
                ps.children.retain(|c| *c != id);
            }
        }
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if let Some(s) = self.slot_mut(n) {
                stack.append(&mut s.children);
                s.node = None;
                s.parent = None;
                self.free.push(n.index);
            }
            if self.hovered == Some(n) {
                self.hovered = None;
            }
            if self.pressed == Some(n) {
                self.pressed = None;
            }
            if self.focused == Some(n) {
                self.focused = None;
            }
        }
        let slots = &self.slots;
        let alive = |n: &NodeId| {
            slots
                .get(n.index as usize)
                .is_some_and(|s| s.generation == n.generation && s.node.is_some())
        };
        self.hover_chain.retain(alive);
        self.anims.retain(|a| alive(&a.id));
        self.scrolling.retain(alive);
        self.dirty_layers.retain(alive);
        if self.root == Some(id) {
            self.root = None;
        }
        self.structure_changed();
        self.layout_dirty = true;
    }

    /// Remove every child of `id`.
    pub fn clear_children(&mut self, id: NodeId) {
        let kids: Vec<NodeId> = self.children(id).to_vec();
        for k in kids {
            self.remove(k);
        }
    }

    /// Move `id` to position `index` among its siblings.
    pub fn reorder(&mut self, id: NodeId, index: usize) {
        let Some(parent) = self.parent(id) else {
            return;
        };
        if let Some(p) = self.slot_mut(parent) {
            p.children.retain(|c| *c != id);
            let i = index.min(p.children.len());
            p.children.insert(i, id);
        }
        self.structure_changed();
        self.layout_dirty = true;
        self.mark_paint(parent);
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.slot(id).and_then(|s| s.node.as_ref())
    }

    /// Mutable access without change tracking; call [`Tree::mark_paint`] or
    /// [`Tree::mark_layout`] after. Prefer [`Tree::edit`] and friends.
    pub fn get_mut_untracked(&mut self, id: NodeId) -> Option<&mut Node> {
        self.slot_mut(id).and_then(|s| s.node.as_mut())
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.slot(id).is_some()
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.slot(id).map_or(&[], |s| &s.children)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.slot(id).and_then(|s| s.parent)
    }

    /// Whether `id` is `ancestor` or lies beneath it.
    pub fn is_within(&self, id: NodeId, ancestor: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.parent(c);
        }
        false
    }

    /// The first node in document order whose tag is `tag`.
    pub fn find_tag(&self, tag: u64) -> Option<NodeId> {
        let mut out = None;
        self.walk(|id, n| {
            if out.is_none() && n.tag == tag {
                out = Some(id);
            }
        });
        out
    }

    /// Visit every node in document (paint) order.
    pub fn walk(&self, mut f: impl FnMut(NodeId, &Node)) {
        let Some(root) = self.root else {
            return;
        };
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if let Some(s) = self.slot(id) {
                if let Some(n) = &s.node {
                    f(id, n);
                }
                stack.extend(s.children.iter().rev());
            }
        }
    }

    // — change tracking —

    fn structure_changed(&mut self) {
        self.structure_rev = self.structure_rev.wrapping_add(1);
        self.a11y_rev = self.a11y_rev.wrapping_add(1);
    }

    /// Bumped whenever layers come, go or reorder: the backend rebuilds its
    /// visual tree when this changes.
    pub fn structure_rev(&self) -> u32 {
        self.structure_rev
    }

    /// Bumped when anything UI Automation reports may have changed.
    pub fn a11y_rev(&self) -> u32 {
        self.a11y_rev
    }

    /// The layer `id` paints into: itself if it is one, else the nearest
    /// layer above it.
    pub fn layer_of(&self, id: NodeId) -> Option<NodeId> {
        let mut cur = Some(id);
        while let Some(c) = cur {
            let s = self.slot(c)?;
            if s.node.as_ref().is_some_and(|n| n.flags.layer) || s.parent.is_none() {
                return Some(c);
            }
            cur = s.parent;
        }
        None
    }

    /// How far a [`reveal`](crate::node::Flags::reveal) node is shown: the
    /// eased hover of the nearest pointer target above it (0–1). Hovering
    /// a target lights its target ancestors too, so a revealed button stays
    /// shown while the pointer is on it.
    pub fn reveal_t(&self, id: NodeId) -> f32 {
        self.hover_above(id)
    }

    /// How far a node is shown by its [`reveal`](crate::node::Flags::reveal)
    /// and [`conceal`](crate::node::Flags::conceal) flags (1 when neither).
    pub fn shown_t(&self, id: NodeId) -> f32 {
        let Some(n) = self.get(id) else { return 1.0 };
        let mut t = 1.0;
        if n.flags.reveal {
            t *= self.hover_above(id);
        }
        if n.flags.conceal {
            t *= 1.0 - self.hover_above(id);
        }
        t
    }

    fn hover_above(&self, id: NodeId) -> f32 {
        let mut cur = self.parent(id);
        while let Some(c) = cur {
            match self.get(c) {
                Some(n) if n.flags.hit => return n.state.hover_t.max(n.state.press_t),
                Some(_) => cur = self.parent(c),
                None => break,
            }
        }
        0.0
    }

    /// `id`'s layer must be redrawn.
    pub fn mark_paint(&mut self, id: NodeId) {
        let Some(layer) = self.layer_of(id) else {
            return;
        };
        if let Some(s) = self.slot_mut(layer)
            && !s.queued_paint
        {
            s.queued_paint = true;
            self.dirty_layers.push(layer);
        }
    }

    /// Sizes must be worked out again.
    pub fn mark_layout(&mut self) {
        self.layout_dirty = true;
    }

    pub fn needs_layout(&self) -> bool {
        self.layout_dirty
    }

    /// Some layer moved, faded or scaled.
    pub fn mark_visuals(&mut self) {
        self.visuals_dirty = true;
    }

    /// The layers to redraw, cleared. The caller supplies the buffer so a
    /// steady frame allocates nothing.
    pub fn take_dirty_layers(&mut self, out: &mut Vec<NodeId>) {
        out.clear();
        for id in self.dirty_layers.drain(..) {
            if let Some(s) = self
                .slots
                .get_mut(id.index as usize)
                .filter(|s| s.generation == id.generation && s.node.is_some())
            {
                s.queued_paint = false;
                out.push(id);
            }
        }
    }

    pub fn has_dirty_layers(&self) -> bool {
        !self.dirty_layers.is_empty()
    }

    /// Whether layer transforms changed (without resetting).
    pub fn has_visual_changes(&self) -> bool {
        self.visuals_dirty
    }

    /// Whether layer transforms changed since the last call (and reset).
    pub fn take_visuals_dirty(&mut self) -> bool {
        std::mem::take(&mut self.visuals_dirty)
    }

    /// Change a node's look or content without changing its size.
    pub fn repaint(&mut self, id: NodeId, f: impl FnOnce(&mut Node)) {
        if let Some(n) = self.get_mut_untracked(id) {
            f(n);
            self.mark_paint(id);
            self.a11y_rev = self.a11y_rev.wrapping_add(1);
        }
    }

    /// Change anything about a node (relayout and repaint).
    pub fn edit(&mut self, id: NodeId, f: impl FnOnce(&mut Node)) {
        if let Some(n) = self.get_mut_untracked(id) {
            let was_layer = n.flags.layer;
            f(n);
            if n.scroll.is_some() {
                n.flags.layer = true;
            }
            let is_layer = n.flags.layer;
            if was_layer != is_layer {
                self.structure_changed();
            }
            self.mark_paint(id);
            if let Some(p) = self.parent(id) {
                self.mark_paint(p);
            }
            self.layout_dirty = true;
            self.a11y_rev = self.a11y_rev.wrapping_add(1);
        }
    }

    /// Replace a text node's text; nothing happens when it is unchanged.
    pub fn set_text(&mut self, id: NodeId, text: &str) {
        let same = self.get(id).and_then(Node::text_str) == Some(text);
        if same {
            return;
        }
        self.edit(id, |n| {
            if let Content::Text(t) = &mut n.content {
                t.text.clear();
                t.text.push_str(text);
                t.revision = t.revision.wrapping_add(1);
            }
            if n.a11y.role == crate::node::Role::Text {
                n.a11y.name.clear();
                n.a11y.name.push_str(text);
            }
        });
    }

    /// Change a text node's weight (the live tab's title turns semibold).
    pub fn set_weight(&mut self, id: NodeId, weight: crate::node::Weight) {
        let same = matches!(self.get(id).map(|n| &n.content), Some(Content::Text(t)) if t.weight == weight);
        if same {
            return;
        }
        self.edit(id, |n| {
            if let Content::Text(t) = &mut n.content {
                t.weight = weight;
                t.revision = t.revision.wrapping_add(1);
            }
        });
    }

    pub fn set_hidden(&mut self, id: NodeId, hidden: bool) {
        if self.get(id).is_some_and(|n| n.style.hidden != hidden) {
            self.edit(id, |n| n.style.hidden = hidden);
            self.structure_changed();
        }
    }

    pub fn set_selected(&mut self, id: NodeId, on: bool) {
        if self.get(id).is_some_and(|n| n.flags.selected != on) {
            self.repaint(id, |n| n.flags.selected = on);
        }
    }

    /// Edit a field's text model; repaints and bumps its revision.
    pub fn with_field<R>(
        &mut self,
        id: NodeId,
        f: impl FnOnce(&mut crate::node::Field) -> R,
    ) -> Option<R> {
        let n = self.get_mut_untracked(id)?;
        let field = n.field_mut()?;
        let r = f(field);
        field.revision = field.revision.wrapping_add(1);
        field.caret_on = true;
        self.mark_paint(id);
        self.a11y_rev = self.a11y_rev.wrapping_add(1);
        Some(r)
    }

    // — layout —

    pub(crate) fn begin_layout_pass(&mut self) {
        self.pass = self.pass.wrapping_add(1);
    }

    pub(crate) fn cached_measure(&self, id: NodeId, room: Size) -> Option<Size> {
        let s = self.slot(id)?;
        s.measure
            .iter()
            .find(|c| c.pass == self.pass && c.room == room)
            .map(|c| c.out)
    }

    pub(crate) fn store_measure(&mut self, id: NodeId, room: Size, out: Size) {
        let pass = self.pass;
        if let Some(s) = self.slot_mut(id) {
            s.measure[1] = s.measure[0];
            s.measure[0] = MeasureCache { pass, room, out };
        }
    }

    pub(crate) fn set_frame(&mut self, id: NodeId, frame: Rect) {
        let Some(n) = self.get(id) else {
            return;
        };
        let old = n.frame;
        if old == frame {
            return;
        }
        let layer = n.flags.layer;
        let resized = old.size() != frame.size();
        if let Some(n) = self.get_mut_untracked(id) {
            n.frame = frame;
        }
        if layer {
            self.visuals_dirty = true;
            if resized {
                self.mark_paint(id);
            }
        } else {
            self.mark_paint(id);
        }
        self.a11y_rev = self.a11y_rev.wrapping_add(1);
    }

    /// Lay the tree out in a window of `size` DIPs if anything changed.
    /// Returns whether it ran.
    pub fn layout(&mut self, size: Size, m: &mut dyn Measure) -> bool {
        let Some(root) = self.root else {
            return false;
        };
        if !self.layout_dirty && size == self.last_size {
            return false;
        }
        self.layout_dirty = false;
        self.last_size = size;
        layout::layout(self, root, Rect::from_size(size), m);
        self.sync_scrollers();
        self.visuals_dirty = true;
        true
    }

    /// Hand each scroller its viewport and content heights after layout.
    fn sync_scrollers(&mut self) {
        let mut ids = Vec::new();
        self.walk(|id, n| {
            if n.scroll.is_some() {
                ids.push(id);
            }
        });
        for id in ids {
            let content = self
                .children(id)
                .iter()
                .filter_map(|c| self.get(*c))
                .filter(|c| !c.style.hidden)
                .map(|c| c.frame.bottom())
                .fold(0.0f32, f32::max);
            let Some(n) = self.get_mut_untracked(id) else {
                continue;
            };
            let pad = n.style.padding;
            let viewport = n.frame.h;
            if let Some(s) = n.scroll.as_mut() {
                let before = s.offset();
                s.set_extent(viewport, content + pad.bottom);
                if s.offset() != before {
                    self.mark_paint(id);
                }
            }
        }
    }

    // — geometry —

    /// The scroll offset a scroll node applies to its children.
    pub fn scroll_offset(&self, id: NodeId) -> f32 {
        self.get(id)
            .and_then(|n| n.scroll.as_ref())
            .map_or(0.0, |s| s.offset())
    }

    /// Where the origin of `id`'s parent's content is, in window DIPs.
    fn content_origin(&self, id: NodeId) -> Point {
        let mut p = Point::ZERO;
        let mut cur = self.parent(id);
        while let Some(c) = cur {
            let Some(n) = self.get(c) else {
                break;
            };
            p.x += n.frame.x + n.transform.translate.x;
            p.y += n.frame.y + n.transform.translate.y - self.scroll_offset(c);
            cur = self.parent(c);
        }
        p
    }

    /// `id`'s frame in window DIPs, after scrolling and translation.
    pub fn window_rect(&self, id: NodeId) -> Rect {
        let Some(n) = self.get(id) else {
            return Rect::ZERO;
        };
        n.frame
            .offset(self.content_origin(id))
            .offset(n.transform.translate)
    }

    /// The part of `id` that is not clipped away by its ancestors.
    pub fn visible_rect(&self, id: NodeId) -> Rect {
        let mut r = self.window_rect(id);
        let mut cur = self.parent(id);
        while let Some(c) = cur {
            if self.get(c).is_some_and(|n| n.flags.clip) {
                r = r.intersect(&self.window_rect(c));
            }
            cur = self.parent(c);
        }
        r
    }

    /// Whether `id` and all its ancestors are shown.
    pub fn is_shown(&self, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            match self.get(c) {
                Some(n) if !n.style.hidden && n.transform.opacity > 0.01 => {}
                _ => return false,
            }
            cur = self.parent(c);
        }
        true
    }

    // — hit testing —

    /// What is under `p` (window DIPs).
    pub fn pick(&self, p: Point) -> Pick {
        let Some(root) = self.root else {
            return Pick::default();
        };
        let top = self.topmost(root, Point::ZERO, p);
        let mut pick = Pick::default();
        let mut cur = top;
        while let Some(c) = cur {
            let Some(n) = self.get(c) else {
                break;
            };
            if n.flags.hit && !n.flags.disabled {
                pick.target = Some(c);
                pick.cursor = n.cursor;
                break;
            }
            if n.flags.drag {
                pick.drag = true;
                break;
            }
            cur = self.parent(c);
        }
        pick
    }

    /// The innermost, topmost node that is solid at `p`. `origin` is the
    /// window position of `id`'s parent's content box.
    fn topmost(&self, id: NodeId, origin: Point, p: Point) -> Option<NodeId> {
        let n = self.get(id)?;
        if n.style.hidden || n.flags.inert || n.transform.opacity <= 0.01 {
            return None;
        }
        let frame = n.frame.offset(origin).offset(n.transform.translate);
        let inside = frame.contains(p);
        if n.flags.clip && !inside {
            return None;
        }
        let child_origin = Point::new(frame.x, frame.y - self.scroll_offset(id));
        for c in self.children(id).iter().rev() {
            if let Some(hit) = self.topmost(*c, child_origin, p) {
                return Some(hit);
            }
        }
        let solid = n.flags.hit
            || n.flags.drag
            || !n.look.fill.is_clear()
            || !matches!(n.content, Content::None);
        (inside && solid).then_some(id)
    }

    // — pointer states —

    /// The pointer moved to `p`. Returns the new pick; hover states and their
    /// easing are updated.
    pub fn pointer_move(&mut self, p: Point) -> Pick {
        let pick = self.pick(p);
        self.set_hovered(pick.target);
        pick
    }

    /// The pointer left the window.
    pub fn pointer_leave(&mut self) {
        self.set_hovered(None);
    }

    pub fn hovered(&self) -> Option<NodeId> {
        self.hovered
    }

    /// Set the hovered target; it and its hit ancestors light up.
    pub fn set_hovered(&mut self, target: Option<NodeId>) {
        if self.hovered == target {
            return;
        }
        self.hovered = target;
        let mut chain = std::mem::take(&mut self.hover_chain);
        let old: Vec<NodeId> = chain.clone();
        chain.clear();
        let mut cur = target;
        while let Some(c) = cur {
            if self.get(c).is_some_and(|n| n.flags.hit) {
                chain.push(c);
            }
            cur = self.parent(c);
        }
        for id in &old {
            if !chain.contains(id) {
                self.set_state(*id, Prop::Hover, false);
            }
        }
        for id in &chain {
            if !old.contains(id) {
                self.set_state(*id, Prop::Hover, true);
            }
        }
        self.hover_chain = chain;
    }

    /// A button went down on `target`.
    pub fn press(&mut self, target: Option<NodeId>) {
        if let Some(old) = self.pressed.take() {
            self.set_state(old, Prop::Press, false);
        }
        if let Some(t) = target
            && self.get(t).is_some_and(|n| !n.flags.disabled)
        {
            self.pressed = Some(t);
            self.set_state(t, Prop::Press, true);
        }
    }

    pub fn pressed(&self) -> Option<NodeId> {
        self.pressed
    }

    /// The button came up over `target`. Returns the node clicked, if the
    /// press began and ended on it.
    pub fn release(&mut self, target: Option<NodeId>) -> Option<NodeId> {
        let pressed = self.pressed.take()?;
        self.set_state(pressed, Prop::Press, false);
        let over = target.is_some_and(|t| self.is_within(t, pressed));
        over.then_some(pressed)
    }

    /// Flip a node's hover or press flag and ease its mix weight.
    fn set_state(&mut self, id: NodeId, prop: Prop, on: bool) {
        let Some(n) = self.get_mut_untracked(id) else {
            return;
        };
        match prop {
            Prop::Hover => n.state.hovered = on,
            Prop::Press => n.state.pressed = on,
            _ => {}
        }
        let to = if on { 1.0 } else { 0.0 };
        // Presses land at once and let go gently; hovers ease both ways.
        if prop == Prop::Press && on {
            self.set_prop(id, prop, to);
        } else {
            self.animate(id, prop, to, motion::QUICK);
        }
    }

    // — focus —

    pub fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    /// Focus `id` (or nothing). `visible`: the keyboard moved it, so the
    /// ring shows.
    pub fn focus(&mut self, id: Option<NodeId>, visible: bool) {
        let id = id.filter(|i| self.get(*i).is_some_and(|n| n.flags.focusable));
        if self.focused == id {
            if let Some(i) = id {
                self.repaint(i, |n| n.state.focus_visible = visible);
            }
            return;
        }
        if let Some(old) = self.focused.take() {
            self.repaint(old, |n| {
                n.state.focused = false;
                n.state.focus_visible = false;
            });
        }
        if let Some(i) = id {
            self.repaint(i, |n| {
                n.state.focused = true;
                n.state.focus_visible = visible;
            });
        }
        self.focused = id;
    }

    /// The innermost open modal (the last in document order that is shown).
    pub fn modal(&self) -> Option<NodeId> {
        let mut out = None;
        self.walk(|id, n| {
            if n.flags.modal && !n.style.hidden {
                out = Some(id);
            }
        });
        out.filter(|m| self.is_shown(*m))
    }

    /// Focusable nodes in tab order (document order), inside the open modal
    /// if there is one.
    pub fn tab_order(&self, out: &mut Vec<NodeId>) {
        out.clear();
        let scope = self.modal().or(self.root);
        let Some(scope) = scope else {
            return;
        };
        let mut stack = vec![scope];
        while let Some(id) = stack.pop() {
            let Some(n) = self.get(id) else {
                continue;
            };
            if n.style.hidden || n.transform.opacity <= 0.01 || n.flags.disabled {
                continue;
            }
            if n.flags.focusable {
                out.push(id);
            }
            stack.extend(self.children(id).iter().rev());
        }
    }

    /// The next (or previous) focus stop after the focused one, wrapping.
    pub fn next_focus(&self, forward: bool) -> Option<NodeId> {
        let mut order = Vec::new();
        self.tab_order(&mut order);
        if order.is_empty() {
            return None;
        }
        let at = self
            .focused
            .and_then(|f| order.iter().position(|o| *o == f));
        let i = match (at, forward) {
            (None, true) => 0,
            (None, false) => order.len() - 1,
            (Some(i), true) => (i + 1) % order.len(),
            (Some(i), false) => (i + order.len() - 1) % order.len(),
        };
        Some(order[i])
    }

    // — animation —

    fn read_prop(&self, id: NodeId, prop: Prop) -> f32 {
        let Some(n) = self.get(id) else {
            return 0.0;
        };
        match prop {
            Prop::X => n.transform.translate.x,
            Prop::Y => n.transform.translate.y,
            Prop::Opacity => n.transform.opacity,
            Prop::Scale => n.transform.scale,
            Prop::Width => match n.style.width {
                crate::layout::Dim::Px(v) => v,
                _ => n.frame.w,
            },
            Prop::Height => match n.style.height {
                crate::layout::Dim::Px(v) => v,
                _ => n.frame.h,
            },
            Prop::Hover => n.state.hover_t,
            Prop::Press => n.state.press_t,
            Prop::Shake => n.state.shake_t,
        }
    }

    fn write_prop(&mut self, id: NodeId, prop: Prop, v: f32) {
        let Some(n) = self.get_mut_untracked(id) else {
            return;
        };
        let layer = n.flags.layer;
        match prop {
            Prop::X => n.transform.translate.x = v,
            Prop::Y => n.transform.translate.y = v,
            Prop::Opacity => n.transform.opacity = v,
            Prop::Scale => n.transform.scale = v,
            Prop::Width => n.style.width = crate::layout::Dim::Px(v),
            Prop::Height => n.style.height = crate::layout::Dim::Px(v),
            Prop::Hover => n.state.hover_t = v,
            Prop::Press => n.state.press_t = v,
            Prop::Shake => {
                n.state.shake_t = v;
                n.transform.translate.x = shake_offset(v);
            }
        }
        match prop {
            Prop::X | Prop::Y | Prop::Opacity | Prop::Scale | Prop::Shake if layer => {
                self.visuals_dirty = true
            }
            Prop::X | Prop::Y | Prop::Opacity | Prop::Scale | Prop::Shake => {
                if let Some(p) = self.parent(id) {
                    self.mark_paint(p);
                }
                self.mark_paint(id);
            }
            Prop::Width | Prop::Height => self.layout_dirty = true,
            Prop::Hover | Prop::Press => {
                self.mark_paint(id);
                // A concealed layer below fades through its visual.
                if self.concealed_layers {
                    self.visuals_dirty = true;
                }
            }
        }
        if matches!(prop, Prop::Opacity) {
            self.a11y_rev = self.a11y_rev.wrapping_add(1);
        }
    }

    /// Set a property at once, stopping any animation of it.
    pub fn set_prop(&mut self, id: NodeId, prop: Prop, v: f32) {
        self.anims.retain(|a| !(a.id == id && a.prop == prop));
        self.write_prop(id, prop, v);
    }

    /// Animate a property to `to` with `motion`, from wherever it is now
    /// (keeping its velocity if it was already moving).
    pub fn animate(&mut self, id: NodeId, prop: Prop, to: f32, how: Motion) {
        if self.reduce_motion && !matches!(prop, Prop::Hover | Prop::Press) {
            self.set_prop(id, prop, to);
            return;
        }
        if let Some(a) = self.anims.iter_mut().find(|a| a.id == id && a.prop == prop) {
            a.value.set(to, how);
            return;
        }
        let from = self.read_prop(id, prop);
        if (from - to).abs() < 1e-5 {
            return;
        }
        let mut value = Animated::new(from);
        value.set(to, how);
        self.anims.push(PropAnim { id, prop, value });
    }

    /// Shake `id` side to side: the "not that" of a field that cannot go
    /// where it was asked (`Design.swift` `Shake`, `easeOut(duration: 0.5)`).
    /// Under reduced motion it does not move.
    pub fn shake(&mut self, id: NodeId) {
        self.set_prop(id, Prop::Shake, 0.0);
        self.animate(
            id,
            Prop::Shake,
            1.0,
            Motion::ease_out(motion::SHAKE_SECONDS),
        );
    }

    /// The value a property is heading for.
    pub fn target_of(&self, id: NodeId, prop: Prop) -> f32 {
        self.anims
            .iter()
            .find(|a| a.id == id && a.prop == prop)
            .map_or_else(|| self.read_prop(id, prop), |a| a.value.target())
    }

    /// Scroll `id` with `f` and keep stepping it until it rests.
    pub fn scroll(&mut self, id: NodeId, f: impl FnOnce(&mut Scroller)) {
        let Some(n) = self.get_mut_untracked(id) else {
            return;
        };
        let Some(s) = n.scroll.as_mut() else {
            return;
        };
        let before = s.offset();
        f(s);
        let moved = s.offset() != before;
        if !self.scrolling.contains(&id) {
            self.scrolling.push(id);
        }
        if moved {
            self.scrolled(id);
        }
    }

    /// The nearest scroll container at or above `id`.
    pub fn scroller_of(&self, id: NodeId) -> Option<NodeId> {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if self.get(c).is_some_and(|n| n.scroll.is_some()) {
                return Some(c);
            }
            cur = self.parent(c);
        }
        None
    }

    fn scrolled(&mut self, id: NodeId) {
        self.mark_paint(id);
        // Layers inside moved with the content.
        self.visuals_dirty = true;
        self.a11y_rev = self.a11y_rev.wrapping_add(1);
    }

    /// Whether anything is animating or scrolling.
    pub fn is_animating(&self) -> bool {
        !self.anims.is_empty() || !self.scrolling.is_empty()
    }

    /// Advance every animation and scroller by `dt` seconds. Returns whether
    /// anything is still moving. Allocates nothing.
    pub fn step(&mut self, dt: f32) -> bool {
        let mut i = 0;
        while i < self.anims.len() {
            let moving = self.anims[i].value.step(dt);
            let PropAnim { id, prop, value } = self.anims[i];
            self.write_prop(id, prop, value.value());
            if moving {
                i += 1;
            } else {
                self.anims.swap_remove(i);
            }
        }
        let mut i = 0;
        while i < self.scrolling.len() {
            let id = self.scrolling[i];
            let mut moved = false;
            let mut active = false;
            let mut indicator = 0.0;
            if let Some(s) = self.get_mut_untracked(id).and_then(|n| n.scroll.as_mut()) {
                let before = s.offset();
                let was = s.indicator();
                active = s.step(dt);
                moved = s.offset() != before;
                indicator = (s.indicator() - was).abs();
            }
            if moved {
                self.scrolled(id);
            } else if indicator > 0.0 {
                self.mark_paint(id);
            }
            if active {
                i += 1;
            } else {
                self.scrolling.swap_remove(i);
            }
        }
        self.is_animating()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Insets;
    use crate::layout::{Abs, NoContent};

    fn sample() -> (Tree, NodeId, NodeId, NodeId) {
        let mut t = Tree::new();
        let root = t.add_root(Node::row());
        let side = t.add(
            root,
            Node::column().width(232.0).drag().padding(Insets::all(8.0)),
        );
        let row = t.add(side, Node::row().height(37.0).button("Office Commun"));
        let _title = t.add(row, Node::text("Office Commun", 15.0));
        let page = t.add(root, Node::slot().grow(1.0));
        t.layout(Size::new(1000.0, 700.0), &mut NoContent);
        (t, side, row, page)
    }

    #[test]
    fn pick_bubbles_to_targets_and_drag_regions() {
        let (t, side, row, page) = sample();
        let p = t.pick(Point::new(20.0, 20.0));
        assert_eq!(p.target, Some(row));
        let p = t.pick(Point::new(20.0, 400.0));
        assert_eq!(p.target, None);
        assert!(p.drag);
        let p = t.pick(Point::new(500.0, 400.0));
        assert_eq!(p.target, Some(page));
        assert!(t.is_within(row, side));
    }

    #[test]
    fn stale_ids_read_as_gone() {
        let (mut t, _, row, _) = sample();
        t.remove(row);
        assert!(t.get(row).is_none());
        let side = t.children(t.root().unwrap())[0];
        let again = t.add(side, Node::new());
        assert_ne!(again, row);
        assert!(t.get(row).is_none());
    }

    #[test]
    fn hover_chain_and_click() {
        let (mut t, _, row, _) = sample();
        let close = t.add(row, Node::new().size(15.0, 15.0).button("Close"));
        t.layout(Size::new(1000.0, 700.0), &mut NoContent);
        let r = t.window_rect(close);
        let p = t.pointer_move(r.center());
        assert_eq!(p.target, Some(close));
        assert!(t.get(row).unwrap().state.hovered);
        t.press(p.target);
        assert_eq!(t.release(Some(close)), Some(close));
        t.press(Some(row));
        assert_eq!(t.release(None), None);
        t.pointer_leave();
        assert!(!t.get(row).unwrap().state.hovered);
        while t.step(1.0 / 60.0) {}
        assert_eq!(t.get(row).unwrap().state.hover_t, 0.0);
    }

    #[test]
    fn only_dirty_layers_are_reported() {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let a = t.add(root, Node::new().layer().size(10.0, 10.0));
        let b = t.add(root, Node::new().layer().size(10.0, 10.0));
        let inside_b = t.add(b, Node::new().size(5.0, 5.0));
        t.layout(Size::new(100.0, 100.0), &mut NoContent);
        let mut out = Vec::new();
        t.take_dirty_layers(&mut out);
        t.repaint(inside_b, |n| n.look.radius = 2.0);
        t.take_dirty_layers(&mut out);
        assert_eq!(out, vec![b]);
        // Moving a layer only dirties visuals.
        t.set_prop(a, Prop::Y, 20.0);
        t.take_dirty_layers(&mut out);
        assert!(out.is_empty());
        assert!(t.take_visuals_dirty());
    }

    #[test]
    fn a_shake_sways_and_settles_home() {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let a = t.add(root, Node::new().layer());
        t.shake(a);
        let mut widest = 0.0f32;
        while t.step(1.0 / 120.0) {
            widest = widest.max(t.get(a).unwrap().transform.translate.x.abs());
        }
        assert!(widest > 3.0 && widest <= 7.0, "{widest}");
        assert!(t.get(a).unwrap().transform.translate.x.abs() < 1e-4);
        assert_eq!(shake_offset(0.0), 0.0);
        assert!(shake_offset(1.0).abs() < 1e-6);
    }

    #[test]
    fn tab_order_respects_modals() {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let a = t.add(root, Node::new().button("a"));
        let b = t.add(root, Node::new().button("b"));
        assert_eq!(t.next_focus(true), Some(a));
        t.focus(Some(a), true);
        assert_eq!(t.next_focus(true), Some(b));
        assert_eq!(t.next_focus(false), Some(b));
        let card = t.add(root, Node::column().modal().absolute(Abs::FILL));
        let field = t.add(card, Node::field("Search", 17.5));
        assert_eq!(t.next_focus(true), Some(field));
        t.set_hidden(card, true);
        assert_eq!(t.next_focus(true), Some(b));
    }

    #[test]
    fn springs_drive_layers() {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let pill = t.add(root, Node::new().layer());
        t.animate(pill, Prop::Y, 41.0, motion::GLIDE);
        let mut frames = 0;
        while t.step(1.0 / 60.0) {
            frames += 1;
        }
        assert_eq!(t.get(pill).unwrap().transform.translate.y, 41.0);
        // A glide takes about half a second to rest.
        assert!((15..60).contains(&frames), "{frames}");
        t.reduce_motion = true;
        t.animate(pill, Prop::Y, 0.0, motion::GLIDE);
        assert!(!t.is_animating());
    }

    #[test]
    fn scrolled_content_hit_tests_where_drawn() {
        let mut t = Tree::new();
        let root = t.add_root(Node::column());
        let list = t.add(root, Node::scroll().height(100.0));
        let content = t.add(list, Node::column());
        let mut rows = Vec::new();
        for i in 0..20 {
            rows.push(t.add(content, Node::new().height(20.0).button(&i.to_string())));
        }
        t.layout(Size::new(200.0, 300.0), &mut NoContent);
        t.scroll(list, |s| s.scroll_to(40.0, false));
        assert_eq!(t.pick(Point::new(10.0, 5.0)).target, Some(rows[2]));
        // Below the viewport the list is clipped.
        assert_eq!(t.pick(Point::new(10.0, 150.0)).target, None);
        assert_eq!(t.window_rect(rows[2]).y, 0.0);
    }
}
