//! A thin, owned wrapper over the Win32 registry, enough to apply
//! [`crate::registration`]'s lists and read a few values back.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, KEY_SET_VALUE, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegGetValueW,
    RegOpenKeyExW, RegSetValueExW,
};
use windows_core::{HSTRING, PCWSTR};

use crate::registration::{Removal, Value, Write};

pub use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

/// An open key, closed on drop.
pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: we own the handle and close it once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn check(err: WIN32_ERROR, what: &str) -> anyhow::Result<()> {
    if err.is_ok() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{what}: {}",
            windows_core::Error::from_hresult(err.to_hresult())
        ))
    }
}

fn missing(err: WIN32_ERROR) -> bool {
    err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND
}

/// `None` → the key's default value (a null name).
fn name_param(name: Option<&str>) -> Option<HSTRING> {
    name.map(HSTRING::from)
}

fn pcwstr(name: &Option<HSTRING>) -> PCWSTR {
    name.as_ref().map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr()))
}

impl Key {
    /// Open `root\path`, creating it (and its parents) if missing.
    pub fn create(root: HKEY, path: &str) -> anyhow::Result<Key> {
        let mut out = HKEY::default();
        // SAFETY: valid strings; `out` receives a handle we then own.
        let err = unsafe {
            RegCreateKeyExW(
                root,
                &HSTRING::from(path),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut out,
                None,
            )
        };
        check(err, &format!("create key {path}"))?;
        Ok(Key(out))
    }

    /// Set a `REG_SZ`.
    pub fn set_str(&self, name: Option<&str>, value: &str) -> anyhow::Result<()> {
        let mut wide: Vec<u16> = value.encode_utf16().collect();
        wide.push(0);
        // SAFETY: the byte view covers exactly `wide`, NUL included.
        let bytes =
            unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
        let name = name_param(name);
        // SAFETY: open key, valid name and data.
        let err = unsafe { RegSetValueExW(self.0, pcwstr(&name), None, REG_SZ, Some(bytes)) };
        check(err, "set string value")
    }

    /// Set a `REG_DWORD`.
    pub fn set_dword(&self, name: Option<&str>, value: u32) -> anyhow::Result<()> {
        let name = name_param(name);
        let bytes = value.to_le_bytes();
        // SAFETY: open key, valid name and data.
        let err = unsafe { RegSetValueExW(self.0, pcwstr(&name), None, REG_DWORD, Some(&bytes)) };
        check(err, "set dword value")
    }
}

/// Read a `REG_SZ` (or `REG_EXPAND_SZ`, unexpanded) value; `None` if absent.
pub fn read_str(root: HKEY, path: &str, name: Option<&str>) -> Option<String> {
    let path = HSTRING::from(path);
    let name = name_param(name);
    let mut len = 0u32;
    // SAFETY: size query with no buffer.
    let err = unsafe {
        RegGetValueW(
            root,
            &path,
            pcwstr(&name),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut len),
        )
    };
    if err.is_err() || len < 2 {
        return None;
    }
    let mut buf = vec![0u16; (len as usize).div_ceil(2)];
    // SAFETY: `buf` holds `len` bytes.
    let err = unsafe {
        RegGetValueW(
            root,
            &path,
            pcwstr(&name),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if err.is_err() {
        return None;
    }
    let chars = (len as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..chars]);
    Some(s.trim_end_matches('\0').to_owned())
}

/// Read a `REG_DWORD`; `None` if absent.
pub fn read_dword(root: HKEY, path: &str, name: &str) -> Option<u32> {
    let mut value = 0u32;
    let mut len = 4u32;
    // SAFETY: 4-byte buffer for a DWORD.
    let err = unsafe {
        RegGetValueW(
            root,
            &HSTRING::from(path),
            &HSTRING::from(name),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut len),
        )
    };
    err.is_ok().then_some(value)
}

/// Delete `root\path` and everything below it. Missing is fine.
pub fn delete_tree(root: HKEY, path: &str) -> anyhow::Result<()> {
    // SAFETY: valid root and string.
    let err = unsafe { RegDeleteTreeW(root, &HSTRING::from(path)) };
    if missing(err) {
        return Ok(());
    }
    // Given a subkey, RegDeleteTreeW removes the subkey itself as well.
    check(err, &format!("delete key {path}"))
}

/// Delete one value from `root\path`. Missing key or value is fine.
pub fn delete_value(root: HKEY, path: &str, name: &str) -> anyhow::Result<()> {
    let mut key = HKEY::default();
    // SAFETY: valid strings; `key` receives a handle we close below.
    let err = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_SET_VALUE, &mut key) };
    if missing(err) {
        return Ok(());
    }
    check(err, &format!("open key {path}"))?;
    let key = Key(key);
    // SAFETY: open key, valid name.
    let err = unsafe { RegDeleteValueW(key.0, &HSTRING::from(name)) };
    if missing(err) {
        return Ok(());
    }
    check(err, &format!("delete value {path}\\{name}"))
}

/// Apply every write under `HKEY_CURRENT_USER`, opening each key once.
pub fn apply(writes: &[Write]) -> anyhow::Result<()> {
    let mut open: Option<(String, Key)> = None;
    for w in writes {
        if open.as_ref().is_none_or(|(path, _)| *path != w.key) {
            open = Some((w.key.clone(), Key::create(HKEY_CURRENT_USER, &w.key)?));
        }
        let (_, key) = open.as_ref().expect("opened above");
        let name = w.name.as_deref();
        match &w.value {
            Value::Str(s) => key.set_str(name, s),
            Value::Dword(d) => key.set_dword(name, *d),
        }
        .map_err(|e| anyhow::anyhow!("{}\\{}: {e}", w.key, name.unwrap_or("(default)")))?;
    }
    Ok(())
}

/// Undo [`apply`]: every removal is attempted; the first error is returned.
pub fn remove(removals: &[Removal]) -> anyhow::Result<()> {
    let mut first = None;
    for r in removals {
        let result = match r {
            Removal::Tree(path) => delete_tree(HKEY_CURRENT_USER, path),
            Removal::Value { key, name } => delete_value(HKEY_CURRENT_USER, key, name),
        };
        if let Err(e) = result {
            crate::log::log!("registry: {e}");
            first.get_or_insert(e);
        }
    }
    first.map_or(Ok(()), Err)
}
