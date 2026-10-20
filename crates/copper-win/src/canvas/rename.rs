//! Rename a canvas in its tab's row (`CanvasSidebar` renaming): a field
//! over the row's title; Return or a click elsewhere keeps the name,
//! Escape leaves it. When the row isn't on show (the column folded, the
//! tabs across the top, a pinned row), the Canvas card asks instead.

use super::state::Mode;
use super::{door, ops};
use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_shell_core::canvas::registry;
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Event, Insets, Node, NodeId, Role, Token};
use std::cell::RefCell;

struct Open {
    canvas: String,
    scrim: NodeId,
    field: NodeId,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

pub fn is_open() -> bool {
    OPEN.with(|o| o.try_borrow().map(|o| o.is_some()).unwrap_or(false))
}

/// The row of tab `tab` in the column, if it is on show.
fn row_of(ui: &Ui, st: &crate::chrome::State, tab: &TabId) -> Option<NodeId> {
    let mut stack = vec![st.column.side];
    while let Some(n) = stack.pop() {
        if st.column.tab_at(ui, n).as_ref() == Some(tab) {
            return ui.tree.is_shown(n).then_some(n);
        }
        stack.extend(ui.tree.children(n).iter().rev().copied());
    }
    None
}

/// Rename canvas `canvas`, shown in tab `tab`: in the row, or in the card.
pub fn start(shell: &Shell, tab: &TabId, canvas: &str) {
    close(shell);
    let Some(entry) = registry::entry(canvas) else {
        return;
    };
    let placed = shell
        .view()
        .chrome()
        .read(|ui, st| {
            row_of(ui, st, tab)
                .map(|r| ui.tree.window_rect(r))
                .filter(|r| r.w > 80.0)
        })
        .flatten();
    let Some(rect) = placed else {
        door::open(shell, Mode::Rename(canvas.to_string()));
        return;
    };
    door::close(shell);
    let name = entry.name.clone();
    let id = canvas.to_string();
    shell.view().chrome().edit(move |ui, st| {
        let scrim = ui.tree.add(
            st.ids.plates,
            Node::stack()
                .absolute(Abs::FILL)
                .layer()
                .hit()
                .modal()
                .role(Role::None),
        );
        let lead = 34.0_f32.min(rect.w / 4.0);
        let boxed = ui.tree.add(
            scrim,
            Node::row()
                .absolute(Abs {
                    left: Some(rect.x + lead),
                    top: Some(rect.y + (rect.h - 26.0).max(0.0) / 2.0),
                    right: None,
                    bottom: None,
                })
                .width((rect.w - lead - 6.0).clamp(80.0, 220.0))
                .height(26.0)
                .padding(Insets::hv(7.0, 0.0))
                .radius(6.0)
                .fill(Token::Ground)
                .stroke(Token::Accent, 1.5)
                .align(Align::Center)
                .layer()
                .hit(),
        );
        let field = ui.tree.add(
            boxed,
            Node::field("Canvas name", 12.5)
                .fill_height()
                .grow(1.0)
                .name("Canvas name"),
        );
        ui.set_field_text(field, &name, true);
        ui.focus(Some(field));
        OPEN.with(|o| {
            *o.borrow_mut() = Some(Open {
                canvas: id,
                scrim,
                field,
            })
        });
    });
    shell.view().overlay_changed();
}

/// Away with the field (nothing kept).
pub fn close(shell: &Shell) -> bool {
    let Some(open) = OPEN.with(|o| o.borrow_mut().take()) else {
        return false;
    };
    let scrim = open.scrim;
    shell
        .view()
        .chrome()
        .edit(move |ui, _| ui.tree.remove(scrim));
    shell.view().overlay_changed();
    shell.view().focus_page();
    true
}

/// Keep the name typed.
fn commit(shell: &Shell) {
    let Some((canvas, field)) =
        OPEN.with(|o| o.borrow().as_ref().map(|o| (o.canvas.clone(), o.field)))
    else {
        return;
    };
    let text = shell
        .view()
        .chrome()
        .read(move |ui, _| ui.field_text(field).map(String::from))
        .flatten()
        .unwrap_or_default();
    close(shell);
    if super::copy::submittable(&text) {
        ops::rename(shell, &canvas, &text, false);
    }
}

/// A click: outside the field keeps the name. `true` if it was ours.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let Some((scrim, field)) = OPEN.with(|o| o.borrow().as_ref().map(|o| (o.scrim, o.field)))
    else {
        return false;
    };
    if id == field {
        return true;
    }
    let inside = shell
        .view()
        .chrome()
        .read(move |ui, _| ui.tree.is_within(id, scrim))
        .unwrap_or(false);
    if inside {
        commit(shell);
    }
    inside
}

/// The field's events. `true` if it was ours.
pub fn field_event(shell: &Shell, id: NodeId, e: &Event) -> bool {
    let mine = OPEN.with(|o| o.borrow().as_ref().is_some_and(|o| o.field == id));
    if !mine {
        return false;
    }
    match e {
        Event::FieldSubmit(_) => commit(shell),
        Event::FieldCancel(_) => {
            close(shell);
        }
        _ => {}
    }
    true
}
