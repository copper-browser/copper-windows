//! `copper://cloud/join?code=…` (and `copper:cloud/…`, `copper:///cloud/…`):
//! Copper Cloud's own links, from the command line of a fresh start or
//! handed to the running Copper. They never load as a page: Settings ›
//! Cloud opens with the code in its Connect field, read and classified —
//! a link code, a pairing code or a join link. A join link starts its
//! sign-in in the browser at once (Mac `CloudJoin`); anything else waits
//! for a button there, and no instance is replaced without one. A link
//! for another instance than the linked one waits on the page as a notice.
//!
//! `copper://canvas/join/…` is the canvas's, never this.

use super::page;
use crate::platform::Shell;
use copper_shell_core::cloud::join::{JoinAction, handle};

/// Take `address` if it is a `copper://cloud/…` link (`true`); anything
/// else is left for the caller to open. A join link (`#sso=`) starts its
/// sign-in in the browser at once (Mac `CloudJoin`); any other code only
/// fills the field ([`handle`]).
pub fn open_link(shell: &Shell, address: &str) -> bool {
    let action = handle(address, super::work::opener());
    match &action {
        JoinAction::NotJoin => return false,
        JoinAction::Fill(offer)
        | JoinAction::StartSso {
            offer: Some(offer), ..
        } => page::prefill(&offer.text),
        _ => {}
    }
    if let Some(m) = action.message() {
        shell.toast(&m);
    }
    if action.opens_settings() {
        crate::settings::open(shell, page::ID);
    }
    true
}
