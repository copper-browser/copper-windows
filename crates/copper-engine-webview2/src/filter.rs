//! Compiled content filters on disk: one unpacked MV3 extension folder per
//! filter id under `EnginePaths::content_filter_dir()`.
//!
//! ```text
//! <content-filter-dir>/<id>/manifest.json   MV3 manifest (DNR + content scripts)
//!                          /rules.json      static declarativeNetRequest ruleset
//!                          /hide-<n>.css    hide-element style sheets
//!                          /source.json     the WebKit list it was compiled from
//! ```
//!
//! A filter is written to a sibling temporary folder and renamed into place,
//! so a crash mid-write never leaves a half-written extension that a
//! profile would load. Profiles load the folder with
//! `ICoreWebView2Profile7::AddBrowserExtension`; the extension id Chromium
//! derives from the folder path, so the path is stable per filter id.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use copper_engine::{ContentFilter, EngineError, Result};

use crate::dnr;
use crate::util::fnv1a64;

/// A compiled content-blocker list, ready to attach to pages.
///
/// Unlike WebKit, Chromium applies extension rules to a whole profile, not
/// to one page: a filter is active in a profile while any live page of that
/// profile carries it (see `crate` docs). Cheap to clone.
#[derive(Clone)]
pub struct WebView2ContentFilter {
    inner: Rc<FilterInner>,
}

struct FilterInner {
    id: String,
    dir: PathBuf,
    /// Hash of the compiled files; changes when the list is recompiled.
    version: u64,
}

impl WebView2ContentFilter {
    /// The unpacked extension folder.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// Changes whenever the stored list changes (reload detection).
    pub fn version(&self) -> u64 {
        self.inner.version
    }

    /// The extension's display name (`Copper filter: <id>`).
    pub fn extension_name(&self) -> String {
        format!("{}{}", dnr::EXTENSION_NAME_PREFIX, self.inner.id)
    }
}

impl ContentFilter for WebView2ContentFilter {
    fn id(&self) -> &str {
        &self.inner.id
    }
}

impl std::fmt::Debug for WebView2ContentFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WebView2ContentFilter({:?})", self.inner.id)
    }
}

/// Reads and writes compiled filters under one directory.
#[derive(Clone, Debug)]
pub(crate) struct FilterStore {
    root: PathBuf,
}

/// Filter ids become folder names: `[A-Za-z0-9._-]`, not starting with a dot.
fn check_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(EngineError::InvalidArgument(format!(
            "content filter id {id:?} must be 1-128 chars of [A-Za-z0-9._-] and not start with '.'"
        )))
    }
}

/// A filter's version: a hash of the manifest's `version` string, which is
/// itself derived from the compiled files (so stored and fresh agree).
fn version_of(manifest: &serde_json::Value) -> u64 {
    fnv1a64(manifest["version"].as_str().unwrap_or_default().as_bytes())
}

fn io(what: &str, path: &Path, e: std::io::Error) -> EngineError {
    EngineError::ContentFilter(format!("{what} {}: {e}", path.display()))
}

impl FilterStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        FilterStore { root }
    }

    fn folder(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Compile `rules_json` and store it under `id`, replacing any older one.
    pub(crate) fn compile(&self, id: &str, rules_json: &str) -> Result<WebView2ContentFilter> {
        check_id(id)?;
        let compiled = dnr::compile(rules_json)
            .map_err(|e| EngineError::ContentFilter(format!("compile {id:?}: {e}")))?;
        for why in &compiled.skipped {
            tracing::info!("content filter {id}: {why}");
        }
        let rules = serde_json::to_string_pretty(&compiled.rules)
            .map_err(|e| EngineError::ContentFilter(e.to_string()))?;
        let mut hash_input = rules.clone();
        for s in &compiled.styles {
            hash_input.push_str(&s.css);
            hash_input.push_str(&s.matches.join(","));
            hash_input.push_str(&s.exclude.join(","));
        }
        let manifest = compiled.manifest(id, fnv1a64(hash_input.as_bytes()));
        let version = version_of(&manifest);
        let manifest = serde_json::to_string_pretty(&manifest)
            .map_err(|e| EngineError::ContentFilter(e.to_string()))?;

        std::fs::create_dir_all(&self.root).map_err(|e| io("create", &self.root, e))?;
        let tmp = self
            .root
            .join(format!(".{id}.tmp-{}", crate::util::next_id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| io("create", &tmp, e))?;
        let write = |name: &str, body: &str| {
            let p = tmp.join(name);
            std::fs::write(&p, body).map_err(|e| io("write", &p, e))
        };
        let written = (|| {
            write("manifest.json", &manifest)?;
            write("rules.json", &rules)?;
            write("source.json", rules_json)?;
            for (i, s) in compiled.styles.iter().enumerate() {
                write(&format!("hide-{i}.css"), &s.css)?;
            }
            Ok::<(), EngineError>(())
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
        let dest = self.folder(id);
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| io("replace", &dest, e))?;
        }
        std::fs::rename(&tmp, &dest).map_err(|e| {
            let _ = std::fs::remove_dir_all(&tmp);
            io("store", &dest, e)
        })?;
        Ok(self.handle(id, version))
    }

    /// A stored filter, or `None` when `id` was never compiled.
    pub(crate) fn load(&self, id: &str) -> Result<Option<WebView2ContentFilter>> {
        check_id(id)?;
        let dir = self.folder(id);
        let manifest = dir.join("manifest.json");
        let rules = dir.join("rules.json");
        if !manifest.is_file() || !rules.is_file() {
            return Ok(None);
        }
        let m: serde_json::Value = std::fs::read(&manifest)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or_else(|| {
                EngineError::ContentFilter(format!("{} is not valid JSON", manifest.display()))
            })?;
        Ok(Some(self.handle(id, version_of(&m))))
    }

    /// Delete a stored filter; a missing one is not an error.
    pub(crate) fn remove(&self, id: &str) -> Result<()> {
        check_id(id)?;
        let dir = self.folder(id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io("remove", &dir, e)),
        }
    }

    fn handle(&self, id: &str, version: u64) -> WebView2ContentFilter {
        WebView2ContentFilter {
            inner: Rc::new(FilterInner {
                id: id.to_owned(),
                dir: self.folder(id),
                version,
            }),
        }
    }
}

/// What a profile has installed, as the engine last left it: filter id →
/// (Chromium's extension id, the filter's version). Chromium keeps a
/// profile's extensions across runs but tells only their id and name, so
/// this record says which one is still the right build (kept at start
/// instead of being removed and added again, seconds of a launch during
/// which pages load unfiltered).
pub(crate) type Installed = std::collections::BTreeMap<String, (String, u64)>;

/// Read [`Installed`] from `path` (empty if it is missing or unreadable).
pub(crate) fn read_installed(path: &Path) -> Installed {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Installed::new();
    };
    let Ok(map) =
        serde_json::from_str::<std::collections::BTreeMap<String, (String, String)>>(&text)
    else {
        return Installed::new();
    };
    map.into_iter()
        .filter_map(|(id, (ext, v))| Some((id, (ext, u64::from_str_radix(&v, 16).ok()?))))
        .collect()
}

/// Write [`Installed`] to `path` (best effort: a lost record only costs a
/// reinstall).
pub(crate) fn write_installed(path: &Path, installed: &Installed) {
    let map: std::collections::BTreeMap<&String, (&String, String)> = installed
        .iter()
        .map(|(id, (ext, v))| (id, (ext, format!("{v:x}"))))
        .collect();
    let Ok(text) = serde_json::to_string(&map) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn installed_record_round_trips() {
        let dir = std::env::temp_dir().join(format!("cw3-installed-{}", crate::util::next_id()));
        let path = dir.join("installed").join("default.json");
        assert!(super::read_installed(&path).is_empty());
        let mut rec = super::Installed::new();
        rec.insert("shield".into(), ("abcdef".into(), u64::MAX - 7));
        super::write_installed(&path, &rec);
        assert_eq!(super::read_installed(&path), rec);
        std::fs::write(&path, "not json").unwrap();
        assert!(super::read_installed(&path).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    use super::*;

    #[test]
    fn store_round_trip() {
        let root = std::env::temp_dir().join(format!("cw2-filters-{}", crate::util::next_id()));
        let store = FilterStore::new(root.clone());
        let list = copper_shield::List::standard();
        assert!(store.load(&list.id).unwrap().is_none());
        let f = store.compile(&list.id, &list.json).unwrap();
        assert_eq!(f.id(), list.id);
        assert_eq!(f.dir(), root.join(&list.id));
        for file in ["manifest.json", "rules.json", "source.json", "hide-0.css"] {
            assert!(f.dir().join(file).is_file(), "{file}");
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(f.dir().join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["manifest_version"], 3);
        assert_eq!(manifest["name"], f.extension_name());
        let loaded = store.load(&list.id).unwrap().unwrap();
        assert_eq!(loaded.id(), f.id());
        assert_eq!(loaded.version(), f.version());
        // Recompiling the same id replaces it in place.
        let g = store.compile(&list.id, "[]").unwrap();
        assert_ne!(g.version(), f.version());
        assert!(!g.dir().join("hide-0.css").exists());
        // Nothing temporary is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(leftovers.is_empty());
        store.remove(&list.id).unwrap();
        store.remove(&list.id).unwrap();
        assert!(store.load(&list.id).unwrap().is_none());
        assert!(store.compile("../evil", "[]").is_err());
        assert!(store.compile("bad", "{").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
