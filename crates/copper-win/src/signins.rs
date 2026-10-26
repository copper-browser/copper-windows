//! Sign-ins carried over from another Copper, once: `<data>\import\
//! cookies.dpapi` is a JSON list of [`copper_engine::Cookie`]s sealed with
//! DPAPI for this Windows user (no entropy). At start-up it is opened, put
//! in the default jar and deleted — deleted first, so a bad file is never
//! tried twice. Values are never logged.

use crate::platform::Shell;
use copper_engine::{Cookie, Engine, Profile};
use std::path::PathBuf;

fn file() -> PathBuf {
    copper_core::store::Paths::from_env()
        .data
        .join("import")
        .join("cookies.dpapi")
}

/// Bring the sealed sign-ins in, if any are waiting.
pub fn take_in(shell: &Shell) {
    let path = file();
    let Ok(sealed) = std::fs::read(&path) else {
        return;
    };
    let _ = std::fs::remove_file(&path);
    let cookies: Vec<Cookie> = match unseal(&sealed).map(|p| serde_json::from_slice(&p)) {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => return tracing::warn!("sign-ins file unreadable: {e}"),
        Err(e) => return tracing::warn!("sign-ins file could not be opened: {e}"),
    };
    let Ok(profile) = shell
        .engine()
        .profile(&copper_shell_core::shell::pages::profile_id(None), false)
    else {
        return tracing::warn!("sign-ins: no default jar");
    };
    copper_win_rt::spawn_local(async move {
        let (mut ok, mut failed) = (0usize, 0usize);
        for c in &cookies {
            match profile.set_cookie(c).await {
                Ok(()) => ok += 1,
                Err(_) => failed += 1,
            }
        }
        tracing::info!(ok, failed, "sign-ins brought over");
    });
}

/// `CryptUnprotectData`, user scope, no UI.
fn unseal(sealed: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(sealed.len()).map_err(|_| "too big".to_string())?,
        // Only read by DPAPI.
        pbData: sealed.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` points into a live slice; `out` is DPAPI's LocalAlloc
    // buffer, copied out, zeroed and freed once.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
        .map_err(|e| e.message())?;
        let system = std::slice::from_raw_parts_mut(out.pbData, out.cbData as usize);
        let copy = system.to_vec();
        system.fill(0);
        LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        Ok(copy)
    }
}
