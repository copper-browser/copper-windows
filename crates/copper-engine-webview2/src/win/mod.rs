//! The WebView2 side: everything that talks to the runtime. UI thread only
//! (`copper_win_rt::init` must have run on the calling thread).

#[macro_use]
pub(crate) mod com;
pub(crate) mod backstage;
pub(crate) mod bitmap;
pub(crate) mod cdp;
pub(crate) mod controller;
pub(crate) mod downloads;
pub(crate) mod engine;
pub(crate) mod env;
pub(crate) mod fetcher;
pub(crate) mod file_dialog;
pub(crate) mod loader;
pub(crate) mod page;
pub(crate) mod paths;
pub(crate) mod profile;
pub(crate) mod save_dialog;
pub(crate) mod schemes;
pub(crate) mod window;
pub(crate) mod worker;
