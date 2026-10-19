//! `PageView::input` through CDP `Input.*` (plans from `crate::input_plan`).

use std::rc::Rc;
use std::time::Instant;

use copper_engine::{EngineError, InputEvent, JsonValue, LocalFuture, MouseButton, Result, Trust};
use serde_json::json;

use super::PageInner;
use crate::input_plan::plan;
use crate::win::cdp;
use crate::win::controller::Wv;

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

pub(super) fn input(page: &Rc<PageInner>, event: InputEvent) -> LocalFuture<Result<Trust>> {
    let page = page.clone();
    Box::pin(async move {
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
