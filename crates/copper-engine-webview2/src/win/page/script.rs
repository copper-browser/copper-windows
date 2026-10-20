//! Script worlds: message handlers, user scripts and style sheets (applied
//! as `crate::scripts` chains), messages back from the page, and `eval` /
//! `call_async`.
//!
//! The page world is WebView2's own (`AddScriptToExecuteOnDocumentCreated`,
//! `ExecuteScriptWithResult`, `chrome.webview` messages, routed per frame).
//! Isolated worlds are DevTools worlds named after `World::Isolated`
//! (`Page.addScriptToEvaluateOnNewDocument {worldName}`,
//! `Page.createIsolatedWorld`, a `Runtime.addBinding` per world); globals a
//! world's scripts set persist for the document, as WebKit's do. Isolated
//! worlds reach same-process frames only (CDP's page session does not see
//! out-of-process iframes).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Duration;

use copper_engine::{
    EngineError, Frames, JsonValue, LocalFuture, PageEvent, Result, UserScript, UserStyleSheet,
    World,
};
use copper_win_rt::oneshot;
use serde_json::json;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, ExecuteScriptCompletedHandler,
    ExecuteScriptWithResultCompletedHandler,
};
use windows::core::Interface;

use super::PageInner;
use crate::cdp as build;
use crate::scripts::{
    CONSOLE_HANDLER, CONSOLE_JS, Chain, Registered, binding_name, console_message,
    decode_binding_payload, decode_web_message, page_chain, page_names_update, style_sheet_removal,
    style_sheet_source, user_script_source, world_chain, world_names_update, world_shim,
};
use crate::win::cdp;
use crate::win::com::{get_bool, get_string, hs, within};
use crate::win::controller::Wv;

/// `call_async` bodies may wait on the page (an agent's `waitFor`) for a
/// long time, but not forever.
const ASYNC_LIMIT: Duration = Duration::from_secs(600);
/// `eval` is synchronous in the page; this only bounds a hung renderer.
const EVAL_LIMIT: Duration = Duration::from_secs(60);

/// A page's own script or style sheet.
pub(crate) enum Entry {
    Script(UserScript),
    Sheet(UserStyleSheet),
}

impl Entry {
    fn world(&self) -> Option<String> {
        match self {
            Entry::Script(s) => s.world.name().map(str::to_owned),
            Entry::Sheet(s) => s.world.name().map(str::to_owned),
        }
    }

    fn source(&self, id: u64) -> String {
        match self {
            Entry::Script(s) => user_script_source(s),
            Entry::Sheet(s) => style_sheet_source(s, id),
        }
    }
}

/// A world key: `None` is the page world.
type WorldKey = Option<String>;

#[derive(Default)]
pub(crate) struct Scripts {
    handlers: RefCell<BTreeMap<WorldKey, BTreeSet<String>>>,
    entries: RefCell<Vec<(u64, Entry)>>,
    /// What is registered on the current webview, per world.
    chains: RefCell<BTreeMap<WorldKey, Chain>>,
    /// Isolated worlds whose binding exists on the current webview.
    bindings: RefCell<BTreeSet<String>>,
    /// The webview generation a sync is running for (0: none).
    running: Cell<u64>,
    dirty: Cell<bool>,
    /// [`bind_documents`] is running / was asked again meanwhile.
    binding: Cell<bool>,
    rebind: Cell<bool>,
}

impl Scripts {
    /// A new webview: nothing is registered on it yet.
    pub fn reset(&self) {
        self.chains.borrow_mut().clear();
        self.bindings.borrow_mut().clear();
        self.dirty.set(true);
    }

    fn names(&self, world: &WorldKey) -> Vec<String> {
        self.handlers
            .borrow()
            .get(world)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn allows(&self, world: &WorldKey, name: &str) -> bool {
        self.handlers
            .borrow()
            .get(world)
            .is_some_and(|s| s.contains(name))
    }

    fn world_of_binding(&self, binding: &str) -> Option<String> {
        self.handlers
            .borrow()
            .keys()
            .flatten()
            .find(|w| binding_name(w) == binding)
            .cloned()
    }
}

/// The handler names the page-world shim exposes.
fn page_names(page: &PageInner) -> Vec<String> {
    let mut names = page.scripts.names(&None);
    if page.opts.capture_console {
        names.push(CONSOLE_HANDLER.to_owned());
        names.sort();
    }
    names
}

/// The chain every world should have now.
fn desired(page: &PageInner) -> BTreeMap<WorldKey, Vec<String>> {
    let mut per_world: BTreeMap<WorldKey, Vec<String>> = BTreeMap::new();
    let page_world = per_world.entry(None).or_default();
    if page.opts.capture_console {
        page_world.push(CONSOLE_JS.to_owned());
    }
    for (_, s) in page.profile.engine.scripts.scripts.borrow().iter() {
        per_world
            .entry(s.world.name().map(str::to_owned))
            .or_default()
            .push(user_script_source(s));
    }
    for (id, e) in page.scripts.entries.borrow().iter() {
        per_world.entry(e.world()).or_default().push(e.source(*id));
    }
    for (w, names) in page.scripts.handlers.borrow().iter() {
        if !names.is_empty() {
            per_world.entry(w.clone()).or_default();
        }
    }
    per_world
        .into_iter()
        .map(|(w, list)| {
            let chain = match &w {
                None => page_chain(&page_names(page), list),
                Some(name) => world_chain(name, &page.scripts.names(&w), list),
            };
            (w, chain)
        })
        .collect()
}

impl PageInner {
    /// Scripts, sheets or handlers changed: re-register what differs (applies
    /// from the next document, as WebKit's user content does).
    pub fn scripts_changed(self: &Rc<Self>) {
        self.scripts.dirty.set(true);
        let Some(wv) = self.wv() else {
            return; // applied when the webview is adopted
        };
        if self.scripts.running.get() == self.generation.get() {
            return; // the running sync picks the change up
        }
        let me = Rc::downgrade(self);
        copper_win_rt::spawn_local(async move {
            if let Some(p) = me.upgrade() {
                sync(&p, &wv).await;
            }
        });
    }
}

/// Bring every world's registrations on `wv` in line with [`desired`].
pub(super) async fn sync(page: &Rc<PageInner>, wv: &Wv) {
    let generation = page.generation.get();
    if page.scripts.running.get() == generation {
        return;
    }
    // A sync for an earlier webview stops at its next step.
    page.scripts.running.set(generation);
    let done = |page: &PageInner| {
        if page.scripts.running.get() == generation {
            page.scripts.running.set(0);
        }
    };
    while page.scripts.dirty.replace(false) {
        let want = desired(page);
        let worlds: BTreeSet<WorldKey> = want
            .keys()
            .cloned()
            .chain(page.scripts.chains.borrow().keys().cloned())
            .collect();
        for w in worlds {
            if page.generation.get() != generation {
                done(page);
                return;
            }
            let desired_chain = want.get(&w).cloned().unwrap_or_default();
            let plan = page
                .scripts
                .chains
                .borrow()
                .get(&w)
                .cloned()
                .unwrap_or_default()
                .plan(&desired_chain);
            if plan.is_empty() {
                continue;
            }
            for id in &plan.remove {
                unregister(wv, &w, id).await;
            }
            let mut added = Vec::with_capacity(plan.add.len());
            for source in plan.add {
                let id = register(wv, &w, &source).await;
                added.push(Registered { source, id });
            }
            let mut chains = page.scripts.chains.borrow_mut();
            let chain = chains.entry(w.clone()).or_default();
            chain.applied(plan.keep, added);
            if chain.registered.is_empty() {
                chains.remove(&w);
            }
        }
        sync_bindings(page, wv).await;
    }
    done(page);
}

async fn register(wv: &Wv, world: &WorldKey, source: &str) -> Option<String> {
    let r = match world {
        None => {
            let (tx, rx) = oneshot::<windows::core::Result<String>>();
            let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
                move |hr, id| {
                    tx.send(hr.map(|()| id));
                    Ok(())
                },
            ));
            match unsafe {
                wv.view
                    .AddScriptToExecuteOnDocumentCreated(&hs(source), &handler)
            } {
                Ok(()) => within(rx, cdp::CALL_TIMEOUT)
                    .await
                    .unwrap_or_else(|| Err(windows::Win32::Foundation::E_ABORT.into()))
                    .map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
        Some(name) => {
            let params = json!({ "source": source, "worldName": name });
            cdp::call(&wv.view, "Page.addScriptToEvaluateOnNewDocument", params)
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| {
                    v.get("identifier")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned)
                        .ok_or_else(|| "no identifier".to_owned())
                })
        }
    };
    match r {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!(
                "document script ({}): {e}",
                world.as_deref().unwrap_or("page")
            );
            None
        }
    }
}

async fn unregister(wv: &Wv, world: &WorldKey, id: &str) {
    match world {
        None => unsafe {
            let _ = wv.view.RemoveScriptToExecuteOnDocumentCreated(&hs(id));
        },
        Some(_) => {
            let params = json!({ "identifier": id });
            if let Err(e) =
                cdp::call(&wv.view, "Page.removeScriptToEvaluateOnNewDocument", params).await
            {
                tracing::debug!("remove document script: {e}");
            }
        }
    }
}

/// Most frames a [`bind_documents`] pass binds (main frame first).
const BIND_FRAMES: usize = 24;

/// Put each isolated world's binding into the documents there are now.
///
/// V8 adds a `Runtime.addBinding` (by context name) to contexts made later
/// only while the Runtime domain is enabled, which Copper never does (a page
/// can notice it). So a new document's world would have its shim but no
/// way out: each new document (main frame or child) gets the binding by
/// execution context id, which needs no removal (no gap in which messages
/// are dropped) and does nothing where it is already there. The shim holds
/// messages until it can send them.
pub(super) fn bind_documents(page: &Rc<PageInner>) {
    if page.scripts.bindings.borrow().is_empty() {
        return;
    }
    if page.scripts.binding.replace(true) {
        page.scripts.rebind.set(true);
        return;
    }
    let me = Rc::downgrade(page);
    copper_win_rt::spawn_local(async move {
        loop {
            let (wv, worlds) = {
                let Some(p) = me.upgrade() else { return };
                p.scripts.rebind.set(false);
                let worlds: Vec<String> = p.scripts.bindings.borrow().iter().cloned().collect();
                match p.wv() {
                    Some(wv) if !worlds.is_empty() => (wv, worlds),
                    _ => {
                        p.scripts.binding.set(false);
                        return;
                    }
                }
            };
            bind_in_frames(&wv, &worlds).await;
            let Some(p) = me.upgrade() else { return };
            if !p.scripts.rebind.get() {
                p.scripts.binding.set(false);
                return;
            }
        }
    });
}

async fn bind_in_frames(wv: &Wv, worlds: &[String]) {
    let Ok(tree) = cdp::call(&wv.view, "Page.getFrameTree", json!({})).await else {
        return;
    };
    let mut frames = Vec::new();
    build::frame_ids(&tree, &mut frames, BIND_FRAMES);
    for frame in &frames {
        for w in worlds {
            let params = json!({ "frameId": frame, "worldName": w, "grantUniveralAccess": false });
            let Ok(r) = cdp::call(&wv.view, "Page.createIsolatedWorld", params).await else {
                continue;
            };
            let Some(context) = r.get("executionContextId").and_then(JsonValue::as_i64) else {
                continue;
            };
            let params = json!({ "name": binding_name(w), "executionContextId": context });
            if let Err(e) = cdp::call(&wv.view, "Runtime.addBinding", params).await {
                tracing::debug!("world {w}: binding into frame {frame}: {e}");
            }
        }
    }
}

/// One CDP binding per isolated world that has handlers.
async fn sync_bindings(page: &PageInner, wv: &Wv) {
    let want: BTreeSet<String> = page
        .scripts
        .handlers
        .borrow()
        .iter()
        .filter(|(_, n)| !n.is_empty())
        .filter_map(|(w, _)| w.clone())
        .collect();
    let have = page.scripts.bindings.borrow().clone();
    let mut added = Vec::new();
    for w in want.difference(&have) {
        let params = json!({ "name": binding_name(w), "executionContextName": w });
        match cdp::call(&wv.view, "Runtime.addBinding", params).await {
            Ok(_) => {
                page.scripts.bindings.borrow_mut().insert(w.clone());
                added.push(w.clone());
            }
            Err(e) => tracing::warn!("world {w}: binding: {e}"),
        }
    }
    // The documents there already (made before the binding was) get it too.
    if !added.is_empty() {
        bind_in_frames(wv, &added).await;
    }
    for w in have.difference(&want) {
        let params = json!({ "name": binding_name(w) });
        let _ = cdp::call(&wv.view, "Runtime.removeBinding", params).await;
        page.scripts.bindings.borrow_mut().remove(w);
    }
}

// -- handlers ----------------------------------------------------------------

pub(super) fn add_handler(page: &Rc<PageInner>, name: &str, world: &World) -> bool {
    if name.is_empty() || name == CONSOLE_HANDLER {
        return false;
    }
    let key = world.name().map(str::to_owned);
    let added = page
        .scripts
        .handlers
        .borrow_mut()
        .entry(key.clone())
        .or_default()
        .insert(name.to_owned());
    if added {
        handlers_changed(page, key);
    }
    added
}

pub(super) fn remove_handler(page: &Rc<PageInner>, name: &str, world: &World) {
    let key = world.name().map(str::to_owned);
    let removed = {
        let mut h = page.scripts.handlers.borrow_mut();
        let removed = h.get_mut(&key).is_some_and(|s| s.remove(name));
        if h.get(&key).is_some_and(BTreeSet::is_empty) {
            h.remove(&key);
        }
        removed
    };
    if removed {
        handlers_changed(page, key);
    }
}

/// New documents get the new names through the chain; the live document's
/// shim is told now.
fn handlers_changed(page: &Rc<PageInner>, world: WorldKey) {
    page.scripts_changed();
    let me = Rc::downgrade(page);
    copper_win_rt::spawn_local(async move {
        let Some(p) = me.upgrade() else { return };
        // A page without its webview gets the names with its next one.
        let Some(wv) = p.wv() else { return };
        match &world {
            None => {
                let msg = page_names_update(&page_names(&p));
                unsafe {
                    let _ = wv.view.PostWebMessageAsJson(&hs(&msg));
                }
                // Collected first: posting calls into WebView2, which can
                // raise frame events that change `frames`.
                let frames: Vec<ICoreWebView2Frame2> = p
                    .frames
                    .borrow()
                    .iter()
                    .filter_map(|h| h.frame.cast::<ICoreWebView2Frame2>().ok())
                    .collect();
                for f2 in frames {
                    unsafe {
                        let _ = f2.PostWebMessageAsJson(&hs(&msg));
                    }
                }
            }
            Some(name) => {
                // Bindings first, so the shim finds its binding.
                sync_bindings(&p, &wv).await;
                let names = p.scripts.names(&world);
                let js = format!(
                    "{}\n{};",
                    world_shim(name, &names),
                    world_names_update(&names)
                );
                if let Err(e) = evaluate_in(&p, &wv, &js, Some(name), false, EVAL_LIMIT).await {
                    tracing::debug!("world {name}: live handler update: {e}");
                }
            }
        }
    });
}

/// A page-world message (main frame or a child frame's `WebMessageReceived`).
pub(super) fn route_web_message(page: &PageInner, json: &str) {
    let Some(msg) = decode_web_message(json) else {
        return;
    };
    if msg.name == CONSOLE_HANDLER {
        if page.opts.capture_console
            && let Some(c) = console_message(&msg.body)
        {
            page.emit(PageEvent::Console(c));
        }
        return;
    }
    let allowed = page.scripts.allows(&None, &msg.name);
    // Never the body: sign-in messages carry passwords.
    tracing::trace!(
        name = %msg.name,
        allowed,
        kind = msg.body.get("kind").and_then(|k| k.as_str()).unwrap_or(""),
        "page message"
    );
    if allowed {
        page.emit(PageEvent::ScriptMessage {
            name: msg.name,
            body: msg.body,
        });
    }
}

/// An isolated-world message (`Runtime.bindingCalled`).
pub(super) fn route_binding(page: &PageInner, params: &JsonValue) {
    let Some((binding, payload)) = build::binding_call(params) else {
        return;
    };
    let Some(world) = page.scripts.world_of_binding(&binding) else {
        return;
    };
    let Some(msg) = decode_binding_payload(&payload) else {
        return;
    };
    if page.scripts.allows(&Some(world), &msg.name) {
        page.emit(PageEvent::ScriptMessage {
            name: msg.name,
            body: msg.body,
        });
    }
}

// -- user scripts and sheets ---------------------------------------------------

pub(super) fn add_entry(page: &Rc<PageInner>, entry: Entry) -> u64 {
    let id = crate::util::next_id();
    // A style sheet reaches the open documents too, as WebKit's do (a
    // script does not: it runs as documents are made).
    let live = match &entry {
        Entry::Sheet(s) => Some((
            s.world.name().map(str::to_owned),
            s.frames == Frames::All,
            style_sheet_source(s, id),
        )),
        Entry::Script(_) => None,
    };
    page.scripts.entries.borrow_mut().push((id, entry));
    page.scripts_changed();
    if let Some((world, all_frames, js)) = live {
        run_in_open_documents(page, world, all_frames, js);
    }
    id
}

pub(super) fn remove_entry(page: &Rc<PageInner>, id: u64) -> bool {
    let removed = {
        let mut e = page.scripts.entries.borrow_mut();
        let at = e.iter().position(|(i, _)| *i == id);
        at.map(|at| e.remove(at).1)
    };
    let Some(entry) = removed else {
        return false;
    };
    page.scripts_changed();
    if let Entry::Sheet(s) = entry {
        let world = s.world.name().map(str::to_owned);
        run_in_open_documents(
            page,
            world,
            s.frames == Frames::All,
            style_sheet_removal(id),
        );
    }
    true
}

/// Run `js` in the documents open now: the main frame's, and (`all_frames`,
/// page world only) its child frames'. Nothing when the page has no webview:
/// its next one starts from the registrations. Errors are only logged (a
/// document going away mid-call).
fn run_in_open_documents(page: &Rc<PageInner>, world: WorldKey, all_frames: bool, js: String) {
    let Some(wv) = page.wv() else { return };
    match world {
        None => {
            unsafe {
                let _ = wv.view.ExecuteScript(&hs(&js), None);
            }
            if all_frames {
                // Collected first: calling into WebView2 can raise frame
                // events that change `frames`.
                let frames: Vec<ICoreWebView2Frame2> = page
                    .frames
                    .borrow()
                    .iter()
                    .filter_map(|h| h.frame.cast::<ICoreWebView2Frame2>().ok())
                    .collect();
                for f in frames {
                    unsafe {
                        let _ = f.ExecuteScript(&hs(&js), None);
                    }
                }
            }
        }
        Some(name) => {
            let me = Rc::downgrade(page);
            copper_win_rt::spawn_local(async move {
                let Some(p) = me.upgrade() else { return };
                if let Err(e) = evaluate_in(&p, &wv, &js, Some(&name), false, EVAL_LIMIT).await {
                    tracing::debug!("world {name}: live style sheet: {e}");
                }
            });
        }
    }
}

// -- evaluation ------------------------------------------------------------------

/// The execution context of isolated world `name` in the main frame's
/// current document (made if this document has none yet).
async fn world_context(wv: &Wv, name: &str) -> Result<i64> {
    let tree = cdp::call(&wv.view, "Page.getFrameTree", json!({})).await?;
    let frame = build::main_frame_id(&tree)
        .ok_or_else(|| EngineError::JavaScript("the page has no main frame".into()))?;
    let params = json!({ "frameId": frame, "worldName": name, "grantUniveralAccess": false });
    let r = cdp::call(&wv.view, "Page.createIsolatedWorld", params).await?;
    r.get("executionContextId")
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| EngineError::JavaScript(format!("world {name}: no execution context")))
}

/// `Runtime.evaluate` in the page world (`None`) or an isolated world.
async fn evaluate_in(
    _page: &PageInner,
    wv: &Wv,
    expression: &str,
    world: Option<&str>,
    await_promise: bool,
    limit: Duration,
) -> Result<JsonValue> {
    let context = match world {
        Some(name) => Some(world_context(wv, name).await?),
        None => None,
    };
    let params = build::evaluate(expression, context, await_promise);
    let reply = cdp::call_within(&wv.view, "Runtime.evaluate", params, limit).await?;
    build::remote_value(reply)
}

pub(super) fn eval(
    page: &Rc<PageInner>,
    js: String,
    world: World,
) -> LocalFuture<Result<JsonValue>> {
    let page = page.clone();
    Box::pin(async move {
        page.view().await?;
        page.first_document().await;
        let wv = page.view().await?;
        match world.name() {
            Some(name) => evaluate_in(&page, &wv, &js, Some(name), false, EVAL_LIMIT).await,
            None => execute_script(&wv, &js).await,
        }
    })
}

pub(super) fn evaluate_async(
    page: &Rc<PageInner>,
    expression: String,
    world: World,
) -> LocalFuture<Result<JsonValue>> {
    let page = page.clone();
    Box::pin(async move {
        page.view().await?;
        page.first_document().await;
        let wv = page.view().await?;
        evaluate_in(&page, &wv, &expression, world.name(), true, ASYNC_LIMIT).await
    })
}

/// `ExecuteScriptWithResult` (exceptions come back as errors), or plain
/// `ExecuteScript` on runtimes without it.
async fn execute_script(wv: &Wv, js: &str) -> Result<JsonValue> {
    let fail = |e: String| EngineError::JavaScript(e);
    if let Ok(v21) = wv.view.cast::<ICoreWebView2_21>() {
        let (tx, rx) = oneshot::<std::result::Result<String, String>>();
        let handler =
            ExecuteScriptWithResultCompletedHandler::create(Box::new(move |hr, result| {
                tx.send(read_result(hr, result));
                Ok(())
            }));
        unsafe { v21.ExecuteScriptWithResult(&hs(js), &handler) }
            .map_err(|e| fail(e.to_string()))?;
        let json = within(rx, EVAL_LIMIT)
            .await
            .unwrap_or_else(|| Err("the script did not finish".into()))
            .map_err(fail)?;
        return serde_json::from_str(&json).map_err(|e| fail(e.to_string()));
    }
    let (tx, rx) = oneshot::<windows::core::Result<String>>();
    let handler = ExecuteScriptCompletedHandler::create(Box::new(move |hr, json| {
        tx.send(hr.map(|()| json));
        Ok(())
    }));
    unsafe { wv.view.ExecuteScript(&hs(js), &handler) }.map_err(|e| fail(e.to_string()))?;
    let json = within(rx, EVAL_LIMIT)
        .await
        .unwrap_or_else(|| Err(windows::Win32::Foundation::E_ABORT.into()))
        .map_err(|e| fail(e.to_string()))?;
    serde_json::from_str(&json).map_err(|e| fail(e.to_string()))
}

fn read_result(
    hr: windows::core::Result<()>,
    result: Option<ICoreWebView2ExecuteScriptResult>,
) -> std::result::Result<String, String> {
    hr.map_err(|e| e.to_string())?;
    let r = result.ok_or("no result")?;
    let ok = get_bool(|b| unsafe { r.Succeeded(b) }).map_err(|e| e.to_string())?;
    if ok {
        return get_string(|p| unsafe { r.ResultAsJson(p) }).map_err(|e| e.to_string());
    }
    let ex = unsafe { r.Exception() }.map_err(|e| e.to_string())?;
    let name = get_string(|p| unsafe { ex.Name(p) }).unwrap_or_default();
    let message = get_string(|p| unsafe { ex.Message(p) }).unwrap_or_default();
    Err(match (name.is_empty(), message.is_empty()) {
        (false, false) => format!("{name}: {message}"),
        (true, false) => message,
        (false, true) => name,
        (true, true) => "exception".to_owned(),
    })
}
