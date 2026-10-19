//! The system "Open" dialog for a page's `<input type=file>`
//! (`IFileOpenDialog`), run on a thread of its own, the way Chromium runs
//! its own: its modal loop never nests inside the UI thread's (no
//! WebView2 or shell code re-entered mid-call), while the owner — the
//! Copper window — is disabled for as long as it is open, as for any
//! modal dialog.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, IsWindowVisible, PostMessageW, WM_CLOSE,
};
use windows::core::{BOOL, HSTRING, PCWSTR};

use super::worker;
use crate::accept::FileType;

/// How often, and how long, a close is retried while the dialog is still
/// coming up (it cannot be closed before its window exists).
const CLOSE_RETRY: Duration = Duration::from_millis(50);
const CLOSE_TRIES: u32 = 60;

/// What to ask for.
pub(crate) struct Spec {
    pub title: String,
    pub multiple: bool,
    /// Pick a folder (`webkitdirectory`).
    pub folder: bool,
    pub types: Vec<FileType>,
    /// The Copper window (disabled while the dialog is open), as a number:
    /// `HWND` is not `Send`, and the handle is only named to the dialog.
    pub owner: isize,
}

/// Shared between the UI thread and the dialog's thread.
#[derive(Default)]
struct State {
    /// The dialog's thread (0 until it started).
    thread: AtomicU32,
    /// Asked to close: do not show it, or close it.
    cancelled: AtomicBool,
    /// `Show` returned.
    done: AtomicBool,
}

/// An open dialog. Dropping it does not close it; [`Dialog::close`] does.
pub(crate) struct Dialog {
    state: Arc<State>,
}

/// Open the dialog. The future resolves with the chosen paths (empty:
/// cancelled, closed, or the dialog failed).
pub(crate) fn open(spec: Spec) -> (Dialog, impl Future<Output = Vec<PathBuf>>) {
    let state = Arc::new(State::default());
    let st = state.clone();
    let done = worker::run("copper-file-dialog", move || {
        st.thread
            .store(unsafe { GetCurrentThreadId() }, Ordering::Release);
        let r = show(&spec, HWND(spec.owner as *mut _), &st);
        st.done.store(true, Ordering::Release);
        r
    });
    let result = async move {
        match done.await {
            Some(Ok(paths)) => paths,
            Some(Err(e)) => {
                tracing::warn!("file dialog: {e}");
                Vec::new()
            }
            None => Vec::new(),
        }
    };
    (Dialog { state }, result)
}

impl Dialog {
    /// Close the dialog as if cancelled (its future resolves empty). Safe
    /// to call at any point, also before the window exists.
    pub fn close(&self) {
        self.state.cancelled.store(true, Ordering::Release);
        let state = self.state.clone();
        copper_win_rt::spawn_local(async move {
            for _ in 0..CLOSE_TRIES {
                if state.done.load(Ordering::Acquire) {
                    return;
                }
                post_close(state.thread.load(Ordering::Acquire));
                copper_win_rt::sleep(CLOSE_RETRY).await;
            }
            tracing::warn!("file dialog: did not close");
        });
    }
}

/// Ask every visible top-level window of the dialog's thread (the dialog,
/// and any message box it shows) to close.
fn post_close(thread: u32) {
    if thread == 0 {
        return;
    }
    unsafe extern "system" fn close(hwnd: HWND, _: LPARAM) -> BOOL {
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        BOOL(1)
    }
    unsafe {
        let _ = EnumThreadWindows(thread, Some(close), LPARAM(0));
    }
}

/// On the dialog's thread: show it and read the choice.
fn show(spec: &Spec, owner: HWND, state: &State) -> std::result::Result<Vec<PathBuf>, String> {
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let r = show_with_com(spec, owner, state).map_err(|e| e.to_string());
    if init.is_ok() {
        unsafe { CoUninitialize() };
    }
    r
}

fn show_with_com(spec: &Spec, owner: HWND, state: &State) -> windows::core::Result<Vec<PathBuf>> {
    let started = Instant::now();
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let mut options = dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        if spec.folder {
            options |= FOS_PICKFOLDERS;
        } else {
            options |= FOS_FILEMUSTEXIST;
            if spec.multiple {
                options |= FOS_ALLOWMULTISELECT;
            }
        }
        dialog.SetOptions(options)?;
        dialog.SetTitle(&HSTRING::from(spec.title.as_str()))?;
        if !spec.types.is_empty() && !spec.folder {
            let strings: Vec<(HSTRING, HSTRING)> = spec
                .types
                .iter()
                .map(|t| {
                    (
                        HSTRING::from(t.name.as_str()),
                        HSTRING::from(t.patterns.as_str()),
                    )
                })
                .collect();
            let specs: Vec<COMDLG_FILTERSPEC> = strings
                .iter()
                .map(|(name, spec)| COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                })
                .collect();
            dialog.SetFileTypes(&specs)?;
            dialog.SetFileTypeIndex(1)?;
        }
        // Closed before it could show.
        if state.cancelled.load(Ordering::Acquire) {
            return Ok(Vec::new());
        }
        tracing::debug!(
            "file dialog {:?}: showing after {} ms",
            spec.title,
            started.elapsed().as_millis()
        );
        if let Err(e) = dialog.Show(Some(owner)) {
            // Cancelled by the user or by us, or it could not show: either
            // way the page hears `cancel`.
            if e.code() != ERROR_CANCELLED.to_hresult() {
                tracing::warn!("file dialog: {e}");
            }
            return Ok(Vec::new());
        }
        if state.cancelled.load(Ordering::Acquire) {
            return Ok(Vec::new());
        }
        if spec.multiple && !spec.folder {
            let items = dialog.GetResults()?;
            let mut out = Vec::new();
            for i in 0..items.GetCount()? {
                if let Some(p) = path(&items.GetItemAt(i)?) {
                    out.push(p);
                }
            }
            Ok(out)
        } else {
            Ok(path(&dialog.GetResult()?).into_iter().collect())
        }
    }
}

/// The file-system path of `item`.
fn path(item: &IShellItem) -> Option<PathBuf> {
    unsafe {
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}
