//! The checks a download passes before setup trusts its bytes: the answer
//! must be a plain `200 OK`, and the body must be whole. (The signature
//! check that follows is `webview2`'s.)
//!
//! Platform-free so every rule is unit-tested on Linux; `sys::http` applies
//! them to what WinHTTP reports.

/// Check the HTTP status before reading the body. WinHTTP has already
/// followed redirects, so anything but 200 (a 3xx it refused to follow —
/// HTTPS to HTTP —, a 204, an error page) is a failure.
pub fn check_status(status: u32, url: &str) -> Result<(), String> {
    match status {
        200 => Ok(()),
        0 => Err(format!("{url} sent no HTTP status")),
        s => Err(format!("{url} answered HTTP {s}")),
    }
}

/// Check the body once the connection says it is over: something arrived,
/// and as many bytes as `Content-Length` promised (when it said).
pub fn check_complete(done: u64, total: Option<u64>) -> Result<(), String> {
    match total {
        _ if done == 0 => Err("the download was empty".to_owned()),
        Some(total) if done != total => {
            Err(format!("download ended early: {done} of {total} bytes"))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_200_reads_on() {
        assert!(check_status(200, "https://x/").is_ok());
        for s in [0, 201, 204, 206, 301, 302, 304, 403, 404, 500, 503] {
            let e = check_status(s, "https://x/").unwrap_err();
            assert!(e.contains("https://x/"), "{e}");
        }
    }

    #[test]
    fn whole_bodies_only() {
        assert!(check_complete(1_700_000, Some(1_700_000)).is_ok());
        assert!(check_complete(1_700_000, None).is_ok());
        assert!(check_complete(0, None).is_err());
        assert!(check_complete(0, Some(0)).is_err());
        let e = check_complete(1_000, Some(1_700_000)).unwrap_err();
        assert_eq!(e, "download ended early: 1000 of 1700000 bytes");
        // More than promised is as wrong as less.
        assert!(check_complete(1_700_001, Some(1_700_000)).is_err());
    }
}
