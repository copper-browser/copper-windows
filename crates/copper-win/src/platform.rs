//! Copper on Windows as a copper-shell-core platform: the WebView2 engine,
//! copper-win-rt's loop, the copper-win-ui window, and the user as Windows
//! knows them.
//!
//! Everything the shared shell does (tabs, history, sessions, spaces, the
//! agents' server, the lanes) runs over these types; the window module
//! draws what the shell says.

use crate::frame::WinFrame;
use crate::picture::Picture;
use crate::view::WinView;
use copper_engine_webview2::WebView2Engine;
use copper_shell_core::{MainLoop, Platform};
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

/// The Windows platform (`Shell<Win>`).
pub enum Win {}

/// A decoded favicon: shared, so a tab row and the palette draw the same
/// pixels without copying them.
pub type Icon = Rc<Picture>;

impl Platform for Win {
    type Engine = WebView2Engine;
    type Loop = WinLoop;
    type View = WinView;
    type Icon = Icon;
    type Frame = WinFrame;

    /// Windows' display languages, best first (Settings › Time & language),
    /// as BCP 47 tags; the environment's when Windows has none.
    fn preferred_languages() -> Vec<String> {
        let list = crate::sys::user::preferred_languages();
        if list.is_empty() {
            copper_shell_core::shell::languages::from_env()
        } else {
            list
        }
    }

    fn user_name() -> String {
        crate::sys::user::name()
    }

    /// `GlobalMemoryStatusEx`: physical memory, total and available.
    fn memory() -> Option<(u64, u64)> {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `m` is a properly sized MEMORYSTATUSEX with dwLength set.
        unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
        Some((m.ullTotalPhys, m.ullAvailPhys))
    }

    /// The frame WebView2 kept when the page was last hidden: a hidden
    /// page is not drawn, so there is nothing newer to take.
    fn kept_picture(page: &copper_engine_webview2::WebView2Page) -> Option<(Vec<u8>, u32, u32)> {
        page.cover().map(|c| (c.jpeg, c.width, c.height))
    }
}

/// copper-win-rt's loop: one message loop on the UI thread, futures woken
/// through its hidden message-only window, timers by `SetTimer`.
pub enum WinLoop {}

impl MainLoop for WinLoop {
    fn spawn_local(future: impl Future<Output = ()> + 'static) {
        copper_win_rt::spawn_local(future);
    }

    fn sleep(duration: Duration) -> impl Future<Output = ()> + 'static {
        copper_win_rt::sleep(duration)
    }

    fn post(f: impl FnOnce() + Send + 'static) {
        copper_win_rt::post(f);
    }

    fn is_main_thread() -> bool {
        copper_win_rt::on_ui_thread()
    }
}

/// The shell on Windows.
pub type Shell = copper_shell_core::Shell<Win>;
/// A shell that may be gone.
pub type WeakShell = copper_shell_core::shell::WeakShell<Win>;
/// The live shell on this thread, if one is running.
pub fn current() -> Option<Shell> {
    copper_shell_core::shell::current::<Win>()
}
