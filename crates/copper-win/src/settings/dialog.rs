//! Windows' own file dialogs for Settings: a folder for downloads, a CSV
//! of passwords to bring in, and where to write one out
//! (`IFileOpenDialog` / `IFileSaveDialog`).
//!
//! Each runs on a thread of its own, apartment-threaded, owned by the
//! Copper window — so the window is disabled while it is open, as for any
//! modal dialog, but its modal loop never nests inside the UI thread's
//! (no WebView2 or chrome code re-entered mid-call). The answer comes back
//! through a future the UI loop awaits.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
    IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HSTRING, Interface, PCWSTR};

/// What to ask Windows for.
#[derive(Clone, Debug)]
pub enum Ask {
    /// A folder (`Use this folder`), starting in `start`.
    Folder {
        title: String,
        start: Option<PathBuf>,
    },
    /// A CSV file to read.
    OpenCsv { title: String },
    /// Where to write a CSV, named `name` to begin with.
    SaveCsv { title: String, name: String },
}

/// Show `ask` over the window `owner` (an `HWND` as a number: handles are
/// not `Send`). Resolves to the path chosen, or `None` when cancelled or
/// the dialog could not show.
pub async fn ask(owner: isize, ask: Ask) -> Option<PathBuf> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("copper-settings-dialog".into())
        .spawn(move || {
            let _ = tx.send(show(&ask, HWND(owner as *mut _)));
        });
    if let Err(e) = spawned {
        tracing::warn!("settings dialog: cannot start a thread: {e}");
        return None;
    }
    loop {
        match rx.try_recv() {
            Ok(Ok(path)) => return path,
            Ok(Err(e)) => {
                tracing::warn!("settings dialog: {e}");
                return None;
            }
            Err(mpsc::TryRecvError::Disconnected) => return None,
            Err(mpsc::TryRecvError::Empty) => {
                copper_win_rt::sleep(Duration::from_millis(60)).await;
            }
        }
    }
}

/// On the dialog's thread: COM, the dialog, the choice.
fn show(ask: &Ask, owner: HWND) -> windows::core::Result<Option<PathBuf>> {
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let r = unsafe { show_with_com(ask, owner) };
    if init.is_ok() {
        unsafe { CoUninitialize() };
    }
    r
}

/// The CSV filter, then everything.
fn csv_types(dialog: &IFileDialog) -> windows::core::Result<()> {
    let names = [
        (HSTRING::from("CSV files"), HSTRING::from("*.csv")),
        (HSTRING::from("All files"), HSTRING::from("*.*")),
    ];
    let specs: Vec<COMDLG_FILTERSPEC> = names
        .iter()
        .map(|(n, s)| COMDLG_FILTERSPEC {
            pszName: PCWSTR(n.as_ptr()),
            pszSpec: PCWSTR(s.as_ptr()),
        })
        .collect();
    unsafe {
        dialog.SetFileTypes(&specs)?;
        dialog.SetFileTypeIndex(1)
    }
}

/// Start the dialog in `dir`, when there is one.
fn start_in(dialog: &IFileDialog, dir: &Path) {
    let wide = HSTRING::from(dir.as_os_str());
    let item: windows::core::Result<IShellItem> =
        unsafe { SHCreateItemFromParsingName(&wide, None) };
    if let Ok(item) = item {
        let _ = unsafe { dialog.SetFolder(&item) };
    }
}

unsafe fn show_with_com(ask: &Ask, owner: HWND) -> windows::core::Result<Option<PathBuf>> {
    unsafe {
        let (dialog, title): (IFileDialog, &str) = match ask {
            Ask::Folder { title, start } => {
                let d: IFileOpenDialog =
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
                d.SetOptions(
                    d.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
                )?;
                d.SetOkButtonLabel(&HSTRING::from("Use this folder"))?;
                let d: IFileDialog = d.cast()?;
                if let Some(s) = start.as_deref().filter(|s| s.is_dir()) {
                    start_in(&d, s);
                }
                (d, title)
            }
            Ask::OpenCsv { title } => {
                let d: IFileOpenDialog =
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
                d.SetOptions(
                    d.GetOptions()? | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
                )?;
                d.SetOkButtonLabel(&HSTRING::from("Import"))?;
                let d: IFileDialog = d.cast()?;
                csv_types(&d)?;
                (d, title)
            }
            Ask::SaveCsv { title, name } => {
                let d: IFileSaveDialog =
                    CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
                d.SetOptions(
                    d.GetOptions()? | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
                )?;
                d.SetFileName(&HSTRING::from(name.as_str()))?;
                d.SetDefaultExtension(&HSTRING::from("csv"))?;
                d.SetOkButtonLabel(&HSTRING::from("Export"))?;
                let d: IFileDialog = d.cast()?;
                csv_types(&d)?;
                (d, title)
            }
        };
        dialog.SetTitle(&HSTRING::from(title))?;
        if let Err(e) = dialog.Show(Some(owner)) {
            if e.code() != ERROR_CANCELLED.to_hresult() {
                return Err(e);
            }
            return Ok(None);
        }
        let item = dialog.GetResult()?;
        Ok(path(&item))
    }
}

/// A shell item's file-system path.
fn path(item: &IShellItem) -> Option<PathBuf> {
    unsafe {
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// The Copper window's handle, for owning a dialog.
pub fn owner(shell: &crate::platform::Shell) -> Option<isize> {
    shell.view().chrome().read(|ui, _| ui.hwnd().0 as isize)
}
