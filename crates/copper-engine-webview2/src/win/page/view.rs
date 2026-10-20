//! The `PageView` impl: navigation and state here, the rest delegated to
//! the sibling modules.

use std::rc::Rc;

use copper_engine::{
    DownloadId, EventStream, FindOptions, InputEvent, JsonMap, JsonValue, LoadState, LocalFuture,
    NewWindowHandler, PageEvent, PageOptions, PageView, PermissionId, Result, ScriptId, Snapshot,
    SnapshotRegion, StyleSheetId, Trust, UserScript, UserStyleSheet, WebsiteDataKinds, World,
};
use serde_json::json;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::core::Interface;

use super::{PageInner, WebView2Page, capture, input, life, script};
use crate::filter::WebView2ContentFilter;
use crate::history::Step;
use crate::util::url_host;
use crate::win::cdp;
use crate::win::com::{hs, ready};
use crate::win::controller::Wv;

/// Placeholder origin for `load_html` without a base URL (`.invalid` never
/// resolves; the document is served from memory).
const HTML_BASE: &str = "https://copper.invalid/";

/// Navigate the webview; a URL WebView2 refuses fails the load at once.
pub(super) fn navigate(page: &Rc<PageInner>, wv: &Wv, url: &str) {
    page.document_coming();
    if let Err(e) = unsafe { wv.view.Navigate(&hs(url)) } {
        page.nav.borrow_mut().cancel_pending();
        page.emit(PageEvent::Load(LoadState::Failed {
            url: url.to_owned(),
            error: format!("cannot load this address ({e})"),
            cancelled: false,
        }));
    }
}

/// Serve `html` as the document at `url` (WebResourceRequested answers the
/// one request; see `events::serve_html`).
fn navigate_html(page: &Rc<PageInner>, wv: &Wv, html: String, url: String) {
    // An earlier `load_html` not served yet is void.
    page.clear_html_filter_on(&wv.view);
    // Filters match request URIs, which carry no fragment.
    let doc = match url::Url::parse(&url) {
        Ok(mut u) => {
            u.set_fragment(None);
            u.to_string()
        }
        Err(_) => url.clone(),
    };
    let filter = unsafe {
        wv.view
            .AddWebResourceRequestedFilter(&hs(&doc), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT)
    };
    if let Err(e) = filter {
        tracing::warn!("load_html: {e}");
        // NavigateToString takes at most 2 MB of UTF-16.
        if html.len() < 1_000_000 {
            page.document_coming();
            let _ = unsafe { wv.view.NavigateToString(&hs(&html)) };
        }
        return;
    }
    *page.html.borrow_mut() = Some((doc, html));
    navigate(page, wv, &url);
}

fn go(page: &Rc<PageInner>, delta: isize) {
    let Some(step) = page.nav.borrow_mut().go(delta) else {
        return;
    };
    match step {
        Step::Native(id) => page.navigate_with(move |_, wv| {
            let view = wv.view.clone();
            copper_win_rt::spawn_local(async move {
                let params = json!({ "entryId": id });
                if let Err(e) = cdp::call(&view, "Page.navigateToHistoryEntry", params).await {
                    tracing::debug!("history traversal: {e}");
                    // Fall back to WebView2's own traversal.
                    let _ = if delta < 0 {
                        unsafe { view.GoBack() }
                    } else {
                        unsafe { view.GoForward() }
                    };
                }
            });
        }),
        Step::Load(url) => page.navigate_with(move |p, wv| navigate(p, wv, &url)),
    }
}

impl PageView for WebView2Page {
    type Weak = super::WebView2WeakPage;

    fn page_id(&self) -> copper_engine::PageId {
        self.inner.id
    }

    fn downgrade(&self) -> Self::Weak {
        super::WebView2WeakPage {
            inner: Rc::downgrade(&self.inner),
        }
    }

    /// The page's bounds over its rasterization scale; `(0, 0)` before the
    /// shell (or the backstage) ever laid it out.
    fn view_size(&self) -> (u32, u32) {
        self.inner.host.logical_size()
    }

    /// Attached (to a shell window or the backstage's stage) and visible:
    /// Chromium is rendering it.
    fn is_mapped(&self) -> bool {
        self.inner.host.rendering()
    }

    /// The `Visible` snapshot's frame, decoded into straight-alpha RGBA on a
    /// worker thread (WebView2 only hands out encoded frames).
    fn snapshot_bitmap(&self) -> LocalFuture<Result<copper_engine::Bitmap>> {
        capture::snapshot_bitmap(&self.inner)
    }

    fn trusted_input(&self) -> copper_engine::TrustedInput {
        if self.inner.script_input.get() {
            copper_engine::TrustedInput::NONE
        } else {
            input::CDP_TRUSTED
        }
    }

    /// `false`: every action from script (`input.js`), and no page focus
    /// emulation for a held page; `true`: back to CDP.
    fn set_trusted_input(&self, enabled: bool) {
        if self.inner.script_input.replace(!enabled) != !enabled {
            self.inner.refresh_backstaged();
        }
    }

    fn has_file_chooser(&self) -> bool {
        self.inner.chooser.is_open()
    }

    fn cancel_file_chooser(&self) -> bool {
        self.inner.close_file_chooser(true)
    }

    type Filter = WebView2ContentFilter;

    fn load(&self, url: &str) {
        let url = url.to_owned();
        self.inner.nav.borrow_mut().cancel_pending();
        self.inner.clear_html_filter();
        self.inner.navigate_with(move |p, wv| navigate(p, wv, &url));
    }

    /// Served from memory as the response for `base_url` (so the document
    /// has that origin, and relative URLs resolve against it). Without a
    /// base URL the origin is `https://copper.invalid`.
    fn load_html(&self, html: &str, base_url: Option<&str>) {
        let html = html.to_owned();
        let url = base_url.unwrap_or(HTML_BASE).to_owned();
        self.inner.nav.borrow_mut().cancel_pending();
        self.inner
            .navigate_with(move |p, wv| navigate_html(p, wv, html, url));
    }

    fn back(&self) {
        go(&self.inner, -1);
    }

    fn forward(&self) {
        go(&self.inner, 1);
    }

    fn can_go_back(&self) -> bool {
        self.inner.nav.borrow().can_go(-1)
    }

    fn can_go_forward(&self) -> bool {
        self.inner.nav.borrow().can_go(1)
    }

    fn reload(&self, bypass_cache: bool) {
        if self.inner.wv().is_none() {
            // No webview (asleep, its browser process gone, or still being
            // made): the next one loads the current entry afresh. A queued
            // `Reload` would reload that webview's first document,
            // about:blank, instead.
            self.inner.wake();
            return;
        }
        self.inner.navigate_with(move |_, wv| {
            if bypass_cache {
                let view = wv.view.clone();
                copper_win_rt::spawn_local(async move {
                    let r = cdp::call(&view, "Page.reload", json!({ "ignoreCache": true })).await;
                    if r.is_err() {
                        let _ = unsafe { view.Reload() };
                    }
                });
            } else {
                let _ = unsafe { wv.view.Reload() };
            }
        });
    }

    /// WebView2 cannot clear one site's HTTP cache, so this clears the
    /// profile's whole HTTP cache (more than asked, never less) and reloads
    /// from the origin. Resolves to the page's site.
    fn hard_reload(&self) -> LocalFuture<Result<Vec<String>>> {
        let page = self.inner.clone();
        Box::pin(async move {
            let url = page.status.borrow().url.clone();
            let host = url.as_deref().and_then(url_host);
            let mut sites = Vec::new();
            if let Some(host) = host {
                let kinds = WebsiteDataKinds::MEMORY_CACHE | WebsiteDataKinds::DISK_CACHE;
                sites = page.profile.clear_data(Some(host), kinds).await?;
            }
            WebView2Page { inner: page }.reload(true);
            Ok(sites)
        })
    }

    fn stop(&self) {
        self.inner.with_view(|_, wv| {
            let _ = unsafe { wv.view.Stop() };
        });
    }

    fn url(&self) -> Option<String> {
        self.inner.status.borrow().url.clone()
    }

    fn title(&self) -> Option<String> {
        self.inner.status.borrow().title.clone()
    }

    fn is_loading(&self) -> bool {
        self.inner.status.borrow().loading
    }

    /// WebView2 reports no load progress; this steps through the load's
    /// milestones (started, committed, DOM ready, finished).
    fn progress(&self) -> f64 {
        self.inner.status.borrow().progress
    }

    fn is_audio_playing(&self) -> bool {
        self.inner.status.borrow().audio
    }

    fn set_muted(&self, muted: bool) {
        self.inner.status.borrow_mut().muted = muted;
        self.inner.with_view(move |_, wv| {
            if let Ok(v8) = wv.view.cast::<ICoreWebView2_8>() {
                let _ = unsafe { v8.SetIsMuted(muted) };
            }
        });
    }

    fn zoom(&self) -> f64 {
        self.inner.status.borrow().zoom
    }

    fn set_zoom(&self, level: f64) {
        if !level.is_finite() || level <= 0.0 {
            return;
        }
        let level = level.clamp(0.25, 5.0);
        self.inner.status.borrow_mut().zoom = level;
        self.inner.with_view(move |_, wv| {
            let _ = unsafe { wv.ctl.SetZoomFactor(level) };
        });
    }

    fn eval(&self, js: &str, world: &World) -> LocalFuture<Result<JsonValue>> {
        script::eval(&self.inner, js.to_owned(), world.clone())
    }

    fn call_async(
        &self,
        body: &str,
        args: &JsonMap,
        world: &World,
    ) -> LocalFuture<Result<JsonValue>> {
        match crate::cdp::call_async_expression(body, args) {
            Ok(expr) => script::evaluate_async(&self.inner, expr, world.clone()),
            Err(e) => ready(Err(e)),
        }
    }

    fn add_message_handler(&self, name: &str, world: &World) -> bool {
        script::add_handler(&self.inner, name, world)
    }

    fn remove_message_handler(&self, name: &str, world: &World) {
        script::remove_handler(&self.inner, name, world);
    }

    fn add_user_script(&self, s: &UserScript) -> ScriptId {
        ScriptId(script::add_entry(
            &self.inner,
            script::Entry::Script(s.clone()),
        ))
    }

    fn remove_user_script(&self, id: ScriptId) -> bool {
        script::remove_entry(&self.inner, id.0)
    }

    fn add_user_style_sheet(&self, sheet: &UserStyleSheet) -> StyleSheetId {
        StyleSheetId(script::add_entry(
            &self.inner,
            script::Entry::Sheet(sheet.clone()),
        ))
    }

    fn remove_user_style_sheet(&self, id: StyleSheetId) -> bool {
        script::remove_entry(&self.inner, id.0)
    }

    /// Filters are WebView2 browser extensions, which apply to a whole
    /// profile: a profile blocks what the union of its pages' filters
    /// blocks (Copper gives every page of a profile the same filters).
    fn set_content_filter(&self, filter: Option<&WebView2ContentFilter>) {
        *self.inner.filters.borrow_mut() = filter.into_iter().cloned().collect();
        self.inner.profile.filters_changed();
    }

    fn add_content_filter(&self, filter: &WebView2ContentFilter) {
        {
            let mut f = self.inner.filters.borrow_mut();
            let id = copper_engine::ContentFilter::id(filter);
            f.retain(|x| copper_engine::ContentFilter::id(x) != id);
            f.push(filter.clone());
        }
        self.inner.profile.filters_changed();
    }

    fn remove_content_filter(&self, id: &str) {
        self.inner
            .filters
            .borrow_mut()
            .retain(|x| copper_engine::ContentFilter::id(x) != id);
        self.inner.profile.filters_changed();
    }

    fn snapshot(&self, region: SnapshotRegion) -> LocalFuture<Result<Snapshot>> {
        capture::snapshot(&self.inner, region)
    }

    fn find(&self, query: &str, opts: FindOptions) -> LocalFuture<Result<u32>> {
        capture::find(&self.inner, query.to_owned(), opts)
    }

    fn find_next(&self) {
        capture::find_step(&self.inner, true);
    }

    fn find_previous(&self) {
        capture::find_step(&self.inner, false);
    }

    fn find_finish(&self) {
        capture::find_finish(&self.inner);
    }

    fn events(&self) -> EventStream<PageEvent> {
        self.inner.sink.subscribe()
    }

    fn answer_permission(&self, id: PermissionId, allow: bool) -> bool {
        let Some(p) = self.inner.permissions.borrow_mut().remove(&id) else {
            return false;
        };
        let state = if allow {
            COREWEBVIEW2_PERMISSION_STATE_ALLOW
        } else {
            COREWEBVIEW2_PERMISSION_STATE_DENY
        };
        unsafe {
            let _ = p.args.SetState(state);
            let _ = p.deferral.Complete();
        }
        true
    }

    /// A page for a popup's `window.opener`: same profile, a fresh webview
    /// that becomes the popup's (`NewWindowRequested.NewWindow`) when the
    /// new-window handler returns it.
    fn new_related(&self, opts: PageOptions) -> WebView2Page {
        WebView2Page::create(&self.inner.profile, opts, true)
    }

    fn set_new_window_handler(&self, handler: Option<NewWindowHandler<WebView2Page>>) {
        // The old handler (and what it holds, maybe a page) is dropped after
        // the borrow ends.
        let old = std::mem::replace(&mut *self.inner.new_window.borrow_mut(), handler);
        drop(old);
    }

    fn download(&self, url: &str) -> Result<DownloadId> {
        self.inner
            .profile
            .start_download(url, Some(Rc::downgrade(&self.inner)))
    }

    /// CDP `Input.*`: every event is trusted (see `input_plan`).
    fn input(&self, event: InputEvent) -> LocalFuture<Result<Trust>> {
        input::input(&self.inner, event)
    }

    /// Opens Edge DevTools in its own window (needs
    /// `PageOptions::developer_extras`).
    fn show_inspector(&self) {
        if !self.inner.opts.developer_extras {
            tracing::info!("show_inspector: developer_extras is off for this page");
            return;
        }
        self.inner.with_view(|_, wv| {
            let _ = unsafe { wv.view.OpenDevToolsWindow() };
        });
    }

    fn terminate_web_process(&self) {
        life::terminate(&self.inner);
    }
}
