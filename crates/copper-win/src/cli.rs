//! The front ends that never open a window, dispatched before the browser:
//!
//! * `copper cloud …` — pair with and inspect a Copper Cloud
//!   (`copper_mcp::cloud_cli`);
//! * `copper --mcp-stdio` — the MCP stdio bridge (`Bridge.swift`'s), over
//!   the handles the client gave us: no console is attached, so its pipes
//!   stay the bridge's stdin and stdout;
//! * `copper <verb> …`, `copper --cli <verb> …` — the command-line client
//!   (`copper tabs`, `copper health`…);
//! * `copper --version`, `copper --help`.
//!
//! copper.exe is a GUI-subsystem program (no console flashes up behind the
//! browser), so a front end that prints to a terminal first attaches to its
//! parent's console ([`attach_console`]). Output redirected to a file or a
//! pipe is left alone.

use copper_shell_core::agents::cli as agents_cli;

/// Run a non-browser invocation and return its exit code; `None` for a
/// browser launch.
pub fn dispatch(args: &[String]) -> Option<i32> {
    if copper_mcp::cloud_cli::is_cloud(args) {
        attach_console();
        return Some(copper_mcp::cloud_cli::run(args, &open_in_browser));
    }
    if args.iter().any(|a| a == "--mcp-stdio") {
        // Headless over the inherited handles: no console, no window, no
        // single-instance hand-off. A browser it starts is detached
        // (`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`), and holds
        // none of the client's pipes.
        keep_pipes_to_ourselves();
        return agents_cli::dispatch(args);
    }
    if args.iter().any(|a| a == "--cli") || agents_cli::is_cli(args) {
        attach_console();
        return agents_cli::dispatch(args);
    }
    if let Some(text) = info(args) {
        attach_console();
        println!("{text}");
        return Some(0);
    }
    None
}

/// What `--version` or `--help` print, when asked (and nothing else is).
pub fn info(args: &[String]) -> Option<String> {
    let first = args.first()?;
    match first.as_str() {
        "--version" | "-V" => Some(format!("copper {}", env!("CARGO_PKG_VERSION"))),
        "--help" | "-h" | "/?" => Some(usage()),
        _ => None,
    }
}

/// The browser's usage, with the Windows front ends.
pub fn usage() -> String {
    format!(
        "{}\n\
         also:\n  \
           cloud …                pair with a Copper Cloud (copper cloud help)\n  \
           --mcp-stdio            the MCP bridge over stdin/stdout\n  \
           <verb> …               the command-line client (copper help)\n  \
           --single-argument URL  open URL (how Windows hands links over)",
        copper_shell_core::options::USAGE.trim_end()
    )
}

/// Print to the console Copper was started from, if any: attach to the
/// parent's console when stdout or stderr has no handle of its own. A
/// redirect (`copper tabs > out.txt`, a pipe) keeps its handle.
#[cfg(windows)]
pub fn attach_console() {
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };
    // SAFETY: plain queries of this process's standard handles.
    let missing =
        |which| unsafe { GetStdHandle(which) }.map_or(true, |h| h.is_invalid() || h.0.is_null());
    if missing(STD_OUTPUT_HANDLE) || missing(STD_ERROR_HANDLE) {
        // SAFETY: attaching to the parent's console; failure (no console:
        // started from Explorer) leaves output going nowhere, as before.
        let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(not(windows))]
pub fn attach_console() {}

/// The client's pipes are ours alone: a Copper the bridge starts must not
/// inherit them. (`CreateProcess` hands every inheritable handle to the
/// child even when its own stdio is null, so a browser started from the
/// bridge would keep the client's stdout open after the bridge exits, and
/// a client reading to the end would wait for the browser to quit.)
#[cfg(windows)]
fn keep_pipes_to_ourselves() {
    use windows::Win32::Foundation::{HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation};
    use windows::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: this process's own standard handles; only the inherit
        // flag changes.
        unsafe {
            if let Ok(h) = GetStdHandle(which)
                && !h.is_invalid()
                && !h.0.is_null()
            {
                let _ = SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
            }
        }
    }
}

#[cfg(not(windows))]
fn keep_pipes_to_ourselves() {}

/// Open `url` in the default browser (`copper cloud sso`'s sign-in page).
#[cfg(windows)]
pub fn open_in_browser(url: &str) -> Result<(), String> {
    crate::sys::shell::open(url)
}

#[cfg(not(windows))]
pub fn open_in_browser(url: &str) -> Result<(), String> {
    Err(format!("open {url} in a browser"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn version_and_help() {
        assert!(info(&v(&["--version"])).unwrap().starts_with("copper "));
        assert!(info(&v(&["--help"])).unwrap().contains("--mcp-stdio"));
        assert_eq!(info(&v(&["example.com"])), None);
        assert_eq!(info(&[]), None);
    }

    #[test]
    fn addresses_are_the_browsers() {
        assert_eq!(dispatch(&v(&["example.com"])), None);
        assert_eq!(dispatch(&v(&["--screenshot", "x.png"])), None);
        assert_eq!(dispatch(&[]), None);
    }
}
