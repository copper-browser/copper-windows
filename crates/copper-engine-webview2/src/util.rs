//! Small platform-free helpers: ids, file names, hosts, JS literals.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Process-unique ids for downloads, permissions, scripts and style sheets
/// (one counter, like the WebKitGTK engine, so ids never collide across kinds).
pub(crate) fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// 64-bit FNV-1a: a stable, dependency-free hash for names that must not
/// change between runs (profile names, extension versions).
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Whether a website-data record or cookie domain belongs to `domain` (same
/// host, a subdomain of it, or a parent of it), ignoring a leading dot.
pub(crate) fn site_matches(record: &str, domain: &str) -> bool {
    let r = record.trim_start_matches('.').to_ascii_lowercase();
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    if r.is_empty() || d.is_empty() {
        return false;
    }
    r == d || r.ends_with(&format!(".{d}")) || d.ends_with(&format!(".{r}"))
}

/// A file name safe on Windows: no path separators, reserved characters,
/// control characters, trailing dots/spaces or DOS device names.
pub(crate) fn sanitize_filename(suggested: &str) -> String {
    let base = suggested
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    let mut name: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while name.ends_with('.') || name.ends_with(' ') {
        name.pop();
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        name.insert(0, '_');
    }
    if name.is_empty() || name == "." || name == ".." {
        return "download".into();
    }
    name
}

/// `dir/name`, or Mac-style `dir/stem n.ext` (`name 2.pdf`, `name 3.pdf`…,
/// as the Mac's `Browser.free`) for the first `n` from 2 whose path is
/// neither on disk nor reserved by another running download. Only the last
/// extension is kept apart (`a.tar 2.gz`), and a leading dot starts no
/// extension (`.env 2`), as with `NSString.pathExtension`.
pub(crate) fn unique_path(dir: &Path, suggested: &str, reserved: &HashSet<PathBuf>) -> PathBuf {
    let name = sanitize_filename(suggested);
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    let free = |p: &PathBuf| !reserved.contains(p) && std::fs::symlink_metadata(p).is_err();
    let first = dir.join(&name);
    if free(&first) {
        return first;
    }
    (2u32..)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(free)
        .expect("an unused file name exists")
}

/// The name a download was given before Chromium numbered it: Chromium's
/// default target for `a.pdf` is `a (1).pdf` (or `a (2).pdf`…) when `a.pdf`
/// is `taken` in the download folder. That number is dropped, so the
/// engine numbers names its own way ([`unique_path`]) and the Save As
/// dialog offers the page's name. A name whose unnumbered form is free
/// keeps its number (the page named it so).
pub(crate) fn unnumbered(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let Some(open) = name.rfind(" (") else {
        return name.to_owned();
    };
    let rest = &name[open + 2..];
    let Some(close) = rest.find(')') else {
        return name.to_owned();
    };
    let (digits, after) = (&rest[..close], &rest[close + 1..]);
    let numbered = open > 0
        && !digits.is_empty()
        && digits.len() <= 6
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (after.is_empty() || after.starts_with('.'));
    let plain = format!("{}{after}", &name[..open]);
    if numbered && taken(&plain) {
        plain
    } else {
        name.to_owned()
    }
}

/// The extension of file `name` when it is a plain one a Save As filter
/// can name (`pdf` of `a.pdf`, `gz` of `a.tar.gz`; none for `.env`, `a.`
/// or `a.b c`).
pub(crate) fn plain_extension(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    let plain = !stem.is_empty()
        && !ext.is_empty()
        && ext.len() <= 16
        && ext.chars().all(|c| c.is_ascii_alphanumeric());
    plain.then_some(ext)
}

/// A plain JavaScript identifier (`call_async` argument names).
pub(crate) fn is_js_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// The host of `url`, if it has one.
pub(crate) fn url_host(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .host_str()
        .map(|h| h.trim_matches(['[', ']']).to_owned())
        .filter(|h| !h.is_empty())
}

/// A JSON string literal, which is also a valid JavaScript string literal
/// (`serde_json` escapes U+2028/U+2029 is not required since ES2019).
pub(crate) fn js_string(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

/// A WebView2 profile name for a Copper profile id.
///
/// WebView2 accepts at most 64 characters of `[A-Za-z0-9#@$()+-_~. ]` and
/// compares names case-insensitively, while Copper ids are case-sensitive and
/// up to 128 characters. Ids that are already lowercase and short are used
/// as they are; others get a lowercase prefix plus a hash of the exact id,
/// which keeps distinct ids distinct and maps each id to the same name on
/// every run. Every name is a profile of its own, `default` included
/// (`WV2Profile_default`, not WebView2's default profile; see
/// `profile_home`).
pub(crate) fn webview2_profile_name(id: &str) -> String {
    if id.len() <= 64 && !id.bytes().any(|b| b.is_ascii_uppercase()) {
        return id.to_owned();
    }
    let prefix: String = id.chars().take(40).collect::<String>().to_ascii_lowercase();
    format!("{prefix}~{:016x}", fnv1a64(id.as_bytes()))
}

/// Width and height from a PNG's `IHDR` chunk.
pub(crate) fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if png.len() < 24 || &png[..8] != SIG || &png[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
    Some((w, h))
}

/// Which of two futures finished first.
pub(crate) enum Either<A, B> {
    Left(A),
    Right(B),
}

/// Poll both futures; resolve with whichever finishes first (`a` wins ties).
/// Used to bound waits on WebView2 with a UI-thread timer.
pub(crate) async fn race<A, B>(a: A, b: B) -> Either<A::Output, B::Output>
where
    A: std::future::Future,
    B: std::future::Future,
{
    let mut a = std::pin::pin!(a);
    let mut b = std::pin::pin!(b);
    std::future::poll_fn(move |cx| {
        if let std::task::Poll::Ready(v) = a.as_mut().poll(cx) {
            return std::task::Poll::Ready(Either::Left(v));
        }
        if let std::task::Poll::Ready(v) = b.as_mut().poll(cx) {
            return std::task::Poll::Ready(Either::Right(v));
        }
        std::task::Poll::Pending
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((640, 480)));
        assert_eq!(png_size(b"GIF89a"), None);
    }

    #[test]
    fn races() {
        let out = poll_once(race(std::future::ready(1), std::future::pending::<()>()));
        assert!(matches!(out, Some(Either::Left(1))));
        let out = poll_once(race(std::future::pending::<()>(), std::future::ready(2)));
        assert!(matches!(out, Some(Either::Right(2))));
    }

    fn poll_once<F: std::future::Future>(f: F) -> Option<F::Output> {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => Some(v),
            std::task::Poll::Pending => None,
        }
    }

    #[test]
    fn filenames() {
        assert_eq!(sanitize_filename("a/b\\c.pdf"), "c.pdf");
        assert_eq!(sanitize_filename("re:port?.txt"), "re_port_.txt");
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename("name. . "), "name");
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename(""), "download");
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("cw2-unique-{}", next_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        std::fs::write(dir.join("b"), b"x").unwrap();
        std::fs::write(dir.join(".env"), b"x").unwrap();
        std::fs::write(dir.join("c.tar.gz"), b"x").unwrap();
        let mut reserved = HashSet::new();
        assert_eq!(
            unique_path(&dir, "fresh.pdf", &reserved),
            dir.join("fresh.pdf")
        );
        assert_eq!(unique_path(&dir, "a.txt", &reserved), dir.join("a 2.txt"));
        reserved.insert(dir.join("a 2.txt"));
        assert_eq!(unique_path(&dir, "a.txt", &reserved), dir.join("a 3.txt"));
        std::fs::write(dir.join("a 3.txt"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt", &reserved), dir.join("a 4.txt"));
        assert_eq!(unique_path(&dir, "b", &reserved), dir.join("b 2"));
        assert_eq!(unique_path(&dir, ".env", &reserved), dir.join(".env 2"));
        assert_eq!(
            unique_path(&dir, "c.tar.gz", &reserved),
            dir.join("c.tar 2.gz")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn plain_extensions() {
        assert_eq!(plain_extension("report.pdf"), Some("pdf"));
        assert_eq!(plain_extension("a.tar.gz"), Some("gz"));
        assert_eq!(plain_extension(".env"), None);
        assert_eq!(plain_extension("noext"), None);
        assert_eq!(plain_extension("a."), None);
        assert_eq!(plain_extension("a.b c"), None);
    }

    #[test]
    fn sites_and_hosts() {
        assert!(site_matches(".mail.example.com", "example.com"));
        assert!(site_matches("example.com", "www.example.com"));
        assert!(!site_matches("badexample.com", "example.com"));
        assert_eq!(
            url_host("https://a.b.com:8080/x").as_deref(),
            Some("a.b.com")
        );
        assert_eq!(url_host("http://[::1]/").as_deref(), Some("::1"));
        assert_eq!(url_host("about:blank"), None);
    }

    #[test]
    fn identifiers_and_literals() {
        assert!(is_js_identifier("$a_1"));
        assert!(!is_js_identifier("1a"));
        assert!(!is_js_identifier("a-b"));
        assert_eq!(js_string("a\"b\n"), r#""a\"b\n""#);
    }

    #[test]
    fn profile_names() {
        assert_eq!(webview2_profile_name("default"), "default");
        let a = webview2_profile_name("Work");
        let b = webview2_profile_name("work");
        assert_ne!(a.to_ascii_lowercase(), b.to_ascii_lowercase());
        let long = "x".repeat(128);
        let n = webview2_profile_name(&long);
        assert!(n.len() <= 64, "{n}");
        assert_eq!(n, webview2_profile_name(&long));
        let uuid = webview2_profile_name("6F9619FF-8B86-D011-B42D-00C04FC964FF");
        assert!(uuid.starts_with("6f9619ff-8b86-d011-b42d-00c04fc964ff~"));
        assert!(uuid.len() <= 64);
    }

    #[test]
    fn chromium_numbers_dropped() {
        let taken = |n: &str| ["a.pdf", "c.tar.gz", "notes", "x (1).bin"].contains(&n);
        assert_eq!(unnumbered("a (1).pdf", taken), "a.pdf");
        assert_eq!(unnumbered("a (12).pdf", taken), "a.pdf");
        assert_eq!(unnumbered("c (1).tar.gz", taken), "c.tar.gz");
        assert_eq!(unnumbered("notes (3)", taken), "notes");
        assert_eq!(unnumbered("x (1) (1).bin", taken), "x (1).bin");
        // Free unnumbered names: the page's own name.
        assert_eq!(unnumbered("b (1).pdf", taken), "b (1).pdf");
        // Not Chromium's form.
        assert_eq!(unnumbered("a (one).pdf", taken), "a (one).pdf");
        assert_eq!(unnumbered("a (1)x.pdf", taken), "a (1)x.pdf");
        assert_eq!(unnumbered(" (1).pdf", |_| true), " (1).pdf");
        assert_eq!(unnumbered("a.pdf", taken), "a.pdf");
    }
}
