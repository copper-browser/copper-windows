//! The navigation filter's request (`WebView2Page::set_navigation_filter`)
//! and the bookkeeping that keeps a refused navigation out of the page's
//! load events. Platform-free.

use std::collections::VecDeque;

/// A main-frame navigation about to start, for the page's navigation
/// filter to allow or refuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationRequest {
    pub url: String,
    /// Started by the user (a click, typing, a form) rather than by script
    /// or a redirect.
    pub user_initiated: bool,
    /// A server redirect of a navigation already under way (its
    /// `LoadState::Started` is out).
    pub redirected: bool,
}

/// How many refused navigations are remembered at once. Their completions
/// follow at once, so a handful is plenty; the oldest is forgotten first.
const REMEMBERED: usize = 32;

/// The ids of navigations the filter refused before they started, whose
/// (cancelled) completion must not reach the page's events: no
/// `LoadState::Started` was sent for them, so no `Failed` or `Finished`
/// may follow.
#[derive(Debug, Default)]
pub(crate) struct Refused {
    ids: VecDeque<u64>,
}

impl Refused {
    /// Navigation `id` was refused.
    pub fn add(&mut self, id: u64) {
        if self.ids.len() == REMEMBERED {
            self.ids.pop_front();
        }
        self.ids.push_back(id);
    }

    /// Whether navigation `id` was refused (forgetting it: each completes
    /// once).
    pub fn take(&mut self, id: u64) -> bool {
        match self.ids.iter().position(|&i| i == id) {
            Some(at) => {
                self.ids.remove(at);
                true
            }
            None => false,
        }
    }

    /// Forget them all (the webview they belong to is gone).
    pub fn clear(&mut self) {
        self.ids.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_ids_complete_once() {
        let mut r = Refused::default();
        r.add(7);
        r.add(9);
        assert!(r.take(9));
        assert!(!r.take(9));
        assert!(!r.take(8));
        assert!(r.take(7));
    }

    #[test]
    fn oldest_forgotten() {
        let mut r = Refused::default();
        for id in 0..(REMEMBERED as u64 + 1) {
            r.add(id);
        }
        assert!(!r.take(0));
        assert!(r.take(1));
        assert!(r.take(REMEMBERED as u64));
        r.add(5);
        r.clear();
        assert!(!r.take(5));
    }
}
