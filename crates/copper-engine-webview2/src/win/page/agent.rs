//! What agents' page tools need of a page (`PageView`'s "agents' page
//! tools"): its questions and file choosers taken before anyone sees them,
//! the requests it makes (CDP `Network.*`), a PDF of it, permissions
//! granted without asking, and the position it is told.
//!
//! WebView2 raises `ScriptDialogOpening` only while its own dialogs are off,
//! which Copper does for pages held backstage (`life::refresh_backstaged`):
//! a question on a page on the user's screen is the user's to answer.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use copper_engine::{
    ChosenFiles, Decision, DialogHandler, DialogId, DialogKind, DialogRequest, EngineError,
    FileChooserHandler, FileChooserInfo, GeoPosition, JsonValue, LocalFuture, NetLog,
    PermissionHandler, PermissionKind, Result,
};
use copper_win_rt::oneshot;
use serde_json::json;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::PrintToPdfCompletedHandler;
use windows::core::Interface;

use super::PageInner;
use crate::convert::{dialog_kind, net_kind, starts_document};
use crate::win::cdp;
use crate::win::com::{get, get_string, hs};
use crate::win::controller::Wv;

/// The CDP events the request log is kept from.
pub(super) const NET_EVENTS: [&str; 4] = [
    "Network.requestWillBeSent",
    "Network.responseReceived",
    "Network.loadingFinished",
    "Network.loadingFailed",
];

/// A question taken and held open (by deferral) until `answer_dialog`.
struct HeldDialog {
    args: ICoreWebView2ScriptDialogOpeningEventArgs,
    deferral: ICoreWebView2Deferral,
}

/// `PageInner::agent`. Handlers and the log outlive the page's webviews;
/// what is held of a document goes with it.
#[derive(Default)]
pub(crate) struct Agent {
    dialog_handler: RefCell<Option<DialogHandler>>,
    dialogs: RefCell<HashMap<DialogId, HeldDialog>>,
    chooser_handler: RefCell<Option<FileChooserHandler>>,
    /// The file chooser taken (its input's `backendNodeId`), waiting for
    /// `answer_file_chooser`.
    chooser: RefCell<Option<Option<i64>>>,
    permission_handler: RefCell<Option<PermissionHandler>>,
    /// Kept from `watch_network` on.
    net: RefCell<Option<NetLog>>,
    /// CDP `requestId`s of the requests in flight → the log's ids.
    requests: RefCell<HashMap<String, u64>>,
    /// The current webview's main frame (CDP `frameId`), once known.
    main_frame: RefCell<Option<String>>,
    geolocation: Cell<Option<GeoPosition>>,
}

/// Run the handler in `slot`, taken out while it runs (it may re-enter the
/// page, or replace itself); `None` without one.
fn offer<H, R>(slot: &RefCell<Option<H>>, f: impl FnOnce(&H) -> R) -> Option<R> {
    let handler = slot.borrow_mut().take()?;
    let r = f(&handler);
    if slot.borrow().is_none() {
        *slot.borrow_mut() = Some(handler);
    }
    Some(r)
}

/// Put `value` in `slot`; the old one is dropped after the borrow ends.
fn replace<T>(slot: &RefCell<T>, value: T) {
    let old = std::mem::replace(&mut *slot.borrow_mut(), value);
    drop(old);
}

impl PageInner {
    pub(super) fn set_dialog_handler(&self, handler: Option<DialogHandler>) {
        replace(&self.agent.dialog_handler, handler);
    }

    pub(super) fn set_chooser_handler(&self, handler: Option<FileChooserHandler>) {
        replace(&self.agent.chooser_handler, handler);
    }

    pub(super) fn set_permission_handler(&self, handler: Option<PermissionHandler>) {
        replace(&self.agent.permission_handler, handler);
    }

    /// Whether the handler grants `kind` for the page at `url` now.
    pub(super) fn grants(&self, kind: &PermissionKind, url: Option<&str>) -> bool {
        offer(&self.agent.permission_handler, |h| h(kind, url)).unwrap_or(false)
    }

    /// `ScriptDialogOpening`: offered to the handler first; `false` leaves
    /// it to the caller.
    pub(super) fn take_dialog(
        &self,
        a: &ICoreWebView2ScriptDialogOpeningEventArgs,
    ) -> windows::core::Result<bool> {
        let Some(kind) = get(|k| unsafe { a.Kind(k) })
            .ok()
            .and_then(|k| dialog_kind(k.0))
        else {
            return Ok(false);
        };
        let request = DialogRequest {
            id: DialogId(crate::util::next_id()),
            kind,
            message: get_string(|s| unsafe { a.Message(s) }).unwrap_or_default(),
            default_text: (kind == DialogKind::Prompt)
                .then(|| get_string(|s| unsafe { a.DefaultText(s) }).ok())
                .flatten(),
        };
        match offer(&self.agent.dialog_handler, |h| h(&request)) {
            None | Some(Decision::Pass) => return Ok(false),
            Some(Decision::Hold) => {
                let deferral = unsafe { a.GetDeferral()? };
                let held = HeldDialog {
                    args: a.clone(),
                    deferral,
                };
                self.agent.dialogs.borrow_mut().insert(request.id, held);
            }
            Some(Decision::Answer((accept, text))) => answer(a, accept, text.as_deref()),
        }
        Ok(true)
    }

    pub(super) fn answer_dialog(&self, id: DialogId, accept: bool, text: Option<&str>) -> bool {
        let Some(held) = self.agent.dialogs.borrow_mut().remove(&id) else {
            return false;
        };
        answer(&held.args, accept, text);
        unsafe {
            let _ = held.deferral.Complete();
        }
        true
    }

    /// The document, its renderer or the webview went: what agents held of
    /// it closes unanswered (a question as dismissed).
    pub(crate) fn release_agent_holds(&self) {
        // Taken out first: completing a deferral can raise events.
        let held: Vec<HeldDialog> = self
            .agent
            .dialogs
            .borrow_mut()
            .drain()
            .map(|(_, d)| d)
            .collect();
        for d in held {
            unsafe {
                let _ = d.deferral.Complete();
            }
        }
        self.agent.chooser.borrow_mut().take();
    }

    /// `Page.fileChooserOpened` for input `node`: offered to the handler
    /// first; `true` when taken.
    pub(super) fn take_chooser(&self, node: Option<i64>, multiple: bool) -> bool {
        let info = FileChooserInfo {
            multiple,
            directories: false,
        };
        match offer(&self.agent.chooser_handler, |h| h(&info)) {
            None | Some(Decision::Pass) => false,
            Some(Decision::Hold) => {
                let old = self.agent.chooser.borrow_mut().replace(node);
                if let Some(old) = old {
                    self.hand_files(old, None);
                }
                true
            }
            Some(Decision::Answer(files)) => {
                self.hand_files(node, files);
                true
            }
        }
    }

    pub(super) fn answer_agent_chooser(&self, files: ChosenFiles) -> bool {
        let Some(node) = self.agent.chooser.borrow_mut().take() else {
            return false;
        };
        self.hand_files(node, files);
        true
    }

    pub(super) fn network(&self) -> Option<NetLog> {
        self.agent.net.borrow().clone()
    }

    /// Keep the request log from now on (once).
    pub(super) fn watch_network(self: &Rc<Self>) {
        if self.agent.net.borrow().is_some() {
            return;
        }
        *self.agent.net.borrow_mut() = Some(NetLog::default());
        // Else on the next webview (`agent_adopted`).
        if let Some(wv) = self.wv() {
            self.enable_network(&wv);
        }
    }

    /// A new webview: the log and the position carry over to it (CDP state
    /// is per webview).
    pub(super) fn agent_adopted(self: &Rc<Self>, wv: &Wv) {
        self.agent.requests.borrow_mut().clear();
        *self.agent.main_frame.borrow_mut() = None;
        if self.agent.net.borrow().is_some() {
            self.enable_network(wv);
        }
        if let Some(p) = self.agent.geolocation.get() {
            geolocate(wv, Some(p));
        }
    }

    fn enable_network(self: &Rc<Self>, wv: &Wv) {
        let me = Rc::downgrade(self);
        let view = wv.view.clone();
        copper_win_rt::spawn_local(async move {
            if let Err(e) = cdp::call(&view, "Network.enable", json!({})).await {
                tracing::warn!("request log: {e}");
                return;
            }
            let tree = cdp::call(&view, "Page.getFrameTree", json!({})).await;
            let main = tree.ok().and_then(|t| {
                t.pointer("/frameTree/frame/id")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned)
            });
            if let (Some(p), Some(main)) = (me.upgrade(), main)
                && p.wv().is_some_and(|wv| wv.view == view)
            {
                *p.agent.main_frame.borrow_mut() = Some(main);
            }
        });
    }

    /// One of [`NET_EVENTS`], into the log.
    pub(super) fn net_event(&self, event: &str, p: &JsonValue) {
        let mut net = self.agent.net.borrow_mut();
        let Some(log) = net.as_mut() else { return };
        let Some(rid) = p.get("requestId").and_then(JsonValue::as_str) else {
            return;
        };
        fn text(v: Option<&JsonValue>) -> Option<&str> {
            v.and_then(JsonValue::as_str)
        }
        let mut ids = self.agent.requests.borrow_mut();
        match event {
            "Network.requestWillBeSent" => {
                // A redirect goes on as the same request, as on WebKit.
                if p.get("redirectResponse").is_some() && ids.contains_key(rid) {
                    return;
                }
                let ty = text(p.get("type")).unwrap_or("");
                let new_document = starts_document(
                    ty,
                    rid,
                    text(p.get("loaderId")),
                    text(p.get("frameId")),
                    self.agent.main_frame.borrow().as_deref(),
                );
                if new_document {
                    // The old page's loads are no longer listed.
                    ids.clear();
                }
                let id = crate::util::next_id();
                ids.insert(rid.to_owned(), id);
                let method = text(p.pointer("/request/method")).unwrap_or("");
                let url = text(p.pointer("/request/url")).unwrap_or("");
                log.sent(id, method, url, net_kind(ty), new_document);
            }
            "Network.responseReceived" => {
                let Some(&id) = ids.get(rid) else { return };
                if let Some(status) = p.pointer("/response/status").and_then(JsonValue::as_f64)
                    && status > 0.0
                {
                    log.answered(id, status.min(f64::from(u16::MAX)) as u16);
                }
                if let Some(ty) = text(p.get("type")) {
                    log.kind(id, net_kind(ty));
                }
            }
            "Network.loadingFinished" => {
                if let Some(id) = ids.remove(rid) {
                    log.finished(id, None);
                }
            }
            "Network.loadingFailed" => {
                if let Some(id) = ids.remove(rid) {
                    let why = text(p.get("errorText")).unwrap_or("failed");
                    log.finished(id, Some(why.to_owned()));
                }
            }
            _ => {}
        }
    }

    /// The position this page is told from now on (this webview and the
    /// next).
    pub(super) fn set_geolocation(&self, position: Option<GeoPosition>) {
        self.agent.geolocation.set(position);
        if let Some(wv) = self.wv() {
            geolocate(&wv, position);
        }
    }
}

/// A taken question's answer, as WebView2's own dialog would give it.
fn answer(a: &ICoreWebView2ScriptDialogOpeningEventArgs, accept: bool, text: Option<&str>) {
    if !accept {
        return;
    }
    unsafe {
        if get(|k| a.Kind(k)).is_ok_and(|k| k == COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT) {
            let default = get_string(|s| a.DefaultText(s)).ok();
            let _ = a.SetResultText(&hs(text.or(default.as_deref()).unwrap_or("")));
        }
        let _ = a.Accept();
    }
}

fn geolocate(wv: &Wv, position: Option<GeoPosition>) {
    let view = wv.view.clone();
    copper_win_rt::spawn_local(async move {
        let r = match position {
            Some(p) => {
                let params = json!({
                    "latitude": p.latitude,
                    "longitude": p.longitude,
                    "accuracy": p.accuracy,
                });
                cdp::call(&view, "Emulation.setGeolocationOverride", params).await
            }
            None => cdp::call(&view, "Emulation.clearGeolocationOverride", json!({})).await,
        };
        if let Err(e) = r {
            tracing::warn!("geolocation: {e}");
        }
    });
}

/// Print `page` to a PDF at `path`: letter, portrait, with backgrounds.
pub(super) fn print_pdf(page: &Rc<PageInner>, path: &Path) -> LocalFuture<Result<()>> {
    let page = page.clone();
    let path = path.to_string_lossy().into_owned();
    Box::pin(async move {
        let failed = |why: String| EngineError::Other(format!("printing failed: {why}"));
        let wv = page.view().await?;
        let (tx, rx) = oneshot::<bool>();
        let start = || -> windows::core::Result<()> {
            unsafe {
                let env = wv.view.cast::<ICoreWebView2_2>()?.Environment()?;
                let settings = env
                    .cast::<ICoreWebView2Environment6>()?
                    .CreatePrintSettings()?;
                settings.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)?;
                settings.SetPageWidth(8.5)?;
                settings.SetPageHeight(11.0)?;
                settings.SetShouldPrintBackgrounds(true)?;
                let handler = PrintToPdfCompletedHandler::create(Box::new(move |hr, ok| {
                    tx.send(hr.is_ok() && ok);
                    Ok(())
                }));
                let v7 = wv.view.cast::<ICoreWebView2_7>()?;
                v7.PrintToPdf(&hs(&path), &settings, &handler)
            }
        };
        start().map_err(|e| failed(e.to_string()))?;
        match rx.await {
            Some(true) => Ok(()),
            Some(false) => Err(failed("WebView2 could not write the file".into())),
            None => Err(failed("the page went away".into())),
        }
    })
}
