//! A small HTTPS GET to a file over WinHTTP — enough to fetch the WebView2
//! bootstrapper without carrying an HTTP stack in copper.exe.
//!
//! WinHTTP uses the system's proxy settings (`AUTOMATIC_PROXY`), TLS stack
//! and certificate store, and follows redirects (go.microsoft.com's fwlink
//! is one) but never from HTTPS down to HTTP.

use std::fs::File;
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    WINHTTP_OPTION_SECURE_PROTOCOLS, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};
use windows_core::{HSTRING, PCWSTR, w};

/// A WinHTTP handle, closed on drop.
struct Handle(*mut core::ffi::c_void);

impl Handle {
    fn new(h: *mut core::ffi::c_void, what: &str) -> anyhow::Result<Handle> {
        if h.is_null() {
            anyhow::bail!("{what}: {}", windows_core::Error::from_thread());
        }
        Ok(Handle(h))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle WinHTTP gave us, closed once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn query_number(request: &Handle, level: u32) -> Option<u32> {
    let mut value = 0u32;
    let mut len = 4u32;
    let mut index = 0u32;
    // SAFETY: 4-byte buffer for a numeric header.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            level | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut value as *mut u32).cast()),
            &mut len,
            &mut index,
        )
    }
    .ok()
    .map(|_| value)
}

/// GET `https://{host}{path}` into `to`. `progress(done, total)` is called as
/// bytes arrive; setting `cancel` stops the download (the file is removed).
pub fn download(
    host: &str,
    path: &str,
    to: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> anyhow::Result<u64> {
    let agent = format!("Copper-Setup/{}", crate::registration::VERSION);
    // SAFETY: WinHTTP calls on handles we own; buffers are sized.
    unsafe {
        let session = Handle::new(
            WinHttpOpen(
                &HSTRING::from(agent),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "WinHttpOpen",
        )?;
        // Resolve, connect, send: 15 s each; receive: 30 s between chunks.
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 15_000, 30_000)?;
        let protocols = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;
        // TLS 1.3 is unknown to older Windows 10 builds; retry with 1.2 alone.
        if WinHttpSetOption(
            Some(session.0),
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            Some(&protocols.to_le_bytes()),
        )
        .is_err()
        {
            let _ = WinHttpSetOption(
                Some(session.0),
                WINHTTP_OPTION_SECURE_PROTOCOLS,
                Some(&WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_le_bytes()),
            );
        }
        let connection = Handle::new(
            WinHttpConnect(
                session.0,
                &HSTRING::from(host),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            ),
            "WinHttpConnect",
        )?;
        let request = Handle::new(
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                &HSTRING::from(path),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "WinHttpOpenRequest",
        )?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;

        let status = query_number(&request, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
        if status != 200 {
            anyhow::bail!("https://{host}{path} answered HTTP {status}");
        }
        let total = query_number(&request, WINHTTP_QUERY_CONTENT_LENGTH).map(u64::from);

        let mut file = File::create(to)?;
        let mut done = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        progress(0, total);
        let result = (|| -> anyhow::Result<()> {
            loop {
                if cancel.load(Ordering::Relaxed) {
                    anyhow::bail!("cancelled");
                }
                let mut available = 0u32;
                WinHttpQueryDataAvailable(request.0, &mut available)?;
                if available == 0 {
                    return Ok(());
                }
                let want = (available as usize).min(buf.len()) as u32;
                let mut read = 0u32;
                WinHttpReadData(request.0, buf.as_mut_ptr().cast(), want, &mut read)?;
                if read == 0 {
                    return Ok(());
                }
                file.write_all(&buf[..read as usize])?;
                done += u64::from(read);
                progress(done, total);
            }
        })();
        drop(file);
        if let Err(e) = result {
            let _ = std::fs::remove_file(to);
            return Err(e);
        }
        if let Some(total) = total
            && done != total
        {
            let _ = std::fs::remove_file(to);
            anyhow::bail!("download ended early: {done} of {total} bytes");
        }
        Ok(done)
    }
}
