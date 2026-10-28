//! The pointer and the card (Mac: `PinSquare.settlePeek`): resting half a
//! second on a Calendar pin opens the card beside it, and it goes a
//! quarter second after the pointer has left both the pin and the card
//! (`calendar::settle`). Each change of the pointer target calls off what
//! was waiting to happen.

use super::Act;
use crate::platform::Shell;
use crate::sys::calendar::WinCalendar;
use copper_core::browser::TabId;
use copper_shell_core::OpenTab;
use copper_shell_core::calendar::{self, Card as Model};
use copper_win_ui::NodeId;
use std::cell::RefCell;

#[derive(Default)]
struct Pointer {
    /// The Calendar pin under the pointer.
    pin: Option<TabId>,
    /// The pointer is on the card.
    card: bool,
    /// The pin the card is beside, while it is up.
    open: Option<TabId>,
    /// Bumped by every change: an older timer does nothing.
    generation: u64,
}

thread_local! {
    static POINTER: RefCell<Pointer> = RefCell::default();
}

/// The pointer target under the pointer changed (`Event::Hover`).
pub fn hover(shell: &Shell, target: Option<NodeId>) {
    let chrome = shell.view().chrome().clone();
    let Some((pin, card)) = chrome.read(|ui, st| {
        let Some(t) = target else {
            return (None, false);
        };
        (st.column.pin_at(t), st.peek.contains(&ui.tree, t))
    }) else {
        return;
    };
    let pin = pin.filter(|id| shell.tab_view(id).is_some_and(|t| calendar::peeks(&t.url)));
    let (open, generation) = POINTER.with_borrow_mut(|p| {
        p.pin = pin;
        p.card = card;
        p.generation += 1;
        (
            calendar::settle(p.pin.as_ref(), p.card, p.open.as_ref()),
            p.generation,
        )
    });
    let Some(open) = open else { return };
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(if open {
            calendar::REST
        } else {
            calendar::LEAVE
        })
        .await;
        let Some(shell) = weak.upgrade() else { return };
        if POINTER.with_borrow(|p| p.generation) != generation {
            return;
        }
        if open {
            show(&shell);
        } else {
            hide(&shell);
        }
    });
}

/// A click in the card; `true` when it was the card's.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let Some(Some(act)) = shell.view().chrome().read(|_, st| st.peek.act(id)) else {
        return false;
    };
    hide(shell);
    match act {
        Act::Join(link) => {
            shell.open(
                &link,
                OpenTab {
                    foreground: true,
                    ..Default::default()
                },
            );
        }
        Act::Settings => crate::settings::open(shell, "tabs"),
    }
    true
}

fn show(shell: &Shell) {
    let Some(tab) = POINTER.with_borrow(|p| p.pin.clone()) else {
        return;
    };
    POINTER.with_borrow_mut(|p| p.open = Some(tab.clone()));
    let beside = tab.clone();
    let model = calendar::open(shell, move |s, model| {
        // What landed, if the card is still up beside the same pin.
        if POINTER.with_borrow(|p| p.open.as_ref() == Some(&beside)) {
            draw(s, &beside, model);
        }
    });
    draw(shell, &tab, model);
}

fn draw(shell: &Shell, tab: &TabId, model: Model) {
    let now = copper_core::date::now_unix() as i64;
    let tab = tab.clone();
    shell.view().chrome().edit(move |ui, st| {
        let Some(pin) = st.column.pin_rect(ui, &tab) else {
            return;
        };
        st.peek.draw::<WinCalendar>(&mut ui.tree, &model, now);
        st.peek.place(&mut ui.tree, pin);
    });
}

fn hide(shell: &Shell) {
    POINTER.with_borrow_mut(|p| {
        p.open = None;
        p.card = false;
    });
    shell
        .view()
        .chrome()
        .edit(|ui, st| st.peek.hide(&mut ui.tree));
}
