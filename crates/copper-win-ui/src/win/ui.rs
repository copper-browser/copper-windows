//! The window's state as the app sees it: the tree, the theme, focus,
//! menus, slots — and the frame pipeline that turns them into pixels.

use std::collections::{HashMap, VecDeque};
use std::rc::Weak;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{GetCaretBlinkTime, KillTimer, SetTimer};

use super::clock::Clock;
use super::compose::Scene;
use super::frame::{self, Backdrop};
use super::gfx::{self, Gfx};
use super::paint::Painter;
use super::popup::{MenuPopup, TipPopup};
use super::slot::{FocusReason, SlotHost, Told};
use super::text::{self, Fonts, Measurer};
use super::window::Host;
use super::{ime, uia};
use crate::chrome::Caption;
use crate::event::Event;
use crate::geom::{Point, Rect, Size};
use crate::layout::Abs;
use crate::menu::Menu;
use crate::node::Node;
use crate::stats::{FrameStats, Summary};
use crate::theme::{Appearance, Theme};
use crate::tokens::Theme as SpaceTheme;
use crate::tree::{NodeId, Tree};

/// Timer: the caret blinks.
pub(super) const TIMER_CARET: usize = 1;
/// Timer: a tooltip is due.
pub(super) const TIMER_TIP: usize = 2;

/// Something to do once the window's state is no longer borrowed (these
/// calls send messages straight back to the window).
#[derive(Clone, Copy, Debug)]
pub(super) enum Command {
    SetPos(RECT),
    /// Give the window keyboard focus (UI Automation moved focus into it).
    FocusWindow,
    Minimize,
    ToggleMaximize,
    Destroy,
}

/// Who gets the mouse until the button comes up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Capture {
    #[default]
    None,
    /// A press on a node (click on release; drag-select in a field).
    Node(NodeId),
    /// A press in a page.
    Slot(NodeId),
}

/// A finger or pen panning a scroller.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pan {
    pub id: u32,
    pub scroller: Option<NodeId>,
    pub start: Point,
    pub last: Point,
    pub last_at: Instant,
    pub velocity: f32,
    pub moved: bool,
}

/// The window's state.
pub struct Ui {
    /// The nodes. Change them freely; the window redraws what changed.
    pub tree: Tree,
    pub(super) hwnd: HWND,
    pub(super) host: Weak<Host>,
    pub(super) content: NodeId,
    pub(super) caption: Caption,
    pub(super) gfx: Gfx,
    pub(super) scene: Scene,
    pub(super) painter: Painter,
    pub(super) fonts: Fonts,
    pub(super) theme: Theme,
    pub(super) appearance: Appearance,
    pub(super) system_dark: bool,
    pub(super) space: Option<SpaceTheme>,
    pub(super) backdrop: Backdrop,
    pub(super) mica: bool,
    pub(super) dpi: u32,
    pub(super) size: Size,
    pub(super) active: bool,
    pub(super) focused_window: bool,
    pub(super) clock: Clock,
    pub(super) events: VecDeque<Event>,
    pub(super) commands: Vec<Command>,
    dirty: Vec<NodeId>,
    new_slots: Vec<NodeId>,
    pub(super) slots: HashMap<NodeId, (Box<dyn SlotHost>, Told)>,
    pub(super) capture: Capture,
    pub(super) tracking: bool,
    pub(super) nc_tracking: bool,
    pub(super) clicks: (Instant, Point, u32),
    pub(super) high_surrogate: Option<u16>,
    pub(super) menu: Option<MenuPopup>,
    pub(super) tip: Option<TipPopup>,
    pub(super) tip_for: Option<NodeId>,
    pub(super) pan: Option<Pan>,
    pub(super) over_slot: Option<NodeId>,
    pub(super) ime_on: bool,
    pub(super) uia: uia::Bridge,
    stats: FrameStats,
    last_step: Option<Instant>,
    last_frame: Option<Instant>,
    pub(super) created: Instant,
    pub(super) shown: bool,
}

/// What the window is made with.
pub(super) struct Parts {
    pub hwnd: HWND,
    pub host: Weak<Host>,
    pub gfx: Gfx,
    pub fonts: Fonts,
    pub appearance: Appearance,
    pub backdrop: Backdrop,
    pub created: Instant,
}

impl Ui {
    pub(super) fn new(p: Parts) -> Ui {
        let system_dark = frame::system_dark();
        let dark = p.appearance.is_dark(system_dark);
        let mica = frame::backdrop(p.hwnd, p.backdrop, dark);
        let mut tree = Tree::new();
        tree.reduce_motion = !frame::animations_on();
        let root = tree.add_root(Node::stack());
        let content = tree.add(root, Node::stack().absolute(Abs::FILL));
        let caption = Caption::build(&mut tree, root);
        // SAFETY: a DPI query on our own window.
        let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(p.hwnd) }.max(96);
        let mut scene = Scene::new(p.hwnd);
        scene.set_scale(dpi as f32 / 96.0);
        let mut ui = Ui {
            tree,
            hwnd: p.hwnd,
            host: p.host.clone(),
            content,
            caption,
            gfx: p.gfx,
            scene,
            painter: Painter::new(),
            fonts: p.fonts,
            theme: Theme::new(dark),
            appearance: p.appearance,
            system_dark,
            space: None,
            backdrop: p.backdrop,
            mica,
            dpi,
            size: Size::ZERO,
            active: true,
            focused_window: false,
            clock: Clock::start(p.hwnd),
            events: VecDeque::with_capacity(16),
            commands: Vec::with_capacity(4),
            dirty: Vec::with_capacity(32),
            new_slots: Vec::new(),
            slots: HashMap::new(),
            capture: Capture::None,
            tracking: false,
            nc_tracking: false,
            clicks: (p.created, Point::ZERO, 0),
            high_surrogate: None,
            menu: None,
            tip: None,
            tip_for: None,
            pan: None,
            over_slot: None,
            ime_on: false,
            uia: uia::Bridge::new(p.host.clone(), p.hwnd),
            stats: FrameStats::new(),
            last_step: None,
            last_frame: None,
            created: p.created,
            shown: false,
        };
        ui.apply_theme();
        ui.sync_client_size();
        ui
    }

    // — what the app reads —

    /// Where the app builds: a stack filling the window, under the caption
    /// buttons.
    pub fn content(&self) -> NodeId {
        self.content
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Physical pixels per DIP.
    pub fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    /// The client area in DIPs.
    pub fn size(&self) -> Size {
        self.size
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn is_dark(&self) -> bool {
        self.theme.dark
    }

    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    /// Whether Mica shows through (Windows 11 with the material on).
    pub fn has_backdrop(&self) -> bool {
        self.mica
    }

    /// Frame timings of the recent frames (resets the slow-frame count).
    pub fn stats(&mut self) -> Summary {
        self.stats.summary()
    }

    // — what the app changes —

    /// Light, dark, or follow Windows.
    pub fn set_appearance(&mut self, a: Appearance) {
        self.appearance = a;
        self.apply_theme();
    }

    /// Colour the column for a space (`None`: the app's paper).
    pub fn set_space(&mut self, space: Option<SpaceTheme>) {
        self.space = space;
        self.apply_theme();
    }

    pub(super) fn apply_theme(&mut self) {
        let dark = self.appearance.is_dark(self.system_dark);
        let changed = dark != self.theme.dark;
        let mut t = Theme::with_space(dark, self.space);
        t.active = self.active;
        self.mica = frame::backdrop(self.hwnd, self.backdrop, dark);
        // Mica tints the column from behind; elsewhere the column is solid.
        t.column_alpha = if self.mica {
            crate::tokens::side::MICA_ALPHA
        } else {
            1.0
        };
        // Without the material (Windows 10, Mica off) the window's own
        // ground fills whatever the app leaves uncovered.
        let ground = if self.mica {
            crate::theme::Swatch::Clear
        } else {
            crate::theme::Swatch::Token(crate::theme::Token::Ground)
        };
        if let Some(root) = self.tree.root()
            && self.tree.get(root).is_some_and(|n| n.look.fill != ground)
        {
            self.tree.repaint(root, |n| n.look.fill = ground);
        }
        if t != self.theme {
            self.theme = t;
            self.repaint_all();
        }
        if changed {
            self.events.push_back(Event::Appearance { dark });
        }
    }

    /// Mark every layer for redrawing.
    pub(super) fn repaint_all(&mut self) {
        let mut layers = Vec::new();
        self.tree.walk(|id, n| {
            if n.flags.layer {
                layers.push(id);
            }
        });
        for id in layers {
            self.tree.mark_paint(id);
        }
        if let Some(m) = &mut self.menu {
            m.repaint_all();
        }
    }

    /// Move keyboard focus within the chrome (`None`: nowhere). Focus set
    /// this way shows no ring; Tab shows it.
    pub fn focus(&mut self, id: Option<NodeId>) {
        self.set_focus(id, false);
    }

    pub(super) fn set_focus(&mut self, id: Option<NodeId>, visible: bool) {
        let before = self.tree.focused();
        if before == id {
            if let Some(i) = id {
                self.tree.focus(Some(i), visible);
            }
            return;
        }
        if let Some(b) = before
            && self.tree.get(b).is_some_and(|n| n.field_ref().is_some())
        {
            ime::complete(self.hwnd);
            self.tree.with_field(b, |f| f.edit.cancel_composition());
        }
        self.tree.focus(id, visible);
        let field = id.is_some_and(|i| self.tree.get(i).is_some_and(|n| n.field_ref().is_some()));
        if field != self.ime_on {
            self.ime_on = field;
            ime::enable(self.hwnd, field);
        }
        self.restart_caret();
        self.events.push_back(Event::Focus(id));
        if let Some(i) = id {
            self.uia.focus(&self.tree, i);
        }
    }

    /// Give a page keyboard focus (Tab into it, or the app's choice).
    pub fn focus_page(&mut self, slot: NodeId, reason: FocusReason) {
        self.set_focus(None, false);
        if let Some((h, _)) = self.slots.get(&slot) {
            h.focus(reason);
        }
    }

    /// Replace a field's text (selecting it all, as the address field does
    /// when it takes focus, when `select_all`).
    pub fn set_field_text(&mut self, id: NodeId, text: &str, select_all: bool) {
        let composing = self
            .tree
            .get(id)
            .and_then(|n| n.field_ref())
            .is_some_and(|f| f.edit.composition().is_some());
        if composing {
            // The app replaced the text under an IME composition: drop it.
            ime::cancel(self.hwnd);
        }
        self.tree.with_field(id, |f| {
            f.edit.cancel_composition();
            f.edit.set_text(text, select_all);
        });
        self.fit_field(id);
    }

    /// A field's text.
    pub fn field_text(&self, id: NodeId) -> Option<&str> {
        self.tree.get(id)?.field_ref().map(|f| f.edit.text())
    }

    /// Open a context menu at `at` (window DIPs). The choice arrives as
    /// [`Event::MenuChosen`] tagged `menu`.
    pub fn show_menu(&mut self, menu: u32, items: &Menu, at: Point) {
        self.hide_tip();
        self.close_menu();
        match MenuPopup::open(self, menu, items, at) {
            Ok(m) => {
                self.uia.menu_opened(m.hwnd(), m.card());
                self.menu = Some(m);
            }
            Err(e) => self.recover(&e),
        }
    }

    /// Close the open menu without a choice.
    pub fn close_menu(&mut self) {
        if let Some(id) = self.drop_menu() {
            self.events.push_back(Event::MenuDismissed { menu: id });
        }
    }

    /// Close the menu's window; its id, if one was open.
    pub(super) fn drop_menu(&mut self) -> Option<u32> {
        let m = self.menu.take()?;
        self.uia.menu_closed();
        let id = m.id;
        m.close(&mut self.fonts);
        Some(id)
    }

    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Host a page in slot node `id`.
    pub fn set_slot(&mut self, id: NodeId, host: Box<dyn SlotHost>) {
        if let Some(v) = self.scene.slot_visual(id) {
            host.attach(self.hwnd, &v);
        }
        self.slots.insert(id, (host, Told::default()));
        self.tree.mark_visuals();
    }

    /// Stop hosting a page in `id`.
    pub fn remove_slot(&mut self, id: NodeId) {
        if let Some((h, _)) = self.slots.remove(&id) {
            h.detach();
        }
    }

    /// Register picture `id` for image nodes: premultiplied BGRA, `w`×`h`.
    pub fn set_image(&mut self, id: u32, pixels: Vec<u8>, w: u32, h: u32) {
        self.painter.set_image(id, pixels, w, h);
        self.repaint_all();
    }

    /// Forget picture `id`.
    pub fn remove_image(&mut self, id: u32) {
        self.painter.remove_image(id);
        self.repaint_all();
    }

    /// Close the window (after [`Event::CloseRequested`], or whenever).
    pub fn close(&mut self) {
        self.commands.push(Command::Destroy);
    }

    // — fields —

    /// Keep a field's caret in view after an edit or a caret move.
    pub(super) fn fit_field(&mut self, id: NodeId) {
        self.fonts.set_space(0);
        let Some(n) = self.tree.get(id) else {
            return;
        };
        let Some(f) = n.field_ref() else {
            return;
        };
        let (w, h) = (n.frame.w, n.frame.h);
        if w <= 0.0 {
            return;
        }
        let Some(fl) = self.fonts.field_layout(id, f, w, h) else {
            return;
        };
        let (x, _, _) = fl.caret();
        let Some(m) = text::metrics(&fl.layout) else {
            return;
        };
        let width = m.widthIncludingTrailingWhitespace;
        let left = m.left;
        let slack = 2.0;
        let mut sx = f.scroll_x;
        if width <= w - slack {
            sx = 0.0;
        } else {
            if x - sx < 0.0 {
                sx = x;
            } else if x - sx > w - slack {
                sx = x - w + slack;
            }
            sx = sx.clamp(left.min(0.0), (left + width - w + slack).max(0.0));
        }
        if (sx - f.scroll_x).abs() > 0.01 {
            self.tree.repaint(id, |n| {
                if let Some(f) = n.field_mut() {
                    f.scroll_x = sx;
                }
            });
        }
        self.place_ime(id);
    }

    /// Put the IME's candidate window under the caret of field `id`.
    pub(super) fn place_ime(&mut self, id: NodeId) {
        let Some(r) = self.caret_rect(id) else {
            return;
        };
        let s = self.scale();
        let rect = RECT {
            left: (r.x * s).floor() as i32,
            top: (r.y * s).floor() as i32,
            right: (r.right() * s).ceil() as i32,
            bottom: (r.bottom() * s).ceil() as i32,
        };
        ime::place(self.hwnd, rect);
    }

    /// The caret of field `id`, in window DIPs.
    pub(super) fn caret_rect(&mut self, id: NodeId) -> Option<Rect> {
        self.fonts.set_space(0);
        let n = self.tree.get(id)?;
        let f = n.field_ref()?;
        let w = self.tree.window_rect(id);
        let fl = self.fonts.field_layout(id, f, w.w, w.h)?;
        let (x, top, h) = fl.caret();
        Some(Rect::new(w.x + x - f.scroll_x, w.y + top, 1.0, h))
    }

    /// Show the caret now and restart its blinking.
    pub(super) fn restart_caret(&mut self) {
        // SAFETY: timers on our own window.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CARET);
        }
        let Some(id) = self.tree.focused() else {
            return;
        };
        if self.tree.get(id).and_then(|n| n.field_ref()).is_none() {
            return;
        }
        self.set_caret_on(id, true);
        // SAFETY: as above.
        let blink = unsafe { GetCaretBlinkTime() };
        if blink != u32::MAX && blink > 0 && self.focused_window {
            // SAFETY: as above.
            unsafe { SetTimer(Some(self.hwnd), TIMER_CARET, blink, None) };
        }
    }

    pub(super) fn set_caret_on(&mut self, id: NodeId, on: bool) {
        if self
            .tree
            .get(id)
            .and_then(|n| n.field_ref())
            .is_some_and(|f| f.caret_on != on)
        {
            self.tree.repaint(id, |n| {
                if let Some(f) = n.field_mut() {
                    f.caret_on = on;
                }
            });
        }
    }

    // — the frame —

    /// Whether anything waits to be drawn or moved.
    pub(super) fn needs_frame(&self) -> bool {
        self.tree.has_dirty_layers()
            || self.tree.needs_layout()
            || self.tree.has_visual_changes()
            || self.tree.is_animating()
            || self.menu.as_ref().is_some_and(|m| m.needs_frame())
            || self.tip.as_ref().is_some_and(|t| t.needs_frame())
    }

    /// Step animations, lay out, compose, draw what changed, commit.
    pub(super) fn render(&mut self) {
        if let Err(e) = self.try_render() {
            self.recover(&e);
            if let Err(e) = self.try_render() {
                self.recover(&e);
            }
        }
    }

    fn try_render(&mut self) -> windows::core::Result<()> {
        let started = Instant::now();
        let dt = self
            .last_step
            .map_or(0.0, |t| started.duration_since(t).as_secs_f32().min(0.1));
        self.last_step = Some(started);
        let animating = self.tree.step(dt);
        if let Some(m) = &mut self.menu {
            m.step(dt);
        }
        if let Some(t) = &mut self.tip {
            t.step(dt);
        }

        self.fonts.set_space(0);
        if self.tree.layout(
            self.size,
            &mut Measurer {
                fonts: &mut self.fonts,
            },
        ) {
            self.fonts.prune(&self.tree);
            self.uia.prune(&self.tree);
        }
        self.new_slots.clear();
        self.scene
            .sync_structure(&self.gfx, &self.tree, &mut self.new_slots)?;
        for i in 0..self.new_slots.len() {
            let id = self.new_slots[i];
            if let (Some((h, told)), Some(v)) =
                (self.slots.get_mut(&id), self.scene.slot_visual(id))
            {
                h.attach(self.hwnd, &v);
                *told = Told::default();
            }
        }
        if self.tree.take_visuals_dirty() {
            self.scene.sync_visuals(&self.tree)?;
            self.tell_slots();
        }
        self.tree.take_dirty_layers(&mut self.dirty);
        let drawn = self.scene.draw(
            &self.gfx,
            &self.tree,
            &self.dirty,
            &mut self.painter,
            &mut self.fonts,
            &self.theme,
            self.focused_window,
        )?;
        let mut popups = 0;
        if let Some(m) = &mut self.menu {
            popups += m.render(&self.gfx, &mut self.painter, &mut self.fonts, &self.theme)?;
        }
        if let Some(t) = &mut self.tip {
            popups += t.render(&self.gfx, &mut self.painter, &mut self.fonts, &self.theme)?;
        }
        self.fonts.set_space(0);
        // SAFETY: committing our own device's batch.
        unsafe { self.gfx.dcomp.Commit()? };

        let now = Instant::now();
        let cpu = now.duration_since(started).as_secs_f32() * 1000.0;
        let interval = self
            .last_frame
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32() * 1000.0);
        self.last_frame = Some(now);
        if drawn + popups > 0 || animating {
            self.stats.record(cpu);
            self.events.push_back(Event::Frame {
                cpu_ms: cpu,
                layers: drawn + popups,
                interval_ms: interval,
            });
        }
        if self.needs_frame() {
            self.clock.request();
        } else {
            self.clock.rest();
            self.last_step = None;
        }
        Ok(())
    }

    /// Tell each slot where it is, how big, and whether it shows.
    fn tell_slots(&mut self) {
        let s = self.scale();
        for (&id, (h, told)) in self.slots.iter_mut() {
            let r = self.tree.visible_rect(id);
            let shown = self.tree.is_shown(id) && !r.is_empty();
            let b = (
                (r.x * s).round() as i32,
                (r.y * s).round() as i32,
                (r.right() * s).round() as i32,
                (r.bottom() * s).round() as i32,
            );
            if told.bounds != Some(b) {
                told.bounds = Some(b);
                h.set_bounds(RECT {
                    left: b.0,
                    top: b.1,
                    right: b.2,
                    bottom: b.3,
                });
            }
            if told.visible != Some(shown) {
                told.visible = Some(shown);
                h.set_visible(shown);
            }
            if told.scale != Some(s) {
                told.scale = Some(s);
                h.set_scale(f64::from(s));
            }
        }
    }

    /// Rebuild the devices after the GPU went away; anything else is
    /// logged once and dropped (the next frame tries again).
    pub(super) fn recover(&mut self, e: &windows::core::Error) {
        if !(gfx::is_device_loss(e) || self.gfx.is_lost()) {
            crate::win::log(&format!("copper-win-ui: frame failed: {e}"));
            return;
        }
        let generation = self.gfx.generation.wrapping_add(1);
        match Gfx::new(None, generation) {
            Ok(g) => {
                self.gfx = g;
                self.scene.reset();
                if let Some(m) = &mut self.menu {
                    m.reset();
                }
                if let Some(t) = &mut self.tip {
                    t.reset();
                }
                for (_, told) in self.slots.values_mut() {
                    *told = Told::default();
                }
                self.tree.mark_visuals();
                self.repaint_all();
            }
            Err(e) => crate::win::log(&format!("copper-win-ui: device rebuild failed: {e}")),
        }
    }

    // — size and scale —

    /// Read the client size and lay out for it.
    pub(super) fn sync_client_size(&mut self) {
        let mut r = RECT::default();
        // SAFETY: a size query on our own window.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(self.hwnd, &mut r);
        }
        let s = self.scale();
        let size = Size::new((r.right - r.left) as f32 / s, (r.bottom - r.top) as f32 / s);
        if size != self.size {
            self.size = size;
            self.tree.mark_layout();
            self.events.push_back(Event::Resized(size));
        }
        let max = frame::is_maximized(self.hwnd);
        self.caption.set_maximized(&mut self.tree, max);
    }

    pub(super) fn set_dpi(&mut self, dpi: u32) {
        if dpi == self.dpi || dpi == 0 {
            return;
        }
        self.dpi = dpi;
        self.scene.set_scale(self.scale());
        self.tree.mark_layout();
        self.tree.mark_visuals();
        self.events.push_back(Event::ScaleChanged(self.scale()));
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        for (h, _) in self.slots.values() {
            h.detach();
        }
    }
}
