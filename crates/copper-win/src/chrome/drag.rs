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
//! [`DRAG_FROM`]: super::column::DRAG_FROM

use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_shell_core::agents::groups;
use copper_shell_core::sidedrop::{self, Target};
use copper_win_ui::Point;
use std::cell::RefCell;

thread_local! {
    /// A press on a row: where it went down (window DIPs, y) and its tab.
    static PRESS: RefCell<Option<(f32, TabId)>> = const { RefCell::new(None) };
    /// The row in the hand and where it would land.
    static HELD: RefCell<Option<(TabId, Option<Target>)>> = const { RefCell::new(None) };
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
                        let k = ui.scale().max(0.5);
                        let p = Point::new(x / k, y / k);
                        st.column.row_at(ui, p).map(|id| (p.y, id))
                    })
                    .flatten();
                PRESS.with(|s| *s.borrow_mut() = press);
            }
            WM_MOUSEMOVE if m.wparam & MK_LBUTTON != 0 => {
                let Some((y0, id)) = PRESS.with(|s| s.borrow().clone()) else {
                    return false;
                };
                // What `stays` needs: the space's row and the folders.
                let (row, group, filed) = {
                    let b = shell.browser();
                    let row: Vec<(TabId, bool, Option<String>)> = b
                        .row(b.current_space_id())
                        .iter()
                        .map(|t| (t.id.clone(), t.saved, t.group.clone()))
                        .collect();
                    let group = b.tab(&id).and_then(|t| t.group.clone());
                    (row, group, groups::all(&shell))
                };
                chrome.read(|ui, st| {
                    let k = ui.scale().max(0.5);
                    let y = y / k;
                    let held = HELD.with(|h| h.borrow().is_some());
                    if !held {
                        if (y - y0).abs() < super::column::DRAG_FROM {
                            return;
                        }
                        st.column.lift(ui, &id);
                    }
                    let target = match sidedrop::target(&st.column.drop_layout(ui), y) {
                        Target::Spot(s) => {
                            let order: Vec<(TabId, bool)> =
                                row.iter().map(|(t, s, _)| (t.clone(), *s)).collect();
                            let in_folder = |t: &TabId, folder: &str| {
                                let g = row.iter().find(|r| r.0 == *t).and_then(|r| r.2.as_deref());
                                groups::in_folder(&filed, g, folder)
                            };
                            let stays =
                                sidedrop::stays(&s, &id, group.as_deref(), &order, in_folder);
                            (!stays).then_some(Target::Spot(s))
                        }
                        pins => Some(pins),
                    };
                    st.column.show_drop(ui, &id, target.as_ref());
                    HELD.with(|h| *h.borrow_mut() = Some((id.clone(), target)));
                });
            }
            WM_MOUSEMOVE => {
                // The button came up where this hook did not see it.
                PRESS.with(|s| s.borrow_mut().take());
                chrome.read(end);
                HELD.with(|h| h.borrow_mut().take());
            }
            _ => {
                PRESS.with(|s| s.borrow_mut().take());
                chrome.read(end);
                let Some((id, target)) = HELD.with(|h| h.borrow_mut().take()) else {
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

/// Put the row in the hand back down, and what showed where it would go.
fn end(ui: &mut copper_win_ui::win::Ui, st: &mut super::State) {
    if let Some((id, _)) = HELD.with(|h| h.borrow().clone()) {
        st.column.end_drag(ui, &id);
    }
}
