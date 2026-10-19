//! Installing (and upgrading) Copper for the current user, with no window:
//! the steps both the setup window and silent mode run.
//!
//! Per-user, no admin: copper.exe goes to `%LOCALAPPDATA%\Programs\Copper`,
//! the shortcut to the user's Start menu, and the registration to HKCU.
//! WebView2 comes first, so a machine that can't get it is left untouched.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::cli::InstallOptions;
use anyhow::Context;

use crate::files::{self, EXE_NAME, STAGED_NAME, aside_name, is_leftover};
use crate::log::log;
use crate::plan::{Plan, Step};
use crate::registration::{self, APP_NAME, APP_USER_MODEL_ID, DESCRIPTION, Install};
use crate::sys::{process, reg, shell};
use crate::webview2;

/// How long a running Copper gets to close before setup ends it.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Why an install stopped.
#[derive(Debug)]
pub enum Failure {
    /// The user closed the window mid-way (before anything was changed).
    Cancelled,
    /// WebView2 couldn't be had: offline, blocked, refused.
    WebView2(anyhow::Error),
    /// Copying or registering failed.
    Install(anyhow::Error),
}

impl Failure {
    /// One sentence for the window.
    pub fn sentence(&self) -> String {
        match self {
            Failure::Cancelled => "Setup was cancelled.".to_owned(),
            Failure::WebView2(_) => {
                "Copper needs Microsoft Edge WebView2, and it couldn't be downloaded. Check your connection and try again."
                    .to_owned()
            }
            Failure::Install(e) => files::failure_sentence(e).to_owned(),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Cancelled => write!(f, "cancelled"),
            Failure::WebView2(e) => write!(f, "WebView2: {e:#}"),
            Failure::Install(e) => write!(f, "install: {e:#}"),
        }
    }
}

/// `%LOCALAPPDATA%\Programs\Copper`.
pub fn install_dir() -> PathBuf {
    let base = shell::local_app_data()
        .ok()
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local"));
    base.join("Programs").join(APP_NAME)
}

/// `%LOCALAPPDATA%\Programs\Copper\copper.exe`.
pub fn installed_exe() -> PathBuf {
    install_dir().join(EXE_NAME)
}

/// Is Copper installed for this user?
pub fn is_installed() -> bool {
    installed_exe().is_file()
}

/// The version Settings › Apps lists, if Copper is registered.
pub fn installed_version() -> Option<String> {
    reg::read_str(
        reg::HKEY_CURRENT_USER,
        registration::UNINSTALL_KEY,
        Some("DisplayVersion"),
    )
}

/// The Start menu shortcut.
pub fn start_menu_shortcut() -> anyhow::Result<PathBuf> {
    Ok(shell::start_menu_programs()?.join(format!("{APP_NAME}.lnk")))
}

/// The desktop shortcut (if the user asked for one).
pub fn desktop_shortcut() -> anyhow::Result<PathBuf> {
    Ok(shell::desktop()?.join(format!("{APP_NAME}.lnk")))
}

/// Delete what earlier upgrades left in the install folder: moved-aside
/// exes that were still running then, half-written copies. Anything still
/// locked stays for next time. Cheap; the browser may call it at start-up.
pub fn cleanup_stale() {
    let Ok(entries) = std::fs::read_dir(install_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if is_leftover(&name.to_string_lossy()) && std::fs::remove_file(entry.path()).is_ok() {
            log!("removed leftover {}", entry.path().display());
        }
    }
}

/// Paths equal the way NTFS compares them (case-insensitively), after
/// resolving `..` and links where possible.
fn same_file(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| {
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .to_lowercase()
    };
    canon(a) == canon(b)
}

/// Put `src` at `dir\copper.exe`, replacing a running one if need be.
fn place_exe(src: &Path, dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("couldn't create {}", dir.display()))?;
    let target = dir.join(EXE_NAME);
    if target.exists() && same_file(src, &target) {
        log!("already running from {}; nothing to copy", target.display());
        return Ok(target);
    }
    let staged = dir.join(STAGED_NAME);
    std::fs::copy(src, &staged)
        .with_context(|| format!("couldn't copy Copper into {}", dir.display()))?;

    // The easy case: nothing there, or nothing holding it.
    if std::fs::rename(&staged, &target).is_ok() {
        return Ok(target);
    }
    // Still running (a straggler, the CLI, an MCP bridge): a running exe can
    // be renamed, just not replaced. Move it aside and take its place.
    let aside = (0..1000)
        .map(|n| dir.join(aside_name(n)))
        .find(|p| !p.exists())
        .ok_or_else(|| anyhow::anyhow!("too many old copies of Copper in {}", dir.display()))?;
    if let Err(e) = std::fs::rename(&target, &aside) {
        let _ = std::fs::remove_file(&staged);
        return Err(e).context("couldn't move the old Copper aside");
    }
    if let Err(e) = std::fs::rename(&staged, &target) {
        // Put the old one back rather than leave no Copper at all.
        let _ = std::fs::rename(&aside, &target);
        let _ = std::fs::remove_file(&staged);
        return Err(e).context("couldn't put the new Copper in place");
    }
    log!("moved running exe aside to {}", aside.display());
    Ok(target)
}

/// Today as `YYYYMMDD`, local time.
fn today() -> String {
    // SAFETY: plain query.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}{:02}{:02}", t.wYear, t.wMonth, t.wDay)
}

/// What the registry should say about the copy at `exe`.
fn describe(exe: &Path) -> Install {
    let dir = exe.parent().unwrap_or(Path::new("."));
    let size_kib = std::fs::metadata(exe)
        .map(|m| (m.len().div_ceil(1024)).min(u64::from(u32::MAX)) as u32)
        .unwrap_or(0);
    Install {
        dir: dir.to_string_lossy().into_owned(),
        exe: exe.to_string_lossy().into_owned(),
        size_kib,
        date: today(),
    }
}

/// Register the installed copy: Settings › Apps, App Paths, Default apps.
pub fn register() -> anyhow::Result<()> {
    let exe = installed_exe();
    if !exe.is_file() {
        anyhow::bail!("Copper isn't installed at {}", exe.display());
    }
    reg::apply(&registration::writes(&describe(&exe)))?;
    shell::notify_associations_changed();
    Ok(())
}

/// Remove everything [`register`] wrote.
pub fn unregister() -> anyhow::Result<()> {
    let r = reg::remove(&registration::removals());
    shell::notify_associations_changed();
    r
}

/// Make the shortcut(s).
fn make_shortcuts(exe: &Path, desktop: bool) -> anyhow::Result<()> {
    let dir = exe.parent().unwrap_or(Path::new("."));
    let link = shell::Shortcut {
        target: exe,
        working_dir: dir,
        description: DESCRIPTION,
        app_user_model_id: APP_USER_MODEL_ID,
    };
    shell::create_shortcut(&start_menu_shortcut()?, &link)?;
    if desktop {
        shell::create_shortcut(&desktop_shortcut()?, &link)?;
    }
    Ok(())
}

/// Is WebView2 going to be needed? (Decides the shape of the bar.)
pub fn plan() -> Plan {
    Plan {
        webview2: !webview2::is_installed(),
    }
}

/// Install Copper. `report` hears each step; `cancel` is honoured up to the
/// point where Copper's files start changing.
pub fn run(
    options: &InstallOptions,
    plan: Plan,
    cancel: &AtomicBool,
    mut report: impl FnMut(Step),
) -> Result<PathBuf, Failure> {
    log!(
        "install {} options {options:?} plan {plan:?}",
        registration::VERSION
    );
    // Shortcuts are COM objects; this may run on a worker thread.
    let _com = shell::Com::init();
    cleanup_stale();
    let cancelled = || cancel.load(Ordering::Relaxed);

    if plan.webview2 {
        webview2::install(cancel, |p| {
            report(match p {
                webview2::Progress::Downloading { done, total } => {
                    Step::DownloadWebView2 { done, total }
                }
                webview2::Progress::Installing { elapsed } => Step::InstallWebView2 {
                    seconds: elapsed.as_secs_f32(),
                },
            })
        })
        .map_err(|e| {
            if cancelled() {
                Failure::Cancelled
            } else {
                log!("WebView2 failed: {e:#}");
                Failure::WebView2(e)
            }
        })?;
    }
    if cancelled() {
        return Err(Failure::Cancelled);
    }

    let src = std::env::current_exe().map_err(|e| Failure::Install(e.into()))?;
    let dir = install_dir();
    let fail = |e: anyhow::Error| {
        log!("install failed: {e:#}");
        Failure::Install(e)
    };

    if !same_file(&src, &dir.join(EXE_NAME)) {
        process::close_running_copper(&dir, CLOSE_TIMEOUT, || report(Step::CloseCopper));
    }

    report(Step::CopyFiles);
    let exe = place_exe(&src, &dir).map_err(fail)?;

    report(Step::Shortcuts);
    make_shortcuts(&exe, options.desktop_shortcut).map_err(fail)?;

    report(Step::Register);
    register().map_err(fail)?;

    report(Step::Done);
    log!("installed {}", exe.display());
    Ok(exe)
}
