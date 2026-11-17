//! CDP request builders and reply readers (platform-free; `win::cdp` sends
//! them).

use copper_engine::{CaptureArea, EngineError, JsonMap, JsonValue, Result};
use serde_json::json;

use crate::util::is_js_identifier;

/// `Runtime.evaluate` params. `context` is an isolated world's execution
/// context (`None` = the main frame's page world). Results come back by
/// value (JSON); `undefined` becomes `null`.
pub(crate) fn evaluate(expression: &str, context: Option<i64>, await_promise: bool) -> JsonValue {
    let mut p = json!({
        "expression": expression,
        "returnByValue": true,
        "awaitPromise": await_promise,
        "userGesture": false,
        "replMode": false,
    });
    if let Some(id) = context {
        p["contextId"] = json!(id);
    }
    p
}

/// The expression `call_async` evaluates: `body` as an async function body
/// whose parameters are `args`' keys, called with their JSON values.
pub(crate) fn call_async_expression(body: &str, args: &JsonMap) -> Result<String> {
    let mut names = Vec::with_capacity(args.len());
    let mut values = Vec::with_capacity(args.len());
    for (k, v) in args {
        if !is_js_identifier(k) {
            return Err(EngineError::InvalidArgument(format!(
                "argument name {k:?} is not a JavaScript identifier"
            )));
        }
        names.push(k.as_str());
        values.push(serde_json::to_string(v).expect("JSON values serialise"));
    }
    Ok(format!(
        "(async function({}) {{\n{body}\n}}).call(globalThis, {})",
        names.join(", "),
        values.join(", ")
    ))
}

/// A `Runtime.evaluate` reply → its value, or the exception as a
/// JavaScript error.
pub(crate) fn remote_value(reply: JsonValue) -> Result<JsonValue> {
    if let Some(ex) = reply.get("exceptionDetails") {
        let text = ex
            .pointer("/exception/description")
            .and_then(JsonValue::as_str)
            .or_else(|| ex.pointer("/exception/value").and_then(JsonValue::as_str))
            .or_else(|| ex.get("text").and_then(JsonValue::as_str))
            .unwrap_or("exception");
        return Err(EngineError::JavaScript(text.to_owned()));
    }
    Ok(reply
        .pointer("/result/value")
        .cloned()
        .unwrap_or(JsonValue::Null))
}

/// The message of a CDP error reply (`{"code":…,"message":…}`).
pub(crate) fn error_message(reply: &str) -> Option<String> {
    let v: JsonValue = serde_json::from_str(reply).ok()?;
    v.get("message")
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

/// `Page.captureScreenshot` params: the viewport, or the whole document of
/// `size` CSS pixels.
pub(crate) fn screenshot(format: &str, quality: Option<u8>, full: Option<(f64, f64)>) -> JsonValue {
    let mut p = json!({ "format": format, "fromSurface": true });
    if let Some(q) = quality {
        p["quality"] = json!(q);
    }
    if let Some((w, h)) = full {
        p["captureBeyondViewport"] = json!(true);
        p["clip"] = json!({ "x": 0, "y": 0, "width": w, "height": h, "scale": 1 });
    }
    p
}

// -- agent screenshots (`PageView::capture`) ---------------------------------------
//
// `Page.captureScreenshot`'s `clip` is in DIPs (CSS pixels times the browser
// zoom, which is what `copper_engine` calls view pixels) measured from the
// DOCUMENT's top-left, wherever the page is scrolled: Chromium forces the
// viewport to `clip.x/y` (Blink `DevToolsEmulator::ApplyViewportOverride`
// translates by `-clip + scroll offset`), which is why Puppeteer adds
// `visualViewport.pageLeft/pageTop` to an element's box before it asks. The
// picture is `round(clip.width * clip.scale * device scale)` pixels wide
// (`PageHandler::CaptureScreenshot`), so `clip.scale` is the asked scale
// over the device scale. A clip past the viewport needs
// `captureBeyondViewport` (Chromium lays the page out as tall as it is for
// the moment of the picture); one inside it does not, and leaves the page's
// layout alone.

/// A rectangle in DIPs (view pixels).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Dips {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Dips {
    fn contains(&self, other: &Dips) -> bool {
        const EDGE: f64 = 0.5;
        other.x >= self.x - EDGE
            && other.y >= self.y - EDGE
            && other.x + other.width <= self.x + self.width + EDGE
            && other.y + other.height <= self.y + self.height + EDGE
    }

    /// The smallest rectangle on whole DIPs that holds this one (at least
    /// one DIP each way), so the picture covers every pixel asked for.
    pub fn outward(&self) -> Dips {
        let x = self.x.floor();
        let y = self.y.floor();
        Dips {
            x,
            y,
            width: ((self.x + self.width).ceil() - x).max(1.0),
            height: ((self.y + self.height).ceil() - y).max(1.0),
        }
    }
}

/// The page's scroll position and size from `Page.getLayoutMetrics`, in
/// DIPs (`css*` values, in CSS pixels, times `zoom`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout {
    /// The visible viewport, from the document's top-left.
    pub viewport: Dips,
    /// The document's scrollable size.
    pub content: (f64, f64),
}

/// [`Layout`] from a `Page.getLayoutMetrics` reply; `view` is the view's
/// size in DIPs (scrollbars included), the viewport's size when the reply
/// lacks one.
pub(crate) fn layout(metrics: &JsonValue, zoom: f64, view: (f64, f64)) -> Option<Layout> {
    let zoom = if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    };
    let n = |v: Option<&JsonValue>, k: &str| v.and_then(|v| v.get(k)).and_then(JsonValue::as_f64);
    let visual = metrics
        .get("cssVisualViewport")
        .or_else(|| metrics.get("cssLayoutViewport"));
    let x = n(visual, "pageX").unwrap_or(0.0) * zoom;
    let y = n(visual, "pageY").unwrap_or(0.0) * zoom;
    let (cw, ch) = content_size(metrics)?;
    Some(Layout {
        viewport: Dips {
            x,
            y,
            width: view.0,
            height: view.1,
        },
        content: (cw * zoom, ch * zoom),
    })
}

/// What to ask CDP for an agent's screenshot of `area` (view pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShotPlan {
    /// `clip`, on whole DIPs, from the document's top-left.
    pub clip: Dips,
    /// Whether the clip reaches past the viewport.
    pub beyond: bool,
    /// Where the picture's top-left sits in `area`'s own coordinates.
    pub origin: (f64, f64),
}

/// The clip for `area` on a page laid out as `layout`.
pub(crate) fn shot_plan(area: CaptureArea, layout: &Layout) -> ShotPlan {
    let view = layout.viewport;
    let (doc, in_document) = match area {
        CaptureArea::Viewport => (view, false),
        CaptureArea::ViewportRect(r) => (
            Dips {
                x: r.x + view.x,
                y: r.y + view.y,
                width: r.width,
                height: r.height,
            },
            false,
        ),
        CaptureArea::Document => (
            Dips {
                x: 0.0,
                y: 0.0,
                width: layout.content.0,
                height: layout.content.1,
            },
            true,
        ),
        CaptureArea::DocumentRect(r) => (
            Dips {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            },
            true,
        ),
    };
    let clip = doc.outward();
    let origin = if in_document {
        (clip.x, clip.y)
    } else {
        (clip.x - view.x, clip.y - view.y)
    };
    ShotPlan {
        clip,
        beyond: !view.outward().contains(&clip),
        origin,
    }
}

/// `Page.captureScreenshot` params for a PNG of `plan` at `scale` picture
/// pixels per DIP on a display of `device_scale`.
pub(crate) fn shot_params(plan: &ShotPlan, scale: f64, device_scale: f64) -> JsonValue {
    let device_scale = if device_scale.is_finite() && device_scale > 0.0 {
        device_scale
    } else {
        1.0
    };
    let c = plan.clip;
    json!({
        "format": "png",
        "fromSurface": true,
        "captureBeyondViewport": plan.beyond,
        "clip": {
            "x": c.x, "y": c.y, "width": c.width, "height": c.height,
            "scale": scale / device_scale,
        },
    })
}

/// The document size from `Page.getLayoutMetrics` (CSS pixels).
pub(crate) fn content_size(metrics: &JsonValue) -> Option<(f64, f64)> {
    let size = metrics
        .get("cssContentSize")
        .or_else(|| metrics.get("contentSize"))?;
    let w = size.get("width")?.as_f64()?;
    let h = size.get("height")?.as_f64()?;
    (w > 0.0 && h > 0.0).then_some((w.ceil(), h.ceil()))
}

/// `Page.captureScreenshot`'s image bytes.
pub(crate) fn screenshot_bytes(reply: &JsonValue) -> Result<Vec<u8>> {
    use base64::Engine as _;
    let data = reply
        .get("data")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| EngineError::Snapshot("no image in reply".into()))?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| EngineError::Snapshot(format!("bad image: {e}")))
}

/// A `Runtime.bindingCalled` event → (binding name, payload).
pub(crate) fn binding_call(params: &JsonValue) -> Option<(String, String)> {
    Some((
        params.get("name")?.as_str()?.to_owned(),
        params.get("payload")?.as_str()?.to_owned(),
    ))
}

/// Frame ids from `Page.getFrameTree`, main frame first (breadth first),
/// at most `limit`.
pub(crate) fn frame_ids(tree: &JsonValue, out: &mut Vec<String>, limit: usize) {
    let mut level: Vec<&JsonValue> = tree.get("frameTree").into_iter().collect();
    while !level.is_empty() && out.len() < limit {
        let mut next = Vec::new();
        for node in level {
            if out.len() >= limit {
                return;
            }
            if let Some(id) = node.pointer("/frame/id").and_then(JsonValue::as_str) {
                out.push(id.to_owned());
            }
            if let Some(kids) = node.get("childFrames").and_then(JsonValue::as_array) {
                next.extend(kids.iter());
            }
        }
        level = next;
    }
}

/// The main frame's id from `Page.getFrameTree`.
pub(crate) fn main_frame_id(tree: &JsonValue) -> Option<String> {
    tree.pointer("/frameTree/frame/id")
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

/// The reply to a `Fetch.requestPaused` (response stage) of a download the
/// engine started: a successful response is continued with
/// `Content-Disposition: attachment`, so the browser saves it instead of
/// showing it; redirects and failures continue unchanged. Chromium refuses
/// new headers without the status code, so the original code goes too.
pub(crate) fn force_download(paused: &JsonValue) -> Option<(&'static str, JsonValue)> {
    let id = paused.get("requestId")?.as_str()?;
    let status = paused.get("responseStatusCode").and_then(JsonValue::as_i64);
    let failed = paused.get("responseErrorReason").is_some();
    match status {
        Some(code) if !failed && (200..300).contains(&code) => {
            let mut headers: Vec<JsonValue> = paused
                .get("responseHeaders")
                .and_then(JsonValue::as_array)
                .map(|h| {
                    h.iter()
                        .filter(|e| {
                            !e.get("name")
                                .and_then(JsonValue::as_str)
                                .is_some_and(|n| n.eq_ignore_ascii_case("content-disposition"))
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let filename = paused
                .get("responseHeaders")
                .and_then(JsonValue::as_array)
                .and_then(|h| {
                    h.iter().find(|e| {
                        e.get("name")
                            .and_then(JsonValue::as_str)
                            .is_some_and(|n| n.eq_ignore_ascii_case("content-disposition"))
                    })
                })
                .and_then(|e| e.get("value").and_then(JsonValue::as_str))
                .and_then(|v| v.split_once(';').map(|(_, rest)| rest.trim().to_owned()));
            let value = match filename {
                Some(params) if !params.is_empty() => format!("attachment; {params}"),
                _ => "attachment".to_owned(),
            };
            headers.push(json!({ "name": "Content-Disposition", "value": value }));
            Some((
                "Fetch.continueResponse",
                json!({ "requestId": id, "responseCode": code, "responseHeaders": headers }),
            ))
        }
        _ => Some(("Fetch.continueRequest", json!({ "requestId": id }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_async_builds_a_call() {
        let mut args = JsonMap::new();
        args.insert("a".into(), json!(1));
        args.insert("b".into(), json!("x\"y"));
        let e = call_async_expression("return a + b;", &args).unwrap();
        assert_eq!(
            e,
            "(async function(a, b) {\nreturn a + b;\n}).call(globalThis, 1, \"x\\\"y\")"
        );
        args.insert("not ok".into(), json!(0));
        assert!(matches!(
            call_async_expression("", &args),
            Err(EngineError::InvalidArgument(_))
        ));
        let p = evaluate("1", Some(7), true);
        assert_eq!(p["contextId"], 7);
        assert_eq!(p["awaitPromise"], true);
        assert!(evaluate("1", None, false).get("contextId").is_none());
    }

    #[test]
    fn downloads_are_forced() {
        let paused = json!({
            "requestId": "r1",
            "responseStatusCode": 200,
            "responseHeaders": [
                {"name": "Content-Type", "value": "application/pdf"},
                {"name": "content-disposition", "value": "inline; filename=\"a.pdf\""},
            ],
        });
        let (method, params) = force_download(&paused).unwrap();
        assert_eq!(method, "Fetch.continueResponse");
        assert_eq!(params["responseCode"], 200);
        let headers = params["responseHeaders"].as_array().unwrap();
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[1]["value"], "attachment; filename=\"a.pdf\"");
        let redirect = json!({"requestId": "r2", "responseStatusCode": 302});
        assert_eq!(
            force_download(&redirect).unwrap().0,
            "Fetch.continueRequest"
        );
        let failed = json!({"requestId": "r3", "responseErrorReason": "Failed"});
        assert_eq!(force_download(&failed).unwrap().0, "Fetch.continueRequest");
        let plain = json!({"requestId": "r4", "responseStatusCode": 200, "responseHeaders": []});
        assert_eq!(
            force_download(&plain).unwrap().1["responseHeaders"][0]["value"],
            "attachment"
        );
    }

    /// grid.html (1000 x 3200 CSS px) in a 947 x 746 view at 100 %,
    /// scrolled to y 1160.
    fn grid() -> Layout {
        let metrics = json!({
            "cssVisualViewport": {"pageX": 0, "pageY": 1160, "clientWidth": 932, "clientHeight": 731},
            "cssContentSize": {"x": 0, "y": 0, "width": 1000, "height": 3200},
        });
        layout(&metrics, 1.0, (947.0, 746.0)).unwrap()
    }

    #[test]
    fn clips_are_in_the_document() {
        use copper_engine::ViewRect;
        let g = grid();
        assert_eq!(g.content, (1000.0, 3200.0));
        // A region of the viewport: moved down by the scroll, no relayout.
        let p = shot_plan(
            CaptureArea::ViewportRect(ViewRect::new(100.0, 100.0, 420.0, 220.0)),
            &g,
        );
        assert_eq!(
            p.clip,
            Dips {
                x: 100.0,
                y: 1260.0,
                width: 420.0,
                height: 220.0
            }
        );
        assert_eq!((p.beyond, p.origin), (false, (100.0, 100.0)));
        // Fractions round outward; the origin says where the picture starts.
        let p = shot_plan(
            CaptureArea::ViewportRect(ViewRect::new(10.4, 20.6, 100.0, 50.2)),
            &g,
        );
        assert_eq!(
            p.clip,
            Dips {
                x: 10.0,
                y: 1180.0,
                width: 101.0,
                height: 51.0
            }
        );
        assert_eq!(p.origin, (10.0, 20.0));
        // A rectangle of the document taller than the viewport: beyond it.
        let tall = ViewRect::new(700.0, 400.0, 120.0, 1800.0);
        let p = shot_plan(CaptureArea::DocumentRect(tall), &g);
        assert_eq!(
            p.clip,
            Dips {
                x: 700.0,
                y: 400.0,
                width: 120.0,
                height: 1800.0
            }
        );
        assert_eq!((p.beyond, p.origin), (true, (700.0, 400.0)));
        // One inside the viewport is not.
        let seen = ViewRect::new(400.0, 1500.0, 240.0, 120.0);
        assert!(!shot_plan(CaptureArea::DocumentRect(seen), &g).beyond);
        // The whole document, and the whole viewport.
        let p = shot_plan(CaptureArea::Document, &g);
        assert_eq!(
            p.clip,
            Dips {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 3200.0
            }
        );
        assert!(p.beyond);
        let p = shot_plan(CaptureArea::Viewport, &g);
        assert_eq!(
            p.clip,
            Dips {
                x: 0.0,
                y: 1160.0,
                width: 947.0,
                height: 746.0
            }
        );
        assert_eq!((p.beyond, p.origin), (false, (0.0, 0.0)));
    }

    #[test]
    fn clips_follow_the_zoom_and_the_device_scale() {
        use copper_engine::ViewRect;
        // 150 %: CSS pixels times 1.5 are DIPs.
        let metrics = json!({
            "cssVisualViewport": {"pageX": 10, "pageY": 100},
            "cssContentSize": {"width": 1000, "height": 3200},
        });
        let g = layout(&metrics, 1.5, (1200.0, 900.0)).unwrap();
        assert_eq!(
            g.viewport,
            Dips {
                x: 15.0,
                y: 150.0,
                width: 1200.0,
                height: 900.0
            }
        );
        assert_eq!(g.content, (1500.0, 4800.0));
        let p = shot_plan(
            CaptureArea::ViewportRect(ViewRect::new(150.0, 150.0, 300.0, 150.0)),
            &g,
        );
        assert_eq!(
            p.clip,
            Dips {
                x: 165.0,
                y: 300.0,
                width: 300.0,
                height: 150.0
            }
        );
        // Scale 3 on a 1.25 display: clip.scale 2.4.
        let params = shot_params(&p, 3.0, 1.25);
        assert_eq!(params["clip"]["scale"], 2.4);
        assert_eq!(params["clip"]["x"], 165.0);
        assert_eq!(params["captureBeyondViewport"], false);
        assert_eq!(params["fromSurface"], true);
        assert_eq!(params["format"], "png");
        assert_eq!(shot_params(&p, 2.0, 0.0)["clip"]["scale"], 2.0);
        assert!(layout(&json!({}), 1.0, (1.0, 1.0)).is_none());
    }

    #[test]
    fn replies() {
        assert_eq!(
            remote_value(json!({"result": {"type": "number", "value": 3}})).unwrap(),
            json!(3)
        );
        assert_eq!(
            remote_value(json!({"result": {"type": "undefined"}})).unwrap(),
            JsonValue::Null
        );
        let err = remote_value(json!({
            "result": {},
            "exceptionDetails": {"text": "Uncaught", "exception": {"description": "Error: boom"}}
        }));
        assert_eq!(err, Err(EngineError::JavaScript("Error: boom".into())));
        assert_eq!(
            error_message(r#"{"code":-32000,"message":"No frame"}"#).as_deref(),
            Some("No frame")
        );
        assert_eq!(error_message("nope"), None);
        let m = json!({"cssContentSize": {"x": 0, "y": 0, "width": 800.2, "height": 3000}});
        assert_eq!(content_size(&m), Some((801.0, 3000.0)));
        let s = screenshot("png", None, Some((801.0, 3000.0)));
        assert_eq!(s["clip"]["height"], 3000.0);
        assert_eq!(screenshot_bytes(&json!({"data": "aGk="})).unwrap(), b"hi");
        assert_eq!(
            binding_call(&json!({"name": "n", "payload": "p", "executionContextId": 3})),
            Some(("n".into(), "p".into()))
        );
        assert_eq!(
            main_frame_id(&json!({"frameTree": {"frame": {"id": "F1"}}})).as_deref(),
            Some("F1")
        );
        let tree = json!({"frameTree": {"frame": {"id": "M"}, "childFrames": [
            {"frame": {"id": "A"}, "childFrames": [{"frame": {"id": "A1"}}]},
            {"frame": {"id": "B"}}
        ]}});
        let mut ids = Vec::new();
        frame_ids(&tree, &mut ids, 10);
        assert_eq!(ids, ["M", "A", "B", "A1"]);
        let mut ids = Vec::new();
        frame_ids(&tree, &mut ids, 2);
        assert_eq!(ids, ["M", "A"]);
    }
}
