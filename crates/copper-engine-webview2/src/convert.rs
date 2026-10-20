//! WebView2 values ↔ `copper-engine` values.
//!
//! The functions take the raw numbers of WebView2's enums (the `.0` of
//! `COREWEBVIEW2_*`), so they are platform-free and unit-tested on any host;
//! the Windows modules pass `value.0`.

use copper_engine::{
    Cookie, CrashReason, DownloadId, EngineError, PermissionKind, SameSite, WebsiteDataKinds,
};

/// `COREWEBVIEW2_WEB_ERROR_STATUS` as a short, stable error name.
pub(crate) fn web_error_name(status: i32) -> &'static str {
    match status {
        1 => "certificate common name is incorrect",
        2 => "certificate expired",
        3 => "client certificate contains errors",
        4 => "certificate revoked",
        5 => "certificate is invalid",
        6 => "server unreachable",
        7 => "timeout",
        8 => "invalid HTTP response",
        9 => "connection aborted",
        10 => "connection reset",
        11 => "disconnected",
        12 => "cannot connect",
        13 => "host name not resolved",
        14 => "cancelled",
        15 => "redirect failed",
        16 => "unexpected error",
        17 => "authentication required",
        18 => "proxy authentication required",
        _ => "unknown error",
    }
}

/// The error of a load nothing listened for: WebView2 has no status for a
/// refused connection (Chromium's `ERR_CONNECTION_REFUSED` arrives as
/// `UNKNOWN`), so a failed `UNKNOWN` load knocks once (see [`refuses`]).
pub(crate) const CONNECTION_REFUSED: &str = "connection refused";

/// `COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN`.
pub(crate) const WEB_ERROR_UNKNOWN: i32 = 0;

/// Ports Chromium will not load from at all (`net/base/port_util.cc`
/// `kRestrictedPorts`): their failure is that, not a refusal.
const RESTRICTED_PORTS: &[u16] = &[
    1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95, 101, 102,
    103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143, 161, 179, 389, 427, 465,
    512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990, 993,
    995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566, 6665, 6666, 6667, 6668,
    6669, 6679, 6697, 10080,
];

/// Where to knock for a failed load of `url`: an http(s) host and port
/// Chromium would have tried.
pub(crate) fn knock_target(url: &str) -> Option<(String, u16)> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = u.port_or_known_default()?;
    if RESTRICTED_PORTS.contains(&port) {
        return None;
    }
    Some((host, port))
}

/// Whether `host:port` refuses a TCP connection (nothing listens there).
/// Anything else (it answers, a timeout, no address) is not a refusal.
pub(crate) fn refuses(host: &str, port: u16, limit: std::time::Duration) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    // The first address only: each knock on Windows may take its whole limit.
    addrs.into_iter().next().is_some_and(|a| {
        matches!(
            TcpStream::connect_timeout(&a, limit),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused
        )
    })
}

/// `COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED`: the load was stopped
/// or replaced, not failed.
pub(crate) const WEB_ERROR_CANCELED: i32 = 14;

/// `COREWEBVIEW2_PERMISSION_KIND` → the engine's kind. Camera and microphone
/// arrive as two requests on WebView2 (never `CameraAndMicrophone`).
pub(crate) fn permission_kind(kind: i32) -> PermissionKind {
    match kind {
        1 => PermissionKind::Microphone,
        2 => PermissionKind::Camera,
        3 => PermissionKind::Geolocation,
        4 => PermissionKind::Notifications,
        5 => PermissionKind::Other("sensors".into()),
        6 => PermissionKind::Clipboard,
        7 => PermissionKind::Other("multiple-downloads".into()),
        8 => PermissionKind::Other("file-system".into()),
        9 => PermissionKind::Other("autoplay".into()),
        10 => PermissionKind::Other("local-fonts".into()),
        11 => PermissionKind::Other("midi-sysex".into()),
        12 => PermissionKind::Other("window-management".into()),
        _ => PermissionKind::Other("unknown".into()),
    }
}

/// `COREWEBVIEW2_PROCESS_FAILED_REASON` of an exited renderer → why.
/// `requested` is set while `terminate_web_process` is in flight.
pub(crate) fn crash_reason(reason: Option<i32>, requested: bool) -> CrashReason {
    if requested {
        return CrashReason::TerminatedByApi;
    }
    match reason {
        Some(5) => CrashReason::ExceededMemoryLimit,
        _ => CrashReason::Crashed,
    }
}

/// `COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON` → (error text, cancelled).
pub(crate) fn download_failure(reason: i32) -> (String, bool) {
    let text = match reason {
        1 => "file error",
        2 => "access denied",
        3 => "disk full",
        4 => "file name too long",
        5 => "file too large",
        6 => "file is malicious",
        7 => "transient file error",
        8 => "blocked by policy",
        9 => "security check failed",
        10 => "file too short",
        11 => "hash mismatch",
        12 => "network failed",
        13 => "network timeout",
        14 => "network disconnected",
        15 => "server down",
        16 => "invalid request",
        17 => "server failed",
        18 => "server does not support ranges",
        19 => "bad content",
        20 => "unauthorized",
        21 => "certificate problem",
        22 => "forbidden",
        23 => "unexpected server response",
        24 => "content length mismatch",
        25 => "cross-origin redirect",
        26 => "cancelled",
        27 => "browser shut down",
        28 => "paused",
        29 => "download process crashed",
        _ => "interrupted",
    };
    (text.to_owned(), reason == 26)
}

/// `COREWEBVIEW2_BROWSING_DATA_KINDS` bits for `ClearBrowsingData`.
/// Session storage lives in the tab, not the profile, and HSTS and AppCache
/// have no WebView2 kind, so those bits clear nothing.
pub(crate) fn browsing_data_kinds(kinds: WebsiteDataKinds) -> i32 {
    const FILE_SYSTEMS: i32 = 1;
    const INDEXED_DB: i32 = 2;
    const LOCAL_STORAGE: i32 = 4;
    const WEB_SQL: i32 = 8;
    const CACHE_STORAGE: i32 = 16;
    const COOKIES: i32 = 64;
    const DISK_CACHE: i32 = 256;
    const SERVICE_WORKERS: i32 = 32768;
    let mut out = 0;
    let mut add = |kind: WebsiteDataKinds, bits: i32| {
        if kinds.contains(kind) {
            out |= bits;
        }
    };
    add(WebsiteDataKinds::MEMORY_CACHE, DISK_CACHE);
    add(WebsiteDataKinds::DISK_CACHE, DISK_CACHE);
    add(WebsiteDataKinds::COOKIES, COOKIES);
    add(WebsiteDataKinds::LOCAL_STORAGE, LOCAL_STORAGE);
    add(
        WebsiteDataKinds::INDEXEDDB,
        INDEXED_DB | WEB_SQL | FILE_SYSTEMS,
    );
    add(WebsiteDataKinds::SERVICE_WORKERS, SERVICE_WORKERS);
    add(WebsiteDataKinds::DOM_CACHE, CACHE_STORAGE);
    out
}

/// CDP `Storage.clearDataForOrigin`'s `storageTypes` for `kinds` (cookies
/// and the HTTP cache are cleared separately). Empty when nothing applies.
pub(crate) fn storage_types(kinds: WebsiteDataKinds) -> String {
    let mut out = Vec::new();
    if kinds.contains(WebsiteDataKinds::LOCAL_STORAGE) {
        out.push("local_storage");
    }
    if kinds.contains(WebsiteDataKinds::INDEXEDDB) {
        out.extend(["indexeddb", "websql", "file_systems"]);
    }
    if kinds.contains(WebsiteDataKinds::SERVICE_WORKERS) {
        out.push("service_workers");
    }
    if kinds.contains(WebsiteDataKinds::DOM_CACHE) {
        out.push("cache_storage");
    }
    out.join(",")
}

/// `COREWEBVIEW2_COOKIE_SAME_SITE_KIND` (0 None, 1 Lax, 2 Strict).
pub(crate) fn same_site(kind: i32) -> SameSite {
    match kind {
        0 => SameSite::None,
        2 => SameSite::Strict,
        _ => SameSite::Lax,
    }
}

pub(crate) fn same_site_kind(s: SameSite) -> i32 {
    match s {
        SameSite::None => 0,
        SameSite::Lax => 1,
        SameSite::Strict => 2,
    }
}

/// A WebView2 cookie's `Expires` (seconds since the epoch, `-1` for a
/// session cookie) → `Cookie::expires`.
pub(crate) fn expires_from(seconds: f64, session: bool) -> Option<i64> {
    (!session && seconds.is_finite() && seconds >= 0.0).then(|| seconds.floor() as i64)
}

/// `Cookie::expires` → WebView2's `Expires`.
pub(crate) fn expires_to(expires: Option<i64>) -> f64 {
    expires.map_or(-1.0, |s| s as f64)
}

/// Check a cookie before handing it to WebView2.
pub(crate) fn validate_cookie(c: &Cookie) -> Result<(), EngineError> {
    if c.name.is_empty() || c.domain.trim_start_matches('.').is_empty() {
        return Err(EngineError::InvalidArgument(
            "a cookie needs a name and a domain".into(),
        ));
    }
    if c.same_site == SameSite::None && !c.secure {
        return Err(EngineError::InvalidArgument(
            "SameSite=None cookies must be secure".into(),
        ));
    }
    Ok(())
}

/// A download's `Progress` fraction.
pub(crate) fn fraction(received: u64, total: Option<u64>) -> f64 {
    match total {
        Some(t) if t > 0 => (received as f64 / t as f64).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// Download ids are process-unique (`DownloadId` wraps a `u64`).
pub(crate) fn new_download_id() -> DownloadId {
    DownloadId(crate::util::next_id())
}

#[cfg(test)]
mod tests {
    #[test]
    fn knocks_only_where_chromium_would_connect() {
        use super::knock_target;
        assert_eq!(
            knock_target("http://127.0.0.1:59321/"),
            Some(("127.0.0.1".into(), 59321))
        );
        assert_eq!(
            knock_target("https://example.com/a"),
            Some(("example.com".into(), 443))
        );
        assert_eq!(
            knock_target("http://[::1]:8080/"),
            Some(("::1".into(), 8080))
        );
        assert_eq!(knock_target("http://127.0.0.1:9/"), None);
        assert_eq!(knock_target("file:///C:/a.html"), None);
        assert_eq!(knock_target("copper://newtab"), None);
    }

    #[test]
    fn tells_a_refusal() {
        use std::time::Duration;
        let open = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = open.local_addr().unwrap().port();
        assert!(!super::refuses("127.0.0.1", port, Duration::from_secs(2)));
        drop(open);
        assert!(super::refuses("127.0.0.1", port, Duration::from_secs(2)));
        assert!(!super::refuses(
            "no-such-host.invalid",
            80,
            Duration::from_secs(2)
        ));
    }

    use super::*;

    #[test]
    fn errors_and_permissions() {
        assert_eq!(web_error_name(13), "host name not resolved");
        assert_eq!(web_error_name(WEB_ERROR_CANCELED), "cancelled");
        assert_eq!(web_error_name(99), "unknown error");
        assert_eq!(permission_kind(2), PermissionKind::Camera);
        assert_eq!(permission_kind(6), PermissionKind::Clipboard);
        assert_eq!(
            crash_reason(Some(5), false),
            CrashReason::ExceededMemoryLimit
        );
        assert_eq!(crash_reason(Some(3), true), CrashReason::TerminatedByApi);
        assert_eq!(crash_reason(None, false), CrashReason::Crashed);
        assert_eq!(download_failure(26), ("cancelled".into(), true));
        assert!(!download_failure(12).1);
    }

    #[test]
    fn data_kinds() {
        assert_eq!(browsing_data_kinds(WebsiteDataKinds::COOKIES), 64);
        assert_eq!(browsing_data_kinds(WebsiteDataKinds::CACHES), 256);
        assert_eq!(browsing_data_kinds(WebsiteDataKinds::SESSION_STORAGE), 0);
        let all = browsing_data_kinds(WebsiteDataKinds::ALL);
        assert_eq!(all, 1 | 2 | 4 | 8 | 16 | 64 | 256 | 32768);
        assert_eq!(storage_types(WebsiteDataKinds::COOKIES), "");
        assert_eq!(
            storage_types(WebsiteDataKinds::LOCAL_STORAGE | WebsiteDataKinds::DOM_CACHE),
            "local_storage,cache_storage"
        );
    }

    #[test]
    fn cookies() {
        assert_eq!(expires_from(-1.0, true), None);
        assert_eq!(expires_from(1700000000.5, false), Some(1700000000));
        assert_eq!(expires_to(None), -1.0);
        assert_eq!(
            same_site(same_site_kind(SameSite::Strict)),
            SameSite::Strict
        );
        let mut c = Cookie::new("a", "b", ".example.com");
        assert!(validate_cookie(&c).is_ok());
        c.same_site = SameSite::None;
        assert!(validate_cookie(&c).is_err());
        c.secure = true;
        assert!(validate_cookie(&Cookie::new("", "b", "x")).is_err());
        assert_eq!(fraction(5, Some(10)), 0.5);
        assert_eq!(fraction(5, None), 0.0);
    }
}
