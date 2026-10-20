//! CDP request builders and reply readers (platform-free; `win::cdp` sends
//! them).

use copper_engine::{EngineError, JsonMap, JsonValue, Result};
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
    }
}
