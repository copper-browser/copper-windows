//! The forms lane's Windows half (`FormsUi`): the save offer after a
//! sign-in, asked in the window's question bar (Save / Not now), and the
//! account list under a sign-in box, a menu at the box's foot. Watching
//! pages, filling and the vault are copper-shell-core's `forms` lane,
//! started here with `forms::vault::always_there` (Copper's own vault,
//! nothing to unlock).

use crate::chrome::Chrome;
use crate::platform::{Shell, Win};
use copper_core::browser::TabId;
use copper_creds::Source;
use copper_pagejs::messages::FieldRect;
use copper_shell_core::Question;
use copper_shell_core::forms::view::{OfferData, OfferView, PickerView, Row};
use copper_shell_core::forms::{self, Forms, FormsUi, rt, source_title, vault};
use copper_shell_core::shell::pages::PageSlot;
use copper_win_ui::{Menu, Point};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// The account list's menu tag.
pub const PICKER_MENU: u32 = 20;

/// The forms lane's widgets on Windows.
pub enum WinForms {}

/// Start the lane.
pub fn register(shell: &Shell) {
    forms::start::<WinForms>(shell, vault::always_there);
}

/// A save offer on show: a question in the bar.
pub struct Offer {
    chrome: Weak<Chrome>,
    ticket: Rc<Cell<u64>>,
}

impl OfferView for Offer {
    fn remove(&self) {
        let Some(c) = self.chrome.upgrade() else {
            return;
        };
        let ticket = self.ticket.clone();
        c.edit(move |ui, st| st.sheet.withdraw(ui, ticket.get()));
    }
}

/// The account list on show: a menu under the box.
pub struct Picker {
    chrome: Weak<Chrome>,
    users: Vec<String>,
    busy: Rc<Cell<bool>>,
}

impl PickerView for Picker {
    fn remove(&self) {
        let Some(c) = self.chrome.upgrade() else {
            return;
        };
        PICKED.with(|p| p.borrow_mut().take());
        c.edit(|ui, _| ui.close_menu());
    }

    /// The menu stays where it opened: it closes on the next click or key,
    /// before a scroll could leave it behind.
    fn place(&self, _rect: FieldRect, _zoom: f64) {}

    fn busy(&self) -> bool {
        self.busy.get()
    }

    fn users(&self) -> Vec<String> {
        self.users.clone()
    }
}

/// The list on show: its tab and rows, for [`picked`].
struct Shown {
    tab: TabId,
    rows: Vec<Row>,
    busy: Rc<Cell<bool>>,
}

thread_local! {
    static PICKED: RefCell<Option<Shown>> = const { RefCell::new(None) };
}

/// A row of the account list was chosen: fill it.
pub fn picked(item: u32) {
    let Some(shown) = PICKED.with(|p| p.borrow_mut().take()) else {
        return;
    };
    let Some(row) = shown.rows.get(item as usize).cloned() else {
        return;
    };
    if let Some(f) = forms::get::<WinForms>() {
        shown.busy.set(true);
        f.pick(&shown.tab, row);
    }
}

impl FormsUi for WinForms {
    type Platform = Win;
    type Offer = Offer;
    type Picker = Picker;

    fn show_offer(
        shell: &Shell,
        forms: &Rc<Forms<Self>>,
        tab: &TabId,
        _slot: &Rc<PageSlot<Win>>,
        data: OfferData,
    ) -> Offer {
        let host = data.sent.host.clone();
        let question = Question {
            title: if data.changed {
                format!("Update the password for {host}?")
            } else {
                format!("Save the password for {host}?")
            },
            detail: Some(data.sent.user.clone()).filter(|u| !u.is_empty()),
            deny: "Not now".into(),
            allow: if data.changed { "Update" } else { "Save" }.into(),
        };
        let weak = Rc::downgrade(forms);
        let shell_weak = shell.downgrade();
        let tab = tab.clone();
        let answer = Box::new(move |yes: bool| {
            let Some(f) = weak.upgrade() else { return };
            f.drop_offer(&tab);
            if yes && let Some(s) = shell_weak.upgrade() {
                save(&f, &s, data);
            }
        });
        let ticket = Rc::new(Cell::new(0));
        let chrome = shell.view().chrome().clone();
        let t = ticket.clone();
        chrome.edit(move |ui, st| t.set(st.sheet.ask(ui, question, answer)));
        shell.view().overlay_changed();
        Offer {
            chrome: Rc::downgrade(&chrome),
            ticket,
        }
    }

    fn show_picker(
        shell: &Shell,
        _forms: &Rc<Forms<Self>>,
        tab: &TabId,
        _slot: &Rc<PageSlot<Win>>,
        rect: FieldRect,
        rows: Vec<Row>,
    ) -> Picker {
        let users: Vec<String> = rows.iter().map(|r| r.credential.user.clone()).collect();
        let mut menu = Menu::new();
        for (i, r) in rows.iter().enumerate() {
            let label = match r.kind {
                forms::view::RowKind::Login if r.credential.user.is_empty() => {
                    r.credential.host.clone()
                }
                forms::view::RowKind::Login => r.credential.user.clone(),
                forms::view::RowKind::Code => format!("Code for {}", r.credential.user),
            };
            menu = menu.item(i as u32, &label.replace('&', "&&"));
        }
        let busy = Rc::new(Cell::new(false));
        PICKED.with(|p| {
            *p.borrow_mut() = Some(Shown {
                tab: tab.clone(),
                rows,
                busy: busy.clone(),
            })
        });
        let zoom = shell
            .active_page()
            .map(|p| copper_engine::PageView::zoom(&p))
            .unwrap_or(1.0) as f32;
        let chrome = shell.view().chrome().clone();
        let tab = tab.clone();
        chrome.edit(move |ui, st| {
            let Some(slot) = st.stage.slot_of(&tab) else {
                return;
            };
            let Some(frame) = ui.tree.get(slot).map(|n| n.frame) else {
                return;
            };
            let at = Point::new(
                frame.x + rect.x as f32 * zoom,
                frame.y + (rect.y + rect.h) as f32 * zoom + 4.0,
            );
            ui.show_menu(PICKER_MENU, &menu, at);
        });
        Picker {
            chrome: Rc::downgrade(&chrome),
            users,
            busy,
        }
    }
}

/// Keep the password the page sent, and say so.
fn save(f: &Rc<Forms<WinForms>>, shell: &Shell, data: OfferData) {
    let v = f.vault.clone();
    let shell = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let OfferData {
            sent,
            target,
            changed,
        } = data;
        let host = sent.host.clone();
        let r =
            rt::run(async move { v.save(target, &sent.host, &sent.user, &sent.password).await })
                .await
                .unwrap_or_else(|| Err("Copper couldn't reach the vault".into()));
        let vault = target != Source::Keychain;
        let name = source_title(target);
        let said = match r {
            Ok(()) if changed && vault => format!("Password updated in {name} for {host}"),
            Ok(()) if changed => format!("Password updated for {host}"),
            Ok(()) if vault => format!("Password saved to {name}"),
            Ok(()) => format!("Password saved for {host}"),
            Err(e) => {
                tracing::warn!("saving a password: {e}");
                e
            }
        };
        if let Some(s) = shell.upgrade() {
            s.toast(&said);
        }
    });
}
