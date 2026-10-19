//! `PageView::input`: through CDP `Input.*` (plans from
//! `crate::input_plan`; trusted), or — after `set_trusted_input(false)` —
//! DOM events dispatched from script (`input.js`, shared with WebKitGTK;
//! `isTrusted == false`).

use std::rc::Rc;
use std::time::Instant;

use copper_engine::{
    EngineError, InputEvent, JsonMap, JsonValue, LocalFuture, MouseButton, Result, Trust,
    TrustedInput, World,
};
use serde_json::json;

use super::{PageInner, script};
use crate::input_plan::plan;
use crate::scripts::{INPUT_JS, INPUT_WORLD};
use crate::win::cdp;
use crate::win::controller::Wv;

/// What CDP delivers trusted: everything (`Input.dispatchMouseEvent`,
/// `dispatchKeyEvent`, `insertText`, touch and drag), and page focus without
/// window focus (`Emulation.setFocusEmulationEnabled`).
pub(super) const CDP_TRUSTED: TrustedInput = TrustedInput {
    pointer: true,
    editing: true,
    page_focus: true,
};

/// The viewport centre in CSS pixels (where a scroll lands before the
/// agent's pointer ever moved).
async fn viewport_centre(wv: &Wv) -> (f64, f64) {
    let metrics = cdp::call(&wv.view, "Page.getLayoutMetrics", json!({}))
        .await
        .ok();
    let viewport = metrics.as_ref().and_then(|m| {
        m.get("cssLayoutViewport")
            .or_else(|| m.get("layoutViewport"))
    });
    let dim = |k: &str| viewport.and_then(|v| v.get(k)).and_then(JsonValue::as_f64);
    match (dim("clientWidth"), dim("clientHeight")) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => (w / 2.0, h / 2.0),
        _ => (400.0, 300.0),
    }
}

/// Whether an element is under the viewport point `(x, y)`.
async fn element_at(wv: &Wv, x: f64, y: f64) -> Result<bool> {
    let expr = format!("document.elementFromPoint({x}, {y}) !== null");
    let params = crate::cdp::evaluate(&expr, None, false);
    let reply = cdp::call(&wv.view, "Runtime.evaluate", params).await?;
    Ok(crate::cdp::remote_value(reply)? == JsonValue::Bool(true))
}

/// A Ctrl/Shift/Meta or middle click: a popup it opens is a new-tab request.
fn is_modified_click(event: &InputEvent) -> bool {
    match event {
        InputEvent::Click {
            button, modifiers, ..
        } => *button == MouseButton::Middle || modifiers.ctrl || modifiers.meta || modifiers.shift,
        _ => false,
    }
}

/// Whether `event` is an Escape key press.
fn is_escape(event: &InputEvent) -> bool {
    matches!(event, InputEvent::Key { key, .. } if key == "Escape")
}

pub(super) fn input(page: &Rc<PageInner>, event: InputEvent) -> LocalFuture<Result<Trust>> {
    let page = page.clone();
    Box::pin(async move {
        // Escape closes a file chooser the page has open, as it would for a
        // person looking at the dialog; the page hears `cancel`.
        if is_escape(&event) && page.close_file_chooser(true) {
            return Ok(Trust::Trusted);
        }
        if page.script_input.get() {
            return from_script(&page, &event).await;
        }
        let wv = page.view().await?;
        let pointer = page.pointer.get();
        let centre = match (&event, pointer) {
            (InputEvent::Scroll { .. }, None) => viewport_centre(&wv).await,
            _ => (0.0, 0.0),
        };
        let plan = plan(&event, pointer, centre)
            .ok_or_else(|| EngineError::InvalidArgument("coordinates must be finite".into()))?;
        for (x, y) in &plan.hit_test {
            if !element_at(&wv, *x, *y).await? {
                return Err(EngineError::NotFound(format!("no element at ({x}, {y})")));
            }
        }
        if is_modified_click(&event) {
            page.modified_click.set(Some(Instant::now()));
        }
        for call in plan.calls {
            cdp::call(&wv.view, call.method, call.params).await?;
        }
        if let Some(p) = plan.pointer {
            page.pointer.set(Some(p));
        }
        Ok(Trust::Trusted)
    })
}

/// Deliver `event` from script: one `input.js` call in its isolated world.
async fn from_script(page: &Rc<PageInner>, event: &InputEvent) -> Result<Trust> {
    if is_modified_click(event) {
        page.modified_click.set(Some(Instant::now()));
    }
    let ev =
        serde_json::to_value(event).map_err(|e| EngineError::InvalidArgument(e.to_string()))?;
    let mut args = JsonMap::new();
    args.insert("ev".into(), ev);
    let expr = crate::cdp::call_async_expression(INPUT_JS, &args)?;
    let out = script::evaluate_async(page, expr, World::isolated(INPUT_WORLD)).await?;
    if out.get("ok").and_then(JsonValue::as_bool) == Some(true) {
        return Ok(Trust::Script);
    }
    let why = out
        .get("error")
        .and_then(JsonValue::as_str)
        .unwrap_or("input was not delivered");
    Err(EngineError::NotFound(why.to_owned()))
}
