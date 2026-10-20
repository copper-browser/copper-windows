//! Copper Cloud on Windows: copper-shell-core's `cloud` lane (pairing,
//! sync of tabs, canvases and settings), its Settings page, the palette's
//! ways there, and `copper://cloud/join` links. `copper cloud …` on the
//! command line is [`crate::cli`]'s.
//!
//! The page is built in two halves. The pure half — [`model`] (the words
//! and rules), [`state`] (what the cloud says and what the page holds),
//! [`el`] and [`view`] (the page as data) — builds and tests anywhere. The
//! Windows half draws it ([`mount`]), carries out what its buttons say
//! ([`page`], [`work`]) and hands `copper://cloud/join` to it ([`join`]).
//! The protocol itself — codes, pinning, pairing, single sign-on, sync — is
//! copper-cloud's and copper-shell-core's; nothing here re-implements it.

pub mod el;
pub mod model;
pub mod mount;
pub mod state;
pub mod view;

#[cfg(windows)]
mod join;
#[cfg(windows)]
pub mod page;
#[cfg(windows)]
mod work;

#[cfg(windows)]
pub use join::open_link;

#[cfg(windows)]
use crate::chrome::glyph;
#[cfg(windows)]
use crate::commands::Command;
#[cfg(windows)]
use crate::platform::{Shell, Win};

/// Start the lane and its commands (nothing more when this run has no
/// cloud: `cloud.json` couldn't be read).
#[cfg(windows)]
pub fn register(shell: &Shell) {
    let Some(cloud) = copper_shell_core::cloud::start::<Win>(shell) else {
        return;
    };
    let commands = shell.view().commands();
    commands.add(Command::new(
        "copper-cloud",
        "Copper Cloud…",
        glyph::CLOUD,
        |s| crate::settings::open(s, "cloud"),
    ));
    commands.add(
        Command::new(
            "pair-device",
            "Pair another device with Copper Cloud",
            glyph::CLOUD,
            page::pair_from_palette,
        )
        .when(move |_| cloud.client.is_signed_in()),
    );
    page::watch(shell);
}
