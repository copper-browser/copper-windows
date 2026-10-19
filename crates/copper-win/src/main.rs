//! copper.exe: the browser, the CLI, the MCP bridge and its own installer
//! (CONTRACT §5).
//!
//! What a start is for comes from the exe's name and arguments, before
//! anything else happens:
//!
//! - named `CopperSetup*.exe` (the download), or `--install`: install or
//!   upgrade for this user, then open Copper;
//! - `--uninstall` (what Settings › Apps runs): remove Copper;
//! - anything else: the browser.

// No console window flashing up behind the browser or setup.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        use copper_setup::Launch;
        match copper_setup::launch() {
            Launch::Install(_) => std::process::exit(copper_setup::run_installer()),
            Launch::Uninstall(_) => std::process::exit(copper_setup::run_uninstaller()),
            Launch::App => {}
        }
    }
    // Launch::App: the browser, CLI and MCP bridge front ends are wired in
    // here by their own waves. Until then a plain start exits cleanly (0).
}
