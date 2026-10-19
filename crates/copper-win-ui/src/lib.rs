//! copper-win-ui: Copper's own native UI toolkit for Windows.
//!
//! Win32 for the window, DirectComposition for layers, Direct2D and
//! DirectWrite for drawing, UI Automation for assistive technology — and the
//! Mac app's design, token for token (see [`tokens`]).
//!
//! The crate is split down the middle:
//!
//! * Platform-free and tested anywhere: [`geom`], [`spring`], [`tokens`],
//!   [`theme`], [`node`], [`tree`], [`layout`], [`scroll`], [`edit`],
//!   [`input`], [`event`], [`menu`], [`chrome`], [`stats`]. The retained tree, flex layout, hit
//!   testing, focus, the text-editing model and all the motion maths live
//!   here.
//! * Windows-only, in [`win`]: the window with its custom frame and Mica,
//!   the composition and drawing backend, IME, clipboard, popups, the
//!   `WebSlot` and UI Automation.
//!
//! The toolkit never depends on the engine (`docs/CONTRACT.md` §2): a page is
//! shown through a [`win::WebSlot`], whose visual the app hands to the
//! engine, and whose input the app forwards.

pub mod a11y;
pub mod chrome;
pub mod edit;
pub mod event;
pub mod geom;
pub mod input;
pub mod layout;
pub mod menu;
pub mod node;
pub mod scroll;
pub mod spring;
pub mod stats;
pub mod theme;
pub mod tokens;
pub mod tree;

#[cfg(windows)]
pub mod win;

pub use event::Event;
pub use geom::{Color, Insets, Point, Rect, Size};
pub use layout::{Abs, Align, Dim, Dir, Justify};
pub use node::{Cursor, Edge, Node, Role, TextAlign, Trim, Weight};
pub use spring::{Motion, Spring};
pub use theme::{Appearance, Swatch, Theme, Token};
pub use tree::{NodeId, Prop, Tree};
