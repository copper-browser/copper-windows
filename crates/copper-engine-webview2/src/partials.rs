//! Partial files of downloads cut off while their Save As dialog was open.
//!
//! A download that asks where to save (`WebView2Engine::set_download_ask`)
//! is held by a deferral while the dialog is open, but the browser is
//! already writing it: into a `{GUID}.tmp` file (Windows'
//! `CreateTemporaryFileInDir` name) in the profile's default download
//! folder. Saving renames that file and cancelling deletes it; a Copper
//! that is killed with the dialog open does neither, and the browser never
//! hears of the file again. So each open dialog leaves a *mark* — when the
//! download began and in which folder — removed when the dialog closes; a
//! mark found at the next start means Copper died asking, and the `.tmp`
//! files made in that folder around that moment are removed with it.
//! Nothing else is touched: other programs' temporary files have other
//! names or other times.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The extension of a mark file.
pub(crate) const MARK_EXTENSION: &str = "ask";

/// How far a partial file's creation may be from its download's start.
const NEAR: Duration = Duration::from_secs(10);

/// One open Save As dialog: when its download began and where the browser
/// writes it meanwhile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    pub since: SystemTime,
    pub dir: PathBuf,
}

impl Mark {
    fn encode(&self) -> String {
        let ms = self
            .since
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        format!("{ms}\n{}\n", self.dir.display())
    }

    fn decode(text: &str) -> Option<Mark> {
        let mut lines = text.lines();
        let ms: u64 = lines.next()?.trim().parse().ok()?;
        let dir = lines.next()?.trim_end_matches(['\r', '\n']);
        if dir.is_empty() {
            return None;
        }
        Some(Mark {
            since: UNIX_EPOCH + Duration::from_millis(ms),
            dir: PathBuf::from(dir),
        })
    }

    /// Whether a file made at `created` belongs to this mark's download.
    fn near(&self, created: SystemTime) -> bool {
        let gap = match created.duration_since(self.since) {
            Ok(after) => after,
            Err(before) => before.duration(),
        };
        gap <= NEAR
    }

    /// Leave the mark in `marks` as `<id>.ask`; the path to remove it by.
    pub fn leave(&self, marks: &Path, id: u64) -> io::Result<PathBuf> {
        std::fs::create_dir_all(marks)?;
        let path = marks.join(format!("{id}.{MARK_EXTENSION}"));
        std::fs::write(&path, self.encode())?;
        Ok(path)
    }
}

/// Whether `name` is a browser download's partial file: `{GUID}.tmp`
/// (8-4-4-4-12 hexadecimal digits, braces optional, any case).
pub(crate) fn is_partial_name(name: &str) -> bool {
    let Some(stem) = name
        .len()
        .checked_sub(4)
        .filter(|&at| name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".tmp"))
        .map(|at| &name[..at])
    else {
        return false;
    };
    let stem = stem
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(stem);
    let groups: Vec<&str> = stem.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Remove what Copper left asking: for every mark in `marks` older than
/// `before` (marks of this run are kept), the partial files made near its
/// download's start in its folder, then the mark. Returns the files
/// removed. Errors are skipped: a file still open belongs to someone.
pub(crate) fn sweep(marks: &Path, before: SystemTime) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(marks) else {
        return removed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(MARK_EXTENSION) {
            continue;
        }
        let Some(mark) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| Mark::decode(&t))
        else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        if mark.since >= before {
            continue;
        }
        removed.extend(strays(&mark));
        let _ = std::fs::remove_file(&path);
    }
    removed
}

/// Remove `mark`'s partial files.
fn strays(mark: &Mark) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(&mark.dir) else {
        return removed;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_str().is_some_and(is_partial_name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let made = meta.created().or_else(|_| meta.modified());
        if !meta.is_file() || !made.is_ok_and(|t| mark.near(t)) {
            continue;
        }
        let path = entry.path();
        if std::fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUID: &str = "a797474d-25af-440a-8f6f-5db1376e4167";

    #[test]
    fn knows_partial_names() {
        assert!(is_partial_name(&format!("{GUID}.tmp")));
        assert!(is_partial_name(&format!("{{{}}}.TMP", GUID.to_uppercase())));
        assert!(!is_partial_name("a797474d-25af-440a-8f6f-5db1376e416.tmp"));
        assert!(!is_partial_name(&format!("{GUID}.tmp.txt")));
        assert!(!is_partial_name(&format!("{GUID}.crdownload")));
        assert!(!is_partial_name("report.tmp"));
        assert!(!is_partial_name("g797474d-25af-440a-8f6f-5db1376e4167.tmp"));
        assert!(!is_partial_name(".tmp"));
        assert!(!is_partial_name("é.tmp"));
    }

    #[test]
    fn marks_round_trip() {
        let m = Mark {
            since: UNIX_EPOCH + Duration::from_millis(1_760_000_000_123),
            dir: PathBuf::from("C:\\Users\\a b\\Downloads"),
        };
        assert_eq!(Mark::decode(&m.encode()), Some(m));
        assert_eq!(Mark::decode("x\nC:\\"), None);
        assert_eq!(Mark::decode("12\n"), None);
    }

    #[test]
    fn near_is_both_ways() {
        let now = SystemTime::now();
        let m = Mark {
            since: now,
            dir: PathBuf::new(),
        };
        assert!(m.near(now + Duration::from_secs(3)));
        assert!(m.near(now - Duration::from_secs(3)));
        assert!(!m.near(now + Duration::from_secs(60)));
        assert!(!m.near(now - Duration::from_secs(60)));
    }

    #[test]
    fn sweeps_only_what_a_dead_ask_left() {
        let root = std::env::temp_dir().join(format!(
            "copper-partials-{}-{}",
            std::process::id(),
            crate::util::next_id()
        ));
        let marks = root.join("asks");
        let downloads = root.join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let ours = downloads.join(format!("{GUID}.tmp"));
        let keep = downloads.join("notes.tmp");
        let finished = downloads.join("photo.jpg");
        for f in [&ours, &keep, &finished] {
            std::fs::write(f, b"x").unwrap();
        }
        let start = SystemTime::now();
        let dead = Mark {
            since: start,
            dir: downloads.clone(),
        };
        dead.leave(&marks, 1).unwrap();
        // A mark of this run (later than `before`) stays.
        let live = Mark {
            since: start + Duration::from_secs(5),
            dir: downloads.clone(),
        };
        let live_path = live.leave(&marks, 2).unwrap();
        std::fs::write(marks.join("3.ask"), "garbage").unwrap();

        let removed = sweep(&marks, start + Duration::from_secs(1));
        assert_eq!(removed, vec![ours.clone()]);
        assert!(!ours.exists());
        assert!(keep.exists() && finished.exists());
        assert!(live_path.exists());
        assert!(!marks.join("1.ask").exists());
        assert!(!marks.join("3.ask").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
