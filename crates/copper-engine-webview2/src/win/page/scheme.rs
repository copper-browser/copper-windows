//! Serving the engine's custom schemes to one page (see `win::schemes`).
//!
//! Each webview gets one `WebResourceRequested` filter per scheme, for
//! every resource context and — where the runtime has
//! `ICoreWebView2_22` — every request source (documents, shared and
//! service workers). A request is answered synchronously from the
//! handler's body; a handler error leaves the request unanswered (it
//! fails), and when it was the page's navigation, the load fails with the
//! handler's text (`nav_completed`).

use std::cell::RefCell;
use std::collections::BTreeSet;

use copper_engine::{SchemeBody, SchemeRequest};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::Interface;

use super::PageInner;
use crate::scheme::{filter_pattern, response_headers, scheme_of};
use crate::win::com::{get_string, hs};
use crate::win::controller::Wv;

type WinResult<T = ()> = windows::core::Result<T>;

/// A page's part in scheme serving.
#[derive(Default)]
pub(crate) struct PageSchemes {
    /// Schemes with a filter on the current webview.
    filtered: RefCell<BTreeSet<String>>,
    /// The URL the current navigation asked for (after redirects).
    navigating: RefCell<Option<String>>,
    /// The handler's error for the current navigation's document.
    failed: RefCell<Option<String>>,
}

impl PageSchemes {
    /// A new navigation (or redirect) to `url` began.
    pub fn navigation(&self, url: Option<String>) {
        *self.navigating.borrow_mut() = url;
        self.failed.borrow_mut().take();
    }

    /// The handler's error for the navigation that just ended, if it failed
    /// there.
    pub fn take_failure(&self) -> Option<String> {
        self.navigating.borrow_mut().take();
        self.failed.borrow_mut().take()
    }

    /// The webview went: its filters went with it.
    pub fn released(&self) {
        self.filtered.borrow_mut().clear();
        self.navigating.borrow_mut().take();
        self.failed.borrow_mut().take();
    }
}

impl PageInner {
    /// Filter every registered scheme on the page's new webview (before it
    /// first navigates).
    pub(super) fn add_scheme_filters(&self, wv: &Wv) {
        for scheme in self.profile.engine.schemes.names() {
            self.filter_scheme(&wv.view, &scheme);
        }
    }

    /// A scheme was registered: filter it on the current webview (a page
    /// without one gets it when its webview is made).
    pub(crate) fn scheme_added(&self, scheme: &str) {
        if let Some(wv) = self.wv() {
            self.filter_scheme(&wv.view, scheme);
        }
    }

    fn filter_scheme(&self, view: &ICoreWebView2, scheme: &str) {
        if !self.schemes.filtered.borrow_mut().insert(scheme.to_owned()) {
            return;
        }
        let pattern = hs(&filter_pattern(scheme));
        let r = match view.cast::<ICoreWebView2_22>() {
            Ok(v22) => unsafe {
                v22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    &pattern,
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )
            },
            Err(_) => unsafe {
                view.AddWebResourceRequestedFilter(&pattern, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
            },
        };
        if let Err(e) = r {
            self.schemes.filtered.borrow_mut().remove(scheme);
            tracing::warn!("{scheme}: requests filter: {e}");
        }
    }

    /// Answer a request for a registered scheme from its handler; `false`
    /// if `uri` is not one.
    pub(super) fn serve_scheme(
        &self,
        v: &ICoreWebView2,
        a: &ICoreWebView2WebResourceRequestedEventArgs,
        uri: &str,
    ) -> WinResult<bool> {
        let Some(scheme) = scheme_of(uri) else {
            return Ok(false);
        };
        // Cloned out: the handler runs with nothing borrowed.
        let Some(handler) = self.profile.engine.schemes.handler(&scheme) else {
            return Ok(false);
        };
        let request = SchemeRequest {
            uri: uri.to_owned(),
            page: Some(self.id),
        };
        match handler(&request) {
            Ok(body) => {
                let response = self.scheme_response(v, &body)?;
                unsafe { a.SetResponse(&response)? };
            }
            Err(text) => {
                tracing::debug!("{uri}: handler failed: {text}");
                let navigation = self.schemes.navigating.borrow().clone();
                if navigation.is_some_and(|n| super::events::same_document(&n, uri)) {
                    *self.schemes.failed.borrow_mut() = Some(text);
                }
            }
        }
        Ok(true)
    }

    fn scheme_response(
        &self,
        v: &ICoreWebView2,
        body: &SchemeBody,
    ) -> WinResult<ICoreWebView2WebResourceResponse> {
        let env = match self.profile.engine.env.now() {
            Some(env) => env,
            None => unsafe { v.cast::<ICoreWebView2_2>()?.Environment()? },
        };
        let stream = unsafe { SHCreateMemStream(Some(&body.data)) }
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY))?;
        unsafe {
            env.CreateWebResourceResponse(
                &stream,
                200,
                &hs("OK"),
                &hs(&response_headers(&body.mime)),
            )
        }
    }
}

/// The request's URI.
pub(super) fn request_uri(a: &ICoreWebView2WebResourceRequestedEventArgs) -> WinResult<String> {
    let req = unsafe { a.Request()? };
    get_string(|s| unsafe { req.Uri(s) })
}
