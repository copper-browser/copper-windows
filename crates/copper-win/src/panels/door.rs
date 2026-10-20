//! What the doors panels hang from show, and what a click on one opens:
//! the downloads door (`DownloadsDoor`) at the end of the tab strip and
//! inside the column's address, and the bookmarks door at the strip's far
//! end (`TabBar.swift` 107-122; the column's foot has its own). The chrome
//! builds them with the strip and the column (`chrome/doors.rs`); this
//! finds them there and draws into them.
//!
//! The downloads door is there only once something has been downloaded
//! this session. It says what is happening at a glance: a ring filling
//! while files arrive (dashes turning when a size is unknown), a check
//! that pulses as the last one lands, a warning in orange when one
//! failed, a dot until the list has been looked at.

use super::ring::{self, Layer, Stroke};
use crate::chrome::doors::DownloadsDoor;
use crate::chrome::glyph;
use crate::platform::Shell;
use copper_core::downloads::DownloadState;
use copper_shell_core::format;
use copper_win_ui::tokens::motion;
use copper_win_ui::win::Ui;
use copper_win_ui::{Color, NodeId, Prop, Swatch, Token};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

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

/// One downloads door: the chrome's nodes and what was last drawn in them.
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

impl From<DownloadsDoor> for Door {
    fn from(d: DownloadsDoor) -> Door {
        Door {
            door: d.door,
            ring: d.ring,
            glyph: d.glyph,
            dot: d.dot,
            image: d.image,
            size: d.size,
            drawn: None,
            pulsing: false,
        }
    }
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

/// Find the chrome's doors and follow the downloads.
pub fn mount(shell: &Shell) {
    shell.view().chrome().edit(|_, st| {
        DOORS.with(|d| {
            let mut d = d.borrow_mut();
            d.downloads = vec![st.strip.downloads.into(), st.column.downloads.into()];
            d.bookmarks = Some(st.strip.bookmarks);
        });
    });
    let weak = shell.downgrade();
    shell.downloads().watch(move || {
        if let Some(s) = weak.upgrade() {
            changed(&s);
        }
    });
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
