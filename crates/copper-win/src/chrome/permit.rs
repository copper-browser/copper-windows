//! A site asking for the camera, the microphone, the location or to show
//! notifications (`Browser.swift` "the camera and the microphone",
//! `App.swift` `captureAsking`): one sentence in the bars at the foot of
//! the window, `Allow` and `Don't allow`, and the answer remembered for
//! that site and that kind so it is asked once, not every call.
//!
//! * Named by the *requesting* origin (an embedded frame is asked about as
//!   itself, never on its parent's earlier answer).
//! * One question at a time per window: a second site asking while one is
//!   waiting is refused rather than queued (`decide == nil`). The camera
//!   and the microphone WebView2 asks for separately for one
//!   `getUserMedia` become one question about both.
//! * Answered exactly once: Allow, Don't allow, Escape, the page going
//!   somewhere else, or the page going away (the last two refuse).
//! * Remembered in settings as `capture.{host}|camera` /
//!   `capture.{host}|microphone` (the Mac's key, per kind) and
//!   `permit.{host}|location` / `permit.{host}|notifications` (a Windows
//!   extension, kept apart from the Mac's capture choices). A private tab
//!   reads and writes none of them.
//! * Location and notifications need a secure page, and Windows' own
//!   switches win: when Windows has them off the site is refused and told
//!   where the switch is, instead of a yes nobody keeps.
//!
//! Other kinds (the clipboard, pointer lock, …) stay with the shell's own
//! question ([`is_shell_question`] tells the two apart).

use super::glyph;
use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_engine::{LoadState, PageEvent, PageId, PageView, PermissionId, PermissionKind};
use copper_engine_webview2::WebView2Page;
use copper_shell_core::Question;

/// What a site wants, of the kinds asked about here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Camera,
    Microphone,
    CameraAndMicrophone,
    Location,
    Notifications,
}

impl Want {
    /// The kinds asked about here; `None` leaves the request to the shell.
    pub fn of(kind: &PermissionKind) -> Option<Want> {
        Some(match kind {
            PermissionKind::Camera => Want::Camera,
            PermissionKind::Microphone => Want::Microphone,
            PermissionKind::CameraAndMicrophone => Want::CameraAndMicrophone,
            PermissionKind::Geolocation => Want::Location,
            PermissionKind::Notifications => Want::Notifications,
            _ => return None,
        })
    }

    /// The question (`"{host} wants to use your camera"`).
    pub fn sentence(self, host: &str) -> String {
        match self {
            Want::Camera => format!("{host} wants to use your camera"),
            Want::Microphone => format!("{host} wants to use your microphone"),
            Want::CameraAndMicrophone => {
                format!("{host} wants to use your camera and microphone")
            }
            Want::Location => format!("{host} wants to know your location"),
            Want::Notifications => format!("{host} wants to show notifications"),
        }
    }

    /// The mark before the words: the Mac's `mic` for the microphone
    /// alone, `video` for anything with the camera.
    pub fn glyph(self) -> char {
        match self {
            Want::Microphone => glyph::MIC,
            Want::Camera | Want::CameraAndMicrophone => glyph::VIDEO,
            Want::Location => glyph::LOCATION,
            Want::Notifications => glyph::BELL,
        }
    }

    /// The settings keys an answer is kept under: one per kind, so the
    /// camera and the microphone are remembered apart.
    pub fn keys(self, host: &str) -> Vec<String> {
        let one = |k: &str| format!("{CAPTURE}{host}|{k}");
        match self {
            Want::Camera => vec![one("camera")],
            Want::Microphone => vec![one("microphone")],
            Want::CameraAndMicrophone => vec![one("camera"), one("microphone")],
            Want::Location => vec![format!("{PERMIT}{host}|location")],
            Want::Notifications => vec![format!("{PERMIT}{host}|notifications")],
        }
    }

    /// The camera, then the microphone (or the other way round) from one
    /// page: one question about both.
    pub fn with(self, other: Want) -> Option<Want> {
        use Want::*;
        match (self, other) {
            (Camera, Microphone) | (Microphone, Camera) => Some(CameraAndMicrophone),
            (CameraAndMicrophone, Camera | Microphone) => Some(CameraAndMicrophone),
            (a, b) if a == b => Some(a),
            _ => None,
        }
    }

    /// Only a secure page may have it (the Mac's capture always comes
    /// from one: WebKit asks nothing of an insecure page).
    fn needs_secure_page(self) -> bool {
        matches!(self, Want::Location | Want::Notifications)
    }
}

/// The Mac's prefix for camera and microphone answers.
pub const CAPTURE: &str = "capture.";
/// Location and notification answers (Windows only).
pub const PERMIT: &str = "permit.";

/// What was remembered for all of `keys`: `Some(true)` only if each was
/// allowed, `Some(false)` if any was refused, `None` to ask.
pub fn remembered(keys: &[String], get: impl Fn(&str) -> Option<bool>) -> Option<bool> {
    let mut all = true;
    for k in keys {
        match get(k) {
            Some(false) => return Some(false),
            Some(true) => {}
            None => all = false,
        }
    }
    all.then_some(true)
}

/// The host a request is named by: the requesting origin's, else the
/// tab's, else "This page".
pub fn host_of(origin: Option<&str>, tab_url: &str) -> String {
    let host = |u: &str| {
        url::Url::parse(u)
            .ok()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_string())
            })
            .filter(|h| !h.is_empty())
    };
    origin
        .and_then(host)
        .or_else(|| host(tab_url))
        .unwrap_or_else(|| "This page".into())
}

/// A potentially trustworthy origin (the web's secure-context rule):
/// `https:`, or `http:` on this machine.
pub fn trustworthy(origin: Option<&str>) -> bool {
    let Some(u) = origin.and_then(|o| url::Url::parse(o).ok()) else {
        return false;
    };
    match u.scheme() {
        "https" | "wss" => true,
        "http" | "ws" => {
            matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                || u.host_str().is_some_and(|h| h.ends_with(".localhost"))
        }
        _ => false,
    }
}

/// Whether `q` is the shell's own permission question for a kind asked
/// about here (`copper-shell-core` `ask_permission`), which the view then
/// drops: this module has already asked, in the Mac's words.
pub fn is_shell_question(q: &Question) -> bool {
    const OURS: [&str; 5] = [
        " to use your camera?",
        " to use your microphone?",
        " to use your camera and microphone?",
        " to know your location?",
        " to show notifications?",
    ];
    q.deny == "Don\u{2019}t Allow"
        && q.allow == "Allow"
        && q.title.starts_with("Allow ")
        && OURS.iter().any(|t| q.title.ends_with(t))
}

/// The question waiting for an answer. The page is found again by its tab
/// and id when answering (holding it would keep a closed page alive).
struct Asking {
    tab: TabId,
    page_id: PageId,
    host: String,
    want: Want,
    ids: Vec<PermissionId>,
    private: bool,
    ticket: u64,
}

/// The window's one outstanding question.
#[derive(Default)]
pub struct Permits {
    asking: Option<Asking>,
}

/// Listen to a new page for the requests asked about here, and take its
/// question down when it moves on, crashes or goes.
pub fn watch(shell: &Shell, tab: &TabId, page: &WebView2Page) {
    let mut events = page.events();
    let weak = shell.downgrade();
    let tab = tab.clone();
    let page_id = page.page_id();
    // The page is never held here (only found again through its tab): the
    // stream, and this task, end when the page goes.
    copper_win_rt::spawn_local(async move {
        while let Some(ev) = events.next().await {
            let Some(shell) = weak.upgrade() else { break };
            match ev {
                PageEvent::PermissionRequest { id, kind, url } => {
                    tracing::debug!(target: "copper::permit", tab = %tab.short(), ?kind, "asked");
                    let live = shell
                        .slot(&tab)
                        .map(|s| s.page.clone())
                        .filter(|p| p.page_id() == page_id);
                    if let Some(p) = live {
                        requested(&shell, &tab, &p, id, &kind, url.as_deref());
                    }
                }
                PageEvent::Load(LoadState::Committed { .. }) | PageEvent::Crashed(_) => {
                    page_left(&shell, page_id);
                }
                _ => {}
            }
        }
        if let Some(shell) = weak.upgrade() {
            page_left(&shell, page_id);
        }
    });
}

/// A page asked (from its event stream, on the loop's own turn).
pub fn requested(
    shell: &Shell,
    tab: &TabId,
    page: &WebView2Page,
    pid: PermissionId,
    kind: &PermissionKind,
    origin: Option<&str>,
) {
    let Some(want) = Want::of(kind) else {
        return;
    };
    let tab_url = shell
        .browser()
        .tab(tab)
        .map(|t| t.url.clone())
        .unwrap_or_default();
    let host = host_of(origin, &tab_url);
    if want.needs_secure_page() && !trustworthy(origin.or(Some(&tab_url))) {
        page.answer_permission(pid, false);
        return;
    }
    if let Some(off) = windows_says_no(want) {
        page.answer_permission(pid, false);
        shell.toast(off);
        return;
    }
    let private = shell.slot(tab).is_some_and(|s| s.private)
        || shell.browser().tab(tab).is_some_and(|t| t.private);
    if !private {
        let s = shell.settings();
        let known = remembered(&want.keys(&host), |k| s.get(k).and_then(|v| v.as_bool()));
        drop(s);
        if let Some(yes) = known {
            page.answer_permission(pid, yes);
            return;
        }
    }
    let view = shell.view();
    let mut permits = view.permits().borrow_mut();
    if let Some(a) = permits.asking.as_mut() {
        // The same page's camera and microphone: one question.
        let merged = (a.page_id == page.page_id() && a.host == host)
            .then(|| a.want.with(want))
            .flatten();
        let Some(merged) = merged else {
            drop(permits);
            page.answer_permission(pid, false);
            return;
        };
        a.ids.push(pid);
        if merged != a.want {
            a.want = merged;
            let (ticket, title) = (a.ticket, merged.sentence(&host));
            drop(permits);
            view.chrome().edit(move |ui, st| {
                st.sheet.retitle(ui, ticket, &title, Some(merged.glyph()));
            });
        }
        return;
    }
    drop(permits);
    let question = Question {
        title: want.sentence(&host),
        detail: None,
        deny: "Don't allow".into(),
        allow: "Allow".into(),
    };
    let weak = shell.downgrade();
    let answer: Box<dyn FnOnce(bool)> = Box::new(move |yes| {
        if let Some(s) = weak.upgrade() {
            answered(&s, yes);
        }
    });
    let ticket = std::rc::Rc::new(std::cell::Cell::new(0));
    let t = ticket.clone();
    view.chrome().edit(move |ui, st| {
        t.set(
            st.sheet
                .ask_marked(ui, question, Some(want.glyph()), answer),
        );
    });
    view.permits().borrow_mut().asking = Some(Asking {
        tab: tab.clone(),
        page_id: page.page_id(),
        host,
        want,
        ids: vec![pid],
        private,
        ticket: ticket.get(),
    });
    view.overlay_changed();
}

/// Windows has this kind off: what to say instead of asking.
fn windows_says_no(want: Want) -> Option<&'static str> {
    match want {
        Want::Location if crate::sys::privacy::location_off() => {
            Some("Location is off in Windows Settings")
        }
        Want::Notifications if crate::sys::privacy::notifications_off() => {
            Some("Notifications are off in Windows Settings")
        }
        _ => None,
    }
}

/// The user answered (Allow, Don't allow, Escape).
fn answered(shell: &Shell, yes: bool) {
    let Some(a) = shell.view().permits().borrow_mut().asking.take() else {
        return;
    };
    if !a.private {
        let keys = a.want.keys(&a.host);
        shell.update_settings(|s| {
            for k in &keys {
                s.set(k, yes);
            }
        });
    }
    if let Some(page) = page_of(shell, &a) {
        for id in a.ids {
            page.answer_permission(id, yes);
        }
    }
    tracing::debug!(target: "copper::permit", tab = %a.tab.short(), yes, "answered");
}

/// The asking page, if it is still its tab's.
fn page_of(shell: &Shell, a: &Asking) -> Option<WebView2Page> {
    shell
        .slot(&a.tab)
        .map(|s| s.page.clone())
        .filter(|p| p.page_id() == a.page_id)
}

/// The asking page went somewhere else or away: its question goes, and
/// what it asked for is refused (the engine refuses a page's requests
/// when its document changes; this makes sure of it).
pub fn page_left(shell: &Shell, page_id: PageId) {
    let view = shell.view();
    let gone = {
        let mut p = view.permits().borrow_mut();
        match &p.asking {
            Some(a) if a.page_id == page_id => p.asking.take(),
            _ => None,
        }
    };
    let Some(a) = gone else { return };
    if let Some(page) = page_of(shell, &a) {
        for id in a.ids {
            page.answer_permission(id, false);
        }
    }
    let ticket = a.ticket;
    view.chrome()
        .edit(move |ui, st| st.sheet.withdraw(ui, ticket));
    view.overlay_changed();
}

/// Forget every camera and microphone answer (`forgetCaptureChoices`).
pub fn forget_capture(shell: &Shell) {
    forget(shell, CAPTURE);
    shell.toast("Camera and microphone choices forgotten");
}

/// Forget every location and notification answer.
pub fn forget_others(shell: &Shell) {
    forget(shell, PERMIT);
    shell.toast("Location and notification choices forgotten");
}

fn forget(shell: &Shell, prefix: &str) {
    let keys: Vec<String> = shell
        .settings()
        .values()
        .keys()
        .filter(|k| k.starts_with(prefix))
        .cloned()
        .collect();
    if keys.is_empty() {
        return;
    }
    shell.update_settings(|s| {
        for k in &keys {
            s.remove(k);
        }
    });
}

/// Whether any answer under `prefix` is remembered (the forget commands
/// show only then).
pub fn has_choices(shell: &Shell, prefix: &str) -> bool {
    shell
        .settings()
        .values()
        .keys()
        .any(|k| k.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_macs_sentences() {
        assert_eq!(
            Want::Camera.sentence("meet.example"),
            "meet.example wants to use your camera"
        );
        assert_eq!(
            Want::CameraAndMicrophone.sentence("a.b"),
            "a.b wants to use your camera and microphone"
        );
        assert_eq!(
            Want::Location.sentence("maps.example"),
            "maps.example wants to know your location"
        );
        assert_eq!(Want::Microphone.glyph(), glyph::MIC);
        assert_eq!(Want::CameraAndMicrophone.glyph(), glyph::VIDEO);
    }

    #[test]
    fn kept_per_site_and_kind() {
        assert_eq!(
            Want::CameraAndMicrophone.keys("a.b"),
            vec!["capture.a.b|camera", "capture.a.b|microphone"]
        );
        assert_eq!(
            Want::Notifications.keys("a.b"),
            vec!["permit.a.b|notifications"]
        );
        let keys = Want::CameraAndMicrophone.keys("a.b");
        assert_eq!(remembered(&keys, |_| None), None);
        assert_eq!(
            remembered(&keys, |k| k.ends_with("camera").then_some(true)),
            None
        );
        assert_eq!(remembered(&keys, |_| Some(true)), Some(true));
        assert_eq!(
            remembered(&keys, |k| Some(!k.ends_with("microphone"))),
            Some(false)
        );
    }

    #[test]
    fn camera_and_microphone_merge() {
        assert_eq!(
            Want::Camera.with(Want::Microphone),
            Some(Want::CameraAndMicrophone)
        );
        assert_eq!(Want::Camera.with(Want::Location), None);
        assert_eq!(Want::Location.with(Want::Location), Some(Want::Location));
    }

    #[test]
    fn origins() {
        assert_eq!(
            host_of(Some("https://www.meet.example/x"), ""),
            "meet.example"
        );
        assert_eq!(host_of(None, "https://b.example/"), "b.example");
        assert_eq!(host_of(None, ""), "This page");
        assert!(trustworthy(Some("https://a.example")));
        assert!(trustworthy(Some("http://localhost:8080/")));
        assert!(!trustworthy(Some("http://a.example")));
        assert!(!trustworthy(None));
    }

    #[test]
    fn tells_the_shells_question_apart() {
        let q = |title: &str| Question {
            title: title.into(),
            detail: None,
            deny: "Don\u{2019}t Allow".into(),
            allow: "Allow".into(),
        };
        assert!(is_shell_question(&q("Allow a.b to use your camera?")));
        assert!(is_shell_question(&q("Allow a.b to know your location?")));
        assert!(!is_shell_question(&q("Allow a.b to read your clipboard?")));
    }
}
