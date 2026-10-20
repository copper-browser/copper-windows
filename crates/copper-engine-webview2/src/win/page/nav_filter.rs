//! The page's navigation filter (`WebView2Page::set_navigation_filter`):
//! every main-frame `NavigationStarting` asked, synchronously, and a refused
//! navigation cancelled before the page's events or URL stack hear of it.

use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2NavigationStartingEventArgs,
};

use super::events::same_document;
use super::{PageInner, WebView2Page};
use crate::navigation::NavigationRequest;
use crate::win::com::{get, get_bool};

/// Decides whether a main-frame navigation may start.
pub(crate) type NavigationFilter = Rc<dyn Fn(&NavigationRequest) -> bool>;

impl WebView2Page {
    /// Decide every main-frame navigation of this page before it starts,
    /// synchronously on the UI thread (inside WebView2's
    /// `NavigationStarting`): `f` returning `false` cancels it. A refused
    /// navigation sends no `LoadState::Started` (nor anything after it) and
    /// never enters the page's back/forward list; a refused *redirect*
    /// (`redirected`, its load already started) ends that load with
    /// `LoadState::Failed { cancelled: true }`. Covers every main-frame
    /// navigation: links, script, `PageView::load` (a `load_html` without a
    /// base URL is asked about `https://copper.invalid/`), back/forward to
    /// a URL, and the page's reload when its webview is rebuilt. The filter
    /// survives the webview being rebuilt (sleep, crash). `f` must not call
    /// back into the page. `None` removes it.
    #[allow(
        clippy::type_complexity,
        reason = "the agreed signature spells the filter's type out"
    )]
    pub fn set_navigation_filter(&self, f: Option<Box<dyn Fn(&NavigationRequest) -> bool>>) {
        *self.inner.nav_filter.borrow_mut() = f.map(Rc::from);
    }
}

impl PageInner {
    /// Ask the filter about a starting navigation; `false`: refused (and
    /// cancelled), so the page reports nothing for it.
    pub(super) fn navigation_allowed(
        &self,
        args: &ICoreWebView2NavigationStartingEventArgs,
        url: Option<&str>,
        redirected: bool,
    ) -> bool {
        // Out of the cell before it runs.
        let Some(filter) = self.nav_filter.borrow().clone() else {
            return true;
        };
        let request = NavigationRequest {
            url: url.unwrap_or_default().to_owned(),
            user_initiated: get_bool(|b| unsafe { args.IsUserInitiated(b) }).unwrap_or(false),
            redirected,
        };
        if filter(&request) {
            return true;
        }
        if let Err(e) = unsafe { args.SetCancel(true) } {
            // It goes ahead: report it like any other.
            tracing::warn!("navigation filter: cannot cancel {}: {e}", request.url);
            return true;
        }
        // A redirect's load has started: its cancelled completion ends it.
        if !redirected && let Ok(id) = get(|i: *mut u64| unsafe { args.NavigationId(i) }) {
            self.refused.borrow_mut().add(id);
        }
        self.nav.borrow_mut().cancel_pending_to(&request.url);
        self.forget_html_for(&request.url);
        false
    }

    /// Whether this completion is of a navigation the filter refused
    /// before it started (it is then not reported).
    pub(super) fn completion_refused(
        &self,
        args: &ICoreWebView2NavigationCompletedEventArgs,
    ) -> bool {
        get(|i: *mut u64| unsafe { args.NavigationId(i) })
            .is_ok_and(|id| self.refused.borrow_mut().take(id))
    }

    /// A refused `load_html` document is not served later.
    fn forget_html_for(&self, url: &str) {
        let served_here = self
            .html
            .borrow()
            .as_ref()
            .is_some_and(|(doc, _)| same_document(doc, url));
        if served_here {
            self.clear_html_filter();
        }
    }
}
