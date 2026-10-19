//! Canvas on Windows: started after the cloud lane (a canvas syncs through
//! it), with the New Canvas command (`Ctrl+Shift+E`) and the personal
//! canvas in the palette. The `copper://canvas` page, its bridge, the list
//! and the `canvas_*` tools are copper-shell-core's `canvas`.

use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::{Shell, Win};
use copper_shell_core::canvas;

/// Turn canvas on and add its commands.
pub fn register(shell: &Shell) {
    canvas::start::<Win>(shell);
    let r = shell.view().commands();
    r.add(Command::new(
        "new-canvas",
        "New Canvas",
        glyph::CANVAS,
        |s| canvas::new_canvas::<Win>(s, None),
    ));
    r.add(Command::new(
        "personal-canvas",
        "Personal Canvas",
        glyph::CANVAS,
        |s| {
            canvas::show::<Win>(s, copper_core::canvas::PERSONAL_CANVAS_ID, true);
        },
    ));
}
