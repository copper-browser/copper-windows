//! Copper for Windows' installer, uninstaller and shell registration.
//!
//! There is no separate setup program: copper.exe is the browser, the CLI,
//! the MCP bridge and the installer (CONTRACT §5). The download is the same
//! exe named `CopperSetup.exe`; `copper-win`'s `main` asks [`launch`] what
//! this start is for and calls [`run_installer`] or [`run_uninstaller`].
//!
//! Installing is per-user and never asks for admin: copper.exe goes to
//! `%LOCALAPPDATA%\Programs\Copper`, a shortcut to the Start menu, and the
//! registration (Settings › Apps, Default apps, App Paths) to HKCU. If the
//! Microsoft Edge WebView2 Runtime is missing it is fetched first. Upgrades
//! replace a running Copper in place. `/S` (or `--silent`) does it all with
//! no window.
//!
//! The platform-free parts — argument handling, the registry layout, file
//! names, the progress plan, the springs — build and are tested on any OS;
//! everything that touches Windows is `cfg(windows)`.

pub mod cli;
pub mod files;
#[cfg(windows)]
mod log;
pub mod motion;
pub mod plan;
pub mod registration;
pub mod webview2;

#[cfg(windows)]
mod install;
#[cfg(windows)]
mod sys;
#[cfg(windows)]
mod ui;
#[cfg(windows)]
mod uninstall;

pub use cli::{InstallOptions, Launch, UninstallOptions, classify, is_setup_stem};
#[cfg(windows)]
pub use install::{
    cleanup_stale, install_dir, installed_exe, installed_version, is_installed, register,
    unregister,
};
#[cfg(windows)]
pub use sys::process::COPPER_WINDOW_CLASS;

/// Process exit codes for setup runs.
pub mod exit {
    /// Done.
    pub const SUCCESS: i32 = 0;
    /// Something failed (details in `%TEMP%\CopperSetup.log`).
    pub const FAILED: i32 = 1;
    /// The user closed setup before it finished.
    pub const CANCELLED: i32 = 2;
}

/// What this start of copper.exe is for, from its file name and arguments.
pub fn launch() -> Launch {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let stem = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()));
    classify(stem.as_deref(), &args)
}

/// The options from the command line, or the defaults for an install the
/// caller decided on itself.
#[cfg(windows)]
fn install_options() -> InstallOptions {
    match launch() {
        Launch::Install(o) => o,
        _ => InstallOptions {
            launch: true,
            ..InstallOptions::default()
        },
    }
}

#[cfg(windows)]
fn uninstall_options() -> UninstallOptions {
    match launch() {
        Launch::Uninstall(o) => o,
        _ => UninstallOptions::default(),
    }
}

/// Install (or upgrade) Copper for this user and, unless told otherwise,
/// open it. Shows the setup window unless `/S`/`--silent`. Returns the
/// process exit code ([`exit`]).
#[cfg(windows)]
pub fn run_installer() -> i32 {
    let options = install_options();
    let Some(_lock) = single::acquire(options.silent) else {
        return exit::SUCCESS;
    };
    let _com = sys::shell::Com::init();
    if options.silent {
        return silent_install(&options);
    }
    match ui::install(options.clone()) {
        Ok(code) => code,
        Err(e) => {
            // No window (no graphics at all): fall back to a plain dialog.
            log::line(&format!("setup window unavailable: {e:#}"));
            fallback::install(&options)
        }
    }
}

#[cfg(windows)]
fn silent_install(options: &InstallOptions) -> i32 {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    match install::run(options, install::plan(), &cancel, |_| {}) {
        Ok(exe) => {
            if options.launch
                && let Err(e) = sys::process::launch(&exe)
            {
                log::line(&format!("couldn't open Copper: {e:#}"));
            }
            exit::SUCCESS
        }
        Err(e) => {
            log::line(&format!("silent install failed: {e}"));
            exit::FAILED
        }
    }
}

/// Remove Copper for this user. Shows the confirm window unless
/// `/S`/`--silent`; browsing data goes only if ticked (or
/// `--delete-data`). Returns the process exit code ([`exit`]). Call it last:
/// when it is the installed exe, it is deleted shortly after this process
/// exits.
#[cfg(windows)]
pub fn run_uninstaller() -> i32 {
    let options = uninstall_options();
    let Some(_lock) = single::acquire(options.silent) else {
        return exit::SUCCESS;
    };
    let _com = sys::shell::Com::init();
    if options.silent {
        let (pending, result) = uninstall::run(&options, |_| {});
        uninstall::finish(pending);
        return match result {
            Ok(()) => exit::SUCCESS,
            Err(e) => {
                log::line(&format!("silent uninstall failed: {e:#}"));
                exit::FAILED
            }
        };
    }
    match ui::uninstall(options.clone()) {
        Ok((code, pending)) => {
            if let Some(p) = pending {
                uninstall::finish(p);
            }
            code
        }
        Err(e) => {
            log::line(&format!("uninstall window unavailable: {e:#}"));
            fallback::uninstall(&options)
        }
    }
}

/// Open Settings › Apps › Default apps on Copper's page. Windows 10 and 11
/// don't let an app make itself the default browser; this is the sanctioned
/// way to ask the user to.
#[cfg(windows)]
pub fn open_default_apps_settings() -> anyhow::Result<()> {
    sys::shell::open(registration::DEFAULT_APPS_URI)
}

/// One setup at a time per session: a second double-click brings the first
/// window forward instead of racing it.
#[cfg(windows)]
mod single {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    use windows_core::{HSTRING, PCWSTR, w};

    pub struct Lock(HANDLE);

    impl Drop for Lock {
        fn drop(&mut self) {
            // SAFETY: a mutex we own and hold.
            unsafe {
                let _ = ReleaseMutex(self.0);
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Take the setup lock. Silent runs wait for it (two silent installs in
    /// a row from a script); windowed runs focus the other window instead.
    pub fn acquire(silent: bool) -> Option<Lock> {
        // SAFETY: a named mutex in this session's namespace.
        let handle = unsafe { CreateMutexW(None, false, w!("Local\\CopperSetup")) }.ok()?;
        let wait_ms = if silent { 5 * 60 * 1000 } else { 0 };
        // SAFETY: valid handle.
        let r = unsafe { WaitForSingleObject(handle, wait_ms) };
        if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
            return Some(Lock(handle));
        }
        // SAFETY: as above.
        unsafe {
            let _ = CloseHandle(handle);
            if let Ok(other) = FindWindowW(&HSTRING::from(super::ui::CLASS), PCWSTR::null()) {
                let _ = ShowWindow(other, SW_RESTORE);
                let _ = SetForegroundWindow(other);
            }
        }
        crate::log::line("another setup is running");
        None
    }
}

/// Plain dialogs for the (unlikely) machine where the setup window can't
/// draw at all: no Direct3D, not even WARP.
#[cfg(windows)]
mod fallback {
    use windows::Win32::UI::WindowsAndMessaging::{
        IDOK, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_OKCANCEL, MessageBoxW,
    };
    use windows_core::HSTRING;

    use crate::{InstallOptions, UninstallOptions, exit};

    fn ask(text: &str, title: &str) -> bool {
        // SAFETY: modal message box with valid strings.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(text),
                &HSTRING::from(title),
                MB_OKCANCEL | MB_ICONQUESTION,
            ) == IDOK
        }
    }

    fn tell(text: &str, title: &str, ok: bool) {
        let icon = if ok { MB_ICONINFORMATION } else { MB_ICONERROR };
        // SAFETY: as above.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(text),
                &HSTRING::from(title),
                MB_OK | icon,
            );
        }
    }

    pub fn install(options: &InstallOptions) -> i32 {
        if !ask(
            "Install Copper, a small, fast, quiet browser?",
            "Copper Setup",
        ) {
            return exit::CANCELLED;
        }
        let code = super::silent_install(options);
        if code != exit::SUCCESS {
            tell(
                &format!(
                    "Copper wasn't installed. Details are in {}.",
                    crate::log::path().display()
                ),
                "Copper Setup",
                false,
            );
        }
        code
    }

    pub fn uninstall(options: &UninstallOptions) -> i32 {
        if !ask(
            "Uninstall Copper? Your browsing data is kept.",
            "Uninstall Copper",
        ) {
            return exit::CANCELLED;
        }
        let (pending, result) = crate::uninstall::run(options, |_| {});
        crate::uninstall::finish(pending);
        match result {
            Ok(()) => {
                tell("Copper is uninstalled.", "Uninstall Copper", true);
                exit::SUCCESS
            }
            Err(e) => {
                tell(
                    &format!("Copper wasn't fully removed: {e:#}"),
                    "Uninstall Copper",
                    false,
                );
                exit::FAILED
            }
        }
    }
}
