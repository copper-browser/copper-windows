//! The forms lane's Windows half (`FormsUi`). Two things float over a page:
//!
//! - after a sign-in, the save offer ([`offer`]): a capsule rising from
//!   the foot of the window with Save, Not now and (for a new password)
//!   Never here;
//! - under a sign-in or code box, the account list ([`picker`]): it hangs
//!   from the box's left edge, follows it as the page scrolls or zooms,
//!   and fills the row clicked.
//!
//! Watching pages, filling and the vault are copper-shell-core's `forms`
//! lane, started here with `forms::vault::always_there` (Credential
//! Manager is always there, nothing to unlock). What both say, and where
//! the list goes, is in `settings::fill`, where host tests check it.
//!
//! Also here: the Settings keys the Passwords page and the lane share,
//! Mac defaults the lane doesn't know of (stay unlocked), the chosen
//! 1Password vault put back at launch, and Bitwarden's auto-lock.

mod offer;
mod picker;

use crate::platform::{Shell, Win};
use copper_core::browser::TabId;
use copper_pagejs::messages::FieldRect;
use copper_shell_core::forms::view::{OfferData, Row};
use copper_shell_core::forms::{self, Forms, FormsUi, keys, rt, vault};
use copper_shell_core::shell::pages::PageSlot;
use copper_win_ui::NodeId;
use std::rc::Rc;
use std::time::Duration;

pub use offer::Offer;
pub use picker::Picker;

/// The account list's menu tag. The list is drawn by Copper now, not a
/// system menu; the tag stays so the chrome's menu answer
/// ([`picked`]) keeps meaning a row.
pub const PICKER_MENU: u32 = 20;
/// Settings key: minutes without use before Copper locks Bitwarden — 0
/// (never, the default), 5, 15 or 60 (`Bitwarden.autolockMinutes`).
pub const BW_AUTOLOCK: &str = "bitwarden.autolockMinutes";
/// Settings key: the 1Password vault new logins are created in, by id
/// (unset: Private, Personal or Employee, else the first writable one).
pub const OP_SAVE_VAULT: &str = "onepassword.saveVault";
/// How often Bitwarden's idle time is looked at.
const IDLE_CHECK: Duration = Duration::from_secs(30);

/// The forms lane's widgets on Windows.
pub enum WinForms {}

/// Start the lane.
pub fn register(shell: &Shell) {
    // Stay unlocked is on until it is turned off (`Bitwarden.swift`
    // `stayUnlocked`, 1Password's the same); the lane reads a missing key
    // as off, so it is written once here before the lane starts.
    let missing: Vec<&str> = [keys::BW_STAY, keys::OP_STAY]
        .into_iter()
        .filter(|k| shell.settings().get(k).is_none())
        .collect();
    if !missing.is_empty() {
        shell.update_settings(|s| {
            for k in missing {
                s.set(k, true);
            }
        });
    }
    forms::start::<WinForms>(shell, vault::always_there);
    restore_save_vault(shell);
    autolock(shell);
}

/// The 1Password vault chosen in Settings, for new logins from now on.
fn restore_save_vault(shell: &Shell) {
    let Some(id) = shell.settings().string(OP_SAVE_VAULT).map(str::to_string) else {
        return;
    };
    let Some(v) = forms::vault() else { return };
    copper_win_rt::spawn_local(async move {
        rt::run(async move { v.op.lock().await.set_save_vault(Some(id)) }).await;
    });
}

/// Lock Bitwarden after the minutes chosen in Settings without use
/// (`Bitwarden.autolockMinutes`; 0: never).
fn autolock(shell: &Shell) {
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        loop {
            copper_win_rt::sleep(IDLE_CHECK).await;
            let Some(s) = weak.upgrade() else { return };
            let minutes = s.settings().number(BW_AUTOLOCK).unwrap_or(0.0);
            drop(s);
            if minutes <= 0.0 {
                continue;
            }
            let Some(v) = forms::vault() else { continue };
            rt::run(async move {
                // Never wait behind a sign-in or a sync under way.
                if let Ok(mut bw) = v.bw.try_lock() {
                    bw.check_idle(minutes).await;
                }
            })
            .await;
        }
    });
}

/// A click in the chrome: an offer's button or a row of the list. `true`
/// if it was one of theirs.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    offer::click(shell, id) || picker::click(shell, id)
}

/// A row of the account list was chosen, by its place in the list.
pub fn picked(item: u32) {
    picker::pick_at(item as usize);
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
        offer::show(shell, forms, tab, data)
    }

    fn show_picker(
        shell: &Shell,
        _forms: &Rc<Forms<Self>>,
        tab: &TabId,
        slot: &Rc<PageSlot<Win>>,
        rect: FieldRect,
        rows: Vec<Row>,
    ) -> Picker {
        let zoom = copper_engine::PageView::zoom(&slot.page);
        picker::show(shell, tab, rect, zoom, rows)
    }
}
