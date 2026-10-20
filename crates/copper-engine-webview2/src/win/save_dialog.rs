//! The system "Save As" dialog (`IFileSaveDialog`) a download asks with
//! before it writes anything (`WebView2Engine::set_download_ask`).
//!
//! Like `file_dialog`'s Open dialog it runs on a thread of its own, so its
//! modal loop never nests inside the UI thread's (no WebView2 or shell
//! code re-entered mid-call); `Show(owner)` disables the owner — the
//! Copper window — for as long as it is open. Dialogs are shown one at a
//! time ([`SaveQueue`]): of two modal dialogs on one owner, the first to
//! close would re-enable it under the second.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use copper_win_rt::{OneshotRx, oneshot};
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FileSaveDialog, IFileSaveDialog,
    IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HSTRING, PCWSTR};

use super::worker;
use crate::partials::Mark;
use crate::util::plain_extension;

/// Where the engine asks: the window that owns the dialog, as a number
/// (`HWND` is not `Send`, and the handle is only named to the dialog).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AskOwner(pub isize);

/// What to ask.
pub(crate) struct SaveSpec {
    /// The file name filled in (and selected) when the dialog opens.
    pub name: String,
    /// The folder it opens in (made if missing).
    pub folder: PathBuf,
    pub owner: AskOwner,
    /// Left in this folder while the dialog is open (see
    /// [`crate::partials`]): the download is being written meanwhile.
    pub mark: Option<(PathBuf, Mark)>,
}

/// Lets one Save As dialog show at a time, in the order they were asked:
/// each ask waits for the one before it to end (or be dropped).
#[derive(Default)]
pub(crate) struct SaveQueue {
    /// Resolves when the last ask so far has ended.
    tail: RefCell<Option<OneshotRx<()>>>,
}

impl SaveQueue {
    /// Ask where to save; `None` when the dialog was cancelled or closed
    /// (or could not be shown).
    pub async fn ask(&self, spec: SaveSpec) -> Option<PathBuf> {
        // Dropping `_done` (at the end, or with this future) lets the
        // next ask go.
        let (_done, ended) = oneshot::<()>();
        let before = self.tail.replace(Some(ended));
        if let Some(before) = before {
            let _ = before.await;
        }
        show(spec).await
    }
}

/// Show the dialog on its own thread and wait for the answer.
async fn show(spec: SaveSpec) -> Option<PathBuf> {
    if let Err(e) = std::fs::create_dir_all(&spec.folder) {
        tracing::warn!("save dialog folder {}: {e}", spec.folder.display());
    }
    let done = worker::run("copper-save-dialog", move || {
        let left = spec.mark.as_ref().and_then(|(marks, mark)| {
            mark.leave(marks, crate::util::next_id())
                .map_err(|e| tracing::debug!("download ask mark: {e}"))
                .ok()
        });
        let init =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let r = show_with_com(&spec);
        if let Some(path) = left {
            let _ = std::fs::remove_file(path);
        }
        if init.is_ok() {
            unsafe { CoUninitialize() };
        }
        r
    });
    match done.await {
        Some(Ok(path)) => path,
        Some(Err(e)) => {
            tracing::warn!("save dialog: {e}");
            None
        }
        None => None,
    }
}

/// On the dialog's thread: set it up, show it, read the choice.
fn show_with_com(spec: &SaveSpec) -> windows::core::Result<Option<PathBuf>> {
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
        // The dialog's "New folder" is on by default; the overwrite prompt
        // is asked for.
        let options =
            dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT;
        dialog.SetOptions(options)?;
        // The suggested type first, so a name typed without an extension
        // gets it.
        if let Some(ext) = plain_extension(&spec.name) {
            let types = [
                (
                    HSTRING::from(format!("{} file", ext.to_uppercase())),
                    HSTRING::from(format!("*.{ext}")),
                ),
                (HSTRING::from("All files"), HSTRING::from("*.*")),
            ];
            let specs: Vec<COMDLG_FILTERSPEC> = types
                .iter()
                .map(|(name, spec)| COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                })
                .collect();
            dialog.SetFileTypes(&specs)?;
            dialog.SetFileTypeIndex(1)?;
            dialog.SetDefaultExtension(&HSTRING::from(ext))?;
        }
        if let Some(folder) = shell_item(&spec.folder) {
            dialog.SetFolder(&folder)?;
        }
        // Filled in and selected (the dialog selects the stem).
        dialog.SetFileName(&HSTRING::from(spec.name.as_str()))?;
        if let Err(e) = dialog.Show(Some(HWND(spec.owner.0 as *mut _))) {
            if e.code() != ERROR_CANCELLED.to_hresult() {
                tracing::warn!("save dialog: {e}");
            }
            return Ok(None);
        }
        Ok(path(&dialog.GetResult()?))
    }
}

/// The shell item of folder `dir`, if it exists.
fn shell_item(dir: &Path) -> Option<IShellItem> {
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(dir.as_os_str()), None).ok() }
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
