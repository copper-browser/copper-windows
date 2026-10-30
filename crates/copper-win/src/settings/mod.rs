//! Settings (`docs/APP.md` › Settings; `Settings.swift`): a floating panel
//! over the window with a grouped rail of pages and a search field on the
//! left and the chosen page on the right. Each page is a [`Page`] — an id,
//! words, and a [`form::Form`] it draws from what the shell knows; clicks
//! come back to it as actions. Search ([`search`]) finds rows across every
//! page and lands on them.
//!
//! The pieces with no window in them — the description ([`form`]), the
//! greys and sizes ([`look`]), the rail and search index ([`index`]) and
//! the matcher ([`search`]) — build and are tested on any host; the rest is
//! Windows.
//!
//! Two sheets open from here and close Settings first, as on the Mac: the
//! saved-password manager ([`manager`]) and Move in ([`movein`]).

pub mod fill;
pub mod fold;
pub mod form;
pub mod index;
pub mod look;
pub mod search;
#[cfg(any(windows, test))]
pub mod sites;

#[cfg(windows)]
mod controls;
#[cfg(windows)]
mod csv;
#[cfg(windows)]
mod dialog;
#[cfg(windows)]
mod draw;
#[cfg(windows)]
mod frame;
#[cfg(windows)]
mod manager;
#[cfg(windows)]
mod movein;
#[cfg(windows)]
mod pages;
#[cfg(windows)]
mod panel;
#[cfg(windows)]
mod results;

#[cfg(windows)]
pub use panel::{Page, changed, open, open_at, register, showing};

/// `1 password` / `3 passwords`.
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_say_one_and_many() {
        assert_eq!(super::count(1, "site", "sites"), "1 site");
        assert_eq!(super::count(0, "site", "sites"), "0 sites");
    }
}
