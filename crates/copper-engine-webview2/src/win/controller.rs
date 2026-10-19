//! Making a composition controller and giving it Copper's settings.

use copper_engine::{EngineError, PageOptions, Result};
use copper_win_rt::oneshot;
use webview2_com::CreateCoreWebView2CompositionControllerCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::Foundation::{E_POINTER, HWND};
use windows::core::Interface;

use super::com::{hs, other};

/// One webview: its composition controller, controller and core.
#[derive(Clone)]
pub(crate) struct Wv {
    pub comp: ICoreWebView2CompositionController,
    pub ctl: ICoreWebView2Controller,
    pub view: ICoreWebView2,
}

/// Make a hidden composition controller in WebView2 profile `name`.
pub(crate) async fn create(
    env: &ICoreWebView2Environment,
    parent: HWND,
    name: &str,
    private: bool,
) -> Result<Wv> {
    let env10: ICoreWebView2Environment10 = env.cast().map_err(|_| {
        EngineError::Other("the WebView2 Runtime is too old (needs profiles)".into())
    })?;
    let options = unsafe { env10.CreateCoreWebView2ControllerOptions() }
        .map_err(|e| other("controller options", e))?;
    unsafe {
        options
            .SetProfileName(&hs(name))
            .map_err(|e| other("profile name", e))?;
        options
            .SetIsInPrivateModeEnabled(private)
            .map_err(|e| other("InPrivate", e))?;
    }
    let (tx, rx) = oneshot::<windows::core::Result<ICoreWebView2CompositionController>>();
    let handler = CreateCoreWebView2CompositionControllerCompletedHandler::create(Box::new(
        move |hr, comp| {
            tx.send(hr.and_then(|()| comp.ok_or_else(|| E_POINTER.into())));
            Ok(())
        },
    ));
    unsafe { env10.CreateCoreWebView2CompositionControllerWithOptions(parent, &options, &handler) }
        .map_err(|e| other("create controller", e))?;
    let comp = rx
        .await
        .ok_or_else(|| EngineError::Other("controller creation dropped".into()))?
        .map_err(|e| other("create controller", e))?;
    let ctl: ICoreWebView2Controller = comp.cast().map_err(|e| other("controller", e))?;
    let view = unsafe { ctl.CoreWebView2() }.map_err(|e| other("webview", e))?;
    unsafe {
        let _ = ctl.SetIsVisible(false);
        if let Ok(c3) = ctl.cast::<ICoreWebView2Controller3>() {
            // The toolkit gives bounds in physical pixels and tells us the
            // scale itself (it knows which monitor the slot is on).
            let _ = c3.SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS);
            let _ = c3.SetShouldDetectMonitorScaleChanges(false);
        }
    }
    Ok(Wv { comp, ctl, view })
}

/// Per-page settings (a spare is made before its page's options are known).
///
/// Privacy defaults: no password saving, no autofill, no SmartScreen
/// reputation lookups, no host objects. WebView2's status bubble is off
/// (link hovers become `PageEvent::LinkHover`, which the shell draws).
/// The built-in error page stays on: replacing it with `NavigateToString`
/// would lose the failed URL from the address bar and history; the shell can
/// turn it off ([`crate::WebView2Engine::set_builtin_error_page`]) and draw its
/// own from `LoadState::Failed`.
pub(crate) fn configure(wv: &Wv, opts: &PageOptions, builtin_error_page: bool) {
    unsafe {
        if let Ok(s) = wv.view.Settings() {
            let _ = s.SetIsScriptEnabled(true);
            let _ = s.SetIsWebMessageEnabled(true);
            let _ = s.SetAreDefaultScriptDialogsEnabled(true);
            let _ = s.SetIsStatusBarEnabled(false);
            let _ = s.SetAreDevToolsEnabled(opts.developer_extras);
            let _ = s.SetAreDefaultContextMenusEnabled(true);
            let _ = s.SetAreHostObjectsAllowed(false);
            let _ = s.SetIsZoomControlEnabled(true);
            let _ = s.SetIsBuiltInErrorPageEnabled(builtin_error_page);
            if let (Some(ua), Ok(s2)) = (&opts.user_agent, s.cast::<ICoreWebView2Settings2>()) {
                let _ = s2.SetUserAgent(&hs(ua));
            }
            if let Ok(s4) = s.cast::<ICoreWebView2Settings4>() {
                let _ = s4.SetIsPasswordAutosaveEnabled(false);
                let _ = s4.SetIsGeneralAutofillEnabled(false);
            }
            if let Ok(s5) = s.cast::<ICoreWebView2Settings5>() {
                let _ = s5.SetIsPinchZoomEnabled(true);
            }
            if let Ok(s6) = s.cast::<ICoreWebView2Settings6>() {
                let _ = s6.SetIsSwipeNavigationEnabled(true);
            }
            if let Ok(s8) = s.cast::<ICoreWebView2Settings8>() {
                let _ = s8.SetIsReputationCheckingRequired(false);
            }
        }
        if let Ok(v8) = wv.view.cast::<ICoreWebView2_8>() {
            let _ = v8.SetIsMuted(opts.muted);
        }
        if opts.zoom > 0.0 && (opts.zoom - 1.0).abs() > f64::EPSILON {
            let _ = wv.ctl.SetZoomFactor(opts.zoom);
        }
    }
}

/// Profile-wide defaults, set once per profile when its first webview
/// exists: Strict tracking prevention, no password saving or autofill, and
/// the download folder.
pub(crate) fn configure_profile(profile: &ICoreWebView2Profile, download_dir: &std::path::Path) {
    unsafe {
        if let Ok(p3) = profile.cast::<ICoreWebView2Profile3>() {
            let _ = p3
                .SetPreferredTrackingPreventionLevel(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT);
        }
        if let Ok(p6) = profile.cast::<ICoreWebView2Profile6>() {
            let _ = p6.SetIsPasswordAutosaveEnabled(false);
            let _ = p6.SetIsGeneralAutofillEnabled(false);
        }
        let _ = profile.SetDefaultDownloadFolderPath(&hs(&download_dir.to_string_lossy()));
    }
}

/// The webview's profile object.
pub(crate) fn profile_of(wv: &Wv) -> Option<ICoreWebView2Profile> {
    unsafe { wv.view.cast::<ICoreWebView2_13>().ok()?.Profile().ok() }
}

/// Close a controller (its handlers must already be removed).
pub(crate) fn close(wv: &Wv) {
    unsafe {
        let _ = wv
            .comp
            .SetRootVisualTarget(None::<&windows::core::IUnknown>);
        let _ = wv.ctl.Close();
    }
}
