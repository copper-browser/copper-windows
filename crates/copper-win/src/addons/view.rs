//! The addon's page (the Mac's `Fork/Addons/AddonHost.swift`; Linux
//! `addons/view.rs`), on WebView2.
//!
//! * **Profile.** Its own persistent profile, `addons` — never the browsing
//!   one, so no cookies or site data are shared (CONTRACT §5.4).
//! * **Scripts.** At document start, top frame only, page world: the core's
//!   `BRIDGE_JS` (bridge v1). It posts through
//!   `webkit.messageHandlers.copperAddon`, which the engine's page-world
//!   shim lays over `chrome.webview.postMessage` (CONTRACT §5.1).
//!   Sub-frames can't exist: the CSP says `frame-src 'none'`.
//! * **Messages.** Taken only from this page, while it shows
//!   `copper-addon://<the slot's addon>/`; dispatched by the core on the
//!   cloud's tokio runtime; the reply is evaluated back on the UI thread.
//! * **Navigation.** Only `copper-addon://<id>/…`; an `http(s)` link opens
//!   in a new foreground tab; new windows are refused (WebView2 opens none:
//!   the engine hands them to us as `NewWindow`, and only a person's link
//!   becomes a tab). The engine's navigation filter vetoes the rest
//!   before it starts; a commit that still leaves the addon reloads it.
//! * **Inspector.** Only in debug builds or with `COPPER_ADDONS_DEV_DIR`
//!   (`developer_extras` turns WebView2's DevTools and its "Inspect" off).
//! * **Events.** `visibility`, `theme`, `settings`, and `page` (with the
//!   `page` capability).

use super::glue::{self, Active};
use super::host;
use super::policy::{self, Verdict};
use crate::keys::{self, Chord};
use crate::platform::Shell;
use copper_core::addons::Capability;
use copper_engine::{
    Engine, Frames, InjectionTime, LoadState, PageEvent, PageOptions, PageView, ProfileId,
    UserScript, World,
};
use copper_engine_webview2::hosting::{KeyEventKind, PageChange};
use copper_engine_webview2::{NavigationRequest, WebView2Page};
use copper_win_ui::input::Mods;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

/// The message handler's name (`webkit.messageHandlers.copperAddon`).
pub const HANDLER: &str = "copperAddon";
/// Where the link script reports a web link the person clicked.
const LINKS: &str = "copperAddonLink";

/// CONTRACT §5.4, "http(s) link clicks open in a tab": WebView2 tells a
/// link's new window from `window.open` by nothing, so the slot's page
/// gets this: a click (or middle-click) on an `http(s)` link the addon's
/// own handlers left alone opens in a tab instead; every popup is refused.
const LINK_JS: &str = r#"(() => {
  const h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.copperAddonLink;
  if (!h) return;
  const post = h.postMessage.bind(h);
  const on = (e) => {
    if (e.defaultPrevented || (e.button !== 0 && e.button !== 1)) return;
    const t = e.target;
    const a = t && t.closest ? t.closest('a[href]') : null;
    if (!a) return;
    let u;
    try { u = new URL(a.href, location.href); } catch (_) { return; }
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return;
    e.preventDefault();
    post(u.href);
  };
  addEventListener('click', on);
  addEventListener('auxclick', on);
})();"#;

/// At most one tab per this long from the link script.
const LINK_GAP: Duration = Duration::from_millis(700);
/// The profile the slot's pages use.
pub const PROFILE: &str = "addons";

thread_local! {
    /// The live addon page (one window, one slot).
    static VIEW: RefCell<Weak<AddonView>> = const { RefCell::new(Weak::new()) };
    /// The `page` event last sent, so a tab's every title flicker is one event.
    static LAST_PAGE: RefCell<Value> = const { RefCell::new(Value::Null) };
}

/// One addon page in the slot.
pub struct AddonView {
    page: WebView2Page,
    addon: RefCell<Active>,
    visible: Cell<bool>,
    retired: Cell<bool>,
    /// The page has the keyboard.
    focused: Cell<bool>,
    /// When the link script last opened a tab.
    last_link: Cell<Option<std::time::Instant>>,
}

impl AddonView {
    pub fn new(shell: &Shell, active: &Active) -> Option<Rc<AddonView>> {
        let profile = ProfileId::new(PROFILE)
            .and_then(|id| shell.engine().profile(&id, false))
            .or_else(|e| {
                tracing::warn!(error = %e, "the addons profile is unavailable; using an InPrivate one");
                ProfileId::new(PROFILE).and_then(|id| shell.engine().profile(&id, true))
            })
            .map_err(|e| tracing::error!(error = %e, "no profile for the addon page"))
            .ok()?;
        let page = shell.engine().new_page(
            &profile,
            PageOptions {
                developer_extras: developer_mode(),
                allow_script_popups: false,
                ..Default::default()
            },
        );
        let view = Rc::new(AddonView {
            page,
            addon: RefCell::new(active.clone()),
            visible: Cell::new(false),
            retired: Cell::new(false),
            focused: Cell::new(false),
            last_link: Cell::new(None),
        });
        view.install_scripts();
        view.install_keys(shell);
        view.listen(shell);
        VIEW.with(|v| *v.borrow_mut() = Rc::downgrade(&view));
        view.load();
        tracing::info!(id = %active.id, version = %active.version, "addon page made");
        Some(view)
    }

    pub fn page(&self) -> &WebView2Page {
        &self.page
    }

    pub fn addon_id(&self) -> String {
        self.addon.borrow().id.clone()
    }

    pub fn addon(&self) -> Active {
        self.addon.borrow().clone()
    }

    /// The slot's addon again, maybe changed: a new bundle reloads the
    /// page; new settings are sent as a `settings` event.
    pub fn update(&self, active: &Active) {
        let before = self.addon.replace(active.clone());
        if before.id != active.id || before.sha256 != active.sha256 || before.dir != active.dir {
            tracing::info!(id = %active.id, version = %active.version, "addon updated; reloading");
            super::scheme::forget();
            self.load();
        } else if before.settings != active.settings {
            self.emit("settings", active.settings.clone());
        }
    }

    /// The addon left the slot: stop answering it and let the page go.
    pub fn retire(&self) {
        self.retired.set(true);
        self.page.stop();
        self.page.load("about:blank");
    }

    /// The page has the keyboard.
    pub fn focused(&self) -> bool {
        self.focused.get()
    }

    /// The pane showed or hid: tell the addon (`visibility {visible}`).
    pub fn set_visible(&self, visible: bool) {
        if self.visible.replace(visible) != visible {
            self.emit("visibility", json!({ "visible": visible }));
        }
    }

    fn root(&self) -> String {
        copper_core::addons::scheme::root_url(&self.addon.borrow().id)
    }

    fn load(&self) {
        self.page.load(&self.root());
    }

    fn install_scripts(&self) {
        self.page.add_message_handler(HANDLER, &World::Page);
        let mut shim = UserScript::new(glue::bridge_js());
        shim.injection_time = InjectionTime::DocumentStart;
        shim.frames = Frames::TopOnly;
        shim.world = World::Page;
        self.page.add_user_script(&shim);
        self.page.add_message_handler(LINKS, &World::Page);
        let mut links = UserScript::new(LINK_JS);
        links.injection_time = InjectionTime::DocumentStart;
        links.frames = Frames::TopOnly;
        links.world = World::Page;
        self.page.add_user_script(&links);
    }

    /// The link script saw a click on a web link: open it in a tab.
    fn link(&self, shell: &crate::platform::WeakShell, body: Value) {
        let Some(url) = body.as_str() else { return };
        let ok = self
            .page
            .url()
            .is_some_and(|u| policy::in_slot(&u, &self.addon_id()));
        if self.retired.get() || !ok || !policy::is_web(url) {
            return;
        }
        let now = std::time::Instant::now();
        if self
            .last_link
            .get()
            .is_some_and(|t| now.duration_since(t) < LINK_GAP)
        {
            tracing::debug!("addon link refused: too soon after the last");
            return;
        }
        self.last_link.set(Some(now));
        if let Some(s) = shell.upgrade() {
            open_tab(&s, url);
        }
    }

    /// The window's shortcuts work from inside the pane too (Ctrl+E puts
    /// it away, Ctrl+T opens a tab…), as they do from a tab's page.
    fn install_keys(&self, shell: &Shell) {
        let weak = shell.downgrade();
        self.page.on_accelerator_key(move |k| {
            if !matches!(k.kind, KeyEventKind::KeyDown | KeyEventKind::SystemKeyDown) {
                return false;
            }
            let Some(shell) = weak.upgrade() else {
                return false;
            };
            let chord = Chord::new(
                keys::key_of(k.virtual_key),
                Mods {
                    ctrl: k.ctrl,
                    shift: k.shift,
                    alt: k.alt,
                },
            );
            let Some(id) = keys::page_takes(chord, shell.view().chrome().has_overlay()) else {
                return false;
            };
            let weak = shell.downgrade();
            copper_win_rt::spawn_local(async move {
                if let Some(s) = weak.upgrade() {
                    crate::commands::run(&s, id);
                }
            });
            true
        });
    }

    fn listen(self: &Rc<Self>, shell: &Shell) {
        self.filter_navigation(shell);
        let mut changes = self.page.changes();
        let weak = Rc::downgrade(self);
        copper_win_rt::spawn_local(async move {
            while let Some(c) = changes.next().await {
                let Some(view) = weak.upgrade() else { break };
                if let PageChange::Focus(on) = c {
                    view.focused.set(on);
                }
            }
        });
        let mut events = self.page.events();
        let weak = Rc::downgrade(self);
        let shell_weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            while let Some(ev) = events.next().await {
                let Some(view) = weak.upgrade() else { break };
                match ev {
                    PageEvent::ScriptMessage { name, body } if name == HANDLER => {
                        let text = match body {
                            Value::String(s) => s,
                            other => other.to_string(),
                        };
                        view.received(text);
                    }
                    PageEvent::ScriptMessage { name, body } if name == LINKS => {
                        view.link(&shell_weak, body);
                    }
                    // `window.open` (WebView2 opened nothing: the page has
                    // no new-window handler). Only a Ctrl- or middle-click
                    // on a web link the link script missed becomes a tab.
                    PageEvent::NewWindow(req) => {
                        if let (Some(url), Some(s)) = (req.url, shell_weak.upgrade())
                            && policy::is_web(&url)
                            && req.user_gesture
                            && req.disposition == copper_engine::WindowDisposition::NewTab
                        {
                            open_tab(&s, &url);
                        } else {
                            tracing::debug!("addon popup refused");
                        }
                    }
                    PageEvent::Load(LoadState::Committed { url: Some(url) }) => {
                        // Stopped too late: the addon comes back.
                        let id = view.addon_id();
                        if policy::decide(&url, &id, view.retired.get(), false) != Verdict::Allow {
                            tracing::warn!("addon page left its addon; reloading it");
                            view.page.stop();
                            if !view.retired.get() {
                                view.load();
                            }
                        }
                    }
                    PageEvent::Crashed(reason) => {
                        tracing::warn!(?reason, id = %view.addon_id(), "addon page crashed; reloading");
                        let w = Rc::downgrade(&view);
                        copper_win_rt::spawn_local(async move {
                            copper_win_rt::sleep(Duration::from_secs(1)).await;
                            if let Some(v) = w.upgrade()
                                && !v.retired.get()
                            {
                                v.load();
                            }
                        });
                    }
                    _ => {}
                }
            }
        });
    }

    /// CONTRACT §5.4: stay inside the addon; a person's web link goes to
    /// a tab; everything else (script, redirects) is refused before it
    /// starts (the engine's synchronous veto).
    fn filter_navigation(self: &Rc<Self>, shell: &Shell) {
        let weak = Rc::downgrade(self);
        let shell_weak = shell.downgrade();
        let filter = move |req: &NavigationRequest| -> bool {
            let Some(view) = weak.upgrade() else {
                return false;
            };
            let allow = policy::decide(&req.url, &view.addon_id(), view.retired.get(), false)
                == Verdict::Allow;
            if !allow {
                REFUSED.with(|n| n.set(n.get() + 1));
            }
            let person = req.user_initiated && !req.redirected;
            match policy::decide(&req.url, &view.addon_id(), view.retired.get(), person) {
                Verdict::Allow => true,
                Verdict::OpenInTab => {
                    // Never inside WebView2's callback: on the loop's next turn.
                    let (url, shell_weak) = (req.url.clone(), shell_weak.clone());
                    copper_win_rt::spawn_local(async move {
                        if let Some(s) = shell_weak.upgrade() {
                            open_tab(&s, &url);
                        }
                    });
                    false
                }
                Verdict::Refuse => {
                    tracing::debug!("addon navigation refused");
                    false
                }
            }
        };
        self.page.set_navigation_filter(Some(Box::new(filter)));
    }

    /// One bridge message from the page.
    fn received(self: &Rc<Self>, text: String) {
        if self.retired.get() {
            return;
        }
        // CONTRACT §5.1: only from the slot's own page, showing its addon.
        let active = glue::active();
        let ok = active
            .as_ref()
            .is_some_and(|a| a.id == self.addon_id() && a.sha256 == self.addon.borrow().sha256)
            && self
                .page
                .url()
                .is_some_and(|u| policy::in_slot(&u, &self.addon_id()));
        if !ok {
            tracing::warn!("bridge message refused: not the slot's addon");
            return;
        }
        let Some(active) = active else { return };
        let weak = Rc::downgrade(self);
        let sha = active.sha256.clone();
        let host = host::Host::new(&active);
        glue::dispatch(&active, text, dark(), host, move |js| {
            let Some(view) = weak.upgrade() else { return };
            if view.retired.get() || view.addon.borrow().sha256 != sha {
                return;
            }
            view.run(&js);
        });
    }

    /// Run `js` in the page world (replies and events).
    fn run(&self, js: &str) {
        let fut = self.page.eval(js, &World::Page);
        copper_win_rt::spawn_local(async move {
            if let Err(e) = fut.await {
                tracing::debug!(error = %e, "addon script failed");
            }
        });
    }

    /// `window.__copperAddon.emit(name, payload)`.
    pub fn emit(&self, name: &str, payload: Value) {
        if self.retired.get() {
            return;
        }
        self.run(&event_js(name, &payload));
    }
}

/// The JS that fires one bridge event (CONTRACT §5.1): the core's
/// script-safe `emit`, skipped while the shim isn't there yet.
pub fn event_js(name: &str, payload: &Value) -> String {
    format!("window.__copperAddon && {};", glue::emit_js(name, payload))
}

/// The live addon page, if one is up.
pub fn live() -> Option<Rc<AddonView>> {
    VIEW.with(|v| v.borrow().upgrade())
        .filter(|v| !v.retired.get())
}

/// The inspector is there only for people building addons.
pub fn developer_mode() -> bool {
    cfg!(debug_assertions) || glue::dev_dir().is_some()
}

/// The window is dark now.
pub fn dark() -> bool {
    crate::platform::current().is_some_and(|s| s.view().chrome().is_dark())
}

/// Open `url` in a new foreground tab, next to the current one.
pub fn open_tab(shell: &Shell, url: &str) {
    shell.open(
        url,
        copper_shell_core::OpenTab {
            foreground: true,
            after: shell.active_id(),
            ..Default::default()
        },
    );
}

/// The active tab's `{url, title}` (`null` for none, and for private tabs).
pub fn current_page(shell: &Shell) -> Value {
    let private = shell.active_is_private();
    let Some(id) = shell.active_id() else {
        return Value::Null;
    };
    match shell.tab_view(&id) {
        Some(t) => policy::page_payload(&t.url, &t.label, private),
        None => Value::Null,
    }
}

/// Theme and page events, from the shell to the addon page.
pub fn watch_shell(shell: &Shell) {
    shell.on_dark_changed(|_| {
        if let Some(v) = live() {
            v.emit("theme", json!({ "dark": dark() }));
        }
    });
    let mut rx = shell.hub().subscribe();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        use copper_shell_core::control::ShellEvent as E;
        use tokio::sync::broadcast::error::RecvError;
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            let Some(s) = weak.upgrade() else { break };
            let relevant = match &ev {
                E::Selected(_) | E::SpaceSwitched(_) | E::TabClosed(_) => true,
                E::TabChanged(id) | E::Loaded(id) => s.active_id().as_ref() == Some(id),
                _ => false,
            };
            if relevant {
                page_changed(&s);
            }
        }
    });
}

/// The active tab changed: an addon with the `page` capability hears of it.
fn page_changed(shell: &Shell) {
    let now = current_page(shell);
    let same = LAST_PAGE.with(|l| {
        let mut l = l.borrow_mut();
        if *l == now {
            true
        } else {
            *l = now.clone();
            false
        }
    });
    if same {
        return;
    }
    if let Some(v) = live()
        && glue::granted(&v.addon(), Capability::Page)
    {
        v.emit("page", now);
    }
}

thread_local! {
    /// Navigations out of the addon the filter vetoed (the self-test).
    static REFUSED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// How many navigations out of the addon the filter has vetoed.
pub fn refused() -> u32 {
    REFUSED.with(std::cell::Cell::get)
}
