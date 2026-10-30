//! Dragging a row down the column (Mac: `Fork/SavedDrop.swift`). The
//! toolkit has no drag events, so the window's mouse messages are read
//! before it sees them, as the space chips' are: a press on a row that
//! travels [`DRAG_FROM`] picks it up and the row dims. While it is held, a
//! line shows where it will land, read off the lines on screen by
//! [`sidedrop`]: between two rows, inside a folder or out of it, and
//! nothing over its own place. Carried up over the favourites, a ghost
//! square shows where it will be pinned. Letting go puts it there. Nothing
//! is swallowed, so the toolkit's own press and capture stay whole.
//!
//! A favourite, or a tab in the top row, moves as it is carried (Mac
//! `Side.swift` `pinReorder`, `TabBar.swift` `reorder`): over another of
//! its kind it takes that one's place at once ([`Shell::drop_tab`]), a
//! favourite among favourites, a title among titles.
//!
//! [`DRAG_FROM`]: super::column::DRAG_FROM

use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_shell_core::agents::groups;
use copper_shell_core::sidedrop::{self, Target};
use copper_win_ui::Point;
use std::cell::RefCell;

/// What a press went down on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Grip {
    /// A row down the column.
    Row,
    /// A favourite, or a tab in the top row.
    Square,
}

/// A square in the hand, and the tab it was last moved past (so a move
/// is asked once, not again until the squares are redrawn).
struct Moving {
    id: TabId,
    last: Option<TabId>,
}

thread_local! {
    /// A press: where it went down (window DIPs), its tab, on what.
    static PRESS: RefCell<Option<(Point, TabId, Grip)>> = const { RefCell::new(None) };
    /// The row in the hand.
    static HELD: RefCell<Option<Held>> = const { RefCell::new(None) };
    /// The square in the hand.
    static MOVING: RefCell<Option<Moving>> = const { RefCell::new(None) };
    /// The tab whose row was dragged: the click the toolkit makes of the
    /// release is not a click.
    static DRAGGED: RefCell<Option<TabId>> = const { RefCell::new(None) };
}

/// Whether the click on `id`'s row is the end of its drag.
pub fn was_dragged(id: &TabId) -> bool {
    DRAGGED.with(|d| d.borrow_mut().take_if(|d| d == id).is_some())
}

/// Follow the window's mouse (see the module docs).
pub fn pointer(shell: &Shell) {
    const WM_MOUSEMOVE: u32 = 0x0200;
    const WM_LBUTTONDOWN: u32 = 0x0201;
    const WM_LBUTTONUP: u32 = 0x0202;
    const MK_LBUTTON: usize = 0x0001;
    let weak = shell.downgrade();
    let hwnd = shell.view().chrome().window().hwnd();
    copper_win_rt::add_message_hook(move |m| {
        if m.hwnd != hwnd.0 as isize
            || !matches!(m.message, WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP)
        {
            return false;
        }
        // A plain move with nothing pressed is most of them: no look at
        // the window.
        if m.message != WM_LBUTTONDOWN && PRESS.with(|p| p.borrow().is_none()) {
            return false;
        }
        let Some(shell) = weak.upgrade() else {
            return false;
        };
        let chrome = shell.view().chrome().clone();
        let words = m.lparam as usize;
        let (x, y) = (
            f32::from((words & 0xFFFF) as u16 as i16),
            f32::from(((words >> 16) & 0xFFFF) as u16 as i16),
        );
        match m.message {
            WM_LBUTTONDOWN => {
                DRAGGED.with(|d| d.borrow_mut().take());
                let press = chrome
                    .read(|ui, st| {
                        // A row still up from a release never seen.
                        end(ui, st);
                        HELD.with(|h| h.borrow_mut().take());
                        MOVING.with(|m| m.borrow_mut().take());
                        let k = ui.scale().max(0.5);
                        let p = Point::new(x / k, y / k);
                        st.column
                            .row_at(ui, p)
                            .map(|id| (p, id, Grip::Row))
                            .or_else(|| square_at(ui, st, p).map(|(id, _)| (p, id, Grip::Square)))
                    })
                    .flatten();
                PRESS.with(|s| *s.borrow_mut() = press);
            }
            WM_MOUSEMOVE if m.wparam & MK_LBUTTON != 0 => {
                let Some((p0, id, grip)) = PRESS.with(|s| s.borrow().clone()) else {
                    return false;
                };
                let Some(p) = chrome.read(|ui, _| {
                    let k = ui.scale().max(0.5);
                    Point::new(x / k, y / k)
                }) else {
                    return false;
                };
                if grip == Grip::Square {
                    move_square(&shell, p0, p, id);
                    return false;
                }
                let (y0, y) = (p0.y, p.y);
                if HELD.with(|h| h.borrow().is_none()) {
                    if (y - y0).abs() < super::column::DRAG_FROM {
                        return false;
                    }
                    let held = Held::of(&shell, &id);
                    chrome.read(|ui, st| st.column.lift(ui, &id));
                    HELD.with(|h| *h.borrow_mut() = Some(held));
                }
                HELD.with(|h| {
                    let mut h = h.borrow_mut();
                    let Some(held) = h.as_mut() else { return };
                    chrome.read(|ui, st| {
                        held.target = held.target(&st.column.drop_layout(ui), y);
                        st.column.show_drop(ui, &held.id, held.target.as_ref());
                    });
                });
            }
            WM_MOUSEMOVE => {
                // The button came up where this hook did not see it.
                PRESS.with(|s| s.borrow_mut().take());
                MOVING.with(|m| m.borrow_mut().take());
                chrome.read(end);
                HELD.with(|h| h.borrow_mut().take());
            }
            _ => {
                PRESS.with(|s| s.borrow_mut().take());
                if let Some(m) = MOVING.with(|m| m.borrow_mut().take()) {
                    DRAGGED.with(|d| *d.borrow_mut() = Some(m.id));
                    return false;
                }
                chrome.read(end);
                let Some(Held { id, target, .. }) = HELD.with(|h| h.borrow_mut().take()) else {
                    return false;
                };
                DRAGGED.with(|d| *d.borrow_mut() = Some(id.clone()));
                // On the loop's next turn: moving a tab reaches the pages,
                // which may run messages where this hook does not see them.
                let weak = shell.downgrade();
                copper_win_rt::spawn_local(async move {
                    let Some(shell) = weak.upgrade() else {
                        return;
                    };
                    match target {
                        Some(Target::Pins) => {
                            shell.pin(&id);
                        }
                        Some(Target::Spot(s)) => shell.drop_at_spot(&id, &s),
                        None => {}
                    }
                });
            }
        }
        false
    });
}

/// The favourite, or top-row tab, under `p` and whether it is pinned.
fn square_at(ui: &copper_win_ui::win::Ui, st: &super::State, p: Point) -> Option<(TabId, bool)> {
    st.column
        .pin_under(ui, p)
        .map(|id| (id, true))
        .or_else(|| st.strip.tab_under(ui, p))
}

/// The pointer, pressed on square `id` at `from`, is at `p`: past
/// [`DRAG_FROM`] it is carried, and over another square of its kind it
/// moves to that one's place.
///
/// [`DRAG_FROM`]: super::column::DRAG_FROM
fn move_square(shell: &Shell, from: Point, p: Point, id: TabId) {
    if MOVING.with(|m| m.borrow().is_none()) {
        if (p.x - from.x).hypot(p.y - from.y) < super::column::DRAG_FROM {
            return;
        }
        MOVING.with(|m| *m.borrow_mut() = Some(Moving { id, last: None }));
    }
    let Some((id, last)) = MOVING.with(|m| {
        let m = m.borrow();
        m.as_ref().map(|m| (m.id.clone(), m.last.clone()))
    }) else {
        return;
    };
    let pinned = shell.browser().tab(&id).is_some_and(|t| t.is_pinned());
    let over = shell
        .view()
        .chrome()
        .read(|ui, st| square_at(ui, st, p))
        .flatten();
    let target = match over {
        // Over itself, where the last move put it: the next may go on.
        Some((t, _)) if t == id => {
            MOVING.with(|m| m.borrow_mut().as_mut().map(|m| m.last = None));
            return;
        }
        Some((t, kind)) if kind == pinned && last.as_ref() != Some(&t) => t,
        _ => return,
    };
    MOVING.with(|m| {
        m.borrow_mut()
            .as_mut()
            .map(|m| m.last = Some(target.clone()))
    });
    // On the loop's next turn, as a row's drop.
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let Some(shell) = weak.upgrade() else { return };
        let after = {
            let b = shell.browser();
            let order: Vec<TabId> = if pinned {
                b.pins().iter().map(|t| t.id.clone()).collect()
            } else {
                b.row(b.current_space_id())
                    .iter()
                    .map(|t| t.id.clone())
                    .collect()
            };
            let at = |t: &TabId| order.iter().position(|o| o == t);
            at(&id) < at(&target)
        };
        shell.drop_tab(&id, &target, after);
    });
}

/// The row in the hand: what [`sidedrop::stays`] needs, read once when it
/// is picked up, and where it would land.
struct Held {
    id: TabId,
    /// Its folder.
    group: Option<String>,
    /// The space's row: each tab, whether it is saved, its folder.
    row: Vec<(TabId, bool, Option<String>)>,
    folders: Vec<copper_core::groups::TabGroup>,
    target: Option<Target>,
}

impl Held {
    fn of(shell: &Shell, id: &TabId) -> Held {
        let b = shell.browser();
        Held {
            id: id.clone(),
            group: b.tab(id).and_then(|t| t.group.clone()),
            row: b
                .row(b.current_space_id())
                .iter()
                .map(|t| (t.id.clone(), t.saved, t.group.clone()))
                .collect(),
            folders: groups::all(shell),
            target: None,
        }
    }

    /// Where it would land with the pointer at `y`; `None` over its own
    /// place.
    fn target(&self, layout: &sidedrop::Layout, y: f32) -> Option<Target> {
        let Target::Spot(spot) = sidedrop::target(layout, y) else {
            return Some(Target::Pins);
        };
        let order: Vec<(TabId, bool)> = self.row.iter().map(|(t, s, _)| (t.clone(), *s)).collect();
        let in_folder = |t: &TabId, folder: &str| {
            let g = self
                .row
                .iter()
                .find(|r| r.0 == *t)
                .and_then(|r| r.2.as_deref());
            groups::in_folder(&self.folders, g, folder)
        };
        let stays = sidedrop::stays(&spot, &self.id, self.group.as_deref(), &order, in_folder);
        (!stays).then_some(Target::Spot(spot))
    }
}

/// Put the row in the hand back down, and what showed where it would go.
fn end(ui: &mut copper_win_ui::win::Ui, st: &mut super::State) {
    if let Some(id) = HELD.with(|h| h.borrow().as_ref().map(|h| h.id.clone())) {
        st.column.end_drag(ui, &id);
    }
}
