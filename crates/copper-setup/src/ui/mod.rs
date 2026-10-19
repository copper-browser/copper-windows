//! Setup's own small window, drawn in Copper's design. Self-contained: it
//! shares nothing with the browser's toolkit (`copper-win-ui`), so setup
//! works before — and without — the rest of Copper.

mod card;
mod gfx;
pub mod installer;
mod theme;
pub mod uninstaller;
mod window;

pub use window::CLASS;

use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

/// Per-monitor DPI v2, in case the exe's manifest didn't already say so.
fn dpi_aware() {
    // SAFETY: process-wide setting; fails harmlessly if already set.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Run the install window. `Err` if it couldn't be shown at all.
pub fn install(options: crate::cli::InstallOptions) -> anyhow::Result<i32> {
    use window::Controller as _;
    dpi_aware();
    let (card, controller) = installer::Installer::new(options);
    let done = window::run("Copper Setup", card, controller)?;
    Ok(done.exit_code())
}

/// Run the uninstall window: the exit code and what is left to do at exit.
pub fn uninstall(
    options: crate::cli::UninstallOptions,
) -> anyhow::Result<(i32, Option<crate::uninstall::Pending>)> {
    use window::Controller as _;
    dpi_aware();
    let (card, controller) = uninstaller::Uninstaller::new(options);
    let mut done = window::run("Uninstall Copper", card, controller)?;
    Ok((done.exit_code(), done.pending.take()))
}
