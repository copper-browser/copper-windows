//! Background tabs and the page's life: suspend/resume, sleep (closing the
//! webview while the page and its URL stack live on), history export and
//! restore, the wake cover, backstage dialog suppression and
//! `terminate_web_process`.
//!
//! Policy (which tabs sleep, after how long) is the shell's; these are the
//! mechanisms.

use std::rc::Rc;
use std::time::Duration;

use copper_engine::{EngineError, LocalFuture, Result};
use copper_win_rt::oneshot;
use serde_json::json;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::TrySuspendCompletedHandler;
use windows::core::Interface;

use super::{Life, PageInner, WebView2Page, capture, view};
use crate::history::{HistoryEntry, NavStack};
use crate::hosting::{Cover, PageChange};
use crate::win::cdp;
use crate::win::com::{get_bool, within};
use crate::win::controller;

/// If no `ProcessFailed` follows `terminate_web_process` by then, the flag
/// is dropped (so a later real crash is not reported as requested).
const TERMINATE_GRACE: Duration = Duration::from_secs(5);

impl WebView2Page {
    /// Suspend a hidden page: its script and timers stop and Chromium trims
    /// its memory (`MemoryUsageTargetLevel::Low`, then `TrySuspend`).
    /// Resolves to whether WebView2 suspended it (it declines for pages
    /// playing audio, holding locks, etc.). Showing the page resumes it.
    pub fn suspend(&self) -> LocalFuture<Result<bool>> {
        let page = self.inner.clone();
        Box::pin(async move {
            if page.host.rendering() {
                return Err(EngineError::InvalidArgument(
                    "a page on screen cannot be suspended".into(),
                ));
            }
            let wv = page.view().await?;
            let v3: ICoreWebView2_3 = wv.view.cast().map_err(|_| {
                EngineError::Other("this WebView2 Runtime cannot suspend pages".into())
            })?;
            if let Ok(v19) = wv.view.cast::<ICoreWebView2_19>() {
                unsafe {
                    let _ =
                        v19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
                }
            }
            if page.cover.borrow().is_none()
                && let Ok(c) = capture::cover_now(&wv).await
            {
                *page.cover.borrow_mut() = Some(c);
            }
            let (tx, rx) = oneshot::<windows::core::Result<bool>>();
            let handler = TrySuspendCompletedHandler::create(Box::new(move |hr, ok| {
                tx.send(hr.map(|()| ok));
                Ok(())
            }));
            unsafe { v3.TrySuspend(&handler) }
                .map_err(|e| EngineError::Other(format!("suspend: {e}")))?;
            let ok = within(rx, cdp::CALL_TIMEOUT)
                .await
                .ok_or_else(|| EngineError::Other("suspend: no reply".into()))?
                .map_err(|e| EngineError::Other(format!("suspend: {e}")))?;
            if ok {
                page.change(PageChange::Suspended(true));
            }
            Ok(ok)
        })
    }

    /// Resume a suspended page without showing it.
    pub fn resume(&self) {
        let Some(wv) = self.inner.wv() else { return };
        unsafe {
            if let Ok(v19) = wv.view.cast::<ICoreWebView2_19>() {
                let _ =
                    v19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
            }
            if let Ok(v3) = wv.view.cast::<ICoreWebView2_3>()
                && v3.Resume().is_ok()
            {
                self.inner.change(PageChange::Suspended(false));
            }
        }
    }

    pub fn is_suspended(&self) -> bool {
        self.inner
            .wv()
            .and_then(|wv| wv.view.cast::<ICoreWebView2_3>().ok())
            .and_then(|v3| get_bool(|b| unsafe { v3.IsSuspended(b) }).ok())
            .unwrap_or(false)
    }

    /// Close the page's webview to free its memory and processes. The page
    /// stays (URL, title, URL stack, scripts, handlers, filters); anything
    /// that needs the webview — attaching, loading, script, input — makes a
    /// new one and reloads the current entry. `false` if the page is on
    /// screen (or already asleep).
    pub fn sleep(&self) -> bool {
        let p = &self.inner;
        if p.shown() || !matches!(*p.life.borrow(), Life::Ready(_)) {
            return false;
        }
        if p.host.in_stage() {
            return false;
        }
        if let Some(wv) = p.release_webview(Life::Asleep) {
            controller::close(&wv);
        }
        p.status.borrow_mut().loading = false;
        true
    }

    pub fn is_asleep(&self) -> bool {
        matches!(*self.inner.life.borrow(), Life::Asleep)
    }

    /// The page's back/forward list and the index of its current entry
    /// (what the shell stores to rebuild a tab).
    pub fn history(&self) -> (Vec<HistoryEntry>, usize) {
        self.inner.nav.borrow().entries()
    }

    /// Give a fresh page a stored back/forward list and load its current
    /// entry. Back/forward then walk the stored entries.
    pub fn restore_history(&self, entries: Vec<HistoryEntry>, index: usize) {
        let (stack, url) = NavStack::restore(entries, index);
        *self.inner.nav.borrow_mut() = stack;
        self.inner.change(PageChange::History);
        if let Some(url) = url {
            self.inner
                .navigate_with(move |p, wv| view::navigate(p, wv, &url));
        }
    }

    /// The page's last frame from when it was hidden, for the shell to show
    /// until the page paints again after waking.
    pub fn cover(&self) -> Option<Cover> {
        self.inner.cover.borrow().clone()
    }

    /// Forget the cover (the shell has seen the page paint).
    pub fn clear_cover(&self) {
        self.inner.cover.borrow_mut().take();
    }
}

impl PageInner {
    /// Apply what a page held hidden in the backstage needs, or undo it:
    /// WebView2's own script dialogs off (`ScriptDialogOpening` answers
    /// them), file choosers intercepted (counted, never shown), and page
    /// focus emulated (`WebView2Backstage::set_page_focus`).
    pub(crate) fn refresh_backstaged(self: &Rc<Self>) {
        let on = self.backstaged();
        let focus = on && self.profile.engine.backstage.page_focus();
        if self.staged_settings.replace(Some((on, focus))) == Some((on, focus)) {
            return;
        }
        let Some(wv) = self.wv() else {
            // Applied when the webview is adopted.
            self.staged_settings.set(None);
            return;
        };
        unsafe {
            if let Ok(s) = wv.view.Settings() {
                let _ = s.SetAreDefaultScriptDialogsEnabled(!on);
            }
        }
        let view = wv.view.clone();
        copper_win_rt::spawn_local(async move {
            let calls = [
                (
                    "Page.setInterceptFileChooserDialog",
                    json!({ "enabled": on }),
                ),
                (
                    "Emulation.setFocusEmulationEnabled",
                    json!({ "enabled": focus }),
                ),
            ];
            for (method, params) in calls {
                if let Err(e) = cdp::call(&view, method, params).await {
                    tracing::debug!("backstage {method}: {e}");
                }
            }
        });
    }
}

/// Kill the page's renderer (CDP `Page.crash`); `PageEvent::Crashed`
/// follows with `TerminatedByApi`.
pub(super) fn terminate(page: &Rc<PageInner>) {
    let Some(wv) = page.wv() else { return };
    page.terminating.set(true);
    let me = Rc::downgrade(page);
    copper_win_rt::spawn_local(async move {
        // The renderer dies before it can answer.
        let _ = cdp::call_within(&wv.view, "Page.crash", json!({}), TERMINATE_GRACE).await;
        copper_win_rt::sleep(TERMINATE_GRACE).await;
        if let Some(p) = me.upgrade() {
            p.terminating.set(false);
        }
    });
}
