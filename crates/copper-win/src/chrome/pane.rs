//! The Agent sidebar: the `agent-sidebar` slot (CONTRACT §1) right of the
//! stage — `Row[column → stage → 1 px hairline → pane]` (Mac
//! `Fork/Split.swift`, `AgentSlot` in `Fork/Addons/AddonUI.swift`; Linux
//! `addons/pane.rs`).
//!
//! ```text
//! pane ············ the slot: full stage height, `agent.paneWidth` wide
//! ├─ body
//! │  ├─ placeholder · Copper's own face: a head, then "No agent here yet"
//! │  │                and the way to Settings › Addons
//! │  └─ slot ······ the addon's page (made on first show, kept while hidden)
//! └─ grip ········· the leading 6 px: drag left to widen
//! ```
//!
//! The nodes are built with the window, hidden; the addon's page is made
//! the first time the pane shows and kept alive while it is hidden (an
//! addon's unsent draft survives Ctrl+E). Ctrl+E fades the pane in or out
//! (`Motion.glide`; none with reduced motion); the stage takes the room at
//! once, as the Mac's does. The pane has no state in the chrome's `State`:
//! it is this module's, on the UI thread.

use crate::addons::policy::{self, GRIP};
use crate::addons::view::AddonView;
use crate::platform::Shell;
use crate::slot::PageHost;
use copper_win_ui::tokens::motion;
use copper_win_ui::win::{FocusReason, Ui};
use copper_win_ui::{
    Abs, Align, Cursor, Edge, Insets, Justify, Node, NodeId, Prop, Role, Swatch, TextAlign, Token,
    Trim, Weight,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// The slot's name, as people read it.
pub const NAME: &str = "Agent sidebar";
/// The placeholder's words (CONTRACT §1).
pub const EMPTY_TITLE: &str = "No agent here yet";
const EMPTY_DETAIL: &str = "Addons from your Copper Cloud can add one here.";
pub const LINK: &str = "Settings › Addons";
/// The head over the placeholder (Mac `AgentPane` head: 42 high, 16 / 8).
const HEAD: f32 = 42.0;
const DOOR: f32 = 26.0;

#[derive(Clone, Copy)]
struct Nodes {
    pane: NodeId,
    body: NodeId,
    placeholder: NodeId,
    link: NodeId,
    close: NodeId,
    grip: NodeId,
}

#[derive(Default)]
struct Pane {
    nodes: Cell<Option<Nodes>>,
    /// On screen (or fading in).
    shown: Cell<bool>,
    /// Bumped on every show/hide, so a late "now hide it" from a fade that
    /// was overtaken does nothing.
    generation: Cell<u64>,
    width: Cell<f32>,
    /// The addon's page, once made.
    view: RefCell<Option<Rc<AddonView>>>,
    /// The slot node hosting it (set by the edit that makes it).
    slot: Cell<Option<NodeId>>,
    /// The grip is being dragged: the width and pointer x (DIPs) it began at.
    drag: Cell<Option<(f32, f32)>>,
    hooked: Cell<bool>,
}

thread_local! {
    static PANE: Rc<Pane> = Rc::new(Pane {
        width: Cell::new(policy::DEFAULT_WIDTH),
        ..Default::default()
    });
}

fn pane() -> Rc<Pane> {
    PANE.with(Rc::clone)
}

/// Build the slot into `well`, after the stage (`chrome/build.rs`).
pub fn build(ui: &mut Ui, well: NodeId) {
    let p = pane();
    let pane_node = ui.tree.add(
        well,
        Node::stack()
            .width(p.width.get())
            .shrink(0.0)
            .fill(Token::Ground)
            .edge(Edge::Left, Token::Hairline)
            .clip()
            .layer()
            .hidden(true)
            .role(Role::Pane)
            .name(NAME),
    );
    // Inside the hairline.
    let body = ui.tree.add(
        pane_node,
        Node::stack()
            .absolute(Abs {
                left: Some(1.0),
                top: Some(0.0),
                right: Some(0.0),
                bottom: Some(0.0),
            })
            .role(Role::None),
    );
    let placeholder = ui.tree.add(
        body,
        Node::column()
            .absolute(Abs::FILL)
            .fill(Token::Ground)
            .role(Role::Group),
    );
    // The head: the slot's name and the door that puts it away.
    let head = ui.tree.add(
        placeholder,
        Node::row()
            .height(HEAD)
            .shrink(0.0)
            .padding(Insets::new(16.0, 0.0, 8.0, 0.0))
            .align(Align::Center)
            .gap(2.0)
            .edge(Edge::Bottom, Token::Hairline),
    );
    ui.tree.add(
        head,
        Node::text(NAME, 13.0)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .grow(1.0),
    );
    let close = ui.tree.add(
        head,
        Node::stack()
            .size(DOOR, DOOR)
            .shrink(0.0)
            .radius(7.0)
            .hover_fill(Token::Hover)
            .press_fill(Token::Hover)
            .align(Align::Center)
            .justify(Justify::Center)
            .button("Close")
            .tooltip(&super::build::tip("Close", "agent")),
    );
    ui.tree.add(
        close,
        Node::glyph(super::glyph::CLOSE, 10.0, DOOR)
            .ink(Token::Muted)
            .role(Role::None),
    );
    // The empty face, centred under the head.
    let center = ui.tree.add(
        placeholder,
        Node::column()
            .grow(1.0)
            .align(Align::Center)
            .justify(Justify::Center)
            .padding(Insets::new(28.0, 0.0, 28.0, HEAD))
            .gap(8.0),
    );
    ui.tree.add(
        center,
        Node::glyph(super::glyph::AGENT_SIDEBAR, 30.0, 44.0)
            .ink(Token::Faint)
            .role(Role::None),
    );
    ui.tree.add(
        center,
        Node::text(EMPTY_TITLE, 17.0)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .text_align(TextAlign::Center),
    );
    ui.tree.add(
        center,
        Node::text(EMPTY_DETAIL, 12.0)
            .trim(Trim::Wrap)
            .ink(Token::Muted)
            .text_align(TextAlign::Center)
            .max_width(290.0),
    );
    let link = ui.tree.add(
        center,
        Node::row()
            .height(26.0)
            .padding(Insets::hv(8.0, 0.0))
            .radius(7.0)
            .hover_fill(Token::Hover)
            .press_fill(Token::Hover)
            .align(Align::Center)
            .cursor(Cursor::Hand)
            .button(LINK)
            .tooltip("Open Settings › Addons"),
    );
    ui.tree.add(
        link,
        Node::text(LINK, 12.5)
            .weight(Weight::Medium)
            .ink(Token::Accent)
            .role(Role::None),
    );
    // The grip, over the leading edge (and over the addon's page).
    let grip = ui.tree.add(
        pane_node,
        Node::new()
            .absolute(Abs {
                left: Some(0.0),
                top: Some(0.0),
                right: None,
                bottom: Some(0.0),
            })
            .width(GRIP)
            .hover_fill(Swatch::Fade(Token::Accent, 0.35))
            .cursor(Cursor::ResizeEw)
            .hit()
            .layer()
            .tooltip("Drag to resize")
            .role(Role::None),
    );
    p.nodes.set(Some(Nodes {
        pane: pane_node,
        body,
        placeholder,
        link,
        close,
        grip,
    }));
}

/// Whether the slot is on screen.
pub fn showing() -> bool {
    pane().shown.get()
}

/// The pane's width now (DIPs).
pub fn width() -> f32 {
    pane().width.get()
}

/// The addon's page, once made.
pub fn view() -> Option<Rc<AddonView>> {
    pane().view.borrow().clone()
}

/// Which face shows: `"addon"`, `"placeholder"`.
pub fn face() -> &'static str {
    if view().is_some() {
        "addon"
    } else {
        "placeholder"
    }
}

/// The shell is up: the kept width, and the grip's drag.
pub fn start(shell: &Shell) {
    let p = pane();
    let w = policy::clamp_width(shell.settings().number(policy::WIDTH_KEY));
    p.width.set(w);
    let chrome = shell.view().chrome().clone();
    if let Some(n) = p.nodes.get() {
        chrome.edit(move |ui, _| ui.tree.set_prop(n.pane, Prop::Width, w));
    }
    if !p.hooked.replace(true) {
        hook_grip(shell);
    }
}

/// Ctrl+E.
pub fn toggle(shell: &Shell) {
    if showing() {
        hide(shell);
    } else {
        show(shell);
    }
}

/// Show the slot and give it the keyboard.
pub fn show(shell: &Shell) {
    let p = pane();
    let Some(n) = p.nodes.get() else { return };
    if p.shown.replace(true) {
        focus_inside(shell);
        return;
    }
    let generation = p.generation.get() + 1;
    p.generation.set(generation);
    sync(shell);
    let chrome = shell.view().chrome().clone();
    chrome.watch("agent sidebar");
    chrome.edit(move |ui, _| {
        let was_hidden = ui.tree.get(n.pane).is_some_and(|node| node.style.hidden);
        if was_hidden {
            ui.tree.set_prop(n.pane, Prop::Opacity, 0.0);
        }
        ui.tree.set_hidden(n.pane, false);
        ui.tree.animate(n.pane, Prop::Opacity, 1.0, motion::GLIDE);
    });
    if let Some(v) = view() {
        v.set_visible(true);
    }
    focus_inside(shell);
}

/// Put the slot away; the page gets the keyboard back if the slot had it.
pub fn hide(shell: &Shell) {
    let p = pane();
    let Some(n) = p.nodes.get() else { return };
    if !p.shown.replace(false) {
        return;
    }
    let generation = p.generation.get() + 1;
    p.generation.set(generation);
    p.drag.set(None);
    if let Some(v) = view() {
        v.set_visible(false);
    }
    let chrome = shell.view().chrome().clone();
    let had_focus = chrome
        .read(move |ui, _| {
            ui.tree
                .focused()
                .is_some_and(|f| ui.tree.is_within(f, n.pane))
        })
        .unwrap_or(false)
        || view().is_some_and(|v| v.focused());
    chrome.edit(move |ui, _| {
        ui.tree.animate(n.pane, Prop::Opacity, 0.0, motion::QUICK);
    });
    if had_focus {
        shell.view().focus_page();
    }
    // Out of the layout once the fade is done (unless shown again).
    let weak = Rc::downgrade(&p);
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(160)).await;
        let Some(p) = weak.upgrade() else { return };
        if p.generation.get() != generation || p.shown.get() {
            return;
        }
        if let Some(s) = crate::platform::current() {
            s.view()
                .chrome()
                .edit(move |ui, _| ui.tree.set_hidden(n.pane, true));
        }
    });
}

fn focus_inside(shell: &Shell) {
    let p = pane();
    let Some(n) = p.nodes.get() else { return };
    shell.view().chrome().edit(move |ui, _| {
        match p.slot.get().filter(|_| p.view.borrow().is_some()) {
            Some(slot) => ui.focus_page(slot, FocusReason::Programmatic),
            None => ui.focus(Some(n.link)),
        }
    });
}

/// A click in the pane: `true` if it was the pane's.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let Some(n) = pane().nodes.get() else {
        return false;
    };
    if id == n.close {
        hide(shell);
        true
    } else if id == n.link {
        crate::addons::open_settings(shell);
        true
    } else {
        id == n.grip
    }
}

/// Bring the slot in line with the addon the core says it shows: nothing
/// (placeholder), the same one (settings may have changed, or a new
/// version: reload), or another addon (a new page). Before the first show
/// nothing is made: the page is made from the state then.
pub fn sync(shell: &Shell) {
    let p = pane();
    let Some(n) = p.nodes.get() else { return };
    if !p.shown.get() && p.view.borrow().is_none() {
        return;
    }
    let active = crate::addons::glue::active();
    let current = p.view.borrow().clone();
    let chrome = shell.view().chrome().clone();
    match (active, current) {
        (None, None) => {}
        (None, Some(v)) => {
            tracing::info!(id = %v.addon_id(), "addon left the slot; showing the placeholder");
            v.retire();
            p.view.borrow_mut().take();
            let slot = p.slot.take();
            chrome.edit(move |ui, _| {
                if let Some(slot) = slot {
                    ui.remove_slot(slot);
                    ui.tree.remove(slot);
                }
                ui.tree.set_hidden(n.placeholder, false);
            });
        }
        (Some(a), Some(v)) if v.addon_id() == a.id => {
            crate::addons::scheme::preload(&a);
            v.update(&a);
        }
        (Some(a), current) => {
            if let Some(old) = current {
                old.retire();
                p.view.borrow_mut().take();
                let slot = p.slot.take();
                chrome.edit(move |ui, _| {
                    if let Some(slot) = slot {
                        ui.remove_slot(slot);
                        ui.tree.remove(slot);
                    }
                });
            }
            crate::addons::scheme::preload(&a);
            let Some(v) = AddonView::new(shell, &a) else {
                return;
            };
            let page = v.page().clone();
            *p.view.borrow_mut() = Some(v.clone());
            let p2 = p.clone();
            chrome.edit(move |ui, _| {
                let s = ui
                    .tree
                    .add(n.body, Node::slot().absolute(Abs::FILL).name(NAME));
                ui.set_slot(s, Box::new(PageHost::new(page)));
                ui.tree.set_hidden(n.placeholder, true);
                p2.slot.set(Some(s));
            });
            v.set_visible(p.shown.get());
        }
    }
}

/// The grip's drag, from the window's raw mouse messages (the toolkit has
/// no drag events): press on the grip captures the mouse, moves resize,
/// release keeps the width (`agent.paneWidth`).
fn hook_grip(shell: &Shell) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    };
    let chrome = shell.view().chrome().clone();
    let hwnd = chrome.window().hwnd();
    let weak = shell.downgrade();
    copper_win_rt::add_message_hook(move |m| {
        if m.hwnd != hwnd.0 as isize {
            return false;
        }
        let p = pane();
        let Some(n) = p.nodes.get() else { return false };
        // Client pixels → DIPs.
        let lx = f32::from(m.lparam as u32 as u16 as i16);
        let ly = f32::from((m.lparam as u32 >> 16) as u16 as i16);
        match m.message {
            WM_LBUTTONDOWN if p.shown.get() => {
                let Some((x, on_grip)) = chrome.read(move |ui, _| {
                    let s = ui.scale().max(0.5);
                    let (x, y) = (lx / s, ly / s);
                    let r = ui.tree.window_rect(n.grip);
                    let on = ui.tree.is_shown(n.grip)
                        && x >= r.x
                        && x < r.x + r.w
                        && y >= r.y
                        && y < r.y + r.h;
                    (x, on)
                }) else {
                    return false;
                };
                if !on_grip {
                    return false;
                }
                p.drag.set(Some((p.width.get(), x)));
                // SAFETY: capturing our own window's mouse on its thread.
                unsafe {
                    SetCapture(hwnd);
                }
                true
            }
            WM_MOUSEMOVE => {
                let Some((from, x0)) = p.drag.get() else {
                    return false;
                };
                // The button came up where we didn't see it (capture lost).
                if m.wparam & 0x0001 == 0 {
                    p.drag.set(None);
                    if let Some(s) = weak.upgrade() {
                        let w = p.width.get();
                        s.update_settings(|st| st.set(policy::WIDTH_KEY, f64::from(w)));
                    }
                    return false;
                }
                let Some(x) = chrome.read(move |ui, _| lx / ui.scale().max(0.5)) else {
                    return true;
                };
                let w = policy::dragged_width(from, x0, x);
                if w != p.width.get() {
                    p.width.set(w);
                    chrome.edit(move |ui, _| ui.tree.set_prop(n.pane, Prop::Width, w));
                }
                true
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                if p.drag.take().is_none() {
                    return false;
                }
                if m.message == WM_LBUTTONUP {
                    // SAFETY: releasing the capture this hook took.
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                let w = p.width.get();
                if let Some(s) = weak.upgrade() {
                    s.update_settings(|st| st.set(policy::WIDTH_KEY, f64::from(w)));
                }
                tracing::debug!(width = w, "agent sidebar resized");
                m.message == WM_LBUTTONUP
            }
            _ => false,
        }
    });
}

/// The middle of the grip, in the window's client pixels (the self-test
/// drags it with posted mouse messages).
pub fn grip_point(shell: &Shell) -> Option<(i32, i32)> {
    let n = pane().nodes.get()?;
    shell.view().chrome().read(move |ui, _| {
        let s = ui.scale();
        let r = ui.tree.window_rect(n.grip);
        (
            ((r.x + r.w / 2.0) * s).round() as i32,
            ((r.y + r.h / 2.0) * s).round() as i32,
        )
    })
}

/// Settings or a test changed the width: apply `w` now and keep it.
pub fn set_width(shell: &Shell, w: f64) {
    let p = pane();
    let width = policy::clamp_width(Some(w));
    p.width.set(width);
    if let Some(n) = p.nodes.get() {
        shell
            .view()
            .chrome()
            .edit(move |ui, _| ui.tree.set_prop(n.pane, Prop::Width, width));
    }
    shell.update_settings(|st| st.set(policy::WIDTH_KEY, f64::from(width)));
}
