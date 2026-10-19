//! The browser process's command line (`AdditionalBrowserArguments`).
//!
//! The environment is one per process, so these apply to every page. Each
//! entry is here for a stated reason; nothing is added "just in case".
//!
//! # Backgrounding
//!
//! Ordinary background tabs must still be throttled and suspendable
//! (`IsVisible = false`, `TrySuspend`, `MemoryUsageTargetLevel::Low`), so no
//! renderer- or timer-backgrounding switch is set. The only two switches
//! here concern *visible webviews in windows Windows considers covered* —
//! which is what the backstage host is (an off-screen tool window):
//!
//! | Switch | Why |
//! |---|---|
//! | `--disable-features=CalculateNativeWinOcclusion` | Chromium otherwise marks the off-screen backstage window occluded and stops rendering its pages |
//! | `--disable-backgrounding-occluded-windows` | the same, for occlusion reported by other paths |
//!
//! # Talking to Microsoft (audit)
//!
//! | Channel | Off? | How |
//! |---|---|---|
//! | SmartScreen URL / download reputation | yes | `--disable-features=msSmartScreenProtection` here, and `ICoreWebView2Settings8::IsReputationCheckingRequired = FALSE` per page |
//! | Crash dump upload | yes | `IsCustomCrashReportingEnabled = TRUE` (dumps stay in the user data folder's `Crashpad\reports`) |
//! | Autofill server queries (form-type prediction) | yes | `--disable-features=AutofillServerCommunication`; autofill itself is off per profile |
//! | Client-side phishing model and pings | yes | `--disable-client-side-phishing-detection` |
//! | Domain Reliability (network error beacons) | yes | `--disable-domain-reliability` |
//! | Optimization-guide model / hint downloads | yes | `--disable-features=OptimizationHints,OptimizationGuideModelDownloading` |
//! | Media Router (cast discovery on the LAN) | yes | `--disable-features=MediaRouter` |
//! | Single sign-on with the Windows account | yes | `AllowSingleSignOnUsingOSPrimaryAccount = FALSE` |
//! | WebView2 runtime updates (Edge Update) | no | security updates; the Evergreen runtime is shared with the OS |
//! | Component updates (CRLSets, certificate transparency logs, origin trials) | no | revocation and CT data are security-relevant |
//! | Runtime diagnostic data | no | follows Windows' own diagnostic-data setting (`AllowTelemetry` policy); no switch exists |
//!
//! Tracking prevention needs `EnableTrackingPrevention = TRUE` at
//! environment creation; each profile then sets *Strict*.

/// Features disabled for every page (see the module docs).
const DISABLED_FEATURES: &[&str] = &[
    "CalculateNativeWinOcclusion",
    "msSmartScreenProtection",
    "AutofillServerCommunication",
    "OptimizationHints",
    "OptimizationGuideModelDownloading",
    "MediaRouter",
];

/// Plain switches for every page (see the module docs).
const SWITCHES: &[&str] = &[
    "--disable-backgrounding-occluded-windows",
    "--disable-client-side-phishing-detection",
    "--disable-domain-reliability",
];

/// The command line: Copper's switches, `--accept-lang` for the preferred
/// `languages` (normalized; see `languages`), then `extra` (e.g. from
/// `COPPER_WEBVIEW2_ARGS`; an `--accept-lang` there wins). Chromium reads
/// only the *last* `--disable-features`/`--enable-features`, so every list
/// is merged into one switch.
pub(crate) fn browser_arguments(extra: &[String], languages: &[String]) -> String {
    let mut disabled: Vec<String> = DISABLED_FEATURES.iter().map(|s| s.to_string()).collect();
    let mut enabled: Vec<String> = Vec::new();
    let mut switches: Vec<String> = SWITCHES.iter().map(|s| s.to_string()).collect();
    let caller_langs = extra
        .iter()
        .flat_map(|a| a.split_whitespace())
        .any(|a| a.starts_with("--accept-lang="));
    if !caller_langs && let Some(s) = crate::languages::accept_lang_switch(languages) {
        switches.push(s);
    }
    for arg in extra.iter().flat_map(|a| a.split_whitespace()) {
        let push_features = |list: &mut Vec<String>, v: &str| {
            for f in v.split(',').map(str::trim).filter(|f| !f.is_empty()) {
                if !list.iter().any(|x| x == f) {
                    list.push(f.to_owned());
                }
            }
        };
        if let Some(v) = arg.strip_prefix("--disable-features=") {
            push_features(&mut disabled, v);
        } else if let Some(v) = arg.strip_prefix("--enable-features=") {
            push_features(&mut enabled, v);
        } else if !switches.iter().any(|s| s == arg) {
            switches.push(arg.to_owned());
        }
    }
    // A feature both asked on and off is on (the caller's explicit ask wins).
    disabled.retain(|f| !enabled.contains(f));
    let mut out = switches;
    out.push(format!("--disable-features={}", disabled.join(",")));
    if !enabled.is_empty() {
        out.push(format!("--enable-features={}", enabled.join(",")));
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_line() {
        let a = browser_arguments(&[], &[]);
        assert!(!a.contains("--accept-lang"));
        assert_eq!(a.matches("--disable-features=").count(), 1);
        assert!(a.contains("CalculateNativeWinOcclusion"));
        assert!(a.contains("msSmartScreenProtection"));
        assert!(a.contains("--disable-backgrounding-occluded-windows"));
        // Background tabs stay throttled.
        assert!(!a.contains("--disable-renderer-backgrounding"));
        assert!(!a.contains("--disable-background-timer-throttling"));
        assert!(!a.contains("--enable-features"));
    }

    #[test]
    fn extra_arguments_merge() {
        let a = browser_arguments(
            &[
                "--disable-features=Foo,MediaRouter --enable-features=Bar".into(),
                "--remote-debugging-port=9222 --enable-features=MediaRouter".into(),
                "--disable-domain-reliability".into(),
            ],
            &[],
        );
        assert_eq!(a.matches("--disable-features=").count(), 1);
        assert_eq!(a.matches("--enable-features=").count(), 1);
        assert!(a.contains(",Foo"));
        assert!(a.contains("--enable-features=Bar,MediaRouter"));
        assert!(a.contains("--remote-debugging-port=9222"));
        assert_eq!(a.matches("--disable-domain-reliability").count(), 1);
        let disabled = a
            .split("--disable-features=")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap();
        assert!(!disabled.split(',').any(|f| f == "MediaRouter"));
    }

    #[test]
    fn languages() {
        let langs = vec!["de-DE".to_owned(), "fr".to_owned()];
        let a = browser_arguments(&[], &langs);
        assert_eq!(a.matches("--accept-lang=de-DE,fr").count(), 1);
        // The caller's own switch wins.
        let a = browser_arguments(&["--accept-lang=ja".into()], &langs);
        assert!(a.contains("--accept-lang=ja"));
        assert!(!a.contains("de-DE"));
    }
}
