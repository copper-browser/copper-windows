//! The Microsoft Edge WebView2 Runtime: is it there, and if not, get it.
//!
//! Copper draws every page with WebView2 (Evergreen). Windows 11 ships it;
//! Windows 10 machines usually have it via Edge, but not always. The check
//! is what `GetAvailableCoreWebView2BrowserVersionString` does under the
//! hood — the EdgeUpdate client key's `pv` value, machine-wide (either
//! registry view) or per-user — so setup needs no WebView2 loader. When it
//! is missing, setup downloads Microsoft's Evergreen bootstrapper over HTTPS,
//! checks its Authenticode signature, and runs it silently; run without
//! elevation it installs per-user.

/// The WebView2 Runtime's EdgeUpdate client id.
pub const CLIENT_ID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// The Evergreen bootstrapper, as Microsoft publishes it.
pub const BOOTSTRAPPER_HOST: &str = "go.microsoft.com";
pub const BOOTSTRAPPER_PATH: &str = "/fwlink/p/?LinkId=2124703";
/// What the bootstrapper is saved as before it is run.
pub const BOOTSTRAPPER_FILE: &str = "MicrosoftEdgeWebview2Setup.exe";
/// No UI, install now.
pub const BOOTSTRAPPER_ARGS: [&str; 2] = ["/silent", "/install"];

/// Where `pv` is looked for, in order: (machine-wide?, key under the root).
pub fn version_keys() -> [(bool, String); 3] {
    [
        (
            true,
            format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{CLIENT_ID}"),
        ),
        (
            true,
            format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{CLIENT_ID}"),
        ),
        (
            false,
            format!(r"Software\Microsoft\EdgeUpdate\Clients\{CLIENT_ID}"),
        ),
    ]
}

/// Is `pv` a real installed version? EdgeUpdate leaves `""` or `0.0.0.0`
/// behind when the runtime has been removed.
pub fn is_real_version(pv: &str) -> bool {
    let parts: Vec<&str> = pv.trim().split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && parts.iter().any(|p| p.bytes().any(|b| b != b'0'))
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::WinTrust::{
        WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
        WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_WHOLECHAIN,
        WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
    };
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;
    use windows_core::{GUID, HSTRING, PCWSTR};

    use super::*;
    use crate::log::log;
    use crate::sys::reg::{self, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    /// The installed WebView2 Runtime version, if there is one.
    pub fn installed_version() -> Option<String> {
        version_keys().into_iter().find_map(|(machine, key)| {
            let root = if machine {
                HKEY_LOCAL_MACHINE
            } else {
                HKEY_CURRENT_USER
            };
            reg::read_str(root, &key, Some("pv")).filter(|pv| is_real_version(pv))
        })
    }

    /// Is the runtime installed?
    pub fn is_installed() -> bool {
        installed_version().is_some()
    }

    /// What getting WebView2 is up to, for the progress bar.
    pub enum Progress {
        Downloading { done: u64, total: Option<u64> },
        Installing { elapsed: Duration },
    }

    /// The bootstrapper ran for too long: something is stuck. Generous: on a
    /// slow line the runtime is a ~150 MB download.
    const INSTALL_LIMIT: Duration = Duration::from_secs(20 * 60);

    /// Is `file` validly Authenticode-signed (chain to a trusted root, not
    /// revoked)? HTTPS from go.microsoft.com already vouches for it; this
    /// makes sure a captive portal or a proxy never gets an exe run.
    fn is_signed(file: &Path) -> bool {
        let path = HSTRING::from(file);
        let mut info = WINTRUST_FILE_INFO {
            cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: PCWSTR(path.as_ptr()),
            hFile: HANDLE::default(),
            pgKnownSubject: std::ptr::null_mut(),
        };
        let mut data = WINTRUST_DATA {
            cbStruct: size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
            dwStateAction: WTD_STATEACTION_VERIFY,
            dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
            ..Default::default()
        };
        let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        // SAFETY: `data` and `info` outlive both calls; the second releases
        // the state the first allocated.
        let status = unsafe {
            let s = WinVerifyTrust(
                HWND(-1isize as *mut _),
                &mut action,
                (&mut data as *mut WINTRUST_DATA).cast(),
            );
            data.dwStateAction = WTD_STATEACTION_CLOSE;
            WinVerifyTrust(
                HWND(-1isize as *mut _),
                &mut action,
                (&mut data as *mut WINTRUST_DATA).cast(),
            );
            s
        };
        // A revocation server out of reach (a strict proxy, a flaky line)
        // doesn't make a valid signature invalid; anything else does.
        const CRYPT_E_REVOCATION_OFFLINE: i32 = 0x8009_2013_u32 as i32;
        const CERT_E_REVOCATION_FAILURE: i32 = 0x800B_010E_u32 as i32;
        match status {
            0 => true,
            CRYPT_E_REVOCATION_OFFLINE | CERT_E_REVOCATION_FAILURE => {
                log!("WebView2 bootstrapper signed; revocation unchecked (0x{status:08x})");
                true
            }
            _ => {
                log!("WebView2 bootstrapper signature check failed: 0x{status:08x}");
                false
            }
        }
    }

    fn scratch_dir() -> anyhow::Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("CopperSetup-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Download and run the Evergreen bootstrapper, reporting progress.
    /// Returns once the runtime is installed (checked in the registry, not
    /// just the bootstrapper's exit code).
    pub fn install(cancel: &AtomicBool, mut progress: impl FnMut(Progress)) -> anyhow::Result<()> {
        let dir = scratch_dir()?;
        let exe = dir.join(BOOTSTRAPPER_FILE);
        let result = (|| {
            let bytes = crate::sys::http::download(
                BOOTSTRAPPER_HOST,
                BOOTSTRAPPER_PATH,
                &exe,
                cancel,
                |done, total| progress(Progress::Downloading { done, total }),
            )?;
            log!("downloaded WebView2 bootstrapper: {bytes} bytes");
            if !is_signed(&exe) {
                anyhow::bail!("the WebView2 download isn't signed by Microsoft");
            }

            let started = Instant::now();
            let mut child = Command::new(&exe)
                .args(BOOTSTRAPPER_ARGS)
                .current_dir(&dir)
                .creation_flags(CREATE_NO_WINDOW.0)
                .spawn()?;
            let status = loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = child.kill();
                    anyhow::bail!("cancelled");
                }
                let elapsed = started.elapsed();
                if elapsed > INSTALL_LIMIT {
                    let _ = child.kill();
                    anyhow::bail!("the WebView2 installer didn't finish in {INSTALL_LIMIT:?}");
                }
                progress(Progress::Installing { elapsed });
                std::thread::sleep(Duration::from_millis(200));
            };
            log!(
                "WebView2 bootstrapper exited with {status} after {:?}",
                started.elapsed()
            );
            // The bootstrapper's exit codes are not documented as a contract;
            // the registry is the truth.
            match installed_version() {
                Some(v) => {
                    log!("WebView2 Runtime {v} installed");
                    Ok(())
                }
                None => anyhow::bail!(
                    "the WebView2 installer finished ({status}) but the runtime isn't there"
                ),
            }
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_versions() {
        for pv in ["130.0.2849.80", "86.0.622.38", " 120.0.0.1 "] {
            assert!(is_real_version(pv), "{pv}");
        }
        for pv in ["", "0.0.0.0", "1.2.3", "a.b.c.d", "1..2.3", "1.2.3.4.5"] {
            assert!(!is_real_version(pv), "{pv}");
        }
    }

    #[test]
    fn keys_cover_both_views_and_the_user() {
        let keys = version_keys();
        assert!(keys.iter().any(|(m, k)| *m && k.contains("WOW6432Node")));
        assert!(keys.iter().any(|(m, k)| *m && !k.contains("WOW6432Node")));
        assert!(keys.iter().any(|(m, _)| !*m));
        assert!(keys.iter().all(|(_, k)| k.ends_with(CLIENT_ID)));
    }
}
