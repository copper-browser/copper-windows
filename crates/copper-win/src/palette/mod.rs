//! The cards and the one field: what `Ctrl+K`, `Ctrl+T` and `Ctrl+L` offer
//! for what was typed, and what taking a row does; and the Spaces doors
//! around the column (the header, the chips, their menus, the delete
//! question).
//!
//! [`rank`] and [`symbols`] are plain logic, tested everywhere; the rest
//! talks to the window and is Windows only. The card itself is
//! [`crate::chrome::card`].

pub mod rank;
pub mod symbols;

#[cfg(windows)]
pub mod emoji;
#[cfg(windows)]
mod live;
#[cfg(windows)]
pub mod spaces;
#[cfg(windows)]
mod strip_edit;

#[cfg(windows)]
pub use live::*;
#[cfg(windows)]
pub use strip_edit::strip_field;
