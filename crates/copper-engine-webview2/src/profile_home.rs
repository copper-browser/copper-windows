//! Which WebView2 profile a Copper profile lives in.
//!
//! The WebView2 browser process always sets up its own default profile
//! (`EBWebView\Default`) when it starts, whether a webview uses it or not.
//! Every named profile (`ControllerOptions.ProfileName`) is a second,
//! separate one (`EBWebView\WV2Profile_<name>`). On a fresh user data
//! folder each costs the same first-run work — about ten SQLite databases
//! (cookies, history, web data, trust tokens, …) created at once — and the
//! first request waits for its cookie database. With Defender's real-time
//! scanning those writes contend, so two profiles take twice as long. The
//! main profile therefore lives in WebView2's default profile. On the Win11
//! VM, a fresh install's first navigation dropped from 12–16 s to 6–10 s.
//!
//! A main profile already kept in its named folder (made before this
//! existed) stays there, so nobody's cookies or site data move. Ephemeral
//! profiles are always named (InPrivate on the default profile would share
//! its name with the main one).

use std::path::{Path, PathBuf};

use copper_engine::ProfileId;

use crate::util::webview2_profile_name;

/// The WebView2 profile name for Copper profile `id`: `None` for WebView2's
/// own default profile. `id` is the `main` profile, not ephemeral, and
/// `named_kept` says no folder exists yet for its named profile.
pub(crate) fn webview2_profile(
    id: &ProfileId,
    ephemeral: bool,
    main: Option<&ProfileId>,
    named_kept: impl FnOnce(&str) -> bool,
) -> Option<String> {
    let name = webview2_profile_name(id.as_str());
    let is_main = !ephemeral && main == Some(id);
    if is_main && !named_kept(&name) {
        None
    } else {
        Some(name)
    }
}

/// The folder WebView2 keeps named profile `name` in under user data folder
/// `user_data_dir` (the runtime's layout: `EBWebView\WV2Profile_<name>`).
pub(crate) fn named_profile_dir(user_data_dir: &Path, name: &str) -> PathBuf {
    user_data_dir
        .join("EBWebView")
        .join(format!("WV2Profile_{name}"))
}

/// Whether `dir` may hold a profile's data: anything but a clear "not
/// found" (a folder that cannot be read is kept, never shadowed).
pub(crate) fn kept(dir: &Path) -> bool {
    !matches!(
        std::fs::symlink_metadata(dir),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> ProfileId {
        ProfileId::new(s).unwrap()
    }

    #[test]
    fn main_profile_uses_the_default_one() {
        let main = id("default");
        assert_eq!(webview2_profile(&main, false, Some(&main), |_| false), None);
        // Already kept in its named folder: stays there.
        assert_eq!(
            webview2_profile(&main, false, Some(&main), |n| n == "default"),
            Some("default".into())
        );
        // Ephemeral: InPrivate on its own name.
        assert_eq!(
            webview2_profile(&main, true, Some(&main), |_| false),
            Some("default".into())
        );
    }

    #[test]
    fn other_profiles_are_named() {
        let main = id("default");
        assert_eq!(
            webview2_profile(&id("Work"), false, Some(&main), |_| false),
            Some(webview2_profile_name("Work"))
        );
        assert_eq!(
            webview2_profile(&main, false, None, |_| false),
            Some("default".into())
        );
    }

    #[test]
    fn named_folder_layout() {
        let dir = named_profile_dir(Path::new("C:/u/WebView2"), "work");
        assert!(dir.ends_with("EBWebView/WV2Profile_work"));
    }

    #[test]
    fn missing_folder_is_not_kept() {
        let base = std::env::temp_dir().join(format!("copper-home-{}", std::process::id()));
        assert!(!kept(&base.join("absent")));
        std::fs::create_dir_all(&base).unwrap();
        assert!(kept(&base));
        let _ = std::fs::remove_dir_all(&base);
    }
}
