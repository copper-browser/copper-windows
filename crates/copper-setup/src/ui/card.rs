//! The one card both setup windows are: icon, title, one line, then either
//! a row of buttons or a progress bar, an optional quiet link (an "Options"
//! disclosure or "View log") and an optional row of switches.
//!
//! A controller describes what should be on it as a [`Scene`]; the card
//! animates from whatever it shows now to that — text cross-fades, rows
//! fade and the stack re-centres itself on the Mac's springs — and reports
//! clicks as [`Target`]s. Everything is in DIPs.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use super::gfx::{Align, Gfx, Painter, Rect, Style};
use super::theme::{Palette, alpha, mix};
use crate::motion::{GLIDE, QUICK, SETTLE, Spring, lerp};

/// The window's size in DIPs.
pub const WIDTH: f32 = 420.0;
pub const HEIGHT: f32 = 340.0;

/// Something on the card that can be clicked or focused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The × in the corner.
    Close,
    /// A button, by index into [`Scene::buttons`].
    Button(usize),
    /// A switch, by index into [`Card::checks`].
    Check(usize),
    /// The quiet link.
    Link,
}

/// A button as the controller asks for it.
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonSpec {
    pub label: String,
    /// Filled with ink (the default action) rather than a quiet wash.
    pub primary: bool,
}

impl ButtonSpec {
    pub fn primary(label: &str) -> ButtonSpec {
        ButtonSpec {
            label: label.to_owned(),
            primary: true,
        }
    }
    pub fn secondary(label: &str) -> ButtonSpec {
        ButtonSpec {
            label: label.to_owned(),
            primary: false,
        }
    }
}

/// The quiet link under the buttons.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkSpec {
    pub label: String,
    /// A disclosure: draws a chevron that turns when the switches show.
    pub disclosure: bool,
}

/// Where the switches sit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksAt {
    /// Above the buttons (the uninstaller's one question).
    Above,
    /// Below the link (the installer's options).
    Below,
}

/// What should be on the card.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub title: String,
    pub line: String,
    pub buttons: Vec<ButtonSpec>,
    pub link: Option<LinkSpec>,
    pub checks: Option<ChecksAt>,
    /// Progress bar: fraction 0…1 and the words under it.
    pub progress: Option<(f32, String)>,
}

/// A switch the user can flip; the card owns the state.
pub struct Check {
    pub label: String,
    pub on: bool,
    tick: Spring,
}

impl Check {
    pub fn new(label: &str, on: bool) -> Check {
        Check {
            label: label.to_owned(),
            on,
            tick: Spring::new(if on { 1.0 } else { 0.0 }, QUICK),
        }
    }
}

/// Text that cross-fades when it changes.
struct Fade {
    now: String,
    was: String,
    mix: Spring,
}

impl Fade {
    fn new(text: &str) -> Fade {
        Fade {
            now: text.to_owned(),
            was: String::new(),
            mix: Spring::new(1.0, SETTLE),
        }
    }
    fn set(&mut self, text: &str) {
        if self.now == text {
            return;
        }
        // Mid-fade, keep whichever of the two is more visible as the one
        // going out, so nothing pops.
        if self.mix.value() >= 0.5 {
            self.was = std::mem::take(&mut self.now);
        }
        self.now = text.to_owned();
        self.mix.snap(0.0);
        self.mix.set(1.0);
    }
    fn step(&mut self, dt: f32) -> bool {
        self.mix.step(dt)
    }
    /// (text, opacity) pairs to draw.
    fn layers(&self) -> [(&str, f32); 2] {
        let t = self.mix.value().clamp(0.0, 1.0);
        [(&self.was, 1.0 - t), (&self.now, t)]
    }
}

/// How a button is being touched, and how visible it is.
#[derive(Clone, Copy)]
struct Look {
    hover: f32,
    pressed: bool,
    alpha: f32,
}

/// Per-frame geometry, shared by drawing and hit-testing so they agree.
struct Layout {
    icon: Rect,
    title_y: f32,
    line_y: f32,
    zone_y: f32,
    buttons: Vec<Rect>,
    link: Rect,
    checks: Vec<Rect>,
    close: Rect,
}

const ICON: f32 = 72.0;
const BUTTON_H: f32 = 36.0;
const ZONE_H: f32 = 40.0;
const CHECK_H: f32 = 24.0;
const LINK_H: f32 = 20.0;
const TEXT_W: f32 = WIDTH - 72.0;
const BAR_W: f32 = 240.0;
const CLOSE: Rect = Rect::new(WIDTH - 46.0, 0.0, 46.0, 32.0);

/// The card.
pub struct Card {
    pub checks: Vec<Check>,
    scene: Scene,
    title: Fade,
    line: Fade,
    status: Fade,
    link_label: Fade,
    /// The buttons last asked for, kept while they fade out.
    shown_buttons: Vec<ButtonSpec>,
    shown_link_disclosure: bool,
    enter: Spring,
    buttons_vis: Spring,
    link_vis: Spring,
    checks_vis: Spring,
    progress_vis: Spring,
    progress: Spring,
    hover_springs: Vec<Spring>,
    close_hover: Spring,
    link_hover: Spring,
    hover: Option<Target>,
    pressed: Option<Target>,
    focus: Option<Target>,
    focus_visible: bool,
}

fn vis(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

impl Card {
    pub fn new(scene: Scene, checks: Vec<Check>) -> Card {
        let mut card = Card {
            checks,
            title: Fade::new(&scene.title),
            line: Fade::new(&scene.line),
            status: Fade::new(""),
            link_label: Fade::new(scene.link.as_ref().map_or("", |l| &l.label)),
            shown_buttons: scene.buttons.clone(),
            shown_link_disclosure: scene.link.as_ref().is_some_and(|l| l.disclosure),
            enter: Spring::new(0.0, SETTLE),
            buttons_vis: Spring::new(vis(!scene.buttons.is_empty()), SETTLE),
            link_vis: Spring::new(vis(scene.link.is_some()), SETTLE),
            checks_vis: Spring::new(vis(scene.checks.is_some()), GLIDE),
            progress_vis: Spring::new(vis(scene.progress.is_some()), SETTLE),
            progress: Spring::new(0.0, GLIDE),
            hover_springs: Vec::new(),
            close_hover: Spring::new(0.0, QUICK),
            link_hover: Spring::new(0.0, QUICK),
            hover: None,
            pressed: None,
            focus: None,
            focus_visible: false,
            scene: scene.clone(),
        };
        card.enter.set(1.0);
        card.show(scene);
        card
    }

    /// Animate to `scene`.
    pub fn show(&mut self, scene: Scene) {
        self.title.set(&scene.title);
        self.line.set(&scene.line);
        // An empty row keeps the last buttons on screen while they fade.
        if !scene.buttons.is_empty() {
            self.shown_buttons = scene.buttons.clone();
        }
        if let Some(link) = &scene.link {
            self.link_label.set(&link.label);
            self.shown_link_disclosure = link.disclosure;
        }
        if let Some((fraction, label)) = &scene.progress {
            self.status.set(label);
            self.progress.set(fraction.clamp(0.0, 1.0));
        }
        self.buttons_vis.set(vis(!scene.buttons.is_empty()));
        self.link_vis.set(vis(scene.link.is_some()));
        self.checks_vis.set(vis(scene.checks.is_some()));
        self.progress_vis.set(vis(scene.progress.is_some()));
        if scene.progress.is_none() && self.progress_vis.value() < 0.05 {
            self.progress.snap(0.0);
        }
        // Focus can't stay on something that's going away.
        if let Some(f) = self.focus
            && !self.is_live(f, &scene)
        {
            self.focus = None;
        }
        self.scene = scene;
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Flip a switch.
    pub fn toggle(&mut self, i: usize) {
        if let Some(c) = self.checks.get_mut(i) {
            c.on = !c.on;
            c.tick.set(vis(c.on));
        }
    }

    /// Advance every animation by `dt` seconds; true while anything moves.
    pub fn step(&mut self, dt: f32) -> bool {
        self.hover_springs
            .resize_with(self.shown_buttons.len(), || Spring::new(0.0, QUICK));
        for (i, s) in self.hover_springs.iter_mut().enumerate() {
            s.set(vis(self.hover == Some(Target::Button(i))));
        }
        self.close_hover.set(vis(self.hover == Some(Target::Close)));
        self.link_hover.set(vis(self.hover == Some(Target::Link)));

        let mut moving = false;
        for s in [
            &mut self.enter,
            &mut self.buttons_vis,
            &mut self.link_vis,
            &mut self.checks_vis,
            &mut self.progress_vis,
            &mut self.progress,
            &mut self.close_hover,
            &mut self.link_hover,
        ] {
            moving |= s.step(dt);
        }
        for s in &mut self.hover_springs {
            moving |= s.step(dt);
        }
        for c in &mut self.checks {
            moving |= c.tick.step(dt);
        }
        for f in [
            &mut self.title,
            &mut self.line,
            &mut self.status,
            &mut self.link_label,
        ] {
            moving |= f.step(dt);
        }
        moving
    }

    fn is_live(&self, t: Target, scene: &Scene) -> bool {
        match t {
            Target::Close => true,
            Target::Button(i) => i < scene.buttons.len(),
            Target::Check(i) => scene.checks.is_some() && i < self.checks.len(),
            Target::Link => scene.link.is_some(),
        }
    }

    /// Things that can take focus right now, in Tab order.
    fn focus_order(&self) -> Vec<Target> {
        let mut out = Vec::new();
        let above = self.scene.checks == Some(ChecksAt::Above);
        let checks = || (0..self.checks.len()).map(Target::Check);
        if above {
            out.extend(checks());
        }
        out.extend((0..self.scene.buttons.len()).map(Target::Button));
        if self.scene.link.is_some() {
            out.push(Target::Link);
        }
        if self.scene.checks == Some(ChecksAt::Below) {
            out.extend(checks());
        }
        out.push(Target::Close);
        out
    }

    /// Tab / Shift+Tab.
    pub fn move_focus(&mut self, back: bool) {
        let order = self.focus_order();
        let at = self.focus.and_then(|f| order.iter().position(|&t| t == f));
        let n = order.len();
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        self.focus = order.get(next).copied();
        self.focus_visible = true;
    }

    /// What Enter/Space act on: the focused thing, else the primary button.
    pub fn activation_target(&self, enter: bool) -> Option<Target> {
        if let Some(f) = self.focus {
            return Some(f);
        }
        if enter {
            return self
                .scene
                .buttons
                .iter()
                .position(|b| b.primary)
                .map(Target::Button);
        }
        None
    }

    pub fn set_hover(&mut self, t: Option<Target>) -> bool {
        let changed = self.hover != t;
        self.hover = t;
        changed
    }

    pub fn set_pressed(&mut self, t: Option<Target>) {
        self.pressed = t;
    }

    pub fn pressed(&self) -> Option<Target> {
        self.pressed
    }

    /// A pointer click hides the keyboard focus ring (Windows' convention).
    pub fn pointer_used(&mut self) {
        self.focus_visible = false;
    }

    fn layout(&self, gfx: &Gfx) -> Layout {
        let cv = self.checks_vis.value().clamp(0.0, 1.0);
        let lv = self.link_vis.value().clamp(0.0, 1.0);
        // The line may wrap (an error sentence): room for however many
        // lines the longer of the two cross-fading texts needs.
        let line_h = self
            .line
            .layers()
            .iter()
            .filter(|(_, a)| *a > 0.0)
            .map(|(t, _)| gfx.height(t, Style::Body, TEXT_W))
            .fold(18.0f32, f32::max);
        let checks_above = self.scene.checks == Some(ChecksAt::Above);

        // The stack, with optional parts weighted by how visible they are,
        // so it re-centres smoothly as they come and go.
        let mut total = ICON + 16.0 + 30.0 + 4.0 + line_h + 26.0 + ZONE_H;
        total += lv * (10.0 + LINK_H);
        total += cv * (CHECK_H + 12.0);
        // A little below centre: the top 32 DIPs belong to the caption.
        let top = ((HEIGHT - total) / 2.0 + 10.0).max(36.0);

        let icon = Rect::new((WIDTH - ICON) / 2.0, top, ICON, ICON);
        let title_y = icon.y + ICON + 16.0;
        let line_y = title_y + 34.0;
        let mut y = line_y + line_h + 26.0;
        let mut checks_y = 0.0;
        if checks_above {
            checks_y = y;
            y += cv * (CHECK_H + 12.0);
        }
        let zone_y = y;
        y += ZONE_H;

        // Buttons: a centred row, each at least 112 wide.
        let widths: Vec<f32> = self
            .shown_buttons
            .iter()
            .map(|b| (gfx.measure(&b.label, Style::Button) + 40.0).max(112.0))
            .collect();
        let gap = 10.0;
        let row_w = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
        let mut x = (WIDTH - row_w) / 2.0;
        let by = zone_y + (ZONE_H - BUTTON_H) / 2.0;
        let buttons = widths
            .iter()
            .map(|&w| {
                let r = Rect::new(x, by, w, BUTTON_H);
                x += w + gap;
                r
            })
            .collect();

        y += lv * 10.0;
        let link_w = gfx.measure(&self.link_label.now, Style::Small)
            + if self.shown_link_disclosure {
                14.0
            } else {
                0.0
            };
        let link = Rect::new((WIDTH - link_w) / 2.0 - 8.0, y, link_w + 16.0, LINK_H);
        y += lv * LINK_H;
        if !checks_above {
            y += cv * 12.0;
            checks_y = y;
        }

        // Switches: a centred row, box + label each, 24 apart.
        let check_ws: Vec<f32> = self
            .checks
            .iter()
            .map(|c| 16.0 + 8.0 + gfx.measure(&c.label, Style::Small))
            .collect();
        let row_w = check_ws.iter().sum::<f32>() + 24.0 * check_ws.len().saturating_sub(1) as f32;
        let mut x = (WIDTH - row_w) / 2.0;
        let checks = check_ws
            .iter()
            .map(|&w| {
                let r = Rect::new(x, checks_y, w, CHECK_H);
                x += w + 24.0;
                r
            })
            .collect();

        Layout {
            icon,
            title_y,
            line_y,
            zone_y,
            buttons,
            link,
            checks,
            close: CLOSE,
        }
    }

    /// What is under the pointer at `(x, y)` DIPs, if anything clickable.
    pub fn hit(&self, gfx: &Gfx, x: f32, y: f32) -> Option<Target> {
        let l = self.layout(gfx);
        if l.close.contains(x, y) {
            return Some(Target::Close);
        }
        if !self.scene.buttons.is_empty()
            && self.buttons_vis.target() > 0.5
            && let Some(i) = l.buttons.iter().position(|r| r.contains(x, y))
        {
            return Some(Target::Button(i));
        }
        if self.scene.link.is_some() && l.link.contains(x, y) {
            return Some(Target::Link);
        }
        if self.scene.checks.is_some()
            && let Some(i) = l.checks.iter().position(|r| r.contains(x, y))
        {
            return Some(Target::Check(i));
        }
        None
    }

    /// Paint the card.
    pub fn draw(&self, p: &Painter<'_>, pal: &Palette) {
        let gfx = p.gfx();
        let l = self.layout(gfx);
        let enter = self.enter.value().clamp(0.0, 1.0);
        // Arrive: fade in and rise the last few DIPs.
        let rise = (1.0 - enter) * 8.0;

        self.draw_close(p, pal, l.close);

        p.icon(l.icon.offset(0.0, rise), enter);
        for (text, a) in self.title.layers() {
            if !text.is_empty() {
                p.text(
                    text,
                    Style::Title,
                    (36.0, l.title_y + rise),
                    TEXT_W,
                    Align::Center,
                    alpha(pal.ink, a * enter),
                );
            }
        }
        for (text, a) in self.line.layers() {
            if !text.is_empty() {
                p.text(
                    text,
                    Style::Body,
                    (36.0, l.line_y + rise),
                    TEXT_W,
                    Align::Center,
                    alpha(pal.muted, a * enter),
                );
            }
        }

        // Buttons and progress share one zone and cross-fade.
        let bv = self.buttons_vis.value().clamp(0.0, 1.0) * enter;
        for (i, (spec, r)) in self.shown_buttons.iter().zip(&l.buttons).enumerate() {
            let hover = self.hover_springs.get(i).map_or(0.0, |s| s.value());
            let pressed =
                self.pressed == Some(Target::Button(i)) && self.hover == Some(Target::Button(i));
            self.draw_button(
                p,
                pal,
                spec,
                *r,
                Look {
                    hover,
                    pressed,
                    alpha: bv,
                },
            );
            if self.focus_visible && self.focus == Some(Target::Button(i)) {
                p.stroke_round(
                    r.inset(-3.0),
                    BUTTON_H / 2.0 + 3.0,
                    2.0,
                    alpha(pal.focus, bv),
                );
            }
        }
        let pv = self.progress_vis.value().clamp(0.0, 1.0) * enter;
        if pv > 0.0 {
            let bar = Rect::new((WIDTH - BAR_W) / 2.0, l.zone_y + 8.0, BAR_W, 4.0);
            p.fill_round(bar, 2.0, alpha(pal.hairline, pv));
            let f = self.progress.value().clamp(0.0, 1.0);
            if f > 0.0 {
                let fill = Rect::new(bar.x, bar.y, (BAR_W * f).max(4.0), bar.h);
                p.fill_round(fill, 2.0, alpha(pal.ink, pv));
            }
            for (text, a) in self.status.layers() {
                if !text.is_empty() {
                    p.text(
                        text,
                        Style::Small,
                        (36.0, bar.y + 12.0),
                        TEXT_W,
                        Align::Center,
                        alpha(pal.muted, a * pv),
                    );
                }
            }
        }

        let lv = self.link_vis.value().clamp(0.0, 1.0) * enter;
        if lv > 0.0 {
            self.draw_link(p, gfx, pal, l.link, lv);
        }

        let cv = self.checks_vis.value().clamp(0.0, 1.0) * enter;
        if cv > 0.0 {
            // Below the link, the row also slides down out from under it.
            let slide =
                if self.scene.checks == Some(ChecksAt::Below) || self.checks_vis.target() == 0.0 {
                    (1.0 - cv) * -6.0
                } else {
                    0.0
                };
            for (i, (c, r)) in self.checks.iter().zip(&l.checks).enumerate() {
                let r = r.offset(0.0, slide);
                let focused = self.focus_visible && self.focus == Some(Target::Check(i));
                self.draw_check(p, pal, c, r, cv, focused);
            }
        }
    }

    fn draw_button(&self, p: &Painter<'_>, pal: &Palette, spec: &ButtonSpec, r: Rect, look: Look) {
        let Look {
            hover,
            pressed,
            alpha: a,
        } = look;
        if a <= 0.0 {
            return;
        }
        let radius = r.h / 2.0;
        if spec.primary {
            // Ink, lifting toward muted under the pointer, deeper when held.
            let mut fill = mix(pal.ink, pal.muted, 0.18 * hover);
            if pressed {
                fill = mix(pal.ink, pal.muted, 0.30);
            }
            p.fill_round(r, radius, alpha(fill, a));
            p.label(
                &spec.label,
                Style::Button,
                r,
                Align::Center,
                alpha(pal.on_ink, a),
            );
        } else {
            let wash = if pressed { 0.04 } else { 0.06 + 0.05 * hover };
            p.fill_round(r, radius, alpha(pal.ink, wash * a));
            p.stroke_round(r, radius, 1.0, alpha(pal.ink, 0.08 * a));
            p.label(
                &spec.label,
                Style::Button,
                r,
                Align::Center,
                alpha(pal.ink, a),
            );
        }
        if pal.high_contrast {
            p.stroke_round(r, radius, 1.0, alpha(pal.ink, a));
        }
    }

    fn draw_link(&self, p: &Painter<'_>, gfx: &Gfx, pal: &Palette, r: Rect, a: f32) {
        let color = mix(pal.muted, pal.ink, self.link_hover.value());
        let label_w = gfx.measure(&self.link_label.now, Style::Small);
        let x = r.x + 8.0;
        for (text, la) in self.link_label.layers() {
            if !text.is_empty() {
                p.label(
                    text,
                    Style::Small,
                    Rect::new(x, r.y, label_w + 2.0, r.h),
                    Align::Leading,
                    alpha(color, la * a),
                );
            }
        }
        if self.shown_link_disclosure {
            // A chevron that turns from › to ⌄ as the switches open.
            let t = self.checks_vis.value().clamp(0.0, 1.2);
            let angle = t * std::f32::consts::FRAC_PI_2;
            let (cx, cy) = (x + label_w + 8.0, r.y + r.h / 2.0 + 1.0);
            let rot = |dx: f32, dy: f32| {
                let (s, c) = angle.sin_cos();
                (cx + dx * c - dy * s, cy + dx * s + dy * c)
            };
            let col = alpha(color, a);
            p.line(rot(-1.5, -3.5), rot(2.0, 0.0), 1.2, col);
            p.line(rot(2.0, 0.0), rot(-1.5, 3.5), 1.2, col);
        }
        if self.focus_visible && self.focus == Some(Target::Link) {
            p.stroke_round(r, 6.0, 2.0, alpha(pal.focus, a));
        }
    }

    fn draw_check(
        &self,
        p: &Painter<'_>,
        pal: &Palette,
        c: &Check,
        r: Rect,
        a: f32,
        focused: bool,
    ) {
        let tick = c.tick.value().clamp(0.0, 1.0);
        let b = Rect::new(r.x, r.y + (r.h - 16.0) / 2.0, 16.0, 16.0);
        p.fill_round(b, 4.0, alpha(pal.ink, tick * a));
        p.stroke_round(b, 4.0, 1.0, alpha(pal.ink, lerp(0.45, 1.0, tick) * a));
        if tick > 0.0 {
            let col = alpha(pal.on_ink, tick * a);
            p.line((b.x + 4.0, b.y + 8.3), (b.x + 6.8, b.y + 11.0), 1.6, col);
            p.line((b.x + 6.8, b.y + 11.0), (b.x + 12.0, b.y + 5.2), 1.6, col);
        }
        p.label(
            &c.label,
            Style::Small,
            Rect::new(b.x + 24.0, r.y, r.w - 22.0, r.h),
            Align::Leading,
            alpha(pal.ink, a),
        );
        if focused {
            p.stroke_round(r.inset(-3.0), 6.0, 2.0, alpha(pal.focus, a));
        }
    }

    fn draw_close(&self, p: &Painter<'_>, pal: &Palette, r: Rect) {
        // Windows 11's caption close: red under the pointer, white ×.
        const RED: D2D1_COLOR_F = D2D1_COLOR_F {
            r: 0.769,
            g: 0.169,
            b: 0.110,
            a: 1.0,
        };
        let h = self.close_hover.value().clamp(0.0, 1.0);
        let pressed = self.pressed == Some(Target::Close) && self.hover == Some(Target::Close);
        let bg = if pressed {
            alpha(RED, 0.85)
        } else {
            alpha(RED, h)
        };
        // Only the corner of the window is rounded; DWM clips the rest.
        p.fill_rect(r, bg);
        let white = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        let glyph = mix(pal.ink, white, h);
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let s = 5.0;
        p.line((cx - s, cy - s), (cx + s, cy + s), 1.0, glyph);
        p.line((cx - s, cy + s), (cx + s, cy - s), 1.0, glyph);
        if self.focus_visible && self.focus == Some(Target::Close) {
            p.stroke_round(r.inset(2.0), 4.0, 2.0, pal.focus);
        }
    }
}
