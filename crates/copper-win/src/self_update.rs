//! Copper for Windows updating itself (Mac: `Fork/Updates.swift`'s stage,
//! upgrade and settle). The shared check (`updates`) says a newer Copper is
//! out; when this is the installed copy (`%LOCALAPPDATA%\Programs\Copper`):
//!
//! 1. Everything that can fail happens before anyone is asked: the feed's
//!    installer for this CPU is downloaded to `%LOCALAPPDATA%\Copper\Updates`
//!    and must have the feed's size and SHA-256 and — when this Copper is
//!    Authenticode-signed — the same publisher's signature. Only then is it
//!    staged: "Copper X is ready".
//! 2. Restart to Update hashes the staged file again (held so it can't
//!    change), writes down what is pending, starts it as
//!    `--install --silent` (after this Copper has quit, only over this
//!    version, then open Copper again: `copper_setup::cli::update_args`) and
//!    quits. Quitting with an update staged installs it the same way,
//!    opening nothing.
//! 3. The next start turns the pending note into the outcome Settings ›
//!    About shows: "Updated to X", or why not.
//!
//! A portable or developer copy keeps the release page. The rules are
//! platform-free and tested on Linux; the rest is `cfg(windows)`.

use serde_json::Value;
use std::io::Read;

/// This build's CPU, as the feed's `assets` name it.
pub const ARCH: &str = if cfg!(target_arch = "aarch64") {
    "arm64"
} else {
    "x64"
};

/// No installer is anywhere near this big; a feed saying so is wrong.
const MAX_SIZE: u64 = 1 << 30;

/// The feed's installer for one CPU (`packaging/version-manifest.py`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub size: u64,
    /// Lower-case hex.
    pub sha256: String,
    pub url: String,
}

/// The installer for `arch` in the feed's `manifest`. Its address must be
/// `https` (plain `http` only when `allow_http`: a test feed).
pub fn pick(manifest: &Value, arch: &str, allow_http: bool) -> Result<Asset, String> {
    let asset = manifest
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|a| a["kind"] == "installer" && a["arch"] == arch)
        .ok_or_else(|| format!("The release has no installer for {arch}"))?;
    let sha256 = asset["sha256"].as_str().unwrap_or("").to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("The release doesn’t say the installer’s SHA-256".into());
    }
    let size = asset["size"]
        .as_u64()
        .filter(|s| (1..=MAX_SIZE).contains(s))
        .ok_or("The release doesn’t say how big the installer is")?;
    let url = asset["url"].as_str().unwrap_or("");
    match url::Url::parse(url).map(|u| u.scheme().to_owned()) {
        Ok(s) if s == "https" || (allow_http && s == "http") => {}
        _ => return Err(format!("Won’t download the installer from “{url}”")),
    }
    Ok(Asset {
        size,
        sha256,
        url: url.to_owned(),
    })
}

/// The staged installer's file name, or `None` for a version that is not
/// a plain version (it becomes part of a path).
pub fn staged_name(version: &str) -> Option<String> {
    let plain = !version.is_empty()
        && version.len() <= 64
        && version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b));
    plain.then(|| format!("CopperSetup-{version}-{ARCH}.exe"))
}

/// The downloaded bytes are the feed's: as many, with the same SHA-256.
pub fn verify(asset: &Asset, size: u64, sha256: &str) -> Result<(), String> {
    if size != asset.size {
        return Err(format!(
            "The download is {size} bytes, not the {} the release says",
            asset.size
        ));
    }
    if !sha256.eq_ignore_ascii_case(&asset.sha256) {
        return Err("The download doesn’t match the release’s SHA-256".into());
    }
    Ok(())
}

/// A signed Copper takes only an update signed by the same publisher
/// (`copper_setup::signature::valid_signer` of each). An unsigned one has
/// no publisher to hold it to; the feed's hash, over HTTPS, vouches.
pub fn same_publisher(running: Option<&str>, update: Option<&str>) -> Result<(), String> {
    match running {
        Some(r) if update != Some(r) => Err(format!("The download isn’t signed by {r}")),
        _ => Ok(()),
    }
}

/// What a pending update to `pending` came to, seen from the Copper that
/// started next (`running`): `(ok, detail)` — the version, or why not.
pub fn settle(pending: &str, running: &str) -> (bool, String) {
    if pending == running {
        (true, running.to_owned())
    } else {
        (
            false,
            format!(
                "Copper {pending} wasn’t installed, so this is still {running} — %TEMP%\\CopperSetup.log says why"
            ),
        )
    }
}

/// The size and SHA-256 (lower-case hex) of everything `r` reads.
pub fn sha256_of(r: &mut impl Read) -> std::io::Result<(u64, String)> {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    let size = std::io::copy(r, &mut hasher)?;
    Ok((size, format!("{:x}", hasher.finalize())))
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::platform::{Shell, Win};
    use copper_core::date::SwiftDate;
    use copper_core::updates::{PendingUpdate, StagedUpdate, UpdateOutcome, UpdatesFile};
    use copper_setup::signature;
    use copper_shell_core::main_loop::spawn_blocking;
    use copper_shell_core::updates;
    use std::cell::RefCell;
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// `FILE_SHARE_READ`: others may read (and run) it, not change it.
    const SHARE_READ: u32 = 1;

    #[derive(Default)]
    struct State {
        /// `staged`, `pending` and `outcome`, as the Mac keeps them in
        /// `updates.json` (here the shared check owns that file).
        saved: UpdatesFile,
        /// A download is under way.
        busy: bool,
        /// Why the last download was refused or failed.
        error: Option<String>,
        /// The version a failure was last toasted for (once is enough).
        trouble: Option<String>,
        /// The installer has been started; this Copper is on its way out.
        applying: bool,
        /// This start settled a pending update.
        settled: bool,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::default();
    }

    fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
        STATE.with(|s| f(&mut s.borrow_mut()))
    }

    /// This is the installed copy, so it may replace itself.
    pub fn enabled() -> bool {
        static INSTALLED: OnceLock<bool> = OnceLock::new();
        *INSTALLED.get_or_init(|| {
            let canon = |p: &Path| {
                std::fs::canonicalize(p)
                    .ok()
                    .map(|p| p.to_string_lossy().to_lowercase())
            };
            let me = std::env::current_exe().ok().and_then(|p| canon(&p));
            me.is_some() && me == canon(&copper_setup::installed_exe())
        })
    }

    /// `%LOCALAPPDATA%\Copper\Updates` (`Copper-<world>` in a world).
    fn dir() -> PathBuf {
        copper_core::platform::WindowsLayout::from_env()
            .root
            .join("Updates")
    }

    fn state_file() -> PathBuf {
        dir().join("state.json")
    }

    fn feed() -> (String, bool) {
        match std::env::var("COPPER_UPDATE_FEED") {
            Ok(f) if !f.trim().is_empty() => (f, true),
            _ => (crate::updates::FEED.to_owned(), false),
        }
    }

    fn save() {
        let saved = with(|s| UpdatesFile {
            staged: s.saved.staged.clone(),
            pending: s.saved.pending.clone(),
            outcome: s.saved.outcome.clone(),
            ..UpdatesFile::default()
        });
        let written = serde_json::to_vec_pretty(&saved)
            .map_err(|e| e.to_string())
            .and_then(|b| {
                copper_core::store::write_atomic(&state_file(), &b, false, false)
                    .map_err(|e| e.to_string())
            });
        if let Err(e) = written {
            tracing::warn!(error = %e, "updates state");
        }
    }

    /// Delete everything in the folder but the state and `keep`: stale
    /// installers, half downloads.
    fn sweep(keep: Option<&Path>) {
        let Ok(entries) = std::fs::read_dir(dir()) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p != state_file() && Some(p.as_path()) != keep && p.is_file() {
                let _ = std::fs::remove_file(&p);
            }
        }
    }

    /// The staged version, when its file is there.
    pub fn ready() -> Option<String> {
        with(|s| {
            s.saved
                .staged
                .as_ref()
                .filter(|st| Path::new(&st.path).is_file())
                .map(|st| st.version.clone())
        })
    }

    /// What Settings › About shows.
    pub struct Status {
        pub enabled: bool,
        pub busy: bool,
        pub error: Option<String>,
        pub ready: Option<String>,
        pub outcome: Option<UpdateOutcome>,
        /// This start settled a pending update (the self-test).
        pub settled: bool,
    }

    pub fn status() -> Status {
        let ready = ready();
        with(|s| Status {
            enabled: enabled(),
            busy: s.busy,
            error: s.error.clone(),
            ready,
            outcome: s.saved.outcome.clone(),
            settled: s.settled,
        })
    }

    /// At launch: read the state, settle a pending update, drop what is
    /// stale.
    pub fn start(shell: &Shell) {
        if !enabled() {
            return;
        }
        let mut saved: UpdatesFile = std::fs::read(state_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let current = updates::version();
        let mut said = None;
        if let Some(p) = saved.pending.take() {
            let (ok, detail) = settle(&p.version, current);
            tracing::info!(ok, %detail, from = %p.from, "update outcome");
            said = Some(if ok {
                format!("Copper updated to {current}")
            } else {
                format!("Copper {} wasn’t installed — Settings › About", p.version)
            });
            if !ok {
                // Fetch and check it afresh before trying again.
                saved.staged = None;
            }
            saved.outcome = Some(UpdateOutcome {
                ok,
                detail,
                at: SwiftDate::now(),
                ..Default::default()
            });
        }
        saved.staged = saved
            .staged
            .filter(|st| updates::is_newer(&st.version, current) && Path::new(&st.path).is_file());
        let keep = saved.staged.as_ref().map(|st| PathBuf::from(&st.path));
        with(|s| {
            s.settled = said.is_some();
            s.saved = saved;
        });
        save();
        sweep(keep.as_deref());
        if let Some(text) = said {
            let weak = shell.downgrade();
            copper_win_rt::spawn_local(async move {
                copper_win_rt::sleep(Duration::from_secs(2)).await;
                if let Some(s) = weak.upgrade() {
                    s.toast(&text);
                }
            });
        }
    }

    /// The check knows of a newer Copper: download and verify it (one at a
    /// time), unless it is staged already.
    pub fn stage(shell: &Shell) {
        let Some(version) = updates::available::<Win>() else {
            return;
        };
        if !enabled()
            || with(|s| s.busy || s.applying)
            || ready().is_some_and(|r| !updates::is_newer(&version, &r))
        {
            return;
        }
        with(|s| {
            s.busy = true;
            s.error = None;
        });
        crate::settings::changed(shell, "about");
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            let result = fetch_and_stage().await;
            let Some(shell) = weak.upgrade() else { return };
            with(|s| s.busy = false);
            match result {
                Ok(staged) => {
                    tracing::info!(version = %staged.version, path = %staged.path, "update staged");
                    let keep = PathBuf::from(&staged.path);
                    let text = format!(
                        "Copper {} is ready — Restart to Update, or it installs when you quit",
                        staged.version
                    );
                    with(|s| s.saved.staged = Some(staged));
                    save();
                    sweep(Some(&keep));
                    shell.toast(&text);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "update download");
                    let first = with(|s| {
                        s.error = Some(e);
                        s.trouble.replace(version.clone()).as_ref() != Some(&version)
                    });
                    if first {
                        shell.toast(&format!(
                            "Copper {version} couldn’t be downloaded — Settings › About"
                        ));
                    }
                }
            }
            crate::settings::changed(&shell, "about");
        });
    }

    /// Read the feed, pick this CPU's installer, download and verify it.
    async fn fetch_and_stage() -> Result<StagedUpdate, String> {
        let (feed, test) = feed();
        let body = updates::get::<Win>(&feed).await?;
        let manifest: Value =
            serde_json::from_slice(&body).map_err(|e| format!("The feed made no sense ({e})"))?;
        let version = manifest["version"].as_str().unwrap_or("").trim().to_owned();
        if !updates::is_newer(&version, updates::version()) {
            return Err(format!(
                "The feed’s Copper {version} is not newer than this one"
            ));
        }
        let asset = pick(&manifest, ARCH, test)?;
        let name = staged_name(&version).ok_or("The feed’s version isn’t one")?;
        let path = spawn_blocking(move || download(&asset, &dir().join(name)))
            .await
            .unwrap_or_else(|| Err("The download stopped".into()))?;
        let sha256 = path.1;
        Ok(StagedUpdate {
            version,
            path: path.0.to_string_lossy().into_owned(),
            sha256,
            at: SwiftDate::now(),
            ..Default::default()
        })
    }

    /// Download `asset` to `to` (via `to.part`) and keep it only if it
    /// verifies (worker thread): the file and its SHA-256.
    fn download(asset: &Asset, to: &Path) -> Result<(PathBuf, String), String> {
        let folder = to.parent().ok_or("No folder")?;
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let part = to.with_extension("exe.part");
        let u = url::Url::parse(&asset.url).map_err(|e| e.to_string())?;
        let host = u.host_str().ok_or("No host")?;
        let port = u.port_or_known_default().unwrap_or(443);
        let path = match u.query() {
            Some(q) => format!("{}?{q}", u.path()),
            None => u.path().to_owned(),
        };
        let cancel = AtomicBool::new(false);
        let got = copper_setup::download(
            host,
            port,
            u.scheme() == "https",
            &path,
            &part,
            &cancel,
            |done, _| {
                if done > asset.size {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        let checked = got
            .map_err(|e| {
                if cancel.load(Ordering::Relaxed) {
                    format!(
                        "The download is bigger than the {} bytes the release says",
                        asset.size
                    )
                } else {
                    format!("Couldn’t download the update: {e:#}")
                }
            })
            .and_then(|_| {
                let mut f = std::fs::File::open(&part).map_err(|e| e.to_string())?;
                let (size, sha) = sha256_of(&mut f).map_err(|e| e.to_string())?;
                verify(asset, size, &sha)?;
                let running = std::env::current_exe()
                    .map(|me| signature::check(&me))
                    .unwrap_or((-1, None));
                let update = signature::check(&part);
                same_publisher(
                    signature::valid_signer(running.0, running.1.as_deref()),
                    signature::valid_signer(update.0, update.1.as_deref()),
                )?;
                Ok(sha)
            });
        let sha = match checked {
            Ok(sha) => sha,
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                return Err(e);
            }
        };
        let _ = std::fs::remove_file(to);
        std::fs::rename(&part, to).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            e.to_string()
        })?;
        Ok((to.to_path_buf(), sha))
    }

    /// Start the staged installer: `relaunch` opens Copper again after.
    /// `false` when nothing was started (the reason is in Settings).
    fn apply(shell: &Shell, relaunch: bool) -> bool {
        let Some(staged) = with(|s| s.saved.staged.clone()).filter(|_| enabled()) else {
            return false;
        };
        // Held, without sharing writes, until the installer is running: the
        // bytes hashed are the bytes run.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(SHARE_READ)
            .open(&staged.path)
            .and_then(|mut f| sha256_of(&mut f).map(|(_, sha)| (f, sha)));
        let held = match held {
            Ok((f, sha)) if sha.eq_ignore_ascii_case(&staged.sha256) => f,
            other => {
                tracing::warn!(
                    ok = other.is_ok(),
                    "the staged update changed; discarding it"
                );
                with(|s| {
                    s.saved.staged = None;
                    s.error = Some("The downloaded update changed on disk".into());
                });
                save();
                sweep(None);
                crate::settings::changed(shell, "about");
                return false;
            }
        };
        let current = updates::version();
        with(|s| {
            s.saved.pending = Some(PendingUpdate {
                version: staged.version.clone(),
                from: current.to_owned(),
                at: SwiftDate::now(),
                ..Default::default()
            });
        });
        save();
        let args = copper_setup::cli::update_args(std::process::id(), current, relaunch);
        let started = std::process::Command::new(&staged.path)
            .args(&args)
            .current_dir(dir())
            .spawn();
        drop(held);
        match started {
            Ok(child) => {
                tracing::info!(pid = child.id(), ?args, "update installer started");
                with(|s| s.applying = true);
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, "update installer");
                with(|s| {
                    s.saved.pending = None;
                    s.saved.outcome = Some(UpdateOutcome {
                        ok: false,
                        detail: format!(
                            "Couldn’t start the Copper {} installer: {e}",
                            staged.version
                        ),
                        at: SwiftDate::now(),
                        ..Default::default()
                    });
                });
                save();
                shell.toast("Couldn’t start the update — Settings › About");
                crate::settings::changed(shell, "about");
                false
            }
        }
    }

    /// Restart to Update: install the staged Copper, then open it again.
    pub fn restart(shell: &Shell) {
        if apply(shell, true) {
            crate::app::quit(shell);
        }
    }

    /// Copper is quitting: a staged update installs now, opening nothing.
    pub fn on_quit(shell: &Shell) {
        if !with(|s| s.applying) && ready().is_some() {
            apply(shell, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn feed(url: &str) -> Value {
        json!({
            "version": "0.3.5",
            "assets": [
                {"kind": "zip", "arch": "x64", "size": 1, "sha256": SHA, "url": "https://x/zip"},
                {"kind": "installer", "arch": "arm64", "size": 2, "sha256": SHA, "url": "https://x/arm"},
                {"kind": "installer", "arch": "x64", "size": 3, "sha256": SHA.to_uppercase(), "url": url},
            ]
        })
    }

    #[test]
    fn picks_this_cpus_installer_over_https() {
        let a = pick(&feed("https://x/CopperSetup-x64.exe"), "x64", false).unwrap();
        assert_eq!((a.size, a.sha256.as_str()), (3, SHA));
        assert_eq!(pick(&feed("https://x/a"), "arm64", false).unwrap().size, 2);
        assert!(pick(&feed("https://x/a"), "x86", false).is_err());
        assert!(pick(&json!({"version": "1"}), "x64", false).is_err());
    }

    #[test]
    fn plain_http_only_for_a_test_feed() {
        let f = feed("http://127.0.0.1:8765/CopperSetup-x64.exe");
        assert!(pick(&f, "x64", false).is_err());
        assert!(pick(&f, "x64", true).is_ok());
        for bad in ["file:///C:/x.exe", "ftp://x/y", "", "not a url"] {
            assert!(pick(&feed(bad), "x64", true).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_feed_without_a_hash_or_size_is_refused() {
        let mut f = feed("https://x/a");
        f["assets"][2]["sha256"] = json!("abc");
        assert!(pick(&f, "x64", false).is_err());
        let mut f = feed("https://x/a");
        f["assets"][2]["size"] = json!(0);
        assert!(pick(&f, "x64", false).is_err());
        f["assets"][2]["size"] = json!("3");
        assert!(pick(&f, "x64", false).is_err());
    }

    #[test]
    fn verify_wants_the_size_and_the_hash() {
        let a = Asset {
            size: 3,
            sha256: SHA.into(),
            url: String::new(),
        };
        let (size, sha) = sha256_of(&mut &b"abc"[..]).unwrap();
        assert_eq!((size, sha.as_str()), (3, SHA));
        assert!(verify(&a, size, &sha).is_ok());
        assert!(verify(&a, size, &SHA.to_uppercase()).is_ok());
        assert!(verify(&a, 4, &sha).is_err());
        let (n, other) = sha256_of(&mut &b"abd"[..]).unwrap();
        assert!(verify(&a, n, &other).is_err());
    }

    #[test]
    fn a_signed_copper_takes_only_its_publishers_update() {
        assert!(same_publisher(None, None).is_ok());
        assert!(same_publisher(None, Some("Anyone")).is_ok());
        assert!(same_publisher(Some("Exowatt, Inc."), Some("Exowatt, Inc.")).is_ok());
        assert!(same_publisher(Some("Exowatt, Inc."), None).is_err());
        assert!(same_publisher(Some("Exowatt, Inc."), Some("Exowatt Inc")).is_err());
    }

    #[test]
    fn staged_names_are_plain() {
        assert_eq!(
            staged_name("0.3.5").as_deref(),
            Some(format!("CopperSetup-0.3.5-{ARCH}.exe").as_str())
        );
        assert!(staged_name("1.0.0-rc.1+b7").is_some());
        for bad in ["", "..\\..\\x", "0.3.5/../a", "v0.3.5", "0.3.5 x", "0.3:5"] {
            assert!(staged_name(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_next_start_says_how_it_went() {
        assert_eq!(settle("0.3.5", "0.3.5"), (true, "0.3.5".into()));
        let (ok, why) = settle("0.3.5", "0.3.4");
        assert!(!ok && why.contains("0.3.5") && why.contains("still 0.3.4"));
    }
}
