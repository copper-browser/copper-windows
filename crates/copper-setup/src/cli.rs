//! How copper.exe was asked to start: install, uninstall, or the browser.
//!
//! One binary is the browser, the CLI, the MCP bridge and the installer
//! (CONTRACT §5). The download is the same exe named `CopperSetup.exe`, so the
//! file name alone can mean "install me"; `--install` and `--uninstall` say it
//! outright. Everything here is platform-free so it is unit-tested on Linux.

use std::ffi::OsString;

/// The flag Copper's ProgIDs put before `%1`: everything after it is one URL
/// or path to open, never a flag — a link to `--uninstall` must not uninstall.
pub const SINGLE_ARGUMENT: &str = "--single-argument";

/// What copper.exe should do with this launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Install (or upgrade) Copper for this user.
    Install(InstallOptions),
    /// Remove Copper for this user.
    Uninstall(UninstallOptions),
    /// Anything else: the browser, the CLI or the MCP bridge.
    App,
}

/// Open Copper after a silent install too (Copper's own Restart to Update).
pub const RELAUNCH: &str = "--relaunch";
/// `--wait-pid=<pid>`: first wait for that Copper (the one updating itself)
/// to finish quitting.
pub const WAIT_PID: &str = "--wait-pid=";
/// `--update-from=<version>`: install only if that version is still the
/// one installed (nothing else changed Copper since it staged the update).
pub const UPDATE_FROM: &str = "--update-from=";

/// Choices for an install. The window offers the same two as switches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstallOptions {
    /// No window, no questions: `/S`, `/silent`, `--silent`.
    pub silent: bool,
    /// Put a shortcut on the desktop as well (off unless asked for).
    pub desktop_shortcut: bool,
    /// Open Copper when done. On for the window; `--no-launch` turns it off.
    /// Silent installs launch only with [`RELAUNCH`].
    pub launch: bool,
    /// [`WAIT_PID`]: the Copper that started this update, to let quit first.
    pub wait_pid: Option<u32>,
    /// [`UPDATE_FROM`]: the version this update replaces.
    pub update_from: Option<String>,
}

/// The arguments a running Copper (`pid`, version `from`) starts its staged
/// installer with: silent, after it has quit, only over itself, and opened
/// again when `relaunch`.
pub fn update_args(pid: u32, from: &str, relaunch: bool) -> Vec<String> {
    let mut args = vec![
        "--install".to_owned(),
        "--silent".to_owned(),
        format!("{WAIT_PID}{pid}"),
        format!("{UPDATE_FROM}{from}"),
    ];
    if relaunch {
        args.push(RELAUNCH.to_owned());
    }
    args
}

/// Choices for an uninstall.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UninstallOptions {
    /// No window, no questions: `/S`, `/silent`, `--silent`.
    pub silent: bool,
    /// Also delete `%LOCALAPPDATA%\Copper` (history, passwords, cache).
    /// Off unless the box is ticked or `--delete-data` is given.
    pub delete_data: bool,
}

/// Does this file stem (`CopperSetup`, `CopperSetup-x64`, `coppersetup (1)`)
/// mean the exe was downloaded as the installer?
pub fn is_setup_stem(stem: &str) -> bool {
    const PREFIX: &str = "coppersetup";
    stem.get(..PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(PREFIX))
}

/// The flags before [`SINGLE_ARGUMENT`], lower-cased; nothing after it.
fn flags(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .take_while(|a| a != SINGLE_ARGUMENT)
        .map(|a| a.to_ascii_lowercase())
        .collect()
}

/// Decide what to do. `exe_stem` is the running exe's file stem; `args`
/// excludes argv\[0\].
pub fn classify(exe_stem: Option<&str>, args: &[OsString]) -> Launch {
    let flags = flags(args);
    let has = |names: &[&str]| flags.iter().any(|f| names.contains(&f.as_str()));
    let value = |prefix: &str| flags.iter().find_map(|f| f.strip_prefix(prefix));
    let silent = has(&["/s", "/silent", "--silent", "-s"]);

    if has(&["--uninstall"]) {
        return Launch::Uninstall(UninstallOptions {
            silent,
            delete_data: has(&["--delete-data"]),
        });
    }
    if has(&["--install"]) || exe_stem.is_some_and(is_setup_stem) {
        return Launch::Install(InstallOptions {
            silent,
            desktop_shortcut: has(&["--desktop-shortcut"]),
            launch: has(&[RELAUNCH]) || (!silent && !has(&["--no-launch"])),
            wait_pid: value(WAIT_PID).and_then(|p| p.parse().ok()),
            update_from: value(UPDATE_FROM)
                .filter(|v| !v.is_empty())
                .map(str::to_owned),
        });
    }
    Launch::App
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn setup_stems() {
        for stem in [
            "CopperSetup",
            "coppersetup",
            "COPPERSETUP-x64",
            "CopperSetup-arm64",
            "CopperSetup (1)",
            "CopperSetup.0.1.0",
        ] {
            assert!(is_setup_stem(stem), "{stem}");
        }
        for stem in [
            "copper",
            "Copper Setup",
            "Setup",
            "MyCopperSetup",
            "Copper",
            "",
        ] {
            assert!(!is_setup_stem(stem), "{stem}");
        }
    }

    #[test]
    fn non_ascii_stem_does_not_panic() {
        assert!(!is_setup_stem("CopperSetü"));
        assert!(!is_setup_stem("ç"));
    }

    #[test]
    fn plain_copper_is_the_app() {
        assert_eq!(classify(Some("copper"), &[]), Launch::App);
        assert_eq!(
            classify(Some("copper"), &args(&["https://example.com"])),
            Launch::App
        );
        assert_eq!(classify(None, &args(&["mcp"])), Launch::App);
    }

    #[test]
    fn setup_name_installs_with_window() {
        assert_eq!(
            classify(Some("CopperSetup"), &[]),
            Launch::Install(InstallOptions {
                silent: false,
                desktop_shortcut: false,
                launch: true,
                ..InstallOptions::default()
            })
        );
    }

    #[test]
    fn an_update_waits_relaunches_and_names_what_it_replaces() {
        let restart = update_args(4242, "0.3.4", true);
        let restart: Vec<OsString> = restart.into_iter().map(OsString::from).collect();
        assert_eq!(
            classify(Some("CopperSetup-0.3.5-x64"), &restart),
            Launch::Install(InstallOptions {
                silent: true,
                desktop_shortcut: false,
                launch: true,
                wait_pid: Some(4242),
                update_from: Some("0.3.4".into()),
            })
        );
        // Installed when Copper quits: the same, but nothing opens.
        let on_quit: Vec<OsString> = update_args(7, "1.0.0-rc.1", false)
            .into_iter()
            .map(OsString::from)
            .collect();
        match classify(Some("copper"), &on_quit) {
            Launch::Install(o) => {
                assert!(o.silent && !o.launch);
                assert_eq!(
                    (o.wait_pid, o.update_from.as_deref()),
                    (Some(7), Some("1.0.0-rc.1"))
                );
            }
            other => panic!("{other:?}"),
        }
        // Malformed values are ignored, not guessed at.
        match classify(
            Some("copper"),
            &args(&["--install", "--wait-pid=x", "--update-from="]),
        ) {
            Launch::Install(o) => assert_eq!((o.wait_pid, o.update_from), (None, None)),
            other => panic!("{other:?}"),
        }
        // A plain silent install still never opens Copper.
        match classify(Some("copper"), &args(&["--install", "/S"])) {
            Launch::Install(o) => assert!(!o.launch && o.wait_pid.is_none()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_prewarm_is_a_browser_start() {
        // `copper --prewarm` (what setup starts after an install) reaches
        // the browser's own start, which warms up and leaves.
        assert_eq!(classify(Some("copper"), &args(&["--prewarm"])), Launch::App);
    }

    #[test]
    fn install_flag_and_silent_forms() {
        for silent in ["/S", "/s", "/silent", "--silent", "--SILENT"] {
            match classify(Some("copper"), &args(&["--install", silent])) {
                Launch::Install(o) => {
                    assert!(o.silent, "{silent}");
                    assert!(!o.launch, "silent installs never launch");
                }
                other => panic!("{other:?}"),
            }
        }
        match classify(
            Some("CopperSetup-x64"),
            &args(&["--desktop-shortcut", "--no-launch"]),
        ) {
            Launch::Install(o) => {
                assert!(!o.silent && o.desktop_shortcut && !o.launch);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn uninstall_wins_even_under_setup_name() {
        assert_eq!(
            classify(Some("CopperSetup"), &args(&["--uninstall"])),
            Launch::Uninstall(UninstallOptions::default())
        );
        assert_eq!(
            classify(
                Some("copper"),
                &args(&["--uninstall", "--silent", "--delete-data"])
            ),
            Launch::Uninstall(UninstallOptions {
                silent: true,
                delete_data: true
            })
        );
    }

    #[test]
    fn nothing_after_single_argument_is_a_flag() {
        assert_eq!(
            classify(Some("copper"), &args(&[SINGLE_ARGUMENT, "--uninstall"])),
            Launch::App
        );
        assert_eq!(
            classify(Some("copper"), &args(&[SINGLE_ARGUMENT, "--install"])),
            Launch::App
        );
    }
}
