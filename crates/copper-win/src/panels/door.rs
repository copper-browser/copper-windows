//! The doors panels hang from, mounted into the chrome at start-up: the
//! downloads door (`DownloadsDoor`) at the end of the tab strip and inside
//! the column's address, and the bookmarks door at the strip's far end
//! (`TabBar.swift` 107-122; the column's foot has its own).
//!
//! The downloads door is there only once something has been downloaded
//! this session. It says what is happening at a glance: a ring filling
//! while files arrive (dashes turning when a size is unknown), a check
//! that pulses as the last one lands, a warning in orange when one
//! failed, a dot until the list has been looked at.
//!
//! (The chrome can take these over as slots of its own; until then they
//! are put in place here, after the chrome is built.)

use super::ring::{self, Layer, Stroke};
use crate::chrome::build::{door, tip};
use crate::chrome::glyph;
use crate::platform::Shell;
use copper_core::downloads::DownloadState;
use copper_shell_core::format;
use copper_win_ui::tokens::motion;
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Color, Node, NodeId, Prop, Role, Swatch, Token};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

/// The tab strip's doors.
const STRIP: f32 = 26.0;
/// The column address's door.
const COLUMN: f32 = 24.0;
/// The pictures the rings are drawn into (the panels' block of ids).
const RING_STRIP: u32 = 0x7061_1001;
const RING_COLUMN: u32 = 0x7061_1002;
/// How long the check stays after the last download lands.
const PULSE: Duration = Duration::from_millis(1200);
/// The dashes turn once in this long.
const TURN: f32 = 2.0;
/// How often the dashes move.
const SPIN: Duration = Duration::from_millis(50);
/// The failed door's orange (`Color.orange.opacity(0.9)`).
const ORANGE: Color = Color {
    r: 1.0,
    g: 0.584,
    b: 0.0,
    a: 0.9,
};

/// One downloads door's nodes.
struct Door {
    door: NodeId,
    ring: NodeId,
    glyph: NodeId,
    dot: NodeId,
    image: u32,
    size: f32,
    /// What its ring was last drawn as (at what scale).
    drawn: Option<(Vec<Layer>, f32)>,
    pulsing: bool,
}

#[derive(Default)]
struct Doors {
    downloads: Vec<Door>,
    bookmarks: Option<NodeId>,
    /// How many rows had finished when last looked.
    finished: usize,
    pulse_until: Option<Instant>,
}

thread_local! {
    static DOORS: RefCell<Doors> = RefCell::default();
    static SCHEDULED: Cell<bool> = const { Cell::new(false) };
    static SPINNING: Cell<bool> = const { Cell::new(false) };
    static EPOCH: Instant = Instant::now();
}

/// What the doors show, worked out from the downloads.
#[derive(Clone, Debug, PartialEq)]
struct Look {
    shown: bool,
    glyph: char,
    failed_tint: bool,
    pulsing: bool,
    unseen: bool,
    tip: String,
    ring: Ring,
    open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ring {
    None,
    Fraction(f32),
    Turning,
    Failed,
    /// Nothing arriving: the ring of `arrow.down.circle`.
    Rest,
}

/// Put the doors in the chrome and follow the downloads.
pub fn mount(shell: &Shell) {
    shell.view().chrome().edit(|ui, st| {
        let strip = st.strip.strip;
        // Before the caption buttons' corner, after the helm.
        let at = ui.tree.children(strip).len().saturating_sub(1);
        let strip_door = build(ui, strip, Some(at), STRIP, RING_STRIP);
        let (bookmarks, _) = door(
            ui,
            strip,
            glyph::BOOKMARK,
            STRIP,
            13.0,
            "Bookmarks",
            &bookmarks_tip(),
        );
        ui.tree.reorder(bookmarks, at + 1);
        // The address's words give way to the door (a long address would
        // push it out of sight), ending in an ellipsis.
        ui.tree.edit(st.column.address, |n| n.style.shrink = 1.0);
        if let Some(&words) = ui.tree.children(st.column.address).first() {
            ui.tree.edit(words, |n| n.style.shrink = 1.0);
        }
        let column_door = build(ui, st.column.address, None, COLUMN, RING_COLUMN);
        DOORS.with(|d| {
            let mut d = d.borrow_mut();
            d.downloads = vec![strip_door, column_door];
            d.bookmarks = Some(bookmarks);
        });
    });
    let weak = shell.downgrade();
    shell.downloads().watch(move || {
        if let Some(s) = weak.upgrade() {
            changed(&s);
        }
    });
}

/// One downloads door, hidden.
fn build(ui: &mut Ui, parent: NodeId, at: Option<usize>, size: f32, image: u32) -> Door {
    let node = Node::stack()
        .size(size, size)
        .shrink(0.0)
        .radius(size * 0.3)
        .hover_fill(Token::ColumnHover)
        .press_fill(Token::ColumnHover)
        .selected_fill(Token::ColumnHover)
        .align(copper_win_ui::Align::Center)
        .justify(copper_win_ui::Justify::Center)
        .hidden(true)
        .button("Downloads")
        .tooltip("Downloads");
    let d = match at {
        Some(i) => ui.tree.insert(parent, i, node),
        None => ui.tree.add(parent, node),
    };
    let ring = ui.tree.add(
        d,
        Node::image(image, size)
            .absolute(Abs::at(0.0, 0.0))
            .hidden(true)
            .role(Role::None),
    );
    let glyph = ui.tree.add(
        d,
        Node::glyph(
            glyph::ARROW_DOWN,
            if size < 26.0 { 9.5 } else { 10.0 },
            size,
        )
        .ink(Token::Muted)
        .role(Role::None),
    );
    let dot = ui.tree.add(
        d,
        Node::new()
            .size(5.0, 5.0)
            .radius(2.5)
            .fill(Token::Ink)
            .absolute(Abs {
                left: None,
                top: Some(2.0),
                right: Some(2.0),
                bottom: None,
            })
            .hidden(true)
            .role(Role::None),
    );
    Door {
        door: d,
        ring,
        glyph,
        dot,
        image,
        size,
        drawn: None,
        pulsing: false,
    }
}

/// The downloads changed: redraw the doors once, on the loop's next turn.
pub fn changed(shell: &Shell) {
    if SCHEDULED.with(|s| s.replace(true)) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        SCHEDULED.with(|s| s.set(false));
        if let Some(s) = weak.upgrade() {
            update(&s);
        }
    });
}

/// A click on one of the doors: open (or put away) what hangs from it.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let (bookmarks, download) = DOORS.with(|d| {
        let d = d.borrow();
        (
            d.bookmarks == Some(id),
            d.downloads.iter().any(|x| x.door == id),
        )
    });
    if !bookmarks && !download {
        return false;
    }
    let rect = shell
        .view()
        .chrome()
        .read(move |ui, _| ui.tree.window_rect(id));
    if bookmarks {
        super::show_from(shell, super::bookmarks::LIST, rect);
    } else {
        super::show_from(shell, super::downloads::LIST, rect);
        changed(shell);
    }
    true
}

/// What the doors should show now.
fn look(shell: &Shell) -> Look {
    let now = Instant::now();
    let m = shell.downloads().model(shell);
    let live = m.live();
    let active: Vec<_> = live
        .iter()
        .filter(|d| d.state == DownloadState::Active)
        .collect();
    let failed = live
        .iter()
        .any(|d| matches!(d.state, DownloadState::Failed { .. }));
    let finished = live
        .iter()
        .filter(|d| d.state == DownloadState::Finished)
        .count();
    let unseen = live.iter().any(|d| d.unseen);
    let fraction = if active.is_empty() || active.iter().any(|d| d.total.is_none()) {
        None
    } else {
        Some(m.total_fraction().unwrap_or(0.0) as f32)
    };
    let pulsing = DOORS.with(|d| {
        let mut d = d.borrow_mut();
        if finished > d.finished && active.is_empty() {
            d.pulse_until = Some(now + PULSE);
        }
        d.finished = finished;
        d.pulse_until.is_some_and(|t| t > now)
    });
    let tip = if !active.is_empty() {
        let n = active.len();
        let speed: f64 = active.iter().map(|d| d.speed).sum();
        format!(
            "{n} download{} · {}/s",
            if n == 1 { "" } else { "s" },
            format::bytes(speed as u64)
        )
    } else if finished > 0 {
        format!(
            "{finished} finished download{}",
            if finished == 1 { "" } else { "s" }
        )
    } else {
        "Downloads".into()
    };
    let glyph = if !active.is_empty() {
        glyph::ARROW_DOWN
    } else if pulsing {
        glyph::CHECK
    } else if failed {
        glyph::WARNING
    } else {
        // `arrow.down.circle`: the arrow in a quiet ring.
        glyph::ARROW_DOWN
    };
    let ring = match (active.is_empty(), fraction) {
        (false, Some(f)) => Ring::Fraction(f),
        (false, None) => Ring::Turning,
        (true, _) if pulsing => Ring::None,
        (true, _) if failed => Ring::Failed,
        _ => Ring::Rest,
    };
    Look {
        shown: !live.is_empty(),
        glyph,
        failed_tint: failed && active.is_empty() && !pulsing,
        pulsing,
        unseen,
        tip,
        ring,
        open: shell.view().panels().showing() == Some(super::downloads::LIST),
    }
}

/// Redraw the doors from the downloads.
pub fn update(shell: &Shell) {
    let look = look(shell);
    if look.pulsing {
        // Back to rest when the pulse is over.
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            copper_win_rt::sleep(PULSE).await;
            if let Some(s) = weak.upgrade() {
                changed(&s);
            }
        });
    }
    if look.ring == Ring::Turning {
        spin(shell);
    }
    let phase = EPOCH.with(|e| e.elapsed().as_secs_f32() / TURN).fract();
    shell.view().chrome().edit(move |ui, _| {
        DOORS.with(|d| {
            for door in &mut d.borrow_mut().downloads {
                paint(ui, door, &look, phase);
            }
        });
    });
}

/// Turn the dashes while a download of unknown size is arriving.
fn spin(shell: &Shell) {
    if SPINNING.with(|s| s.replace(true)) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        loop {
            copper_win_rt::sleep(SPIN).await;
            let Some(s) = weak.upgrade() else { break };
            let reduce = s
                .view()
                .chrome()
                .read(|ui, _| ui.tree.reduce_motion)
                .unwrap_or(false);
            if reduce || look(&s).ring != Ring::Turning {
                break;
            }
            update_ring(&s);
        }
        SPINNING.with(|s| s.set(false));
    });
}

/// Just the turning ring (the rest of the door holds still).
fn update_ring(shell: &Shell) {
    let look = look(shell);
    let phase = EPOCH.with(|e| e.elapsed().as_secs_f32() / TURN).fract();
    shell.view().chrome().edit(move |ui, _| {
        DOORS.with(|d| {
            for door in &mut d.borrow_mut().downloads {
                draw_ring(ui, door, look.ring, phase);
            }
        });
    });
}

/// One door as `look` says.
fn paint(ui: &mut Ui, door: &mut Door, look: &Look, phase: f32) {
    ui.tree.set_hidden(door.door, !look.shown);
    if !look.shown {
        return;
    }
    ui.tree.set_text(door.glyph, &look.glyph.to_string());
    let ink: Swatch = if look.failed_tint {
        Swatch::Fixed(ORANGE)
    } else {
        Token::Muted.into()
    };
    ui.tree.repaint(door.glyph, |n| n.look.ink = ink);
    ui.tree.set_hidden(door.dot, !look.unseen);
    ui.tree.set_selected(door.door, look.open);
    let tip = look.tip.clone();
    ui.tree.edit(door.door, |n| {
        n.a11y.help = tip.clone();
        n.tooltip = Some(tip);
    });
    if look.pulsing != door.pulsing {
        door.pulsing = look.pulsing;
        let to = if look.pulsing { 1.08 } else { 1.0 };
        ui.tree.animate(door.glyph, Prop::Scale, to, motion::SETTLE);
    }
    draw_ring(ui, door, look.ring, phase);
}

/// The door's ring, drawn again only when it would look different.
fn draw_ring(ui: &mut Ui, door: &mut Door, ring: Ring, phase: f32) {
    let theme = ui.theme();
    let hairline = theme.color(Token::Hairline.into());
    let ink = theme.color(Token::Ink.into());
    let muted = theme.color(Swatch::Fade(Token::Muted, 0.7));
    let layers = match ring {
        Ring::None => Vec::new(),
        Ring::Fraction(f) => vec![
            Layer {
                stroke: Stroke::Whole,
                width: 1.5,
                color: hairline,
            },
            Layer {
                stroke: Stroke::Arc(ring::step(f) as f32 / 120.0),
                width: 1.7,
                color: ink,
            },
        ],
        Ring::Turning => vec![Layer {
            stroke: Stroke::Dashed {
                dash: 2.0,
                gap: 2.0,
                phase: (phase * 60.0).round() / 60.0,
            },
            width: 1.2,
            color: muted,
        }],
        Ring::Failed => vec![Layer {
            stroke: Stroke::Whole,
            width: 1.5,
            color: ORANGE,
        }],
        Ring::Rest => vec![Layer {
            stroke: Stroke::Whole,
            width: 1.2,
            color: theme.color(Token::Muted.into()),
        }],
    };
    ui.tree.set_hidden(door.ring, layers.is_empty());
    if layers.is_empty() {
        return;
    }
    let scale = ui.scale();
    if door
        .drawn
        .as_ref()
        .is_some_and(|(l, s)| *l == layers && *s == scale)
    {
        return;
    }
    // The resting ring is the symbol's own; the others go round the door.
    let diameter = if ring == Ring::Rest {
        door.size * 0.56
    } else {
        door.size - 3.0
    };
    let p = ring::draw(door.size, diameter, scale, &layers);
    ui.set_image(door.image, p.bgra, p.w, p.h);
    door.drawn = Some((layers, scale));
}

/// The tooltip of the strip's bookmarks door, with its shortcut if any.
fn bookmarks_tip() -> String {
    tip("Bookmarks", "bookmarks-door")
}
