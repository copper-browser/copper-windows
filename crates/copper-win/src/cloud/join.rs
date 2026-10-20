//! `copper://cloud/join?code=…` (and `copper:cloud/…`, `copper:///cloud/…`):
//! Copper Cloud's own links, from the command line of a fresh start or
//! handed to the running Copper. They never load as a page: Settings ›
//! Cloud opens with the code in its Connect field, read and classified —
//! a link code, a pairing code or a join link — and nothing is sent, and
//! no instance replaced, until a button there is pressed. A link for
//! another instance than the linked one waits on the page as a notice.
//!
//! `copper://canvas/join/…` is the canvas's, never this.

use super::page;
use crate::platform::Shell;
use copper_cloud::{deep_link_code, parse_join_code};
use copper_shell_core::cloud::join::{JoinAction, decide, is_cloud_link};
use copper_shell_core::cloud::sso::{SsoTarget, sign_in};

/// Take `address` if it is a `copper://cloud/…` link (`true`); anything
/// else is left for the caller to open.
pub fn open_link(shell: &Shell, address: &str) -> bool {
    if !is_cloud_link(address) {
        return false;
    }
    let Some(c) = copper_shell_core::cloud::get() else {
        if let Some(m) = JoinAction::Off.message() {
            shell.toast(&m);
        }
        return true;
    };
    let linked = c.client.link();
    let email = c
        .client
        .is_signed_in()
        .then(|| c.client.account().map(|a| a.email))
        .flatten();
    let action = decide(address, linked.as_ref(), email.as_deref());
    tracing::info!(target: "copper::cloud", kind = action.kind(), "copper://cloud link");
    match &action {
        JoinAction::Fill(offer) => page::prefill(&offer.text),
        // Unlinked: the field takes the join link; its button starts the
        // sign-in. Linked there already: the account step offers it.
        JoinAction::StartSso {
            target: SsoTarget::Join(_),
            ..
        } => {
            if let Some(code) = deep_link_code(address) {
                page::prefill(&code);
            }
        }
        JoinAction::OtherInstance { .. } => {
            if let Some(text) = deep_link_code(address)
                && let Some(code) = parse_join_code(&text, linked.as_ref())
            {
                sign_in().offer(&text, code);
            }
        }
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
