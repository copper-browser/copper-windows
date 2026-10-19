//! The browser's command line, before `copper_shell_core::Options` sees it.
//!
//! Windows adds two spellings the shared parser does not know (and would
//! reject as unknown flags):
//!
//! * `--single-argument <url>`: how the installer registers Copper for
//!   `http`, `https` and `.html` (`"copper.exe" --single-argument %1`).
//!   Everything after it is one address, spaces and all, never flags, so a
//!   link can never smuggle an option in.
//! * `--launch`: the CLI's "start Copper if it is not running" flag; alone
//!   it is an ordinary launch.

use std::path::Path;

/// The command line split into what the shared parser takes and the
/// addresses Windows handed over whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    /// For `Options::parse`.
    pub options: Vec<String>,
    /// From `--single-argument`: one address, taken as is.
    pub single: Option<String>,
}

/// Split `args` (without the program name).
pub fn split(args: &[String]) -> Launch {
    let mut out = Launch::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--single-argument" => {
                let rest: Vec<&str> = it.by_ref().map(String::as_str).collect();
                let joined = rest.join(" ");
                if !joined.trim().is_empty() {
                    out.single = Some(joined);
                }
                break;
            }
            "--launch" => {}
            _ => out.options.push(a.clone()),
        }
    }
    out
}

/// An argument that is already a whole address (`data:` with spaces
/// included), or a file that exists: its `file:` URL. `None` leaves the
/// text to the field's rules (`example.com`, words to search for).
pub fn command_line_url(arg: &str) -> Option<String> {
    if let Ok(u) = url::Url::parse(arg)
        && matches!(
            u.scheme(),
            "http" | "https" | "file" | "data" | "about" | "copper"
        )
    {
        return Some(u.to_string());
    }
    if looks_like_path(arg) {
        let path = Path::new(arg);
        if path.exists() {
            let abs = std::fs::canonicalize(path).ok()?;
            return url::Url::from_file_path(strip_verbatim(&abs))
                .ok()
                .map(|u| u.to_string());
        }
    }
    None
}

/// A drive path (`C:\x`), a UNC path (`\\server\share`), or a relative one
/// (`.\x`, `..\x`, `./x`) — never a bare word, which may be a search.
fn looks_like_path(arg: &str) -> bool {
    let b = arg.as_bytes();
    let drive =
        b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/');
    drive
        || arg.starts_with("\\\\")
        || arg.starts_with('/')
        || arg.starts_with(".\\")
        || arg.starts_with("..\\")
        || arg.starts_with("./")
        || arg.starts_with("../")
}

/// `canonicalize` on Windows answers `\\?\C:\x`; a `file:` URL wants `C:\x`.
fn strip_verbatim(p: &Path) -> std::path::PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return std::path::PathBuf::from(format!(r"\\{rest}"));
    }
    match s.strip_prefix(r"\\?\") {
        Some(rest) => std::path::PathBuf::from(rest),
        None => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_argument_takes_the_rest_whole() {
        let l = split(&v(&[
            "--single-argument",
            "https://a.example/x y",
            "--help",
        ]));
        assert_eq!(l.single.as_deref(), Some("https://a.example/x y --help"));
        assert!(l.options.is_empty(), "nothing after it is a flag");
    }

    #[test]
    fn launch_is_dropped_and_the_rest_kept() {
        let l = split(&v(&["--launch", "example.com", "--screenshot", "a.png"]));
        assert_eq!(l.options, v(&["example.com", "--screenshot", "a.png"]));
        assert_eq!(l.single, None);
    }

    #[test]
    fn an_empty_single_argument_opens_nothing() {
        assert_eq!(split(&v(&["--single-argument"])).single, None);
        assert_eq!(split(&v(&["--single-argument", " "])).single, None);
    }

    #[test]
    fn whole_addresses_and_words() {
        assert_eq!(
            command_line_url("https://example.com").as_deref(),
            Some("https://example.com/")
        );
        assert_eq!(
            command_line_url("data:text/html,<b>a b</b>").as_deref(),
            Some("data:text/html,<b>a b</b>")
        );
        assert_eq!(command_line_url("example.com"), None);
        assert_eq!(command_line_url("rust lifetimes"), None);
        assert_eq!(command_line_url(r"C:\no\such\file.html"), None);
    }

    #[test]
    fn paths_are_recognised_by_shape() {
        assert!(looks_like_path(r"C:\x.html"));
        assert!(looks_like_path("c:/x.html"));
        assert!(looks_like_path(r"\\server\share\x.html"));
        assert!(looks_like_path(r".\x.html"));
        assert!(!looks_like_path("x.html"));
        assert!(!looks_like_path("C:"));
    }

    #[test]
    fn verbatim_prefixes_come_off() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\a\b.html")),
            Path::new(r"C:\a\b.html")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\srv\share\b.html")),
            Path::new(r"\\srv\share\b.html")
        );
    }
}
