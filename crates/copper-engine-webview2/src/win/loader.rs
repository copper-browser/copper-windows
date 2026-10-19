//! The WebView2 loader, linked statically.
//!
//! `webview2-com-sys` declares the loader's entry points as `raw-dylib`
//! imports of `WebView2Loader.dll`, which would make `copper.exe` need that
//! DLL next to it. The same crate ships Microsoft's `WebView2LoaderStatic.lib`
//! (and puts its folder on the link path), so the two functions Copper calls
//! are declared here against the static library instead and the DLL imports
//! are never referenced. CONTRACT §5; verified with
//! `llvm-readobj --coff-imports copper.exe` (no `WebView2Loader.dll`).

use std::ffi::c_void;

use windows::core::{HRESULT, PCWSTR, PWSTR};

#[link(name = "WebView2LoaderStatic", kind = "static")]
unsafe extern "system" {
    fn CreateCoreWebView2EnvironmentWithOptions(
        browser_executable_folder: PCWSTR,
        user_data_folder: PCWSTR,
        environment_options: *mut c_void,
        environment_created_handler: *mut c_void,
    ) -> HRESULT;

    fn GetAvailableCoreWebView2BrowserVersionString(
        browser_executable_folder: PCWSTR,
        version_info: *mut PWSTR,
    ) -> HRESULT;
}

// The static loader reads the Edge Update registration and policy keys and
// checks file versions.
#[link(name = "advapi32")]
#[link(name = "shell32")]
#[link(name = "version")]
#[link(name = "ole32")]
unsafe extern "system" {}

/// `CreateCoreWebView2EnvironmentWithOptions` from the static loader.
///
/// # Safety
/// `options` and `handler` must be valid COM pointers (or null for
/// `options`); the strings must be null or NUL-terminated.
pub(crate) unsafe fn create_environment(
    browser_folder: PCWSTR,
    user_data_folder: PCWSTR,
    options: *mut c_void,
    handler: *mut c_void,
) -> windows::core::Result<()> {
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(browser_folder, user_data_folder, options, handler)
    }
    .ok()
}

/// The installed runtime's version (`None` when no runtime is installed).
pub(crate) fn runtime_version(browser_folder: PCWSTR) -> Option<String> {
    let mut out = PWSTR::null();
    let hr = unsafe { GetAvailableCoreWebView2BrowserVersionString(browser_folder, &mut out) };
    if hr.is_err() || out.is_null() {
        return None;
    }
    let v = webview2_com::take_pwstr(out);
    (!v.is_empty()).then_some(v)
}
