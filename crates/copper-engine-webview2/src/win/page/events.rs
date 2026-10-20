//! WebView2's events on one webview → `PageEvent`s, `PageChange`s and the
//! page's cached state.
//!
//! Every handler holds the page weakly and is removed when the webview is
//! let go (`PageInner::tokens`).

use std::rc::Rc;

use copper_engine::{
    CrashReason, Image, LinkHover, LoadState, NewWindowRequest, PageEvent, PermissionId,
    WindowDisposition,
};
use serde_json::json;
use webview2_com::GetFaviconCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::Interface;

use super::hosting::{MODIFIED_CLICK_WINDOW, accelerator_key, focus_changed};
use super::{Life, PageInner, PendingPermission, WebView2Page, scheme, script};
use crate::convert::{
    CONNECTION_REFUSED, WEB_ERROR_CANCELED, WEB_ERROR_UNKNOWN, crash_reason, knock_target,
    permission_kind, refuses, web_error_name,
};

/// `COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED`.
const WEB_ERROR_ABORTED: i32 = 9;
/// How long a failed load's knock may take ([`PageInner::fail_after_knock`]).
/// Windows answers a refused connection only after retrying it (about 2 s
/// even on the loopback; more on a slow or busy machine).
const KNOCK_LIMIT: std::time::Duration = std::time::Duration::from_secs(4);
use crate::history::NativeHistory;
use crate::hosting::{FocusExit, KeyEventKind, PageChange};
use crate::util::{Either, png_size, race};
use crate::win::com::{Tokens, get, get_bool, get_string, hs, read_stream};
use crate::win::controller::{self, Wv};
use crate::win::{cdp, downloads};

type WinResult = windows::core::Result<()>;

/// Load progress milestones (WebView2 reports none).
const PROGRESS_STARTED: f64 = 0.1;
const PROGRESS_COMMITTED: f64 = 0.3;
const PROGRESS_DOM_READY: f64 = 0.7;

/// Register every handler of `page` on `wv` (the old webview's handlers
/// are removed first). The handlers live in a local `Tokens` while they are
/// added, so no borrow is held across WebView2 calls; those registered
/// before a failure are kept.
pub(super) fn wire(page: &Rc<PageInner>, wv: &Wv) -> WinResult {
    let old = std::mem::take(&mut *page.tokens.borrow_mut());
    drop(old);
    // Navigation ids are the webview's own.
    page.refused.borrow_mut().clear();
    let mut t = Tokens::default();
    let r = wire_into(page, wv, &mut t);
    *page.tokens.borrow_mut() = t;
    r
}

fn wire_into(page: &Rc<PageInner>, wv: &Wv, t: &mut Tokens) -> WinResult {
    let view = &wv.view;
    let w = Rc::downgrade(page);

    // -- navigation --------------------------------------------------------
    let pw = w.clone();
    on!(
        t,
        view,
        NavigationStarting,
        NavigationStartingEventHandler,
        move |_, a: Option<ICoreWebView2NavigationStartingEventArgs>| {
            if let (Some(p), Some(a)) = (pw.upgrade(), a) {
                let url = get_string(|s| unsafe { a.Uri(s) }).ok();
                let redirected = get_bool(|b| unsafe { a.IsRedirected(b) }).unwrap_or(false);
                if p.navigation_allowed(&a, url.as_deref(), redirected) {
                    p.nav_started(url, redirected);
                }
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        view,
        ContentLoading,
        ContentLoadingEventHandler,
        move |v: Option<ICoreWebView2>, _| {
            if let (Some(p), Some(v)) = (pw.upgrade(), v) {
                let url = get_string(|s| unsafe { v.Source(s) }).ok();
                p.find.reset();
                // Requests of the old document are void, and so is its
                // file chooser.
                p.deny_pending_permissions();
                p.close_file_chooser(false);
                p.set_progress(PROGRESS_COMMITTED);
                script::bind_documents(&p);
                p.emit(PageEvent::Load(LoadState::Committed { url }));
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        view,
        SourceChanged,
        SourceChangedEventHandler,
        move |v: Option<ICoreWebView2>, _| {
            if let (Some(p), Some(v)) = (pw.upgrade(), v) {
                let url = get_string(|s| unsafe { v.Source(s) })
                    .ok()
                    .filter(|u| !u.is_empty());
                let changed = {
                    let mut st = p.status.borrow_mut();
                    let changed = st.url != url;
                    st.url = url.clone();
                    changed
                };
                if changed {
                    p.emit(PageEvent::Url(url));
                }
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        view,
        HistoryChanged,
        HistoryChangedEventHandler,
        move |_, _| {
            if let Some(p) = pw.upgrade() {
                p.refresh_history();
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        view,
        NavigationCompleted,
        NavigationCompletedEventHandler,
        move |_, a: Option<ICoreWebView2NavigationCompletedEventArgs>| {
            if let (Some(p), Some(a)) = (pw.upgrade(), a) {
                // Whatever became of it (refused too), the navigation is over.
                p.document_ready();
                if !p.completion_refused(&a) {
                    p.nav_completed(&a);
                }
            }
            Ok(())
        }
    )?;
    if let Ok(v2) = view.cast::<ICoreWebView2_2>() {
        let pw = w.clone();
        on!(
            t,
            v2,
            DOMContentLoaded,
            DOMContentLoadedEventHandler,
            move |_, _| {
                if let Some(p) = pw.upgrade() {
                    p.set_progress(PROGRESS_DOM_READY);
                    p.document_ready();
                    script::bind_documents(&p);
                }
                Ok(())
            }
        )?;
    }

    // -- page state ----------------------------------------------------------
    let pw = w.clone();
    on!(
        t,
        view,
        DocumentTitleChanged,
        DocumentTitleChangedEventHandler,
        move |v: Option<ICoreWebView2>, _| {
            if let (Some(p), Some(v)) = (pw.upgrade(), v) {
                let title = get_string(|s| unsafe { v.DocumentTitle(s) })
                    .ok()
                    .filter(|t| !t.is_empty());
                p.status.borrow_mut().title = title.clone();
                p.emit(PageEvent::Title(title));
            }
            Ok(())
        }
    )?;
    if let Ok(v15) = view.cast::<ICoreWebView2_15>() {
        let pw = w.clone();
        let v = v15.clone();
        on!(
            t,
            v15,
            FaviconChanged,
            FaviconChangedEventHandler,
            move |_, _| {
                if let Some(p) = pw.upgrade() {
                    p.favicon_changed(&v);
                }
                Ok(())
            }
        )?;
    }
    if let Ok(v12) = view.cast::<ICoreWebView2_12>() {
        let pw = w.clone();
        let v = v12.clone();
        on!(
            t,
            v12,
            StatusBarTextChanged,
            StatusBarTextChangedEventHandler,
            move |_, _| {
                if let Some(p) = pw.upgrade() {
                    let text = get_string(|s| unsafe { v.StatusBarText(s) })
                        .ok()
                        .filter(|t| !t.is_empty());
                    p.status_text_changed(text);
                }
                Ok(())
            }
        )?;
    }
    if let Ok(v8) = view.cast::<ICoreWebView2_8>() {
        let pw = w.clone();
        let v = v8.clone();
        on!(
            t,
            v8,
            IsDocumentPlayingAudioChanged,
            IsDocumentPlayingAudioChangedEventHandler,
            move |_, _| {
                if let Some(p) = pw.upgrade() {
                    let audio =
                        get_bool(|b| unsafe { v.IsDocumentPlayingAudio(b) }).unwrap_or(false);
                    p.status.borrow_mut().audio = audio;
                    p.emit(PageEvent::AudioPlaying(audio));
                }
                Ok(())
            }
        )?;
        let pw = w.clone();
        let v = v8.clone();
        on!(
            t,
            v8,
            IsMutedChanged,
            IsMutedChangedEventHandler,
            move |_, _| {
                if let Some(p) = pw.upgrade() {
                    let muted = get_bool(|b| unsafe { v.IsMuted(b) }).unwrap_or(false);
                    p.status.borrow_mut().muted = muted;
                    p.change(PageChange::Muted(muted));
                }
                Ok(())
            }
        )?;
    }
    let pw = w.clone();
    on!(
        t,
        wv.ctl,
        ZoomFactorChanged,
        ZoomFactorChangedEventHandler,
        move |c: Option<ICoreWebView2Controller>, _| {
            if let (Some(p), Some(c)) = (pw.upgrade(), c) {
                let zoom: f64 = get(|z| unsafe { c.ZoomFactor(z) }).unwrap_or(1.0);
                p.status.borrow_mut().zoom = zoom;
                p.change(PageChange::Zoom(zoom));
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        view,
        ContainsFullScreenElementChanged,
        ContainsFullScreenElementChangedEventHandler,
        move |v: Option<ICoreWebView2>, _| {
            if let (Some(p), Some(v)) = (pw.upgrade(), v) {
                let full = get_bool(|b| unsafe { v.ContainsFullScreenElement(b) }).unwrap_or(false);
                if full && p.backstaged() {
                    // Nothing a held page opens appears: leave full screen again.
                    p.profile.engine.backstage.blocked();
                    let js = hs("document.fullscreenElement && document.exitFullscreen()");
                    unsafe { v.ExecuteScript(&js, None)? };
                    return Ok(());
                }
                p.status.borrow_mut().fullscreen = full;
                p.change(PageChange::Fullscreen(full));
            }
            Ok(())
        }
    )?;

    // -- crashes, closing ------------------------------------------------------
    let pw = w.clone();
    on!(
        t,
        view,
        ProcessFailed,
        ProcessFailedEventHandler,
        move |_, a: Option<ICoreWebView2ProcessFailedEventArgs>| {
            if let (Some(p), Some(a)) = (pw.upgrade(), a) {
                let kind = get(|k| unsafe { a.ProcessFailedKind(k) })
                    .unwrap_or(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED);
                let reason = a
                    .cast::<ICoreWebView2ProcessFailedEventArgs2>()
                    .ok()
                    .and_then(|a2| {
                        get(|r: *mut COREWEBVIEW2_PROCESS_FAILED_REASON| unsafe { a2.Reason(r) })
                            .ok()
                    })
                    .map(|r| r.0);
                // Not inside the handler: handling it closes this webview.
                copper_win_rt::spawn_local(async move { p.process_failed(kind, reason) });
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        view,
        WindowCloseRequested,
        WindowCloseRequestedEventHandler,
        move |_, _| {
            if let Some(p) = pw.upgrade() {
                p.emit(PageEvent::CloseRequested);
            }
            Ok(())
        }
    )?;

    // -- permissions, popups, dialogs, menus ---------------------------------------
    let pw = w.clone();
    on!(
        t,
        view,
        PermissionRequested,
        PermissionRequestedEventHandler,
        move |_, a: Option<ICoreWebView2PermissionRequestedEventArgs>| {
            match (pw.upgrade(), a) {
                (Some(p), Some(a)) => p.permission_requested(a),
                _ => Ok(()),
            }
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        view,
        NewWindowRequested,
        NewWindowRequestedEventHandler,
        move |_, a: Option<ICoreWebView2NewWindowRequestedEventArgs>| {
            match (pw.upgrade(), a) {
                (Some(p), Some(a)) => p.new_window_requested(a),
                _ => Ok(()),
            }
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        view,
        ScriptDialogOpening,
        ScriptDialogOpeningEventHandler,
        move |_, a: Option<ICoreWebView2ScriptDialogOpeningEventArgs>| {
            if let (Some(p), Some(a)) = (pw.upgrade(), a) {
                p.swallow_dialog(&a);
            }
            Ok(())
        }
    )?;
    if let Ok(v11) = view.cast::<ICoreWebView2_11>() {
        let pw = w.clone();
        on!(
            t,
            v11,
            ContextMenuRequested,
            ContextMenuRequestedEventHandler,
            move |_, a: Option<ICoreWebView2ContextMenuRequestedEventArgs>| {
                match (pw.upgrade(), a) {
                    (Some(p), Some(a)) if p.backstaged() => {
                        unsafe { a.SetHandled(true)? };
                        p.profile.engine.backstage.blocked();
                        Ok(())
                    }
                    (Some(p), Some(a)) => p.context_menu_requested(&a),
                    _ => Ok(()),
                }
            }
        )?;
    }

    // -- messages, frames, resources, downloads ------------------------------------
    let pw = w.clone();
    on!(
        t,
        view,
        WebMessageReceived,
        WebMessageReceivedEventHandler,
        move |_, a: Option<ICoreWebView2WebMessageReceivedEventArgs>| {
            if let (Some(p), Some(a)) = (pw.upgrade(), a)
                && let Ok(json) = get_string(|s| unsafe { a.WebMessageAsJson(s) })
            {
                script::route_web_message(&p, &json);
            }
            Ok(())
        }
    )?;
    if let Ok(v4) = view.cast::<ICoreWebView2_4>() {
        let pw = w.clone();
        on!(
            t,
            v4,
            FrameCreated,
            FrameCreatedEventHandler,
            move |_, a: Option<ICoreWebView2FrameCreatedEventArgs>| {
                if let (Some(p), Some(a)) = (pw.upgrade(), a) {
                    p.frame_created(unsafe { a.Frame()? });
                }
                Ok(())
            }
        )?;
        let pw = w.clone();
        on!(
            t,
            v4,
            DownloadStarting,
            DownloadStartingEventHandler,
            move |_, a: Option<ICoreWebView2DownloadStartingEventArgs>| {
                match (pw.upgrade(), a) {
                    (Some(p), Some(a)) => {
                        downloads::take_over(&p.profile, &a, Some(Rc::downgrade(&p)), None, || {})
                    }
                    _ => Ok(()),
                }
            }
        )?;
    }
    let pw = w.clone();
    on!(
        t,
        view,
        WebResourceRequested,
        WebResourceRequestedEventHandler,
        move |v: Option<ICoreWebView2>, a: Option<ICoreWebView2WebResourceRequestedEventArgs>| {
            let (Some(p), Some(v), Some(a)) = (pw.upgrade(), v, a) else {
                return Ok(());
            };
            let uri = scheme::request_uri(&a)?;
            if !p.serve_html(&v, &a, &uri)? {
                p.serve_scheme(&v, &a, &uri)?;
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    cdp::subscribe(view, "Page.fileChooserOpened", t, move |params| {
        if let Some(p) = pw.upgrade() {
            p.file_chooser_opened(&params);
        }
    })?;
    let pw = w.clone();
    cdp::subscribe(view, "Runtime.bindingCalled", t, move |params| {
        if let Some(p) = pw.upgrade() {
            script::route_binding(&p, &params);
        }
    })?;

    // -- hosting: cursor, keys, focus -------------------------------------------------
    let pw = w.clone();
    on!(
        t,
        wv.comp,
        CursorChanged,
        CursorChangedEventHandler,
        move |c: Option<ICoreWebView2CompositionController>, _| {
            if let (Some(p), Some(c)) = (pw.upgrade(), c) {
                let mut cursor = Default::default();
                if unsafe { c.Cursor(&mut cursor) }.is_ok() {
                    p.host.cursor_changed(cursor);
                }
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        wv.ctl,
        AcceleratorKeyPressed,
        AcceleratorKeyPressedEventHandler,
        move |_, a: Option<ICoreWebView2AcceleratorKeyPressedEventArgs>| {
            if let (Some(p), Some(a)) = (pw.upgrade(), a)
                && let Ok(key) = accelerator_key(&a)
            {
                let handled = p.host.accelerator(&key);
                if matches!(
                    key.kind,
                    KeyEventKind::KeyDown | KeyEventKind::SystemKeyDown
                ) {
                    // Which keys reach Copper while a page has the
                    // keyboard (the chrome's own keys log as commands).
                    tracing::debug!(
                        target: "copper::keys",
                        vk = key.virtual_key,
                        ctrl = key.ctrl,
                        shift = key.shift,
                        alt = key.alt,
                        handled,
                        "key in the page"
                    );
                }
                if handled {
                    unsafe { a.SetHandled(true)? };
                }
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        wv.ctl,
        MoveFocusRequested,
        MoveFocusRequestedEventHandler,
        move |_, a: Option<ICoreWebView2MoveFocusRequestedEventArgs>| {
            if let (Some(p), Some(a)) = (pw.upgrade(), a) {
                let reason =
                    get(|r| unsafe { a.Reason(r) }).unwrap_or(COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT);
                let exit = if reason == COREWEBVIEW2_MOVE_FOCUS_REASON_PREVIOUS {
                    FocusExit::Previous
                } else {
                    FocusExit::Next
                };
                if p.host.focus_exit(exit) {
                    unsafe { a.SetHandled(true)? };
                }
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        wv.ctl,
        GotFocus,
        FocusChangedEventHandler,
        move |_, _| {
            if let Some(p) = pw.upgrade() {
                focus_changed(&p, true);
            }
            Ok(())
        }
    )?;
    let pw = w.clone();
    on!(
        t,
        wv.ctl,
        LostFocus,
        FocusChangedEventHandler,
        move |_, _| {
            if let Some(p) = pw.upgrade() {
                focus_changed(&p, false);
            }
            Ok(())
        }
    )?;
    Ok(())
}

impl PageInner {
    fn set_progress(&self, v: f64) {
        self.status.borrow_mut().progress = v;
        self.emit(PageEvent::Progress(v));
    }

    fn nav_started(&self, url: Option<String>, redirected: bool) {
        self.document_coming();
        self.status.borrow_mut().loading = true;
        self.schemes.navigation(url.clone());
        if redirected {
            self.emit(PageEvent::Load(LoadState::Redirected { url }));
        } else {
            self.set_progress(PROGRESS_STARTED);
            self.emit(PageEvent::Load(LoadState::Started { url }));
        }
    }

    fn nav_completed(self: &Rc<Self>, a: &ICoreWebView2NavigationCompletedEventArgs) {
        self.document_ready();
        script::bind_documents(self);
        let ok = get_bool(|b| unsafe { a.IsSuccess(b) }).unwrap_or(false);
        let http = a
            .cast::<ICoreWebView2NavigationCompletedEventArgs2>()
            .ok()
            .and_then(|a2| get(|s: *mut i32| unsafe { a2.HttpStatusCode(s) }).ok())
            .unwrap_or(0);
        let status = get(|s| unsafe { a.WebErrorStatus(s) })
            .unwrap_or(COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN);
        let url = self.status.borrow().url.clone();
        self.status.borrow_mut().loading = false;
        // A custom-scheme handler refused the document: its text is the
        // error, whatever WebView2 made of the unanswered request.
        if let Some(error) = self.schemes.take_failure() {
            self.nav.borrow_mut().cancel_pending();
            self.clear_html_filter();
            self.status.borrow_mut().progress = 1.0;
            self.emit(PageEvent::Load(LoadState::Failed {
                url: url.unwrap_or_default(),
                error,
                cancelled: false,
            }));
            self.refresh_history();
            return;
        }
        // An HTTP error page the server sent is a page, not a failed load.
        if ok || http >= 400 {
            self.set_progress(1.0);
            self.emit(PageEvent::Load(LoadState::Finished { url }));
        } else {
            tracing::debug!(status = status.0, http, "load failed: {url:?}");
            self.nav.borrow_mut().cancel_pending();
            self.clear_html_filter();
            self.status.borrow_mut().progress = 1.0;
            let knock = (status.0 == WEB_ERROR_UNKNOWN)
                .then(|| url.as_deref().and_then(knock_target))
                .flatten();
            // Chromium's own error page for a load being knocked on ends
            // "aborted": the knock answers for both.
            let knocked =
                status.0 == WEB_ERROR_ABORTED && url.is_some() && *self.knocking.borrow() == url;
            match knock {
                Some((host, port)) => self.fail_after_knock(url, host, port),
                None if knocked => tracing::debug!("its knock answers for it"),
                None => self.emit(PageEvent::Load(LoadState::Failed {
                    url: url.unwrap_or_default(),
                    error: web_error_name(status.0).to_owned(),
                    cancelled: status.0 == WEB_ERROR_CANCELED,
                })),
            }
        }
        self.refresh_history();
    }

    /// Report a failed `UNKNOWN` load once a knock on `host:port` told
    /// whether it was refused (WebView2 has no status for that).
    fn fail_after_knock(self: &Rc<Self>, url: Option<String>, host: String, port: u16) {
        let me = Rc::downgrade(self);
        let generation = self.generation.get();
        self.knocking.replace(url.clone());
        copper_win_rt::spawn_local(async move {
            let knock =
                crate::win::worker::run("copper-knock", move || refuses(&host, port, KNOCK_LIMIT));
            // Name lookup has no limit of its own.
            let refused = match race(
                knock,
                copper_win_rt::sleep(KNOCK_LIMIT + std::time::Duration::from_millis(900)),
            )
            .await
            {
                Either::Left(v) => v.unwrap_or(false),
                Either::Right(()) => false,
            };
            let Some(p) = me.upgrade() else { return };
            if *p.knocking.borrow() == url {
                p.knocking.replace(None);
            }
            // A newer load (or webview) took over meanwhile.
            if p.generation.get() != generation || p.status.borrow().loading {
                return;
            }
            let error = if refused {
                CONNECTION_REFUSED
            } else {
                web_error_name(WEB_ERROR_UNKNOWN)
            };
            p.emit(PageEvent::Load(LoadState::Failed {
                url: url.unwrap_or_default(),
                error: error.to_owned(),
                cancelled: false,
            }));
        });
    }

    /// Fold the webview's history into the page's URL stack.
    fn refresh_history(self: &Rc<Self>) {
        let Some(wv) = self.wv() else { return };
        let me = Rc::downgrade(self);
        let generation = self.generation.get();
        copper_win_rt::spawn_local(async move {
            let reply = cdp::call(&wv.view, "Page.getNavigationHistory", json!({})).await;
            let Some(p) = me.upgrade() else { return };
            if p.generation.get() != generation {
                return;
            }
            let native = reply
                .ok()
                .and_then(|v| serde_json::from_value::<NativeHistory>(v).ok());
            if let Some(native) = native
                && p.nav.borrow_mut().observe(&native)
            {
                p.change(PageChange::History);
            }
        });
    }

    fn favicon_changed(self: &Rc<Self>, v15: &ICoreWebView2_15) {
        let uri = get_string(|s| unsafe { v15.FaviconUri(s) }).unwrap_or_default();
        if uri.is_empty() {
            self.emit(PageEvent::Favicon(None));
            return;
        }
        let me = Rc::downgrade(self);
        let handler = GetFaviconCompletedHandler::create(Box::new(move |hr, stream| {
            let Some(p) = me.upgrade() else { return Ok(()) };
            let png = hr.ok().and(stream).and_then(|s| read_stream(&s).ok());
            let image = png.and_then(|png| {
                let (width, height) = png_size(&png)?;
                Some(Image { png, width, height })
            });
            p.emit(PageEvent::Favicon(image));
            Ok(())
        }));
        unsafe {
            let _ = v15.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &handler);
        }
    }

    /// The status text is the hovered link's URL (the shell draws it).
    fn status_text_changed(&self, text: Option<String>) {
        self.status.borrow_mut().status_text = text.clone();
        self.change(PageChange::StatusText(text.clone()));
        let hover = text
            .filter(|t| url::Url::parse(t).is_ok())
            .map(|url| LinkHover {
                url,
                title: None,
                label: None,
            });
        self.emit(PageEvent::LinkHover(hover));
    }

    fn process_failed(
        self: &Rc<Self>,
        kind: COREWEBVIEW2_PROCESS_FAILED_KIND,
        reason: Option<i32>,
    ) {
        match kind {
            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => {
                tracing::warn!(
                    url = ?self.status.borrow().url,
                    shown = self.host.rendering(),
                    "WebView2 browser process exited under the page"
                );
                if let Some(wv) = self.release_webview(Life::Dead) {
                    controller::close(&wv);
                }
                self.status.borrow_mut().loading = false;
                self.profile.engine.browser_died();
                // The page stays (its tab, its URL stack). The shell
                // decides whether it comes back now (copper-win `frame.rs`
                // reloads it, as the Mac's `Tab.swift` does, and says it
                // stopped once that keeps failing); a page that is
                // reloaded, attached or used is rebuilt then. Only a page
                // the backstage shows (no shell frame over it) comes back
                // at once, as before.
                self.emit(PageEvent::Crashed(CrashReason::Crashed));
                if self.host.in_stage() && self.host.rendering() {
                    self.wake();
                }
            }
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => {
                self.status.borrow_mut().loading = false;
                self.close_file_chooser(false);
                let why = crash_reason(reason, self.terminating.replace(false));
                self.emit(PageEvent::Crashed(why));
            }
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => {
                tracing::warn!("page renderer unresponsive: {:?}", self.status.borrow().url);
            }
            _ => {}
        }
    }

    fn permission_requested(&self, a: ICoreWebView2PermissionRequestedEventArgs) -> WinResult {
        let kind = get(|k| unsafe { a.PermissionKind(k) })?;
        if self.backstaged() {
            unsafe { a.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)? };
            self.profile.engine.backstage.blocked();
            return Ok(());
        }
        let url = get_string(|s| unsafe { a.Uri(s) })
            .ok()
            .filter(|u| !u.is_empty());
        if let Ok(a3) = a.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
            // Answers live in the shell's permission store, not Edge's.
            unsafe { a3.SetSavesInProfile(false)? };
        }
        let deferral = unsafe { a.GetDeferral()? };
        let id = PermissionId(crate::util::next_id());
        self.permissions
            .borrow_mut()
            .insert(id, PendingPermission { args: a, deferral });
        self.emit(PageEvent::PermissionRequest {
            id,
            kind: permission_kind(kind.0),
            url,
        });
        Ok(())
    }

    fn new_window_requested(
        self: &Rc<Self>,
        a: ICoreWebView2NewWindowRequestedEventArgs,
    ) -> WinResult {
        let url = get_string(|s| unsafe { a.Uri(s) })
            .ok()
            .filter(|u| !u.is_empty());
        let user_gesture = get_bool(|b| unsafe { a.IsUserInitiated(b) }).unwrap_or(false);
        let frame_name = a
            .cast::<ICoreWebView2NewWindowRequestedEventArgs2>()
            .ok()
            .and_then(|a2| get_string(|s| unsafe { a2.Name(s) }).ok())
            .filter(|n| !n.is_empty());
        let new_tab = self
            .modified_click
            .take()
            .is_some_and(|t| t.elapsed() < MODIFIED_CLICK_WINDOW);
        let disposition = if new_tab {
            WindowDisposition::NewTab
        } else {
            WindowDisposition::Popup
        };
        let req = NewWindowRequest {
            url,
            user_gesture,
            frame_name,
            disposition,
        };
        let popup = if new_tab {
            None
        } else {
            // Taken out while it runs, so it may replace itself.
            let handler = self.new_window.borrow_mut().take();
            let page = handler.as_ref().and_then(|h| h(&req));
            if handler.is_some() && self.new_window.borrow().is_none() {
                *self.new_window.borrow_mut() = handler;
            }
            page
        };
        match popup {
            Some(popup)
                if popup.inner.related && Rc::ptr_eq(&popup.inner.profile, &self.profile) =>
            {
                let deferral = unsafe { a.GetDeferral()? };
                become_popup(
                    &popup,
                    Opener {
                        args: a,
                        deferral: Some(deferral),
                    },
                );
            }
            other => {
                if other.is_some() {
                    tracing::warn!(
                        "new-window handler returned a page not made by new_related; opening a tab"
                    );
                }
                unsafe { a.SetHandled(true)? };
                self.emit(PageEvent::NewWindow(req));
            }
        }
        Ok(())
    }

    /// A dialog of a page held in the backstage (default dialogs are off
    /// then): alerts and `beforeunload` are accepted, confirms and prompts
    /// declined, nothing is shown.
    fn swallow_dialog(&self, a: &ICoreWebView2ScriptDialogOpeningEventArgs) {
        let kind = get(|k| unsafe { a.Kind(k) }).unwrap_or(COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT);
        if kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT
            || kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD
        {
            unsafe {
                let _ = a.Accept();
            }
        }
        self.profile.engine.backstage.blocked();
    }

    /// Route a child frame's page-world messages (and its children's).
    /// The handlers go with [`FrameHandlers`] (see there for why a
    /// destroyed frame's are never removed).
    fn frame_created(self: &Rc<Self>, frame: ICoreWebView2Frame) {
        let mut tokens = Tokens::default();
        let w = Rc::downgrade(self);
        if let Ok(f2) = frame.cast::<ICoreWebView2Frame2>() {
            let pw = w.clone();
            let r = on!(
                tokens,
                f2,
                FrameWebMessageReceived,
                FrameWebMessageReceivedEventHandler,
                move |_, a: Option<ICoreWebView2WebMessageReceivedEventArgs>| {
                    if let (Some(p), Some(a)) = (pw.upgrade(), a)
                        && let Ok(json) = get_string(|s| unsafe { a.WebMessageAsJson(s) })
                    {
                        script::route_web_message(&p, &json);
                    }
                    Ok(())
                }
            );
            if let Err(e) = r {
                tracing::debug!("frame messages: {e}");
            }
            // A child document's isolated worlds need their bindings too.
            let pw = w.clone();
            let r = on!(
                tokens,
                f2,
                FrameDOMContentLoaded,
                FrameDOMContentLoadedEventHandler,
                move |_, _| {
                    if let Some(p) = pw.upgrade() {
                        script::bind_documents(&p);
                    }
                    Ok(())
                }
            );
            if let Err(e) = r {
                tracing::debug!("frame documents: {e}");
            }
        }
        if let Ok(f7) = frame.cast::<ICoreWebView2Frame7>() {
            let pw = w.clone();
            let r = on!(
                tokens,
                f7,
                FrameChildFrameCreated,
                FrameChildFrameCreatedEventHandler,
                move |_, a: Option<ICoreWebView2FrameCreatedEventArgs>| {
                    if let (Some(p), Some(a)) = (pw.upgrade(), a) {
                        p.frame_created(unsafe { a.Frame()? });
                    }
                    Ok(())
                }
            );
            if let Err(e) = r {
                tracing::debug!("child frames: {e}");
            }
        }
        let pw = w.clone();
        let gone = frame.clone();
        let r = on!(
            tokens,
            frame,
            FrameDestroyed,
            FrameDestroyedEventHandler,
            move |_, _| {
                let pw = pw.clone();
                let gone = gone.clone();
                // Not inside the frame's own callback: removing its handlers
                // there would release the handler that is running.
                copper_win_rt::spawn_local(async move {
                    if let Some(p) = pw.upgrade() {
                        // Dropped (a WebView2 call) after the borrow ends.
                        let removed: Vec<FrameHandlers> = {
                            let mut frames = p.frames.borrow_mut();
                            let (keep, removed) = std::mem::take(&mut *frames)
                                .into_iter()
                                .partition(|h| h.frame != gone);
                            *frames = keep;
                            removed
                        };
                        drop(removed);
                    }
                });
                Ok(())
            }
        );
        if let Err(e) = r {
            tracing::debug!("frame lifetime: {e}");
        }
        self.frames
            .borrow_mut()
            .push(FrameHandlers { frame, tokens });
    }

    /// Answer the one request `load_html` registered a filter for; `false`
    /// if `uri` is not it.
    fn serve_html(
        &self,
        v: &ICoreWebView2,
        a: &ICoreWebView2WebResourceRequestedEventArgs,
        uri: &str,
    ) -> windows::core::Result<bool> {
        let html = match &*self.html.borrow() {
            Some((url, html)) if same_document(url, uri) => html.clone(),
            _ => return Ok(false),
        };
        self.clear_html_filter_on(v);
        let env = unsafe { v.cast::<ICoreWebView2_2>()?.Environment()? };
        let stream = unsafe { SHCreateMemStream(Some(html.as_bytes())) }
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY))?;
        let resp = unsafe {
            env.CreateWebResourceResponse(
                &stream,
                200,
                &hs("OK"),
                &hs("Content-Type: text/html; charset=utf-8\r\nCache-Control: no-store"),
            )?
        };
        unsafe { a.SetResponse(&resp)? };
        Ok(true)
    }

    /// Drop pending `load_html` content (and its filter, if the webview
    /// is there).
    pub(super) fn clear_html_filter(&self) {
        match self.wv() {
            Some(wv) => self.clear_html_filter_on(&wv.view),
            None => drop(self.html.borrow_mut().take()),
        }
    }

    /// Take the `load_html` content off `v` (its filter, then the content).
    /// The URL is taken out first: removing the filter calls into WebView2.
    pub(super) fn clear_html_filter_on(&self, v: &ICoreWebView2) {
        let served = self.html.borrow_mut().take();
        if let Some((url, _)) = served {
            unsafe {
                let _ = v.RemoveWebResourceRequestedFilter(
                    &hs(&url),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                );
            }
        }
    }
}

/// A child frame the page routes messages for, with the handlers it
/// registered on it (`PageInner::frames`).
///
/// Dropping it removes the handlers only while the frame lives. Removing
/// handlers from a destroyed frame is not merely useless: in WebView2
/// (seen on runtime 155) `ICoreWebView2Frame7::remove_FrameCreated` for the
/// frame's last `FrameChildFrameCreated` handler calls
/// `EmbeddedBrowserWebViewFrameCore::UpdateHasFrameCreatedEventHandlers`,
/// which `CHECK`s that the frame still exists, with no destroyed-frame
/// guard (`add_FrameCreated` has one) — a breakpoint (`0x80000003`) that
/// kills the *app* process. Pages that create and remove iframes (Google
/// search, sign-in pages, ads) destroy frames all the time, and so does
/// every navigation away from a page with iframes. A destroyed frame's
/// handlers are released with the frame object instead.
pub(crate) struct FrameHandlers {
    pub frame: ICoreWebView2Frame,
    tokens: Tokens,
}

impl Drop for FrameHandlers {
    fn drop(&mut self) {
        // `IsDestroyed` is true whenever the frame's link to the browser
        // (what the CHECK tests) is gone; if it cannot be asked, the
        // frame is treated as gone too.
        let destroyed = get_bool(|b| unsafe { self.frame.IsDestroyed(b) }).unwrap_or(true);
        if destroyed {
            self.tokens.forget();
        } else {
            self.tokens.clear();
        }
    }
}

/// Whether request URI `b` is the document `a` (ignoring the fragment and
/// URL normalisation).
pub(super) fn same_document(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        url::Url::parse(s).map(|mut u| {
            u.set_fragment(None);
            u.to_string()
        })
    };
    match (norm(a), norm(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The opener's `NewWindowRequested`, held (by deferral) until the popup's
/// webview exists. Completing without a webview (the popup page was
/// dropped, or making its webview failed) marks the request handled, so no
/// window opens.
struct Opener {
    args: ICoreWebView2NewWindowRequestedEventArgs,
    deferral: Option<ICoreWebView2Deferral>,
}

impl Opener {
    fn complete(mut self, view: Option<&ICoreWebView2>) {
        unsafe {
            match view {
                Some(v) => {
                    if let Err(e) = self.args.SetNewWindow(v) {
                        tracing::warn!("popup: {e}");
                        let _ = self.args.SetHandled(true);
                    }
                }
                None => {
                    let _ = self.args.SetHandled(true);
                }
            }
            if let Some(d) = self.deferral.take() {
                let _ = d.Complete();
            }
        }
    }
}

impl Drop for Opener {
    fn drop(&mut self) {
        if let Some(d) = self.deferral.take() {
            unsafe {
                let _ = self.args.SetHandled(true);
                let _ = d.Complete();
            }
        }
    }
}

/// Give the popup page's webview to the opener's request, as soon as it
/// exists (it must not have navigated before).
fn become_popup(popup: &WebView2Page, opener: Opener) {
    popup
        .inner
        .with_view_or_fail(move |_, wv| opener.complete(wv.map(|w| &w.view)));
}

impl PageInner {
    /// Like `with_view`, but `op` also runs (with `None`) when there will be
    /// no webview.
    fn with_view_or_fail(self: &Rc<Self>, op: impl FnOnce(&Rc<PageInner>, Option<&Wv>) + 'static) {
        if let Some(wv) = self.wv() {
            op(self, Some(&wv));
            return;
        }
        self.push_op(Box::new(move |p, wv| op(p, wv.ok())));
        self.wake();
    }
}
