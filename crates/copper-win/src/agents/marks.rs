//! The agents' marks, drawn (Mac `HandsUI.swift` + `Split.swift`; Linux
//! `agents/mark.rs`), from [`super::hands`]:
//!
//! * over the page on screen, top centre: one capsule per hand (at most
//!   three) — the agent's square, its name and thread, what it is doing,
//!   Stop. Solid while a call is in flight, breathing between calls. When
//!   the pointer comes near, a capsule tucks in to its square and Stop so
//!   the page under it can be clicked; it comes back 300 ms after the
//!   pointer leaves the room it took.
//! * a 2 px ring inside the stage, in the busy hand's colour (hit-test
//!   transparent).
//! * top right, when an agent works only in a tab behind this one: the
//!   same capsule, saying so; a click shows that tab.
//! * on the tab's row in the column: the hands' squares (at most three,
//!   overlapped, the latest first), named in their tooltip.
//!
//! None of this needs the Agent sidebar or an addon. The nodes over the
//! stage are this module's, added as the stage's last children (above the
//! pages); a row's squares are added into the column's row for that tab
//! (see `chrome/column.rs`: the hook it would take to own them instead is
//! in the module's report).

use super::hands::{self, Hand, Hands};
use crate::platform::{Shell, Win};
use copper_core::browser::TabId;
use copper_mcp::drive::DriveEvent;
use copper_win_ui::spring::{Bezier, Motion};
use copper_win_ui::tokens::{Shadow, motion};
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Abs, Align, Color, Insets, Justify, Node, NodeId, Prop, Rect, Role, Swatch, Token, Trim, Weight,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A hand's square (Mac `HandsUI` badge).
const SQUARE: f32 = 14.0;
/// The capsules: 24 high, 6 apart, 8 down from the stage's top.
const CAPSULE: f32 = 24.0;
const TOP: f32 = 8.0;
/// The pointer this close (DIPs) tucks a capsule in.
const NEAR: f32 = 16.0;
const UNTUCK_AFTER: Duration = Duration::from_millis(300);
/// Breathing between calls: to .55 and back, 900 ms each way.
const BREATH: Duration = Duration::from_millis(900);
const EASE_IN_OUT: Bezier = Bezier::new(0.42, 0.0, 0.58, 1.0);

#[derive(Clone, Copy)]
struct Nodes {
    bar: NodeId,
    ring: NodeId,
    away: NodeId,
}

/// What a click on one of ours means.
#[derive(Clone, Debug)]
enum Target {
    Stop(String),
    Show(TabId),
}

/// One capsule as drawn.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    key: String,
    action: String,
    color: (f32, f32, f32),
    busy: bool,
    stopped: bool,
}

/// What the bar shows, to redraw only when it changes.
#[derive(Clone, Debug, PartialEq, Default)]
struct BarDrawn {
    tab: Option<TabId>,
    hands: Vec<Seen>,
    tucked: bool,
}

#[derive(Default)]
struct Marks {
    hands: RefCell<Hands>,
    nodes: Cell<Option<Nodes>>,
    targets: RefCell<HashMap<NodeId, Target>>,
    /// Each tab's squares in its column row.
    rows: RefCell<HashMap<TabId, (NodeId, NodeId)>>,
    bar: RefCell<BarDrawn>,
    /// The capsules' text nodes and their frames, for tucking.
    texts: RefCell<Vec<NodeId>>,
    capsules: RefCell<Vec<(NodeId, bool)>>,
    tucked: Cell<bool>,
    /// The room the full capsules took when they tucked in.
    zone: Cell<Option<Rect>>,
    untuck_gen: Cell<u64>,
    expiry_gen: Cell<u64>,
    breathing: Cell<bool>,
    hooked: Cell<bool>,
}

thread_local! {
    static MARKS: Rc<Marks> = Rc::new(Marks::default());
}

fn marks() -> Rc<Marks> {
    MARKS.with(Rc::clone)
}

fn rgb((r, g, b): (f32, f32, f32)) -> Color {
    Color::rgb(r, g, b)
}

/// Build the stage's marks (hidden) and follow the window.
pub fn install(shell: &Shell) {
    shell.view().chrome().edit(|ui, st| {
        let stage = st.stage.stage;
        let bar = ui.tree.add(
            stage,
            Node::row()
                .absolute(Abs {
                    left: Some(0.0),
                    top: Some(TOP),
                    right: Some(0.0),
                    bottom: None,
                })
                .height(CAPSULE)
                .gap(6.0)
                .justify(Justify::Center)
                .align(Align::Center)
                .role(Role::Group)
                .name("Agents working here"),
        );
        let ring = ui.tree.add(
            stage,
            Node::new()
                .absolute(Abs::FILL)
                .stroke(Swatch::Clear, 2.0)
                .layer()
                .opacity(0.0)
                .hidden(true)
                .role(Role::None),
        );
        let away = ui.tree.add(
            stage,
            Node::row()
                .absolute(Abs {
                    left: None,
                    top: Some(TOP),
                    right: Some(8.0),
                    bottom: None,
                })
                .hidden(true)
                .role(Role::Group)
                .name("An agent at work in another tab"),
        );
        marks().nodes.set(Some(Nodes { bar, ring, away }));
    });
    let mut rx = shell.hub().subscribe();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        use copper_shell_core::control::ShellEvent as E;
        use tokio::sync::broadcast::error::RecvError;
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            let Some(s) = weak.upgrade() else { break };
            match &ev {
                E::TabClosed(id) => {
                    marks().hands.borrow_mut().forget(id);
                    marks().rows.borrow_mut().remove(id);
                    redraw(&s);
                }
                E::Selected(_) | E::SpaceSwitched(_) | E::TabChanged(_) => redraw(&s),
                _ => {}
            }
        }
    });
    shell.on_dark_changed(|_| {
        if let Some(s) = crate::platform::current() {
            marks().bar.borrow_mut().hands.clear();
            redraw(&s);
        }
    });
    hook_pointer(shell);
}

/// One drive event from the agents lane.
pub fn event(e: &DriveEvent) {
    let tint = crate::platform::current().and_then(|s| {
        let b = s.browser();
        crate::chrome::model::tint_of(b.current_space())
            .map(|t| hands::hue((t.color.r, t.color.g, t.color.b)))
    });
    let changed = {
        let m = marks();
        let mut h = m.hands.borrow_mut();
        h.tint = tint;
        h.event(e, Instant::now())
    };
    if !changed.is_empty()
        && let Some(s) = crate::platform::current()
    {
        redraw(&s);
    }
}

/// The server stopped: no marks.
pub fn clear() {
    let had = marks().hands.borrow_mut().clear();
    if !had.is_empty()
        && let Some(s) = crate::platform::current()
    {
        redraw(&s);
    }
}

/// A click on one of the marks' nodes; `true` if it was ours.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let Some(t) = marks().targets.borrow().get(&id).cloned() else {
        return false;
    };
    match t {
        Target::Stop(key) => stop(shell, &key),
        Target::Show(tab) => {
            shell.select(&tab);
        }
    }
    true
}

/// Stop `key`: a Jev run stops before its next move, any other agent has
/// its next calls refused for 30 s (`Agents::stop_hand`).
pub fn stop(shell: &Shell, key: &str) {
    if let Some(a) = copper_shell_core::agents::get::<Win>() {
        a.stop_hand(key);
    }
    marks().hands.borrow_mut().stop(key, Instant::now());
    redraw(shell);
}

/// The hands on `tab` (the self-test reads them).
pub fn on(tab: &TabId) -> Vec<Hand> {
    marks().hands.borrow().on(tab, Instant::now())
}

/// Whether the capsules are tucked in (the self-test reads it).
pub fn tucked() -> bool {
    marks().tucked.get()
}

/// The middle of the first capsule, in client pixels (the self-test moves
/// the pointer there).
pub fn capsule_point(shell: &Shell) -> Option<(i32, i32)> {
    let cap = marks().capsules.borrow().first().map(|(n, _)| *n)?;
    shell.view().chrome().read(move |ui, _| {
        let s = ui.scale();
        let r = ui.tree.window_rect(cap);
        (
            ((r.x + r.w / 2.0) * s).round() as i32,
            ((r.y + r.h / 2.0) * s).round() as i32,
        )
    })
}

/// How many capsules the bar shows, whether the ring and the away pill
/// are showing, and how many rows carry squares (the self-test reads them).
pub fn shown(shell: &Shell) -> (usize, bool, bool, usize) {
    let Some(n) = marks().nodes.get() else {
        return (0, false, false, 0);
    };
    let rows: Vec<NodeId> = marks().rows.borrow().values().map(|(_, c)| *c).collect();
    shell
        .view()
        .chrome()
        .read(move |ui, _| {
            let shown = |id| ui.tree.get(id).is_some_and(|n| !n.style.hidden);
            (
                ui.tree.children(n.bar).len(),
                shown(n.ring),
                shown(n.away),
                rows.into_iter().filter(|c| ui.tree.contains(*c)).count(),
            )
        })
        .unwrap_or((0, false, false, 0))
}

/// Draw what the hands say now, and come back when the next mark ages out.
pub fn redraw(shell: &Shell) {
    let m = marks();
    let Some(nodes) = m.nodes.get() else { return };
    let now = Instant::now();
    let shown = shell.active_id();
    let (bar_hands, ring, away, rows, expiry) = {
        let h = m.hands.borrow();
        let bar = shown.as_ref().map(|t| h.on(t, now)).unwrap_or_default();
        let ring = shown.as_ref().and_then(|t| h.ring(t, now));
        let away = if ring.is_none() {
            h.elsewhere(shown.as_ref(), now)
                .and_then(|hand| tab_of(&h, &hand.key, now).map(|t| (hand, t)))
        } else {
            None
        };
        let rows: Vec<(TabId, Vec<Hand>)> = h
            .tabs(now)
            .into_iter()
            .map(|t| (t.clone(), h.on(&t, now)))
            .collect();
        (bar, ring, away, rows, h.next_expiry(now))
    };
    let drawn = BarDrawn {
        tab: shown.clone(),
        hands: bar_hands
            .iter()
            .map(|h| Seen {
                key: h.key.clone(),
                action: h.action.clone(),
                color: h.color,
                busy: h.busy > 0,
                stopped: h.stopped,
            })
            .collect(),
        tucked: m.tucked.get(),
    };
    let bar_changed = *m.bar.borrow() != drawn;
    *m.bar.borrow_mut() = drawn;
    let idle = bar_hands.iter().any(|h| h.busy == 0);
    let me = m.clone();
    shell.view().chrome().edit(move |ui, st| {
        if bar_changed {
            draw_bar(&me, ui, nodes.bar, &bar_hands);
        }
        draw_ring(ui, nodes.ring, ring);
        draw_away(&me, ui, nodes.away, away.as_ref());
        draw_rows(&me, ui, st, &rows);
    });
    if idle {
        breathe(shell);
    }
    if let Some(after) = expiry {
        let gen_now = m.expiry_gen.get() + 1;
        m.expiry_gen.set(gen_now);
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            copper_win_rt::sleep(after + Duration::from_millis(50)).await;
            if marks().expiry_gen.get() == gen_now
                && let Some(s) = weak.upgrade()
            {
                redraw(&s);
            }
        });
    }
}

/// The tab `key` is busy on.
fn tab_of(h: &Hands, key: &str, now: Instant) -> Option<TabId> {
    h.tabs(now)
        .into_iter()
        .find(|t| h.on(t, now).iter().any(|x| x.key == key && x.busy > 0))
}

/// A hand's square: its colour, a white rim, its initials (or Jev's
/// sparkle).
fn square(ui: &mut Ui, parent: NodeId, hand: &Hand, size: f32, abs: Option<Abs>) -> NodeId {
    let mut n = Node::stack()
        .size(size, size)
        .shrink(0.0)
        .radius(0.30 * size)
        .fill(Swatch::Fixed(rgb(hand.color)))
        .stroke(Swatch::Fixed(Color::rgba(1.0, 1.0, 1.0, 0.55)), 1.0)
        .shadow(Shadow::new(0.18, 1.0, 0.0))
        .align(Align::Center)
        .justify(Justify::Center)
        .role(Role::None);
    if let Some(a) = abs {
        n = n.absolute(a);
    }
    let sq = ui.tree.add(parent, n);
    let ink = if hand.dark_ink() {
        Color::grey(0.12)
    } else {
        Color::grey(1.0)
    };
    let (text, scale) = if hand.jev {
        ("\u{2726}".to_string(), 0.55)
    } else {
        (hand.initials.clone(), 0.62)
    };
    ui.tree.add(
        sq,
        Node::text(&text, (scale * size).round())
            .weight(Weight::Bold)
            .ink(Swatch::Fixed(ink))
            .role(Role::None),
    );
    sq
}

/// What the hand is doing, said in one line.
fn saying(h: &Hand) -> String {
    if h.thread.is_empty() {
        h.agent.clone()
    } else {
        format!("{} · {}", h.agent, h.thread)
    }
}

fn draw_bar(m: &Marks, ui: &mut Ui, bar: NodeId, hands: &[Hand]) {
    for c in ui.tree.children(bar).to_vec() {
        forget_targets(m, ui, c);
    }
    ui.tree.clear_children(bar);
    m.texts.borrow_mut().clear();
    m.capsules.borrow_mut().clear();
    let tucked = m.tucked.get();
    for h in hands {
        let cap = capsule(m, ui, bar, h, tucked);
        m.capsules.borrow_mut().push((cap, h.busy > 0));
    }
}

/// One capsule: ground .94 with the hand's colour at .16 over it, a
/// 1.25 px rim in its colour, the square, the words, Stop.
fn capsule(m: &Marks, ui: &mut Ui, parent: NodeId, h: &Hand, tucked: bool) -> NodeId {
    let c = rgb(h.color);
    let busy = h.busy > 0;
    let what = if h.stopped {
        "Stopped".to_string()
    } else if h.action.is_empty() {
        if busy { "Working" } else { "Waiting" }.to_string()
    } else {
        h.action.clone()
    };
    let cap = ui.tree.add(
        parent,
        Node::row()
            .height(CAPSULE)
            .shrink(0.0)
            .padding(Insets::new(5.0, 0.0, 5.0, 0.0))
            .gap(6.0)
            .align(Align::Center)
            .radius(CAPSULE / 2.0)
            .fill(Swatch::Fade(Token::Ground, 0.94))
            .stroke(Swatch::Fixed(c.alpha(0.75)), 1.25)
            .shadow(Shadow::new(0.25, 6.0, 1.0))
            .layer()
            .opacity(if tucked { 0.55 } else { 1.0 })
            .role(Role::Group)
            .name(&format!("{} — {what}", saying(h))),
    );
    ui.tree.add(
        cap,
        Node::new()
            .absolute(Abs::FILL)
            .radius(CAPSULE / 2.0)
            .fill(Swatch::Fixed(c.alpha(0.16)))
            .role(Role::None),
    );
    square(ui, cap, h, SQUARE, None);
    let words = ui.tree.add(
        cap,
        Node::row()
            .gap(5.0)
            .align(Align::Center)
            .max_width(320.0)
            .hidden(tucked)
            .role(Role::None),
    );
    ui.tree.add(
        words,
        Node::text(&saying(h), 11.0)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .trim(Trim::End)
            .max_width(150.0)
            .role(Role::None),
    );
    ui.tree.add(
        words,
        Node::text(&what, 11.0)
            .ink(Token::Muted)
            .trim(Trim::End)
            .max_width(165.0)
            .role(Role::None),
    );
    m.texts.borrow_mut().push(words);
    if !h.stopped {
        let stop = ui.tree.add(
            cap,
            Node::stack()
                .size(16.0, 16.0)
                .shrink(0.0)
                .radius(8.0)
                .hover_fill(Swatch::Fade(Token::Ink, 0.10))
                .press_fill(Swatch::Fade(Token::Ink, 0.16))
                .align(Align::Center)
                .justify(Justify::Center)
                .button(&format!("Stop {}", h.agent))
                .tooltip("Stop — its next calls are turned away"),
        );
        ui.tree.add(
            stop,
            Node::new()
                .size(6.0, 6.0)
                .radius(1.5)
                .fill(Token::Ink)
                .role(Role::None),
        );
        m.targets
            .borrow_mut()
            .insert(stop, Target::Stop(h.key.clone()));
    }
    cap
}

fn draw_ring(ui: &mut Ui, ring: NodeId, color: Option<(f32, f32, f32)>) {
    match color {
        Some(c) => {
            ui.tree.edit(ring, |n| {
                n.look.stroke = Swatch::Fixed(rgb(c).alpha(0.60));
                n.look.stroke_width = 2.0;
            });
            ui.tree.set_hidden(ring, false);
            ui.tree.animate(ring, Prop::Opacity, 1.0, motion::QUICK);
        }
        None => {
            if ui.tree.get(ring).is_some_and(|n| !n.style.hidden) {
                ui.tree.set_prop(ring, Prop::Opacity, 0.0);
                ui.tree.set_hidden(ring, true);
            }
        }
    }
}

fn draw_away(m: &Marks, ui: &mut Ui, away: NodeId, what: Option<&(Hand, TabId)>) {
    for c in ui.tree.children(away).to_vec() {
        forget_targets(m, ui, c);
    }
    ui.tree.clear_children(away);
    let Some((h, tab)) = what else {
        ui.tree.set_hidden(away, true);
        return;
    };
    ui.tree.set_hidden(away, false);
    let cap = capsule(m, ui, away, h, false);
    // The capsule's own click (not its Stop) shows the tab it works in.
    ui.tree.edit(cap, |n| {
        n.flags.hit = true;
        n.look.hover = Some(Swatch::Fade(Token::Ground, 1.0));
        n.tooltip = Some("Show the tab it works in".into());
    });
    m.targets
        .borrow_mut()
        .insert(cap, Target::Show(tab.clone()));
}

/// The squares in each tab's column row.
fn draw_rows(m: &Marks, ui: &mut Ui, st: &crate::chrome::State, rows: &[(TabId, Vec<Hand>)]) {
    let mut placed = m.rows.borrow_mut();
    // Old squares first: a tab with no live hand loses them.
    let keep: Vec<&TabId> = rows.iter().map(|(t, _)| t).collect();
    let stale: Vec<TabId> = placed
        .keys()
        .filter(|t| !keep.contains(t))
        .cloned()
        .collect();
    for t in stale {
        if let Some((_, cluster)) = placed.remove(&t)
            && ui.tree.contains(cluster)
        {
            forget_targets(m, ui, cluster);
            ui.tree.remove(cluster);
        }
    }
    if st.side && st.folded {
        return;
    }
    for (tab, hands) in rows {
        let Some(row) = row_of(ui, st, tab) else {
            continue;
        };
        if let Some((was_row, cluster)) = placed.remove(tab)
            && ui.tree.contains(cluster)
        {
            forget_targets(m, ui, cluster);
            ui.tree.remove(cluster);
            let _ = was_row;
        }
        if hands.is_empty() {
            continue;
        }
        let Some(cluster) = cluster_into(ui, row, hands) else {
            continue;
        };
        m.targets
            .borrow_mut()
            .insert(cluster, Target::Show(tab.clone()));
        placed.insert(tab.clone(), (row, cluster));
    }
}

/// The column row (not a pinned tile) showing `tab`.
fn row_of(ui: &Ui, st: &crate::chrome::State, tab: &TabId) -> Option<NodeId> {
    let mut stack = vec![st.column.side];
    while let Some(n) = stack.pop() {
        if let Some(crate::chrome::column::Hit::Tab(t)) = st.column.hit(n)
            && &t == tab
        {
            let kids = ui.tree.children(n);
            // A row: mark, title, …; a pinned tile has no title.
            let titled = kids
                .get(1)
                .and_then(|k| ui.tree.get(*k))
                .is_some_and(|k| k.content_text().is_some());
            return titled.then_some(n);
        }
        stack.extend_from_slice(ui.tree.children(n));
    }
    None
}

/// The squares, after the row's title: the latest first and on top,
/// each overlapping the one before by .35 of its size.
fn cluster_into(ui: &mut Ui, row: NodeId, hands: &[Hand]) -> Option<NodeId> {
    let step = SQUARE * 0.65;
    let width = SQUARE + step * (hands.len().saturating_sub(1)) as f32;
    let tip = hands
        .iter()
        .map(|h| {
            let what = if h.stopped {
                "stopped".to_string()
            } else if h.busy > 0 {
                format!("working — {}", h.action)
            } else if h.action.is_empty() {
                "done".to_string()
            } else {
                format!("last: {}", h.action)
            };
            format!("{} ({what})", saying(h))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cluster = ui.tree.insert(
        row,
        2,
        Node::stack()
            .size(width, SQUARE)
            .shrink(0.0)
            .hit()
            .tooltip(&tip)
            .role(Role::Image)
            .name(&tip),
    );
    for (i, h) in hands.iter().enumerate().rev() {
        square(
            ui,
            cluster,
            h,
            SQUARE,
            Some(Abs {
                left: Some(step * i as f32),
                top: Some(0.0),
                right: None,
                bottom: None,
            }),
        );
    }
    Some(cluster)
}

fn forget_targets(m: &Marks, ui: &Ui, id: NodeId) {
    let mut stack = vec![id];
    let mut targets = m.targets.borrow_mut();
    while let Some(n) = stack.pop() {
        targets.remove(&n);
        stack.extend_from_slice(ui.tree.children(n));
    }
}

/// Between calls a capsule breathes (to .55 and back, 900 ms each way);
/// nothing moves with reduced motion.
fn breathe(shell: &Shell) {
    let m = marks();
    if m.breathing.replace(true) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let mut low = true;
        loop {
            let Some(s) = weak.upgrade() else { break };
            let m = marks();
            let idle: Vec<NodeId> = m
                .capsules
                .borrow()
                .iter()
                .filter(|(_, busy)| !busy)
                .map(|(n, _)| *n)
                .collect();
            if idle.is_empty() {
                break;
            }
            let to = if m.tucked.get() || low { 0.55 } else { 1.0 };
            s.view().chrome().edit(move |ui, _| {
                if ui.tree.reduce_motion {
                    return;
                }
                for n in idle {
                    if ui.tree.contains(n) {
                        ui.tree.animate(
                            n,
                            Prop::Opacity,
                            to,
                            Motion::Curve {
                                curve: EASE_IN_OUT,
                                seconds: BREATH.as_secs_f32(),
                            },
                        );
                    }
                }
            });
            low = !low;
            drop(s);
            copper_win_rt::sleep(BREATH).await;
        }
        marks().breathing.set(false);
    });
}

/// The pointer near the capsules tucks them in (Linux `mark.rs`): the
/// words go and they fade to .55, leaving the square and Stop; they come
/// back 300 ms after the pointer leaves the room they took.
fn hook_pointer(shell: &Shell) {
    let m = marks();
    if m.hooked.replace(true) {
        return;
    }
    let hwnd = shell.view().chrome().window().hwnd().0 as isize;
    let weak = shell.downgrade();
    copper_win_rt::add_message_hook(move |msg| {
        const WM_MOUSEMOVE: u32 = 0x0200;
        if msg.message != WM_MOUSEMOVE || msg.hwnd != hwnd {
            return false;
        }
        let m = marks();
        if m.capsules.borrow().is_empty() && !m.tucked.get() {
            return false;
        }
        let Some(s) = weak.upgrade() else {
            return false;
        };
        let x = (msg.lparam & 0xffff) as i16 as f32;
        let y = ((msg.lparam >> 16) & 0xffff) as i16 as f32;
        pointer_at(&s, x, y);
        false
    });
}

fn pointer_at(shell: &Shell, px: f32, py: f32) {
    let m = marks();
    let Some(nodes) = m.nodes.get() else { return };
    let caps: Vec<NodeId> = m.capsules.borrow().iter().map(|(n, _)| *n).collect();
    let Some((p, room)) = shell.view().chrome().read(move |ui, _| {
        let s = ui.scale();
        let p = (px / s, py / s);
        let mut room: Option<Rect> = None;
        for c in caps {
            let r = ui.tree.window_rect(c);
            room = Some(match room {
                None => r,
                Some(a) => union(a, r),
            });
        }
        let _ = nodes;
        (p, room)
    }) else {
        return;
    };
    let near = |r: Rect| {
        p.0 >= r.x - NEAR && p.0 <= r.x + r.w + NEAR && p.1 >= r.y - NEAR && p.1 <= r.y + r.h + NEAR
    };
    if m.tucked.get() {
        let inside = m.zone.get().is_some_and(near);
        if inside {
            m.untuck_gen.set(m.untuck_gen.get() + 1);
            return;
        }
        let g = m.untuck_gen.get() + 1;
        m.untuck_gen.set(g);
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            copper_win_rt::sleep(UNTUCK_AFTER).await;
            let m = marks();
            if m.untuck_gen.get() == g
                && let Some(s) = weak.upgrade()
            {
                set_tucked(&s, false);
            }
        });
    } else if let Some(r) = room
        && near(r)
    {
        m.zone.set(Some(r));
        set_tucked(shell, true);
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = (a.x + a.w).max(b.x + b.w);
    let bt = (a.y + a.h).max(b.y + b.h);
    Rect::new(x, y, r - x, bt - y)
}

fn set_tucked(shell: &Shell, on: bool) {
    let m = marks();
    if m.tucked.replace(on) == on {
        return;
    }
    if !on {
        m.zone.set(None);
    }
    let texts = m.texts.borrow().clone();
    let caps: Vec<NodeId> = m.capsules.borrow().iter().map(|(n, _)| *n).collect();
    m.bar.borrow_mut().tucked = on;
    shell.view().chrome().edit(move |ui, _| {
        for t in texts {
            ui.tree.set_hidden(t, on);
        }
        for c in caps {
            ui.tree
                .animate(c, Prop::Opacity, if on { 0.55 } else { 1.0 }, motion::QUICK);
        }
    });
}
