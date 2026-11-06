//! Authenticode: does a file's signature hold up, and whose is it. Setup
//! checks the WebView2 bootstrapper with it (`webview2`); Copper checks its
//! own updates (a signed Copper takes only an update signed by the same
//! publisher).
//!
//! The rules are platform-free so they are unit-tested on Linux;
//! [`check`] asks `WinVerifyTrust`.

/// What `WinVerifyTrust`'s answer means for running a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    /// Signed, the chain ends in a trusted root, nothing revoked.
    Valid,
    /// Signed and chained, but the revocation server couldn't be reached
    /// (a strict proxy, a flaky line). HTTPS from the publisher already
    /// vouches for the bytes, so this still counts as signed.
    RevocationUnchecked,
    /// Unsigned, tampered with, expired, revoked, untrusted: never run it.
    Invalid,
}

/// `CRYPT_E_REVOCATION_OFFLINE`.
const CRYPT_E_REVOCATION_OFFLINE: i32 = 0x8009_2013_u32 as i32;
/// `CERT_E_REVOCATION_FAILURE`.
const CERT_E_REVOCATION_FAILURE: i32 = 0x800B_010E_u32 as i32;

/// Classify a `WinVerifyTrust` status.
pub fn trust(status: i32) -> Trust {
    match status {
        0 => Trust::Valid,
        CRYPT_E_REVOCATION_OFFLINE | CERT_E_REVOCATION_FAILURE => Trust::RevocationUnchecked,
        _ => Trust::Invalid,
    }
}

/// Who signed the file, when the signature holds up (`status`) and names
/// someone; `None` for an unsigned file or a broken signature.
pub fn valid_signer(status: i32, signer: Option<&str>) -> Option<&str> {
    signer
        .map(str::trim)
        .filter(|s| trust(status) != Trust::Invalid && !s.is_empty())
}

#[cfg(windows)]
pub use imp::check;

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::Cryptography::{
        CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW,
    };
    use windows::Win32::Security::WinTrust::{
        WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
        WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_WHOLECHAIN,
        WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain,
        WTHelperProvDataFromStateData, WinVerifyTrust,
    };
    use windows_core::{GUID, HSTRING, PCWSTR};

    /// The simple display name of the certificate that signed the file
    /// WinVerifyTrust has just checked (`state` is its open state data).
    fn signer_name(state: HANDLE) -> Option<String> {
        // SAFETY: provider data owned by WinTrust's open state, read before
        // the state is closed; every pointer is checked before it is used.
        unsafe {
            let data = WTHelperProvDataFromStateData(state);
            if data.is_null() {
                return None;
            }
            let signer = WTHelperGetProvSignerFromChain(data, 0, false, 0);
            if signer.is_null() || (*signer).csCertChain == 0 || (*signer).pasCertChain.is_null() {
                return None;
            }
            let cert = (*(*signer).pasCertChain).pCert;
            if cert.is_null() {
                return None;
            }
            let mut buf = [0u16; 256];
            let n =
                CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut buf));
            // n counts the terminating NUL; 1 means an empty name.
            (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
        }
    }

    /// `file`'s Authenticode signature: `WinVerifyTrust`'s status (chain to
    /// a trusted root, nothing revoked; [`super::trust`] reads it) and the
    /// signing certificate's simple display name, if one could be read.
    /// May go to the network for revocation: not on a UI thread.
    pub fn check(file: &Path) -> (i32, Option<String>) {
        let path = HSTRING::from(file);
        let mut info = WINTRUST_FILE_INFO {
            cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: PCWSTR(path.as_ptr()),
            hFile: HANDLE::default(),
            pgKnownSubject: std::ptr::null_mut(),
        };
        let mut data = WINTRUST_DATA {
            cbStruct: size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
            dwStateAction: WTD_STATEACTION_VERIFY,
            dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT,
            ..Default::default()
        };
        let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        // SAFETY: `data` and `info` outlive both calls; the second releases
        // the state the first allocated, after the signer has been read.
        unsafe {
            let s = WinVerifyTrust(
                HWND(-1isize as *mut _),
                &mut action,
                (&mut data as *mut WINTRUST_DATA).cast(),
            );
            let signer = signer_name(data.hWVTStateData);
            data.dwStateAction = WTD_STATEACTION_CLOSE;
            WinVerifyTrust(
                HWND(-1isize as *mut _),
                &mut action,
                (&mut data as *mut WINTRUST_DATA).cast(),
            );
            (s, signer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_statuses() {
        assert_eq!(trust(0), Trust::Valid);
        assert_eq!(trust(0x8009_2013_u32 as i32), Trust::RevocationUnchecked);
        assert_eq!(trust(0x800B_010E_u32 as i32), Trust::RevocationUnchecked);
        for bad in [
            0x800B_0100_u32, // TRUST_E_NOSIGNATURE
            0x8009_6010,     // TRUST_E_BAD_DIGEST (tampered)
            0x800B_0109,     // CERT_E_UNTRUSTEDROOT
            0x800B_010C,     // CERT_E_REVOKED
            0x800B_0101,     // CERT_E_EXPIRED
            0x800B_0004,     // TRUST_E_SUBJECT_NOT_TRUSTED
            1,
        ] {
            assert_eq!(trust(bad as i32), Trust::Invalid, "0x{bad:08x}");
        }
    }

    #[test]
    fn a_signer_counts_only_on_a_signature_that_holds() {
        assert_eq!(
            valid_signer(0, Some(" Exowatt, Inc. ")),
            Some("Exowatt, Inc.")
        );
        assert_eq!(
            valid_signer(0x8009_2013_u32 as i32, Some("Exowatt, Inc.")),
            Some("Exowatt, Inc.")
        );
        assert_eq!(
            valid_signer(0x800B_0100_u32 as i32, Some("Exowatt, Inc.")),
            None
        );
        assert_eq!(valid_signer(0, None), None);
        assert_eq!(valid_signer(0, Some("  ")), None);
    }
}
