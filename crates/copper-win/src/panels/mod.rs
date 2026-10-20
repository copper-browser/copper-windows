//! Panels (`docs/APP.md` › Panels): History, Bookmarks, Downloads and the
//! rest float over the page on a plate (`App.swift` `sheet`), one at a
//! time, each opened by a command (`show-<id>`) and closed by its cross,
//! a click on the dimmed window around it, or Escape.
//!
//! A panel implements [`Panel`]; [`Panels::add`] registers it (and its
//! `show-<id>` command). The host builds the plate, lets the panel build
//! its body into it, then asks it to draw what the shell knows — the same
//! read-then-edit split as the rest of the chrome: [`Panel::build`] and
//! the closures a panel passes to `Chrome::edit` never call the shell;
//! [`Panel::refresh`] and [`Panel::click`] run with nothing borrowed.
//!
//! Where a panel sits is its [`Place`]: a centred sheet over a dimmed
//! window (History, Bookmarks, Downloads), undimmed in the top-right
//! corner (Hidden), or hanging from the door that opened it (the bookmarks
//! dropdown, the downloads popover; [`show_from`]).
//!
//! The toolkit tells the app about clicks, not about the pointer resting
//! somewhere; a panel that needs that (Hidden's preview, a bookmark being
//! dragged) asks for a [`Panel::tick`] while it is on show and reads the
//! tree's hovered and pressed nodes.
//!
//! History is the worked example ([`history`]).

// The panels and their chrome (Windows only).
#[cfg(windows)]
pub mod bookmarks;
#[cfg(windows)]
mod door;
#[cfg(windows)]
pub mod downloads;
#[cfg(windows)]
mod files;
#[cfg(windows)]
pub mod hidden;
#[cfg(windows)]
pub mod history;
#[cfg(windows)]
mod host;
#[cfg(windows)]
pub mod plate;

// What they work out, platform-free (and tested on any host).
mod ring;
mod spot;
mod take;
mod when;

#[cfg(windows)]
pub use host::*;

/// Run `f` on a thread of its own and wait (without blocking the UI) for
/// what it returns.
#[cfg(windows)]
pub(crate) async fn off_thread<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    loop {
        match rx.try_recv() {
            Ok(v) => return Some(v),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                copper_win_rt::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return None,
        }
    }
}
