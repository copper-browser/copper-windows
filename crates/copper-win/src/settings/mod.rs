//! Settings (`docs/APP.md` › Settings; `Settings.swift`): a panel with a
//! rail of pages on the left and the chosen page on the right. Each page
//! is a [`Page`] — an id, words, and a [`form::Form`] it draws from what
//! the shell knows; clicks come back to it as actions. General is whole;
//! Privacy, Passwords, Cloud, Agents, Updates and Extensions are registered
//! with what Windows can already do, ready to grow.

pub mod form;
#[cfg(windows)]
mod pages;
#[cfg(windows)]
mod panel;

#[cfg(windows)]
pub use panel::{Page, changed, open, register, showing};
