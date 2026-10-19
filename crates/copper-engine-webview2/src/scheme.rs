//! Custom URL schemes (`Engine::register_scheme`): the name rules and the
//! response a handler's answer becomes. Platform-free; the WebView2 side is
//! `win::schemes` (registration) and `win::page::scheme` (serving).

/// Schemes the browser owns: registering one would either be refused by
/// WebView2 or take over the web.
const RESERVED: &[&str] = &[
    "about",
    "blob",
    "chrome",
    "chrome-extension",
    "chrome-untrusted",
    "data",
    "devtools",
    "edge",
    "extension",
    "file",
    "filesystem",
    "ftp",
    "http",
    "https",
    "javascript",
    "view-source",
    "ws",
    "wss",
];

/// `scheme` lowercased if it is one a page may register: RFC 3986's
/// `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`, not one of the browser's.
pub(crate) fn normalize(scheme: &str) -> Result<String, String> {
    let s = scheme.to_ascii_lowercase();
    let mut chars = s.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return Err(format!("not a URL scheme: {scheme:?}"));
    }
    if RESERVED.contains(&s.as_str()) {
        return Err(format!("{s}: is the browser's own scheme"));
    }
    Ok(s)
}

/// The `WebResourceRequested` filter that catches every URL of `scheme`.
pub(crate) fn filter_pattern(scheme: &str) -> String {
    format!("{scheme}:*")
}

/// The scheme of `uri`, lowercased (`None` for a URI without one).
pub(crate) fn scheme_of(uri: &str) -> Option<String> {
    let (scheme, _) = uri.split_once(':')?;
    normalize(scheme).ok()
}

/// The response headers for a body of type `mime` (control characters
/// removed, so a handler cannot inject headers; an empty type is sniffed
/// by the browser as `application/octet-stream`).
pub(crate) fn response_headers(mime: &str) -> String {
    let clean: String = mime.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    let mime = if clean.is_empty() {
        "application/octet-stream"
    } else {
        clean
    };
    // Every answer is made fresh by the handler (canvas state, generated
    // pages): nothing is cached across loads.
    format!("Content-Type: {mime}\r\nCache-Control: no-store")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(normalize("Copper").unwrap(), "copper");
        assert_eq!(normalize("x-my.app+1").unwrap(), "x-my.app+1");
        assert!(normalize("").is_err());
        assert!(normalize("1abc").is_err());
        assert!(normalize("cop per").is_err());
        assert!(normalize("copper:").is_err());
        assert!(normalize("HTTPS").is_err());
        assert!(normalize("file").is_err());
    }

    #[test]
    fn uris() {
        assert_eq!(filter_pattern("copper"), "copper:*");
        assert_eq!(scheme_of("COPPER://canvas/1").as_deref(), Some("copper"));
        assert_eq!(scheme_of("https://example.com").as_deref(), None);
        assert_eq!(scheme_of("no-scheme"), None);
    }

    #[test]
    fn headers() {
        assert_eq!(
            response_headers("text/html; charset=utf-8"),
            "Content-Type: text/html; charset=utf-8\r\nCache-Control: no-store"
        );
        assert_eq!(
            response_headers("text/plain\r\nSet-Cookie: x=1"),
            "Content-Type: text/plainSet-Cookie: x=1\r\nCache-Control: no-store"
        );
        assert!(response_headers(" ").starts_with("Content-Type: application/octet-stream\r\n"));
    }
}
