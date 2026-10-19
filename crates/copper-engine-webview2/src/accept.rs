//! A file input's `accept` attribute as the file-type filter of the
//! Windows file dialog (what Chromium's own dialog offers): one entry with
//! every accepted pattern, then "All files". Platform-free.

/// One filter of the dialog's "file type" box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileType {
    pub name: String,
    /// `*.png;*.jpg` (Windows' pattern-list syntax).
    pub patterns: String,
}

/// Extensions for the MIME type families an `accept` may name.
const FAMILIES: &[(&str, &[&str])] = &[
    (
        "image/",
        &[
            "png", "jpg", "jpeg", "jfif", "pjpeg", "pjp", "gif", "webp", "bmp", "ico", "svg",
            "svgz", "tif", "tiff", "avif", "heic", "heif", "apng",
        ],
    ),
    (
        "audio/",
        &[
            "mp3", "wav", "ogg", "oga", "opus", "flac", "m4a", "aac", "weba", "wma", "mid", "midi",
        ],
    ),
    (
        "video/",
        &[
            "mp4", "m4v", "webm", "ogv", "mov", "avi", "wmv", "mkv", "mpeg", "mpg", "3gp",
        ],
    ),
    (
        "text/",
        &[
            "txt", "text", "csv", "tsv", "html", "htm", "css", "js", "md", "xml", "ics", "vtt",
        ],
    ),
];

/// Extensions for exact MIME types.
const TYPES: &[(&str, &[&str])] = &[
    ("application/pdf", &["pdf"]),
    ("application/json", &["json"]),
    ("application/xml", &["xml"]),
    ("application/zip", &["zip"]),
    ("application/x-zip-compressed", &["zip"]),
    ("application/gzip", &["gz"]),
    ("application/x-7z-compressed", &["7z"]),
    ("application/x-tar", &["tar"]),
    ("application/rtf", &["rtf"]),
    ("application/msword", &["doc"]),
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        &["docx"],
    ),
    ("application/vnd.ms-excel", &["xls"]),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &["xlsx"],
    ),
    ("application/vnd.ms-powerpoint", &["ppt"]),
    (
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        &["pptx"],
    ),
    ("application/vnd.oasis.opendocument.text", &["odt"]),
    ("application/vnd.oasis.opendocument.spreadsheet", &["ods"]),
    ("application/epub+zip", &["epub"]),
    ("application/wasm", &["wasm"]),
    ("application/javascript", &["js", "mjs"]),
    ("image/png", &["png"]),
    ("image/jpeg", &["jpg", "jpeg", "jfif", "pjpeg", "pjp"]),
    ("image/gif", &["gif"]),
    ("image/webp", &["webp"]),
    ("image/svg+xml", &["svg", "svgz"]),
    ("image/bmp", &["bmp"]),
    ("image/avif", &["avif"]),
    ("image/heic", &["heic"]),
    ("image/tiff", &["tif", "tiff"]),
    ("image/x-icon", &["ico"]),
    ("audio/mpeg", &["mp3"]),
    ("audio/wav", &["wav"]),
    ("audio/ogg", &["ogg", "oga", "opus"]),
    ("audio/flac", &["flac"]),
    ("video/mp4", &["mp4", "m4v"]),
    ("video/webm", &["webm"]),
    ("video/quicktime", &["mov"]),
    ("text/plain", &["txt", "text"]),
    ("text/csv", &["csv"]),
    ("text/html", &["html", "htm"]),
    ("text/css", &["css"]),
    ("text/javascript", &["js", "mjs"]),
    ("text/markdown", &["md", "markdown"]),
    ("text/calendar", &["ics"]),
];

/// The dialog's filters for `accept` (empty: no filter, every file shows).
/// Entries the dialog cannot express (unknown MIME types) are skipped; if
/// nothing is left, there is no filter either.
pub(crate) fn file_types(accept: &str) -> Vec<FileType> {
    let mut exts: Vec<String> = Vec::new();
    let mut add = |e: &str| {
        let e = e.to_ascii_lowercase();
        if !exts.contains(&e) {
            exts.push(e);
        }
    };
    for token in accept.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let token = token.to_ascii_lowercase();
        if let Some(ext) = token.strip_prefix('.') {
            if !ext.is_empty()
                && ext
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                add(ext);
            }
        } else if let Some(family) = token.strip_suffix('*') {
            if let Some((_, list)) = FAMILIES.iter().find(|(f, _)| *f == family) {
                list.iter().for_each(|e| add(e));
            }
        } else {
            let mime = token.split(';').next().unwrap_or("").trim();
            if let Some((_, list)) = TYPES.iter().find(|(t, _)| *t == mime) {
                list.iter().for_each(|e| add(e));
            }
        }
    }
    if exts.is_empty() {
        return Vec::new();
    }
    let patterns = exts
        .iter()
        .map(|e| format!("*.{e}"))
        .collect::<Vec<_>>()
        .join(";");
    vec![
        FileType {
            name: "Accepted files".into(),
            patterns,
        },
        FileType {
            name: "All files".into(),
            patterns: "*.*".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_accept_no_filter() {
        assert!(file_types("").is_empty());
        assert!(file_types(" , ").is_empty());
        assert!(file_types("application/x-unknown").is_empty());
    }

    #[test]
    fn extensions_families_and_types() {
        let t = file_types(".TXT, image/*, application/pdf, .txt, text/plain;charset=utf-8");
        assert_eq!(t.len(), 2);
        assert!(t[0].patterns.starts_with("*.txt;*.png;*.jpg;"));
        assert!(t[0].patterns.ends_with(";*.pdf;*.text"));
        assert_eq!(t[0].patterns.matches("*.txt").count(), 1);
        assert_eq!(t[1].patterns, "*.*");
    }

    #[test]
    fn junk_extensions_are_skipped() {
        assert!(file_types(".;.a b;.*").is_empty());
        assert_eq!(file_types(".tar-gz")[0].patterns, "*.tar-gz");
    }
}
