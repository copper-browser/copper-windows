//! The slot's plain rules, toolkit- and engine-free so they are tested on
//! any machine (Linux's `addons/policy.rs` carries the same ones; the Mac's
//! are in `Fork/Addons/AddonHost.swift`). Which files a `copper-addon:`
//! address may name, and as what type, is the core's
//! (`copper_core::addons::scheme`).

/// The settings key the pane's width is kept under (Mac `agent.paneWidth`).
pub const WIDTH_KEY: &str = "agent.paneWidth";
/// The pane's width limits and default, in DIPs (Mac `AgentPane.widths`,
/// 340…620, default 400).
pub const MIN_WIDTH: f32 = 340.0;
pub const MAX_WIDTH: f32 = 620.0;
pub const DEFAULT_WIDTH: f32 = 400.0;
/// The resize grip on the pane's leading edge, in DIPs.
pub const GRIP: f32 = 6.0;

/// A stored or dragged width, inside the limits (anything unreadable is the
/// default).
pub fn clamp_width(w: Option<f64>) -> f32 {
    match w {
        Some(w) if w.is_finite() => (w.round() as f32).clamp(MIN_WIDTH, MAX_WIDTH),
        _ => DEFAULT_WIDTH,
    }
}

/// The width a drag on the leading edge asks for: started at `from` with
/// the pointer at `x0`, now at `x` (all DIPs). Left is wider.
pub fn dragged_width(from: f32, x0: f32, x: f32) -> f32 {
    clamp_width(Some(f64::from(from - (x - x0))))
}

/// `uri` is addon `id`'s own page: `copper-addon://<id>/…`, the host
/// exactly the id (no user, no port).
pub fn in_slot(uri: &str, id: &str) -> bool {
    let Ok(u) = url::Url::parse(uri) else {
        return false;
    };
    u.scheme() == "copper-addon"
        && u.username().is_empty()
        && u.password().is_none()
        && u.port().is_none()
        && u.host_str()
            .is_some_and(|h| !id.is_empty() && h.eq_ignore_ascii_case(id))
}

/// An ordinary web address (what a link in the pane may open in a tab).
pub fn is_web(uri: &str) -> bool {
    url::Url::parse(uri).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.has_host())
}

/// What the slot's page may do with a navigation it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Stay: it is the addon's own page (or the blank page a retired
    /// view goes to).
    Allow,
    /// Cancel it, and open it in a new foreground tab instead (a web link
    /// the person clicked).
    OpenInTab,
    /// Cancel it.
    Refuse,
}

/// CONTRACT §5.4: the slot's page stays inside `copper-addon://<id>/`; a
/// web link the person activates opens in a tab; everything else is
/// cancelled.
pub fn decide(uri: &str, id: &str, retired: bool, user_initiated: bool) -> Verdict {
    if !retired && in_slot(uri, id) {
        return Verdict::Allow;
    }
    if retired && uri == "about:blank" {
        return Verdict::Allow;
    }
    if is_web(uri) && user_initiated {
        return Verdict::OpenInTab;
    }
    Verdict::Refuse
}

/// `uri` is the sign-in redirect the addon waits for.
pub fn redirect_matches(uri: &str, prefix: &str) -> bool {
    !prefix.is_empty() && uri.starts_with(prefix)
}

/// The type WebView2 is given for a served file: the core's
/// `text/html; charset=utf-8` without its parameters (the brief: WebView2's
/// custom-scheme responses misread a `Content-Type` that carries them; the
/// files are UTF-8, which Chromium assumes for these types).
pub fn bare_mime(mime: &str) -> &str {
    mime.split(';').next().unwrap_or(mime).trim()
}

/// The `page` event's payload, compared so a tab's every title flicker is
/// one event: `None` for no tab or a private one.
pub fn page_payload(url: &str, title: &str, private: bool) -> serde_json::Value {
    if private || url.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::json!({ "url": url, "title": title })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_stay_inside_the_limits() {
        assert_eq!(clamp_width(None), 400.0);
        assert_eq!(clamp_width(Some(f64::NAN)), 400.0);
        assert_eq!(clamp_width(Some(100.0)), 340.0);
        assert_eq!(clamp_width(Some(339.6)), 340.0);
        assert_eq!(clamp_width(Some(512.4)), 512.0);
        assert_eq!(clamp_width(Some(9000.0)), 620.0);
        assert_eq!(clamp_width(Some(f64::INFINITY)), 400.0);
    }

    #[test]
    fn dragging_left_widens() {
        assert_eq!(dragged_width(400.0, 1000.0, 950.0), 450.0);
        assert_eq!(dragged_width(400.0, 1000.0, 1030.0), 370.0);
        assert_eq!(dragged_width(400.0, 1000.0, 100.0), 620.0);
        assert_eq!(dragged_width(400.0, 1000.0, 1900.0), 340.0);
    }

    #[test]
    fn the_slot_keeps_to_its_own_addon() {
        let id = "example.hello";
        assert!(in_slot("copper-addon://example.hello/", id));
        assert!(in_slot("copper-addon://example.hello/index.html?x=1#y", id));
        assert!(in_slot("copper-addon://EXAMPLE.hello/app.js", id));
        assert!(!in_slot("copper-addon://example.other/", id));
        assert!(!in_slot("copper-addon://example.hello.evil/", id));
        assert!(!in_slot("copper-addon://user@example.hello/", id));
        assert!(!in_slot("copper-addon://example.hello:8080/", id));
        assert!(!in_slot("https://example.hello/", id));
        assert!(!in_slot("about:blank", id));
        assert!(!in_slot("copper-addon://example.hello/", ""));
    }

    #[test]
    fn navigations_out_are_cancelled() {
        let id = "example.hello";
        let d = |u: &str, retired, gesture| decide(u, id, retired, gesture);
        assert_eq!(
            d("copper-addon://example.hello/x", false, false),
            Verdict::Allow
        );
        assert_eq!(
            d("copper-addon://example.hello/x", true, false),
            Verdict::Refuse
        );
        assert_eq!(d("about:blank", true, false), Verdict::Allow);
        assert_eq!(d("about:blank", false, false), Verdict::Refuse);
        assert_eq!(d("https://example.com/", false, true), Verdict::OpenInTab);
        assert_eq!(d("https://example.com/", false, false), Verdict::Refuse);
        assert_eq!(d("javascript:alert(1)", false, true), Verdict::Refuse);
        assert_eq!(d("file:///C:/Windows/", false, true), Verdict::Refuse);
        assert_eq!(d("copper-addon://other/", false, true), Verdict::Refuse);
        assert_eq!(d("copper://settings", false, true), Verdict::Refuse);
    }

    #[test]
    fn web_links_and_redirects() {
        assert!(is_web("https://example.com/a"));
        assert!(is_web("http://127.0.0.1:8080/"));
        assert!(!is_web("javascript:alert(1)"));
        assert!(!is_web("file:///etc/passwd"));
        assert!(!is_web("copper-addon://example.hello/"));
        assert!(!is_web("data:text/html,hi"));
        let prefix = "http://127.0.0.1:53682/oauth/callback";
        assert!(redirect_matches(
            "http://127.0.0.1:53682/oauth/callback?code=abc&state=s",
            prefix
        ));
        assert!(!redirect_matches("http://127.0.0.1:53682/other", prefix));
        assert!(!redirect_matches("https://example.com/", ""));
    }

    #[test]
    fn served_types_lose_their_parameters() {
        assert_eq!(bare_mime("text/html; charset=utf-8"), "text/html");
        assert_eq!(
            bare_mime("text/javascript;charset=utf-8"),
            "text/javascript"
        );
        assert_eq!(bare_mime("image/png"), "image/png");
        assert_eq!(bare_mime(""), "");
    }

    #[test]
    fn the_page_event_hides_private_tabs() {
        assert_eq!(
            page_payload("https://a.example/", "A", false),
            serde_json::json!({"url": "https://a.example/", "title": "A"})
        );
        assert!(page_payload("https://a.example/", "A", true).is_null());
        assert!(page_payload("", "New Tab", false).is_null());
    }
}
