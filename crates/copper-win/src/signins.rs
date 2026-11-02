//! Sign-ins carried over from another Copper, once. Two files in
//! `<data>\import\`, each sealed with DPAPI for this Windows user (no
//! entropy):
//!
//! * `cookies.dpapi` — a JSON list of [`copper_engine::Cookie`]s, put in
//!   the default jar;
//! * `localstorage.dpapi` — `{"<origin>": {"<key>": "<value>"}}` (the Mac's
//!   `StorageImport` shape), put in each origin's localStorage: the sites
//!   that keep their sign-in there and not in a cookie.
//!
//! At start-up each is opened, brought in and deleted — deleted first, so a
//! bad file is never tried twice. Values are never logged.

use crate::platform::Shell;
use copper_engine::{Cookie, Engine, PageOptions, PageView, Profile};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Origin → its localStorage items.
type Storage = BTreeMap<String, BTreeMap<String, String>>;

/// Enough per script call to keep the round trips down without handing
/// WebView2 one enormous argument; a single bigger value goes alone (the
/// Mac's `StorageImport.chunk`).
const CHUNK: usize = 1_000_000;

/// How long an origin's empty page may take to come up.
const LAND: Duration = Duration::from_secs(15);

fn file(name: &str) -> PathBuf {
    copper_core::store::Paths::from_env()
        .data
        .join("import")
        .join(name)
}

/// Bring the sealed sign-ins in, if any are waiting.
pub fn take_in(shell: &Shell) {
    let cookies: Option<Vec<Cookie>> = open("cookies.dpapi").and_then(|p| {
        serde_json::from_slice(&p)
            .map_err(|e| tracing::warn!("cookies.dpapi unreadable: {e}"))
            .ok()
    });
    let storage: Option<Storage> = open("localstorage.dpapi").and_then(|p| {
        serde_json::from_slice(&p)
            .map_err(|e| tracing::warn!("localstorage.dpapi unreadable: {e}"))
            .ok()
    });
    if cookies.is_none() && storage.is_none() {
        return;
    }
    let Ok(profile) = shell
        .engine()
        .profile(&copper_shell_core::shell::pages::profile_id(None), false)
    else {
        return tracing::warn!("sign-ins: no default jar");
    };
    if let Some(storage) = storage {
        let page = shell.engine().new_page(&profile, PageOptions::default());
        copper_win_rt::spawn_local(put_storage(page, storage));
    }
    let Some(cookies) = cookies else { return };
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

/// Read, delete and unseal one import file; `None` when there is none or
/// it cannot be opened.
fn open(name: &str) -> Option<Vec<u8>> {
    let path = file(name);
    let sealed = std::fs::read(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    unseal(&sealed)
        .map_err(|e| tracing::warn!("{name} could not be opened: {e}"))
        .ok()
}

/// WebView2 has no API for writing another origin's localStorage, so (as
/// the Mac's `StorageImport` does) one hidden page becomes each origin in
/// turn — an empty document served for `<origin>/` by `load_html`, nothing
/// fetched — checks it really is that origin, and sets the keys from a
/// script. One origin at a time, so a failure is pinned to its origin.
async fn put_storage<V: PageView>(page: V, data: Storage) {
    let (mut origins, mut keys, mut failed) = (0usize, 0usize, 0usize);
    for (origin, items) in &data {
        if !web_origin(origin) {
            tracing::warn!(origin, "localStorage: not an http(s) origin");
            failed += 1;
            continue;
        }
        page.load_html(
            "<!doctype html><title>import</title>",
            Some(&format!("{origin}/")),
        );
        // Landed once it stopped loading and the document is the origin's
        // (not the last one's, which a late answer could still be).
        let started = Instant::now();
        let landed = loop {
            copper_win_rt::sleep(Duration::from_millis(100)).await;
            if !page.is_loading()
                && copper_shell_core::util::run(&page, &copper_pagejs::tab::origin())
                    .await
                    .is_ok_and(|o| &o == origin)
            {
                break true;
            }
            if started.elapsed() > LAND {
                break false;
            }
        };
        if !landed {
            tracing::warn!(
                origin,
                "localStorage: the page did not come up as the origin"
            );
            failed += 1;
            continue;
        }
        let mut refused = 0usize;
        for batch in batches(items, CHUNK) {
            match copper_shell_core::util::run(&page, &copper_pagejs::tab::storage_put(&batch))
                .await
            {
                // Each answer is "key: ErrorName"; keys only, never values.
                Ok(bad) => {
                    if !bad.is_empty() {
                        tracing::warn!(origin, refused = ?bad, "localStorage: keys refused");
                    }
                    refused += bad.len();
                    keys += batch.len() - bad.len();
                }
                Err(e) => {
                    tracing::warn!(origin, error = %e, "localStorage: put failed");
                    refused += batch.len();
                }
            }
        }
        if refused > 0 {
            failed += 1;
        }
        origins += 1;
    }
    // Off the last origin, so its storage area is not held open.
    page.load("about:blank");
    tracing::info!(origins, keys, failed, "localStorage brought over");
}

/// An `http(s)://host[:port]` origin, nothing after it.
fn web_origin(origin: &str) -> bool {
    url::Url::parse(origin).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host_str().is_some()
            && u.origin().ascii_serialization() == origin
    })
}

/// Split one origin's items into script calls of at most `limit` UTF-16
/// units of keys and values each; a bigger item goes alone.
fn batches(items: &BTreeMap<String, String>, limit: usize) -> Vec<BTreeMap<String, String>> {
    let mut out = Vec::new();
    let (mut batch, mut size) = (BTreeMap::new(), 0usize);
    for (k, v) in items {
        let weight = k.encode_utf16().count() + v.encode_utf16().count();
        if size + weight > limit && !batch.is_empty() {
            out.push(std::mem::take(&mut batch));
            size = 0;
        }
        batch.insert(k.clone(), v.clone());
        size += weight;
    }
    if !batch.is_empty() {
        out.push(batch);
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_bare_web_origins() {
        assert!(web_origin("https://github.com"));
        assert!(web_origin("http://localhost:3000"));
        assert!(!web_origin("https://github.com/"));
        assert!(!web_origin("https://GitHub.com"));
        assert!(!web_origin("file:///C:/x"));
        assert!(!web_origin("copper://canvas"));
        assert!(!web_origin("github.com"));
    }

    #[test]
    fn batches_stay_under_the_limit_and_a_big_value_goes_alone() {
        let items: BTreeMap<String, String> = [
            ("a", "12345"),
            ("b", "12345"),
            ("big", "x".repeat(50).as_str()),
            ("c", "é"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let out = batches(&items, 12);
        let keys: Vec<Vec<&str>> = out
            .iter()
            .map(|b| b.keys().map(String::as_str).collect())
            .collect();
        assert_eq!(keys, vec![vec!["a", "b"], vec!["big"], vec!["c"]]);
        assert_eq!(out.iter().map(BTreeMap::len).sum::<usize>(), items.len());
        assert!(batches(&BTreeMap::new(), 12).is_empty());
    }
}
