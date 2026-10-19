//! Where Copper keeps things on Windows (CONTRACT §4).
//!
//! | What | Path |
//! |---|---|
//! | profile data, logs (`EnginePaths::data_root`) | `%LOCALAPPDATA%\Copper\Data` |
//! | caches, compiled content filters (`cache_root`) | `%LOCALAPPDATA%\Copper\Cache` |
//! | WebView2 user data folder (all web storage) | `%LOCALAPPDATA%\Copper\WebView2` |
//! | downloads | the user's Downloads known folder |

use std::path::PathBuf;

use copper_engine::{EngineError, EnginePaths, Result};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Downloads, FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::GUID;

fn known_folder(id: &GUID) -> Result<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)
            .map_err(|e| EngineError::Io(format!("known folder: {e}")))?;
        let s = p.to_string();
        CoTaskMemFree(Some(p.as_ptr().cast()));
        s.map(PathBuf::from)
            .map_err(|e| EngineError::Io(format!("known folder path: {e}")))
    }
}

/// `%LOCALAPPDATA%\Copper`.
pub(crate) fn copper_root() -> Result<PathBuf> {
    Ok(known_folder(&FOLDERID_LocalAppData)?.join("Copper"))
}

/// The standard layout under [`copper_root`].
pub(crate) fn default_paths() -> Result<(EnginePaths, PathBuf)> {
    let root = copper_root()?;
    let downloads = known_folder(&FOLDERID_Downloads).unwrap_or_else(|_| root.join("Downloads"));
    Ok((
        EnginePaths {
            data_root: root.join("Data"),
            cache_root: root.join("Cache"),
            downloads_dir: downloads,
        },
        root.join("WebView2"),
    ))
}
