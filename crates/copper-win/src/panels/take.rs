//! Another browser's bookmarks, taken in (Mac `FlowBookmarks.taking` and
//! `relinquish`): the bar's folders and sites come in at the top level;
//! importing from the same browser again replaces only the roots it
//! brought last time that nobody has touched since; a person's own
//! bookmarks are never touched.
//!
//! Which roots a browser brought is its record, `flow-bookmarks-<name>.json`
//! next to `bookmarks.json` (the Mac's own file name). Moving or removing
//! something under a root relinquishes it: the record forgets that root, so
//! the next import leaves it alone.
//!
//! Pure and linear in the size of the trees, so a big import costs little.
//! (Upstream: belongs in `copper-import` next to `bookmarks::read`; it lives
//! here until it moves.)

use copper_core::bookmarks::Bookmark;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What a take comes to: the new top level, and the roots the source now
/// owns (its record).
#[derive(Debug, Clone, PartialEq)]
pub struct Taken {
    pub roots: Vec<Bookmark>,
    pub imported: Vec<Bookmark>,
}

/// What makes two sites at one level the same bookmark: the address and
/// the title (a different title for the same address is kept apart).
fn site_key(node: &Bookmark) -> (Option<String>, String) {
    (node.url.clone(), node.title.clone())
}

/// Take `nodes` (read `complete`ly, or with some files unreadable) from
/// `source` into `current`, given what the source owned before
/// (`previous`). A complete read (even an empty one) replaces the untouched
/// owned roots; an incomplete one cannot authorise deleting any, and folds
/// folders into the roots it made before instead.
pub fn taking(
    nodes: &[Bookmark],
    complete: bool,
    source: &str,
    current: &[Bookmark],
    previous: &[Bookmark],
) -> Taken {
    let mut previous_by_id: HashMap<&str, Vec<&Bookmark>> = HashMap::new();
    for p in previous {
        previous_by_id.entry(p.id.as_str()).or_default().push(p);
    }
    // Untouched since this source brought it (same id, same contents).
    let owned = |root: &Bookmark| {
        previous_by_id
            .get(root.id.as_str())
            .is_some_and(|v| v.contains(&root))
    };

    let mut roots: Vec<Bookmark> = current
        .iter()
        .filter(|r| !complete || !owned(r))
        .cloned()
        .collect();
    let mut imported: Vec<Bookmark> = Vec::new();
    let mut imported_at: HashMap<String, usize> = HashMap::new();
    if !complete {
        let mut roots_by_id: HashMap<&str, Vec<&Bookmark>> = HashMap::new();
        for r in &roots {
            roots_by_id.entry(r.id.as_str()).or_default().push(r);
        }
        for old in previous {
            if roots_by_id
                .get(old.id.as_str())
                .is_some_and(|v| v.contains(&old))
            {
                imported_at.insert(old.id.clone(), imported.len());
                imported.push(old.clone());
            }
        }
    }

    // An incomplete read folds a folder into the root it made before: the
    // same title, or that title with the source's suffix (and a number).
    // Each candidate root is listed under every title that would find it,
    // in top-level order.
    let mut candidates: HashMap<String, Vec<usize>> = HashMap::new();
    let mut stale: HashSet<usize> = HashSet::new();
    if !complete {
        let suffix = format!(" ({source})");
        let numbered = format!("{suffix} ");
        for (index, root) in roots.iter().enumerate() {
            if !root.is_folder() || !owned(root) {
                continue;
            }
            let title = &root.title;
            let mut keys: HashSet<String> = HashSet::from([title.clone()]);
            if let Some(stem) = title.strip_suffix(&suffix) {
                keys.insert(stem.to_string());
            }
            for (at, _) in title.match_indices(&numbered) {
                keys.insert(title[..at].to_string());
            }
            for key in keys {
                candidates.entry(key).or_default().push(index);
            }
        }
        for list in candidates.values_mut() {
            list.sort_unstable();
        }
    }

    let mut folder_titles: HashSet<String> = roots
        .iter()
        .filter(|r| r.is_folder())
        .map(|r| r.title.clone())
        .collect();
    let mut sites: HashSet<(Option<String>, String)> = roots
        .iter()
        .filter(|r| !r.is_folder())
        .map(site_key)
        .collect();
    let mut next_number: HashMap<String, usize> = HashMap::new();
    for node in nodes {
        let mut node = node.clone();
        if !complete
            && node.is_folder()
            && let Some(index) = candidates
                .get(&node.title)
                .and_then(|l| l.iter().copied().find(|i| !stale.contains(i)))
        {
            let mut children = roots[index].children.take().unwrap_or_default();
            copper_import::bookmarks::merge(
                node.children.take().unwrap_or_default(),
                &mut children,
            );
            roots[index].children = Some(children);
            // A root whose contents changed is no longer the one this
            // source left there, so nothing else folds into it.
            if !owned(&roots[index]) {
                stale.insert(index);
            }
            if let Some(&at) = imported_at.get(&roots[index].id) {
                imported[at] = roots[index].clone();
            }
            continue;
        }
        if node.is_folder() {
            if folder_titles.contains(&node.title) {
                node.title = format!("{} ({source})", node.title);
            }
            // A person's same-named folder is still theirs, even if a past
            // imported folder with that name was moved or edited.
            let base = node.title.clone();
            let mut name = base.clone();
            let mut number = next_number.get(&base).copied().unwrap_or(2);
            while folder_titles.contains(&name) {
                name = format!("{base} {number}");
                number += 1;
            }
            next_number.insert(base, number);
            node.title = name.clone();
            folder_titles.insert(name);
        } else if !sites.insert(site_key(&node)) {
            continue;
        }
        imported_at.insert(node.id.clone(), imported.len());
        imported.push(node.clone());
        roots.push(node);
    }
    Taken { roots, imported }
}

/// The roots among `roots` that hold any of `ids`: a person's change under
/// them relinquishes them (Mac `relinquish(touching:in:)`).
pub fn touched(ids: &[&str], roots: &[Bookmark]) -> HashSet<String> {
    roots
        .iter()
        .filter(|r| ids.iter().any(|id| r.holds(id)))
        .map(|r| r.id.clone())
        .collect()
}

/// The record file of `source` in the data folder `dir`.
pub fn record(dir: &Path, source: &str) -> PathBuf {
    dir.join(format!("flow-bookmarks-{}.json", source.to_lowercase()))
}

/// What a source owned, from its record; nothing if it has none or it
/// does not read.
pub fn owned(file: &Path) -> Vec<Bookmark> {
    std::fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Keep what a source now owns in its record.
pub fn keep(file: &Path, imported: &[Bookmark]) {
    match serde_json::to_vec(imported) {
        Ok(bytes) => {
            if let Err(e) = copper_core::store::write_atomic(file, &bytes, false, false) {
                tracing::warn!(error = %e, "bookmark import record");
            }
        }
        Err(e) => tracing::warn!(error = %e, "bookmark import record"),
    }
}

/// Forget the roots holding `ids` in every source's record under `dir`
/// (`sources` names them): they are the person's now.
pub fn relinquish(dir: &Path, sources: &[String], ids: &[&str], roots: &[Bookmark]) {
    let touched = touched(ids, roots);
    if touched.is_empty() {
        return;
    }
    for name in sources {
        let file = record(dir, name);
        if !file.exists() {
            continue;
        }
        let mut list = owned(&file);
        let before = list.len();
        list.retain(|b| !touched.contains(&b.id));
        if list.len() != before {
            keep(&file, &list);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(t: &str, u: &str) -> Bookmark {
        Bookmark::site(t, u)
    }

    fn folder(t: &str, k: Vec<Bookmark>) -> Bookmark {
        Bookmark::folder(t, k)
    }

    fn titles(n: &[Bookmark]) -> Vec<&str> {
        n.iter().map(|b| b.title.as_str()).collect()
    }

    #[test]
    fn a_first_import_lands_after_the_persons_own() {
        let mine = vec![site("Mine", "https://mine.example/")];
        let read = vec![
            folder("Work", vec![site("A", "https://a.example/")]),
            site("B", "https://b.example/"),
        ];
        let t = taking(&read, true, "Chrome", &mine, &[]);
        assert_eq!(titles(&t.roots), ["Mine", "Work", "B"]);
        assert_eq!(titles(&t.imported), ["Work", "B"]);
    }

    #[test]
    fn a_repeat_import_replaces_only_untouched_owned_roots() {
        let mine = site("Mine", "https://mine.example/");
        let first = taking(
            &[folder("Work", vec![]), site("Old", "https://old.example/")],
            true,
            "Chrome",
            std::slice::from_ref(&mine),
            &[],
        );
        let second = taking(
            &[folder("Work", vec![site("New", "https://new.example/")])],
            true,
            "Chrome",
            &first.roots,
            &first.imported,
        );
        assert_eq!(titles(&second.roots), ["Mine", "Work"]);
        assert_eq!(second.roots[1].kids().len(), 1);
        assert_eq!(titles(&second.imported), ["Work"]);
    }

    #[test]
    fn a_touched_root_stays_and_a_same_named_folder_is_suffixed() {
        let first = taking(&[folder("Work", vec![])], true, "Edge", &[], &[]);
        let mut roots = first.roots.clone();
        // The person put something in it: no longer the one Edge left.
        roots[0].children = Some(vec![site("Mine", "https://mine.example/")]);
        let second = taking(
            &[folder("Work", vec![])],
            true,
            "Edge",
            &roots,
            &first.imported,
        );
        assert_eq!(titles(&second.roots), ["Work", "Work (Edge)"]);
        let third = taking(&[folder("Work", vec![])], true, "Edge", &second.roots, &[]);
        assert_eq!(
            titles(&third.roots),
            ["Work", "Work (Edge)", "Work (Edge) 2"]
        );
    }

    #[test]
    fn an_incomplete_read_deletes_nothing_and_folds_into_its_old_folder() {
        let first = taking(
            &[folder("Bar", vec![site("A", "https://a.example/")])],
            true,
            "Chrome",
            &[],
            &[],
        );
        let t = taking(
            &[folder("Bar", vec![site("B", "https://b.example/")])],
            false,
            "Chrome",
            &first.roots,
            &first.imported,
        );
        assert_eq!(titles(&t.roots), ["Bar"]);
        assert_eq!(titles(t.roots[0].kids()), ["A", "B"]);
        assert_eq!(t.imported.len(), 1);
        assert_eq!(t.imported[0], t.roots[0]);
    }

    #[test]
    fn the_same_site_twice_at_the_top_comes_once() {
        let mine = vec![site("A", "https://a.example/")];
        let t = taking(
            &[
                site("A", "https://a.example/"),
                site("A2", "https://a.example/"),
            ],
            true,
            "Chrome",
            &mine,
            &[],
        );
        assert_eq!(titles(&t.roots), ["A", "A2"]);
    }

    #[test]
    fn touching_names_the_root_holding_it() {
        let inner = site("A", "https://a.example/");
        let id = inner.id.clone();
        let roots = vec![folder("F", vec![inner]), site("B", "https://b.example/")];
        let t = touched(&[&id], &roots);
        assert_eq!(t, HashSet::from([roots[0].id.clone()]));
    }

    #[test]
    fn records_round_trip_and_relinquish_forgets_touched_roots() {
        let dir = std::env::temp_dir().join(format!(
            "copper-take-{}-{}",
            std::process::id(),
            copper_core::ids::new_uuid()
        ));
        let roots = vec![folder("F", vec![]), site("B", "https://b.example/")];
        let file = record(&dir, "Chrome");
        assert!(file.ends_with("flow-bookmarks-chrome.json"));
        keep(&file, &roots);
        assert_eq!(owned(&file), roots);
        relinquish(&dir, &["Chrome".into()], &[&roots[0].id], &roots);
        assert_eq!(owned(&file), vec![roots[1].clone()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
