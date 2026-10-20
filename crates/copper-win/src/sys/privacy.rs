//! Windows' own privacy switches, read before a site is let at the
//! location or allowed to notify: when Windows says no, a yes from Copper
//! would be a promise nobody keeps, so the site is refused and the user
//! told where the switch is. And whether Windows sends links to Copper.
//!
//! (`windows`' `Win32_System_Registry` feature is enabled by the toolkit,
//! copper-win-ui, which this crate always links.)

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::core::{PCWSTR, w};

/// Where Settings › Privacy & security › Location keeps its answers.
const LOCATION: PCWSTR = w!(
    "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location"
);
/// "Let desktop apps access your location" (Copper is one).
const LOCATION_DESKTOP: PCWSTR = w!(
    "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location\\NonPackaged"
);

/// Whether Windows keeps the location from Copper: location services off
/// for the device or this user, or off for desktop apps.
pub fn location_off() -> bool {
    denies(HKEY_LOCAL_MACHINE, LOCATION)
        || denies(HKEY_CURRENT_USER, LOCATION)
        || denies(HKEY_CURRENT_USER, LOCATION_DESKTOP)
}

/// Whether Windows shows no notifications from apps at all (Settings ›
/// System › Notifications off).
pub fn notifications_off() -> bool {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reading a DWORD into a local of the right size.
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\PushNotifications"),
            w!("ToastEnabled"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
        .is_ok()
    };
    ok && value == 0
}

/// Whether a consent-store key's `Value` reads `Deny`. A missing key or
/// value is no refusal (the default is to allow).
fn denies(root: HKEY, key: PCWSTR) -> bool {
    let mut buf = [0u16; 16];
    let mut size = std::mem::size_of_val(&buf) as u32;
    // SAFETY: the buffer and its byte size are ours; RegGetValueW writes at
    // most `size` bytes, NUL-terminated.
    let ok = unsafe {
        RegGetValueW(
            root,
            key,
            w!("Value"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .is_ok()
    };
    ok && super::from_wide(&buf).eq_ignore_ascii_case("Deny")
}

/// Where Windows keeps which app opens a scheme's links.
const HTTPS_CHOICE: PCWSTR =
    w!("Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\https\\UserChoice");

/// Whether links from other apps come to Copper: the user's choice for
/// `https` is Copper's link type (copper-setup's `PROGID_URL`). What
/// Windows decided, never what Copper asked for.
pub fn is_default_browser() -> bool {
    let mut buf = [0u16; 128];
    let mut size = std::mem::size_of_val(&buf) as u32;
    // SAFETY: as in `denies`.
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            HTTPS_CHOICE,
            w!("ProgId"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .is_ok()
    };
    ok && super::from_wide(&buf).eq_ignore_ascii_case(copper_setup::registration::PROGID_URL)
}
