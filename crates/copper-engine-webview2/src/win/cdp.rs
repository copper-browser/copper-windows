//! Chrome DevTools Protocol over `CallDevToolsProtocolMethod`.
//!
//! CDP reaches what the WebView2 API does not: trusted input, isolated
//! worlds, off-screen screenshots, per-origin storage clearing, history
//! entries. Calls go to the page's own DevTools agent (no remote debugging
//! port is opened), and work with `AreDevToolsEnabled = FALSE`.

use std::time::Duration;

use copper_engine::{EngineError, JsonValue, Result};
use copper_win_rt::oneshot;
use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2DevToolsProtocolEventReceivedEventArgs,
};
use windows::core::PWSTR;

use super::com::{Tokens, hs, within};

/// How long a CDP call may take before it is reported as failed (a hung
/// renderer never answers some methods).
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Call `method` with `params`; resolves to its result object.
pub(crate) async fn call(
    view: &ICoreWebView2,
    method: &str,
    params: JsonValue,
) -> Result<JsonValue> {
    call_within(view, method, params, CALL_TIMEOUT).await
}

/// [`call`] with its own time limit.
pub(crate) async fn call_within(
    view: &ICoreWebView2,
    method: &str,
    params: JsonValue,
    limit: Duration,
) -> Result<JsonValue> {
    let (tx, rx) = oneshot::<Result<JsonValue>>();
    let name = method.to_owned();
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |hr, json| {
        tx.send(match hr {
            Ok(()) => serde_json::from_str(&json)
                .map_err(|e| EngineError::Other(format!("{name}: bad reply: {e}"))),
            Err(e) => Err(EngineError::Other(format!(
                "{name}: {}",
                crate::cdp::error_message(&json).unwrap_or_else(|| e.to_string())
            ))),
        });
        Ok(())
    }));
    let params = params.to_string();
    unsafe { view.CallDevToolsProtocolMethod(&hs(method), &hs(&params), &handler) }
        .map_err(|e| EngineError::Other(format!("{method}: {e}")))?;
    within(rx, limit)
        .await
        .unwrap_or_else(|| Err(EngineError::Other(format!("{method}: no reply"))))
}

/// Receive CDP event `event` (e.g. `Runtime.bindingCalled`) with its params.
pub(crate) fn subscribe(
    view: &ICoreWebView2,
    event: &str,
    tokens: &mut Tokens,
    mut f: impl FnMut(JsonValue) + 'static,
) -> windows::core::Result<()> {
    let receiver = unsafe { view.GetDevToolsProtocolEventReceiver(&hs(event))? };
    on!(
        tokens,
        receiver,
        DevToolsProtocolEventReceived,
        DevToolsProtocolEventReceivedEventHandler,
        move |_, args: Option<ICoreWebView2DevToolsProtocolEventReceivedEventArgs>| {
            if let Some(args) = args {
                let mut p = PWSTR::null();
                unsafe { args.ParameterObjectAsJson(&mut p)? };
                let json = webview2_com::take_pwstr(p);
                if let Ok(v) = serde_json::from_str(&json) {
                    f(v);
                }
            }
            Ok(())
        }
    )
}
