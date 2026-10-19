//! The user: their display name and the languages they read.

use super::from_wide;
use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};
use windows::Win32::System::WindowsProgramming::GetUserNameW;
use windows::core::PWSTR;

/// The user's display name ("Felipe Arce"), else their account name, else
/// empty. Shown in Settings and handed to the cloud as the device's owner.
pub fn name() -> String {
    display_name().or_else(account_name).unwrap_or_default()
}

/// `GetUserNameExW(NameDisplay)`: the full name of a domain or Microsoft
/// account; fails for a plain local account with no full name.
fn display_name() -> Option<String> {
    let mut len: u32 = 0;
    // SAFETY: a size query (no buffer).
    unsafe {
        let _ = GetUserNameExW(NameDisplay, None, &mut len);
    }
    if len == 0 || len > 1024 {
        return None;
    }
    let mut buf = vec![0u16; len as usize + 1];
    // SAFETY: `buf` holds `len` characters and the NUL.
    let ok = unsafe { GetUserNameExW(NameDisplay, Some(PWSTR(buf.as_mut_ptr())), &mut len) };
    let name = from_wide(&buf);
    (ok && !name.trim().is_empty()).then_some(name)
}

/// `GetUserNameW`: the logon name.
fn account_name() -> Option<String> {
    let mut buf = [0u16; 257];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is `len` characters long.
    unsafe { GetUserNameW(Some(PWSTR(buf.as_mut_ptr())), &mut len) }.ok()?;
    let name = from_wide(&buf);
    (!name.trim().is_empty()).then_some(name)
}

/// The user's display languages in order (`["en-US", "es-MX"]`), as BCP 47
/// tags: what pages are asked for (`Accept-Language`, `navigator.languages`).
pub fn preferred_languages() -> Vec<String> {
    let (mut count, mut len) = (0u32, 0u32);
    // SAFETY: a size query (no buffer).
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len) }
        .is_err()
        || len == 0
    {
        return Vec::new();
    }
    let mut buf = vec![0u16; len as usize];
    // SAFETY: `buf` holds `len` characters.
    if unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(PWSTR(buf.as_mut_ptr())),
            &mut len,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    split_multi(&buf)
}

/// A double-NUL-terminated list of strings.
fn split_multi(buf: &[u16]) -> Vec<String> {
    buf.split(|&c| c == 0)
        .take_while(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_strings_split() {
        let w: Vec<u16> = "en-US\0es-MX\0\0".encode_utf16().collect();
        assert_eq!(split_multi(&w), vec!["en-US", "es-MX"]);
        let empty: Vec<u16> = "\0\0".encode_utf16().collect();
        assert!(split_multi(&empty).is_empty());
    }
}
