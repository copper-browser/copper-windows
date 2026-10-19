//! The Windows backend: a custom-frame window composed with
//! DirectComposition, drawn with Direct2D and DirectWrite, fed by
//! `copper-win-rt`'s message loop (the only one; nothing here pumps).
//!
//! Start with [`Window::new`], build into [`Ui::content`], then
//! [`Window::show`]. Everything runs on the UI thread except the frame
//! clock, which only posts messages.

mod clipboard;
mod clock;
mod compose;
mod events;
mod frame;
mod gfx;
mod ime;
mod keys;
mod paint;
mod popup;
mod shape;
mod slot;
mod text;
mod ui;
mod uia;
mod window;

pub use frame::Backdrop;
pub use slot::{FocusReason, SlotHost, SlotMouse, SlotPointer, request_frame};
pub use ui::Ui;
pub use window::{Options, Window};

/// Report a failure that the toolkit recovers from on its own.
pub(crate) fn log(message: &str) {
    eprintln!("{message}");
}
