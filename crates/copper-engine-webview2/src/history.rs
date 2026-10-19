//! A page's URL stack, kept by Copper rather than read from WebView2.
//!
//! WebView2 can neither export nor restore a back/forward list, so a tab
//! that was slept (its webview closed to free memory) or rebuilt after a
//! renderer crash would come back with no history. Each page therefore keeps
//! its own *logical* stack, fed with CDP `Page.getNavigationHistory` after
//! every history change, and maps logical entries to native entry ids where
//! the webview still has them:
//!
//! - back/forward to an entry the webview still has → native traversal
//!   (`Page.navigateToHistoryEntry`), so bfcache, form state and scroll
//!   restoration work as in Edge;
//! - to an entry it does not have (restored from an earlier webview) → an
//!   ordinary load of its URL, recorded as a traversal so the logical stack
//!   keeps its order.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// One back/forward entry, as the shell stores it for a slept tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

/// An entry of CDP `Page.getNavigationHistory`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct NativeEntry {
    pub id: i64,
    pub url: String,
    #[serde(default)]
    pub title: String,
}

/// CDP `Page.getNavigationHistory`'s result.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeHistory {
    pub current_index: usize,
    pub entries: Vec<NativeEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    url: String,
    title: Option<String>,
    /// The webview's entry id, while it has this entry.
    native: Option<i64>,
}

/// How to reach a history entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// `Page.navigateToHistoryEntry { entryId }`.
    Native(i64),
    /// Load this URL (the webview never had the entry).
    Load(String),
}

/// The logical back/forward list of one page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NavStack {
    slots: Vec<Slot>,
    index: usize,
    /// A load started by [`NavStack::go`] or [`NavStack::restore`] for this
    /// logical index; its commit lands there instead of being pushed.
    pending: Option<usize>,
    /// The webview's current entry id at the last observation.
    native_current: Option<i64>,
}

impl NavStack {
    /// A stack rebuilt from stored entries. Returns the URL to load (the
    /// current entry), or `None` for an empty list.
    pub fn restore(entries: Vec<HistoryEntry>, index: usize) -> (NavStack, Option<String>) {
        let entries: Vec<HistoryEntry> =
            entries.into_iter().filter(|e| !e.url.is_empty()).collect();
        if entries.is_empty() {
            return (NavStack::default(), None);
        }
        let index = index.min(entries.len() - 1);
        let url = entries[index].url.clone();
        let slots = entries
            .into_iter()
            .map(|e| Slot {
                url: e.url,
                title: e.title,
                native: None,
            })
            .collect();
        (
            NavStack {
                slots,
                index,
                pending: Some(index),
                native_current: None,
            },
            Some(url),
        )
    }

    /// The entries and the index of the current one.
    pub fn entries(&self) -> (Vec<HistoryEntry>, usize) {
        let entries = self
            .slots
            .iter()
            .map(|s| HistoryEntry {
                url: s.url.clone(),
                title: s.title.clone(),
            })
            .collect();
        (entries, self.index)
    }

    fn target(&self, delta: isize) -> Option<usize> {
        if self.slots.is_empty() || delta == 0 {
            return None;
        }
        let t = self.index.checked_add_signed(delta)?;
        (t < self.slots.len()).then_some(t)
    }

    pub fn can_go(&self, delta: isize) -> bool {
        self.target(delta).is_some()
    }

    /// How to go `delta` entries back (negative) or forward. A URL load is
    /// remembered as a pending traversal so its commit lands on that entry.
    pub fn go(&mut self, delta: isize) -> Option<Step> {
        let t = self.target(delta)?;
        let slot = &self.slots[t];
        Some(match slot.native {
            Some(id) => Step::Native(id),
            None => {
                self.pending = Some(t);
                Step::Load(slot.url.clone())
            }
        })
    }

    /// Forget a pending traversal (the load it started failed or was
    /// replaced by another navigation the caller started).
    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }

    /// The webview was closed (slept or crashed): every native id is gone,
    /// and the next document is this stack's current entry again.
    pub fn detach_native(&mut self) {
        for s in &mut self.slots {
            s.native = None;
        }
        self.native_current = None;
        if !self.slots.is_empty() {
            self.pending = Some(self.index);
        }
    }

    /// The URL to load when the webview is rebuilt.
    pub fn current_url(&self) -> Option<&str> {
        self.slots.get(self.index).map(|s| s.url.as_str())
    }

    /// Fold in the webview's own history. Returns whether anything changed.
    pub fn observe(&mut self, native: &NativeHistory) -> bool {
        let Some(cur) = native.entries.get(native.current_index) else {
            return false;
        };
        let before = self.entries();
        let ids: HashSet<i64> = native.entries.iter().map(|e| e.id).collect();
        for s in &mut self.slots {
            if s.native.is_some_and(|id| !ids.contains(&id)) {
                s.native = None;
            }
        }
        let title = (!cur.title.is_empty()).then(|| cur.title.clone());
        let fresh = Slot {
            url: cur.url.clone(),
            title,
            native: Some(cur.id),
        };
        let known = self.slots.iter().position(|s| s.native == Some(cur.id));
        match (known, self.pending.take()) {
            (Some(k), _) => {
                // A traversal (ours or the page's) or a same-entry change
                // (redirect, replaceState, title).
                self.index = k;
                self.slots[k] = fresh;
            }
            (None, Some(t)) if t < self.slots.len() => {
                self.index = t;
                self.slots[t] = fresh;
            }
            (None, _) if self.slots.is_empty() => {
                // The first document of a fresh webview. Its initial
                // about:blank is not history.
                if cur.url != "about:blank" || native.entries.len() > 1 {
                    self.slots.push(fresh);
                    self.index = 0;
                }
            }
            (None, _) => {
                // A new entry. If the entry that was current is gone, it was
                // replaced (location.replace, a cross-document replacement);
                // otherwise the new one was pushed after it.
                let replaced = self.native_current.is_some_and(|prev| !ids.contains(&prev));
                if replaced {
                    self.slots[self.index] = fresh;
                } else {
                    self.slots.truncate(self.index + 1);
                    self.slots.push(fresh);
                    self.index = self.slots.len() - 1;
                }
            }
        }
        self.native_current = Some(cur.id);
        self.entries() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(current: usize, entries: &[(i64, &str)]) -> NativeHistory {
        NativeHistory {
            current_index: current,
            entries: entries
                .iter()
                .map(|(id, url)| NativeEntry {
                    id: *id,
                    url: url.to_string(),
                    title: String::new(),
                })
                .collect(),
        }
    }

    fn urls(s: &NavStack) -> (Vec<String>, usize) {
        let (e, i) = s.entries();
        (e.into_iter().map(|e| e.url).collect(), i)
    }

    #[test]
    fn follows_native_history() {
        let mut s = NavStack::default();
        assert!(!s.observe(&native(0, &[(1, "about:blank")])));
        assert_eq!(s.entries().0.len(), 0);
        assert!(s.observe(&native(0, &[(2, "https://a/")])));
        s.observe(&native(1, &[(2, "https://a/"), (3, "https://b/")]));
        s.observe(&native(
            2,
            &[(2, "https://a/"), (3, "https://b/"), (4, "https://c/")],
        ));
        assert_eq!(
            urls(&s),
            (
                vec![
                    "https://a/".into(),
                    "https://b/".into(),
                    "https://c/".into()
                ],
                2
            )
        );
        assert!(s.can_go(-2) && !s.can_go(1) && !s.can_go(-3));
        // Native back.
        assert_eq!(s.go(-1), Some(Step::Native(3)));
        s.observe(&native(
            1,
            &[(2, "https://a/"), (3, "https://b/"), (4, "https://c/")],
        ));
        assert_eq!(s.entries().1, 1);
        // A new navigation from the middle drops the forward entries.
        s.observe(&native(
            2,
            &[(2, "https://a/"), (3, "https://b/"), (5, "https://d/")],
        ));
        assert_eq!(
            urls(&s),
            (
                vec![
                    "https://a/".into(),
                    "https://b/".into(),
                    "https://d/".into()
                ],
                2
            )
        );
        // location.replace: the current entry is swapped, not pushed.
        s.observe(&native(
            2,
            &[(2, "https://a/"), (3, "https://b/"), (6, "https://e/")],
        ));
        assert_eq!(
            urls(&s),
            (
                vec![
                    "https://a/".into(),
                    "https://b/".into(),
                    "https://e/".into()
                ],
                2
            )
        );
        // replaceState keeps the id and changes the URL.
        s.observe(&native(
            2,
            &[(2, "https://a/"), (3, "https://b/"), (6, "https://e/#x")],
        ));
        assert_eq!(urls(&s).0[2], "https://e/#x");
    }

    #[test]
    fn rebuilt_tabs_keep_their_stack() {
        let stored = vec![
            HistoryEntry {
                url: "https://a/".into(),
                title: Some("A".into()),
            },
            HistoryEntry {
                url: "https://b/".into(),
                title: None,
            },
            HistoryEntry {
                url: "https://c/".into(),
                title: None,
            },
        ];
        let (mut s, url) = NavStack::restore(stored, 1);
        assert_eq!(url.as_deref(), Some("https://b/"));
        // The rebuilt webview commits b as its first entry.
        s.observe(&native(0, &[(10, "https://b/")]));
        assert_eq!(
            urls(&s),
            (
                vec![
                    "https://a/".into(),
                    "https://b/".into(),
                    "https://c/".into()
                ],
                1
            )
        );
        assert!(s.can_go(-1) && s.can_go(1));
        // Back to a: the webview never had it, so it is loaded...
        assert_eq!(s.go(-1), Some(Step::Load("https://a/".into())));
        // ...and its commit (a new native entry after b) lands on index 0.
        s.observe(&native(1, &[(10, "https://b/"), (11, "https://a/")]));
        assert_eq!(
            urls(&s),
            (
                vec![
                    "https://a/".into(),
                    "https://b/".into(),
                    "https://c/".into()
                ],
                0
            )
        );
        assert_eq!(s.entries().0[0].title, None);
        // Forward to b now uses its native entry.
        assert_eq!(s.go(1), Some(Step::Native(10)));
        s.observe(&native(0, &[(10, "https://b/"), (11, "https://a/")]));
        assert_eq!(s.entries().1, 1);
        // A new link from b drops c (and the native a entry is pruned when
        // the webview drops it).
        s.observe(&native(1, &[(10, "https://b/"), (12, "https://x/")]));
        assert_eq!(
            urls(&s),
            (
                vec![
                    "https://a/".into(),
                    "https://b/".into(),
                    "https://x/".into()
                ],
                2
            )
        );
        assert_eq!(s.go(-2), Some(Step::Load("https://a/".into())));
    }

    #[test]
    fn sleeping_detaches_native_ids() {
        let mut s = NavStack::default();
        s.observe(&native(0, &[(1, "https://a/")]));
        s.observe(&native(1, &[(1, "https://a/"), (2, "https://b/")]));
        s.detach_native();
        assert_eq!(s.current_url(), Some("https://b/"));
        s.observe(&native(0, &[(7, "https://b/")]));
        assert_eq!(
            urls(&s),
            (vec!["https://a/".into(), "https://b/".into()], 1)
        );
        assert_eq!(s.go(-1), Some(Step::Load("https://a/".into())));
        s.cancel_pending();
        let (empty, url) = NavStack::restore(vec![], 3);
        assert!(empty.entries().0.is_empty() && url.is_none());
        let (clamped, url) = NavStack::restore(
            vec![HistoryEntry {
                url: "https://z/".into(),
                title: None,
            }],
            9,
        );
        assert_eq!(
            (clamped.entries().1, url.as_deref()),
            (0, Some("https://z/"))
        );
    }
}
