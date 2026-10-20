//! copper.exe: the browser, the CLI, the MCP bridge and its own installer
//! (CONTRACT §5).
//!
//! What a start is for comes from the exe's name and arguments, before
//! anything else happens:
//!
//! - named `CopperSetup*.exe` (the download), or `--install`: install or
//!   upgrade for this user, then open Copper;
//! - `--uninstall` (what Settings › Apps runs): remove Copper;
//! - `copper cloud …`: pair with and inspect a Copper Cloud, no window;
//! - `copper --mcp-stdio`: the MCP stdio bridge over the inherited handles,
//!   no window;
//! - `copper <verb> …` / `copper --cli <verb> …`: the command-line client;
//! - anything else: the browser (`docs/APP.md`).

// No console window flashing up behind the browser or setup. The CLI front
// ends attach to their parent's console instead (`cli::attach_console`).
#![cfg_attr(windows, windows_subsystem = "windows")]
// Elsewhere only the platform-free logic is built, for its tests.
#![cfg_attr(not(windows), allow(dead_code))]

mod addons;
mod args;
mod cli;
mod keys;
mod perf;

#[cfg(windows)]
mod actions;
#[cfg(windows)]
mod agents;
#[cfg(windows)]
mod app;
#[cfg(windows)]
mod canvas;
#[cfg(windows)]
mod chrome;
mod cloud;
#[cfg(windows)]
mod commands;
#[cfg(windows)]
mod features;
#[cfg(windows)]
mod find;
#[cfg(windows)]
mod forms;
#[cfg(windows)]
mod frame;
#[cfg(windows)]
mod instance;
mod palette;
mod panels;
mod picture;
#[cfg(windows)]
mod platform;
mod settings;
#[cfg(windows)]
mod slot;
#[cfg(windows)]
mod sys;
#[cfg(windows)]
mod updates;
#[cfg(windows)]
mod view;

fn main() {
    perf::mark_process_start();
    #[cfg(windows)]
    {
        use copper_setup::Launch;
        match copper_setup::launch() {
            Launch::Install(_) => std::process::exit(copper_setup::run_installer()),
            Launch::Uninstall(_) => std::process::exit(copper_setup::run_uninstaller()),
            Launch::App => {}
        }
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::dispatch(&args) {
        std::process::exit(code);
    }
    #[cfg(windows)]
    std::process::exit(app::run(args));
    #[cfg(not(windows))]
    {
        eprintln!("copper: the browser runs on Windows only");
        std::process::exit(1);
    }
}
