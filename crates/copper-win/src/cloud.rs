//! Copper Cloud on Windows: copper-shell-core's `cloud` lane (pairing,
//! sync of tabs, canvases and settings), its Settings page and the
//! palette's way there. `copper cloud …` on the command line is
//! [`crate::cli`]'s.

use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::{Shell, Win};

/// Start the lane (nothing more when this build has no cloud).
pub fn register(shell: &Shell) {
    if copper_shell_core::cloud::start::<Win>(shell).is_none() {
        return;
    }
    shell.view().commands().add(Command::new(
        "copper-cloud",
        "Copper Cloud…",
        glyph::CLOUD,
        |s| crate::settings::open(s, "cloud"),
    ));
}
