//! Known folders, shortcuts and the few shell calls setup makes.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemAlloc, CoTaskMemFree, CoUninitialize, IPersistFile,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT,
    SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, SHGetKnownFolderPath, ShellExecuteW,
    ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_core::{GUID, HSTRING, Interface, PWSTR, w};

/// `System.AppUserModel.ID` (propkey.h). Defined here rather than pulling
/// in the whole `Storage_EnhancedStorage` feature for one constant.
const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

/// COM for this thread, for as long as it lives (apartment-threaded: the
/// shell's objects and the window both want STA).
pub struct Com(bool);

impl Com {
    pub fn init() -> Com {
        // SAFETY: plain COM init; balanced by Drop when it succeeded (S_FALSE
        // — already initialised on this thread — also needs balancing).
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Com(hr.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful CoInitializeEx above.
            unsafe { CoUninitialize() }
        }
    }
}

fn known_folder(id: &GUID) -> anyhow::Result<PathBuf> {
    // SAFETY: the returned string is CoTaskMem-allocated; freed below.
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
        let path = PathBuf::from(String::from_utf16_lossy(p.as_wide()));
        CoTaskMemFree(Some(p.0 as *const _));
        Ok(path)
    }
}

/// `%LOCALAPPDATA%` — `C:\Users\<you>\AppData\Local`.
pub fn local_app_data() -> anyhow::Result<PathBuf> {
    known_folder(&FOLDERID_LocalAppData)
}

/// The user's Start menu programs folder.
pub fn start_menu_programs() -> anyhow::Result<PathBuf> {
    known_folder(&FOLDERID_Programs)
}

/// The user's desktop.
pub fn desktop() -> anyhow::Result<PathBuf> {
    known_folder(&FOLDERID_Desktop)
}

/// A `VT_LPWSTR` PROPVARIANT owning a CoTaskMem copy of `s` — the type the
/// shell stores `System.AppUserModel.ID` as (`InitPropVariantFromString`).
fn lpwstr_variant(s: &str) -> anyhow::Result<PROPVARIANT> {
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: we allocate exactly `wide.len()` u16s, copy into them, and hand
    // ownership to the PROPVARIANT (its Drop, PropVariantClear, frees it).
    unsafe {
        let mem = CoTaskMemAlloc(wide.len() * 2).cast::<u16>();
        if mem.is_null() {
            anyhow::bail!("out of memory");
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), mem, wide.len());
        let mut pv = PROPVARIANT::default();
        let inner = &mut *pv.Anonymous.Anonymous;
        inner.vt = VT_LPWSTR;
        inner.Anonymous.pwszVal = PWSTR(mem);
        Ok(pv)
    }
}

/// What a shortcut points at.
pub struct Shortcut<'a> {
    pub target: &'a Path,
    pub working_dir: &'a Path,
    pub description: &'a str,
    pub app_user_model_id: &'a str,
}

/// Write `lnk` (overwriting), with the AppUserModelID set so the taskbar
/// button of the running browser groups with the pinned shortcut.
pub fn create_shortcut(lnk: &Path, s: &Shortcut<'_>) -> anyhow::Result<()> {
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // SAFETY: COM calls on a freshly created in-proc ShellLink object.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(s.target))?;
        link.SetWorkingDirectory(&HSTRING::from(s.working_dir))?;
        link.SetDescription(&HSTRING::from(s.description))?;
        link.SetIconLocation(&HSTRING::from(s.target), 0)?;

        let store: IPropertyStore = link.cast()?;
        // Dropping the PROPVARIANT clears it (and frees the string).
        let id = lpwstr_variant(s.app_user_model_id)?;
        store.SetValue(&PKEY_APP_USER_MODEL_ID, &id)?;
        store.Commit()?;

        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(lnk), true)?;
    }
    Ok(())
}

/// Tell Explorer file and URL associations changed, so Default apps and
/// Open with see Copper without a sign-out.
pub fn notify_associations_changed() {
    // SAFETY: documented no-argument form of this notification.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) }
}

/// Open a URI or file with its handler (`ms-settings:…`, a folder).
pub fn open(target: &str) -> anyhow::Result<()> {
    // SAFETY: valid strings; the result is an HINSTANCE-encoded status code.
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // > 32 means success (ShellExecute's own convention).
    if r.0 as usize > 32 {
        Ok(())
    } else {
        anyhow::bail!("ShellExecute({target}) failed: {}", r.0 as usize)
    }
}
