//! Background tabs and the page's life: suspend/resume, sleep (closing the
//! webview while the page and its URL stack live on), history export and
//! restore, the wake cover, backstage dialog suppression and
//! `terminate_web_process`.
//!
//! Policy (which tabs sleep, after how long) is the shell's; these are the
//! mechanisms.

use std::rc::Rc;
use std::time::Duration;

use copper_engine::{EngineError, LocalFuture, Result, Weight};
use copper_win_rt::oneshot;
use serde_json::json;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{GetProcessExtendedInfosCompletedHandler, TrySuspendCompletedHandler};
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
            // Read before the await: a borrow must not live across it.
            let has_cover = page.cover.borrow().is_some();
            if !has_cover && let Ok(c) = capture::cover_now(&wv).await {
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
    /// them) and page focus emulated (`WebView2Backstage::set_page_focus`).
    /// Its file choosers are refused by `chooser`.
    pub(crate) fn refresh_backstaged(self: &Rc<Self>) {
        let on = self.backstaged();
        // Script input (`set_trusted_input(false)`) gets no emulated focus.
        let focus = on && self.profile.engine.backstage.page_focus() && !self.script_input.get();
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
        // File choosers are intercepted on every page (`chooser`), held
        // or not.
        let view = wv.view.clone();
        copper_win_rt::spawn_local(async move {
            let params = json!({ "enabled": focus });
            if let Err(e) = cdp::call(&view, "Emulation.setFocusEmulationEnabled", params).await {
                tracing::debug!("backstage focus emulation: {e}");
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

/// What the page weighs (`PageView::weight`): the renderer its main frame
/// runs in — the environment lists every process with the frames it runs,
/// and the webview names its main frame — and that process's working set.
/// `None` while the page sleeps (no webview), before its first frame, or on
/// a Runtime too old to say. Pages of one site may share a renderer.
pub(super) fn weight(page: &Rc<PageInner>) -> LocalFuture<Option<Weight>> {
    let wv = page.wv();
    let env = page.profile.engine.env.now();
    Box::pin(async move {
        let (wv, env) = (wv?, env?);
        let mut frame = 0u32;
        let v20: ICoreWebView2_20 = wv.view.cast().ok()?;
        // SAFETY: an out-parameter on a live webview.
        unsafe { v20.FrameId(&mut frame) }.ok()?;
        let env13: ICoreWebView2Environment13 = env.cast().ok()?;
        let (tx, rx) = oneshot::<Option<ICoreWebView2ProcessExtendedInfoCollection>>();
        let handler = GetProcessExtendedInfosCompletedHandler::create(Box::new(move |hr, list| {
            tx.send(hr.ok().and(list));
            Ok(())
        }));
        // SAFETY: the handler outlives the call (COM holds it).
        unsafe { env13.GetProcessExtendedInfos(&handler) }.ok()?;
        let list = within(rx, cdp::CALL_TIMEOUT).await??;
        let process = renderer_of(&list, frame)?;
        Some(Weight {
            process,
            bytes: working_set(process)?,
        })
    })
}

/// The renderer whose frames include `frame`.
fn renderer_of(list: &ICoreWebView2ProcessExtendedInfoCollection, frame: u32) -> Option<u32> {
    let mut count = 0u32;
    // SAFETY: out-parameters and getters on live COM objects WebView2 gave.
    unsafe {
        list.Count(&mut count).ok()?;
        for i in 0..count {
            let Ok(info) = list.GetValueAtIndex(i) else {
                continue;
            };
            let Ok(process) = info.ProcessInfo() else {
                continue;
            };
            let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
            if process.Kind(&mut kind).is_err() || kind != COREWEBVIEW2_PROCESS_KIND_RENDERER {
                continue;
            }
            let Ok(frames) = info.AssociatedFrameInfos().and_then(|f| f.GetIterator()) else {
                continue;
            };
            let mut more = windows::core::BOOL::default();
            while frames.HasCurrent(&mut more).is_ok() && more.as_bool() {
                let mut id = 0u32;
                let ours = frames
                    .GetCurrent()
                    .ok()
                    .and_then(|f| f.cast::<ICoreWebView2FrameInfo2>().ok())
                    .is_some_and(|f| f.FrameId(&mut id).is_ok() && id == frame);
                if ours {
                    let mut pid = 0i32;
                    process.ProcessId(&mut pid).ok()?;
                    return u32::try_from(pid).ok();
                }
                if frames.MoveNext(&mut more).is_err() {
                    break;
                }
            }
        }
    }
    None
}

/// `pid`'s working set in bytes.
fn working_set(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: a handle opened here, read once and closed; the counters are
    // sized as the call asks.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let ok = K32GetProcessMemoryInfo(handle, &mut counters, counters.cb).as_bool();
        let _ = CloseHandle(handle);
        ok.then_some(counters.WorkingSetSize as u64)
    }
}
