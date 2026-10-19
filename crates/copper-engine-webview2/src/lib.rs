//! `copper-engine` on Microsoft Edge WebView2.
//!
//! The platform-free modules (always built, unit-tested on every host) hold
//! the logic that does not need WebView2: the content-blocker → DNR
//! compiler, the script shims and chains, the URL stack, CDP builders, key
//! tables and value conversions. The `cfg(windows)` modules drive the
//! runtime.
//!
//! # Windows integration
//! - UI thread only; `copper_win_rt::init()` first. The engine runs every
//!   asynchronous step on `copper_win_rt`'s loop and never pumps messages
//!   itself.
//! - Visible pages are hosted as DirectComposition visuals in the shell's
//!   window (CONTRACT §2; see `WebView2Page::attach`); the engine never makes
//!   a visible window.
//! - The WebView2 loader is linked statically: `copper.exe` needs no
//!   `WebView2Loader.dll`.
#![cfg_attr(not(windows), allow(dead_code))]

pub(crate) mod args;
pub(crate) mod cdp;
pub(crate) mod convert;
pub(crate) mod dnr;
pub(crate) mod filter;
pub(crate) mod history;
pub mod hosting;
pub(crate) mod input_plan;
pub(crate) mod keys;
pub(crate) mod scripts;
pub(crate) mod util;

#[cfg(windows)]
mod win;

pub use filter::WebView2ContentFilter;
pub use history::HistoryEntry;
pub use hosting::{Cover, PageChange};

#[cfg(windows)]
pub use win::backstage::WebView2Backstage;
#[cfg(windows)]
pub use win::engine::{EngineConfig, WebView2Engine};
#[cfg(windows)]
pub use win::page::WebView2Page;
#[cfg(windows)]
pub use win::profile::WebView2Profile;
