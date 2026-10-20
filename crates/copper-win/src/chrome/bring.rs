//! Bringing bookmarks over from another browser on this PC (`Import.swift`
//! `Chromium.installed` / `FlowBookmarks.bookmarks(in:)`), for the welcome.
//!
//! Every Chromium browser keeps its bookmarks as JSON in each profile's
//! `Bookmarks` file under `%LOCALAPPDATA%`. The bar's contents come in at the
//! top level, "Other" and "Mobile" as folders after them; a site already
//! kept is not kept twice, and a folder whose title is already a folder at
//! the top level is filled in rather than doubled. The other browser's
//! files are only read.
//!
//! Passwords and history are not read here: Chrome and Edge seal their
//! passwords to themselves on Windows (CSV export is the way in, from
//! Settings › Passwords), and their history is a database file.

use copper_core::bookmarks::{Bookmark, Bookmarks};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// A browser whose bookmarks can be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub name: &'static str,
    /// Its `User Data` folder.
    pub root: PathBuf,
}

/// The Mac's order, with Windows' folders (Dia has no Windows build).
const KNOWN: &[(&str, &str)] = &[
    ("Chrome", r"Google\Chrome\User Data"),
    (
        "Arc",
        r"Packages\TheBrowserCompany.Arc_ttt7gaw1vtxp6\LocalCache\Local\Arc\User Data",
    ),
    ("Brave", r"BraveSoftware\Brave-Browser\User Data"),
    ("Edge", r"Microsoft\Edge\User Data"),
    ("Vivaldi", r"Vivaldi\User Data"),
    ("Chromium", r"Chromium\User Data"),
];

/// The browsers on this PC with at least one bookmarks file.
pub fn installed() -> Vec<Source> {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return Vec::new();
    };
    KNOWN
        .iter()
        .map(|(name, dir)| Source {
            name,
            root: local.join(dir),
        })
        .filter(|s| !files(&s.root).is_empty())
        .collect()
}

/// Every profile's bookmarks files (`Default`, `Profile 1`, …).
fn files(root: &Path) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut profiles: Vec<PathBuf> = dir
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n == "Default" || n.starts_with("Profile ")
        })
        .map(|e| e.path())
        .collect();
    profiles.sort();
    profiles
        .iter()
        .flat_map(|p| ["AccountBookmarks", "Bookmarks"].map(|f| p.join(f)))
        .filter(|f| f.is_file())
        .collect()
}

/// `source`'s bookmarks, as they would be filed: the bar's, then "Other"
/// and "Mobile" folders.
pub fn read(source: &Source) -> Vec<Bookmark> {
    let mut bar = Vec::new();
    let mut other = Vec::new();
    let mut mobile = Vec::new();
    for file in files(&source.root) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Some((b, o, m)) = parse(&text) else {
            tracing::warn!(file = %file.display(), "bookmarks file not readable");
            continue;
        };
        merge(b, &mut bar);
        merge(o, &mut other);
        merge(m, &mut mobile);
    }
    if !other.is_empty() {
        bar.push(Bookmark::folder("Other", other));
    }
    if !mobile.is_empty() {
        bar.push(Bookmark::folder("Mobile", mobile));
    }
    bar
}

/// One Chromium `Bookmarks` file: the bar's, "other"'s and "synced"'s
/// nodes.
fn parse(text: &str) -> Option<(Vec<Bookmark>, Vec<Bookmark>, Vec<Bookmark>)> {
    let top: Value = serde_json::from_str(text).ok()?;
    let roots = top.get("roots")?.as_object()?;
    let kids = |key: &str| -> Vec<Bookmark> {
        roots
            .get(key)
            .and_then(|r| r.get("children"))
            .and_then(Value::as_array)
            .map(|a| nodes(a))
            .unwrap_or_default()
    };
    Some((kids("bookmark_bar"), kids("other"), kids("synced")))
}

fn nodes(raw: &[Value]) -> Vec<Bookmark> {
    let mut out = Vec::new();
    for entry in raw {
        let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
        match entry.get("type").and_then(Value::as_str) {
            Some("url") => {
                let Some(url) = entry.get("url").and_then(Value::as_str) else {
                    continue;
                };
                if url.starts_with("http://") || url.starts_with("https://") {
                    out.push(Bookmark::site(name, url));
                }
            }
            Some("folder") => {
                let kids = entry
                    .get("children")
                    .and_then(Value::as_array)
                    .map(|a| nodes(a))
                    .unwrap_or_default();
                out.push(Bookmark::folder(name, kids));
            }
            _ => {}
        }
    }
    out
}

/// `found` into `into`: sites not already there, folders filled in by
/// title.
fn merge(found: Vec<Bookmark>, into: &mut Vec<Bookmark>) {
    for node in found {
        if node.is_folder() {
            let kids = node.children.clone().unwrap_or_default();
            if let Some(f) = into
                .iter_mut()
                .find(|b| b.is_folder() && b.title == node.title)
            {
                let mut theirs = f.children.take().unwrap_or_default();
                merge(kids, &mut theirs);
                f.children = Some(theirs);
            } else {
                into.push(node);
            }
        } else if !into.iter().any(|b| b.url == node.url) && !holds(into, node.url.as_deref()) {
            into.push(node);
        }
    }
}

/// Whether any site in `nodes` (folders opened) has `url`.
fn holds(nodes: &[Bookmark], url: Option<&str>) -> bool {
    nodes.iter().any(|b| {
        if b.is_folder() {
            holds(b.kids(), url)
        } else {
            b.url.as_deref() == url
        }
    })
}

/// How many sites, folders opened.
pub fn count(nodes: &[Bookmark]) -> usize {
    nodes
        .iter()
        .map(|b| if b.is_folder() { count(b.kids()) } else { 1 })
        .sum()
}

/// File `found` into `kept`; the number of sites newly kept.
pub fn take(found: Vec<Bookmark>, kept: &mut Bookmarks) -> usize {
    let mut roots = kept.roots().to_vec();
    let before = count(&roots);
    merge(found, &mut roots);
    let after = count(&roots);
    if after != before {
        kept.replace(roots);
    }
    after - before
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{"roots":{
        "bookmark_bar":{"children":[
            {"type":"url","name":"Wikipedia","url":"https://en.wikipedia.org/"},
            {"type":"folder","name":"Work","children":[
                {"type":"url","name":"Docs","url":"https://docs.example.com/"},
                {"type":"url","name":"Bad","url":"javascript:alert(1)"}]}]},
        "other":{"children":[{"type":"url","name":"Rust","url":"https://www.rust-lang.org/"}]},
        "synced":{"children":[]}}}"#;

    #[test]
    fn reads_a_chromium_file() {
        let (bar, other, mobile) = parse(FILE).unwrap();
        assert_eq!(bar.len(), 2);
        assert_eq!(bar[1].kids().len(), 1, "javascript: is not kept");
        assert_eq!(other.len(), 1);
        assert!(mobile.is_empty());
        assert_eq!(count(&bar), 2);
    }

    #[test]
    fn taking_twice_keeps_once() {
        let (bar, _, _) = parse(FILE).unwrap();
        let mut kept = Bookmarks::new(Vec::new());
        assert_eq!(take(bar.clone(), &mut kept), 2);
        assert_eq!(take(bar, &mut kept), 0);
        assert_eq!(count(kept.roots()), 2);
        assert_eq!(kept.roots().iter().filter(|b| b.is_folder()).count(), 1);
    }
}
