//! `PageView`: one tab's webview.
//!
//! A page exists before its webview: `Engine::new_page` is synchronous but
//! controllers are made asynchronously, so the page is a handle with a
//! [`Life`] and an ordered queue of everything done to it before the
//! webview arrived. The webview is also replaced over the page's life — when
//! it is slept (`WebView2Page::sleep`) or the browser process died — and the
//! page's URL stack, scripts, handlers and hosting carry over.
//!
//! Submodules: `events` (WebView2 events → `PageEvent`s), `script` (worlds,
//! shims, user scripts, eval), `hosting` (CONTRACT §2), `input` (CDP input),
//! `capture` (snapshots, find, covers), `life` (suspend, sleep, history).

mod capture;
mod chooser;
mod events;
mod hosting;
mod input;
mod life;
mod scheme;
mod script;
mod view;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use copper_engine::{
    EngineError, EventSink, NewWindowHandler, PageEvent, PageOptions, PermissionId, Result,
};
use copper_win_rt::oneshot;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PERMISSION_STATE_DENY, ICoreWebView2_8, ICoreWebView2Deferral, ICoreWebView2Frame,
    ICoreWebView2PermissionRequestedEventArgs,
};
use windows::core::Interface;

use super::com::Tokens;
use super::controller::{self, Wv};
use super::profile::ProfileInner;
use crate::filter::WebView2ContentFilter;
use crate::history::NavStack;
use crate::hosting::{Cover, PageChange};

/// Where the page's webview is.
pub(crate) enum Life {
    /// On its way.
    Pending,
    Ready(Wv),
    /// Closed on purpose (`sleep`); rebuilt when next needed.
    Asleep,
    /// Lost (the browser process died, or creating it failed); rebuilt when
    /// next needed.
    Dead,
}

/// Something done to the page before its webview existed: run with the
/// webview, or with the reason there will be none.
type Op = Box<dyn FnOnce(&Rc<PageInner>, std::result::Result<&Wv, &EngineError>)>;

/// The state `PageView`'s getters read, kept from WebView2's events.
#[derive(Default)]
pub(crate) struct Status {
    pub url: Option<String>,
    pub title: Option<String>,
    pub loading: bool,
    pub progress: f64,
    pub audio: bool,
    pub muted: bool,
    pub zoom: f64,
    pub status_text: Option<String>,
    pub fullscreen: bool,
    pub focused: bool,
}

/// A permission request waiting for `answer_permission`.
pub(crate) struct PendingPermission {
    pub args: ICoreWebView2PermissionRequestedEventArgs,
    pub deferral: ICoreWebView2Deferral,
}

pub(crate) struct PageInner {
    pub profile: Rc<ProfileInner>,
    pub opts: PageOptions,
    pub life: RefCell<Life>,
    /// Bumped whenever the webview is adopted or let go (async work for an
    /// older webview checks it and stops).
    pub generation: Cell<u64>,
    queue: RefCell<Vec<Op>>,
    /// A navigation was queued while the webview was away (so the stack's
    /// current entry is not reloaded over it).
    queued_nav: Cell<bool>,
    pub sink: EventSink<PageEvent>,
    pub changes: EventSink<PageChange>,
    /// Handlers on the current webview (removed when it goes).
    pub tokens: RefCell<Tokens>,
    pub status: RefCell<Status>,
    pub nav: RefCell<NavStack>,
    pub scripts: script::Scripts,
    pub filters: RefCell<Vec<WebView2ContentFilter>>,
    pub permissions: RefCell<HashMap<PermissionId, PendingPermission>>,
    pub new_window: RefCell<Option<NewWindowHandler<WebView2Page>>>,
    pub host: hosting::Host,
    pub find: capture::Find,
    /// The last pointer position of agent input (CSS px).
    pub pointer: Cell<Option<(f64, f64)>>,
    /// When the last Ctrl/Shift/middle click went in (popup → new tab).
    pub modified_click: Cell<Option<Instant>>,
    /// `terminate_web_process` is in flight.
    pub terminating: Cell<bool>,
    /// Held by the backstage.
    pub held: Cell<bool>,
    /// The backstage settings applied to the current webview (`None`: not
    /// yet), as (backstaged, page focus).
    pub staged_settings: Cell<Option<(bool, bool)>>,
    pub cover: RefCell<Option<Cover>>,
    /// `load_html` content waiting to be served for its URL.
    pub html: RefCell<Option<(String, String)>>,
    /// Live child frames (for page-world message routing), with their
    /// handlers.
    pub frames: RefCell<Vec<(ICoreWebView2Frame, Tokens)>>,
    /// Made by `new_related`: waits to become a popup's webview.
    pub related: bool,
    /// Process-unique identity (`PageView::page_id`).
    pub id: copper_engine::PageId,
    /// Custom-scheme serving on the current webview.
    pub schemes: scheme::PageSchemes,
    /// The file chooser the page has open, if any.
    pub chooser: chooser::PageChooser,
    /// `set_trusted_input(false)`: every input action from script.
    pub script_input: Cell<bool>,
}

/// Next `PageId` (never 0, never reused within a run).
static NEXT_PAGE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A WebView2 page. Cheap to clone; the webview closes when the last clone
/// is dropped.
#[derive(Clone)]
pub struct WebView2Page {
    pub(crate) inner: Rc<PageInner>,
}

/// A [`WebView2Page`] that is not kept alive (`PageView::downgrade`).
#[derive(Clone)]
pub struct WebView2WeakPage {
    pub(crate) inner: std::rc::Weak<PageInner>,
}

impl copper_engine::WeakPage for WebView2WeakPage {
    type Page = WebView2Page;
    fn upgrade(&self) -> Option<WebView2Page> {
        self.inner.upgrade().map(|inner| WebView2Page { inner })
    }
}

impl WebView2Page {
    pub(crate) fn create(profile: &Rc<ProfileInner>, opts: PageOptions, related: bool) -> Self {
        let status = Status {
            muted: opts.muted,
            zoom: if opts.zoom > 0.0 { opts.zoom } else { 1.0 },
            ..Status::default()
        };
        let inner = Rc::new(PageInner {
            profile: profile.clone(),
            opts,
            life: RefCell::new(Life::Pending),
            generation: Cell::new(1),
            queue: RefCell::default(),
            queued_nav: Cell::new(false),
            sink: EventSink::new(),
            changes: EventSink::new(),
            tokens: RefCell::default(),
            status: RefCell::new(status),
            nav: RefCell::default(),
            scripts: script::Scripts::default(),
            filters: RefCell::default(),
            permissions: RefCell::default(),
            new_window: RefCell::default(),
            host: hosting::Host::default(),
            find: capture::Find::default(),
            pointer: Cell::new(None),
            modified_click: Cell::new(None),
            terminating: Cell::new(false),
            held: Cell::new(false),
            staged_settings: Cell::new(None),
            cover: RefCell::default(),
            html: RefCell::default(),
            frames: RefCell::default(),
            related,
            id: copper_engine::PageId(
                NEXT_PAGE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            ),
            schemes: scheme::PageSchemes::default(),
            chooser: chooser::PageChooser::default(),
            script_input: Cell::new(false),
        });
        profile.add_page(&inner);
        profile.engine.scripts.register(&inner);
        inner.build();
        WebView2Page { inner }
    }

    /// State changes with no `PageEvent` (status text, mute, zoom, history,
    /// full screen, suspension, focus).
    pub fn changes(&self) -> copper_engine::EventStream<PageChange> {
        self.inner.changes.subscribe()
    }

    /// Whether the webview exists now (it is made lazily; see `Life`).
    pub fn is_ready(&self) -> bool {
        matches!(*self.inner.life.borrow(), Life::Ready(_))
    }

    /// Wait for the webview (making it again if the page was slept).
    pub async fn ready(&self) -> Result<()> {
        self.inner.view().await.map(drop)
    }

    /// The status text (a hovered link's URL), if any.
    pub fn status_text(&self) -> Option<String> {
        self.inner.status.borrow().status_text.clone()
    }

    pub fn is_muted(&self) -> bool {
        self.inner.status.borrow().muted
    }

    pub fn is_fullscreen(&self) -> bool {
        self.inner.status.borrow().fullscreen
    }

    pub fn has_focus(&self) -> bool {
        self.inner.status.borrow().focused
    }
}

impl PageInner {
    pub fn emit(&self, ev: PageEvent) {
        self.sink.emit(ev);
    }

    pub fn change(&self, c: PageChange) {
        self.changes.emit(c);
    }

    pub fn wv(&self) -> Option<Wv> {
        match &*self.life.borrow() {
            Life::Ready(wv) => Some(wv.clone()),
            _ => None,
        }
    }

    pub fn filters(&self) -> Vec<WebView2ContentFilter> {
        self.filters.borrow().clone()
    }

    /// Run `op` with the webview: now, or once it exists (in call order).
    pub fn with_view(self: &Rc<Self>, op: impl FnOnce(&Rc<PageInner>, &Wv) + 'static) {
        if let Some(wv) = self.wv() {
            op(self, &wv);
            return;
        }
        self.push_op(Box::new(move |p, wv| {
            if let Ok(wv) = wv {
                op(p, wv);
            }
        }));
        self.wake();
    }

    /// Queue `op` for when the webview exists (or will not).
    fn push_op(&self, op: Op) {
        self.queue.borrow_mut().push(op);
    }

    /// [`with_view`](Self::with_view) for a navigation.
    pub fn navigate_with(self: &Rc<Self>, op: impl FnOnce(&Rc<PageInner>, &Wv) + 'static) {
        if self.wv().is_none() {
            self.queued_nav.set(true);
        }
        self.with_view(op);
    }

    /// The webview, once it exists.
    pub async fn view(self: &Rc<Self>) -> Result<Wv> {
        if let Some(wv) = self.wv() {
            return Ok(wv);
        }
        let (tx, rx) = oneshot::<Result<Wv>>();
        self.push_op(Box::new(move |_, wv| {
            tx.send(wv.cloned().map_err(Clone::clone));
        }));
        self.wake();
        rx.await
            .unwrap_or_else(|| Err(EngineError::Other("the page was closed".into())))
    }

    /// Make the webview again if it is asleep or lost.
    pub fn wake(self: &Rc<Self>) {
        let rebuild = {
            let mut life = self.life.borrow_mut();
            if matches!(*life, Life::Asleep | Life::Dead) {
                *life = Life::Pending;
                true
            } else {
                false
            }
        };
        if rebuild {
            self.build();
        }
    }

    /// Get a controller and adopt it.
    fn build(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        let profile = self.profile.clone();
        copper_win_rt::spawn_local(async move {
            let made = profile.new_controller().await;
            match me.upgrade() {
                Some(p) => p.adopt(made).await,
                None => {
                    if let Ok(wv) = made {
                        controller::close(&wv);
                    }
                }
            }
        });
    }

    /// Set up a new webview for this page, then run what was queued.
    async fn adopt(self: &Rc<Self>, made: Result<Wv>) {
        let wv = match made {
            Ok(wv) => wv,
            Err(e) => {
                tracing::error!("page webview: {e}");
                *self.life.borrow_mut() = Life::Dead;
                if self.queued_nav.replace(false) {
                    let url = self.status.borrow().url.clone();
                    self.emit(PageEvent::Load(copper_engine::LoadState::Failed {
                        url: url.unwrap_or_default(),
                        error: e.to_string(),
                        cancelled: false,
                    }));
                }
                self.drain(Err(&e));
                return;
            }
        };
        self.generation.set(self.generation.get() + 1);
        let builtin = self.profile.engine.builtin_error_page.get();
        controller::configure(&wv, &self.opts, builtin);
        if let Err(e) = events::wire(self, &wv) {
            tracing::warn!("page events: {e}");
        }
        self.scripts.reset();
        let generation = self.generation.get();
        script::sync(self, &wv).await;
        if self.generation.get() != generation {
            // Let go meanwhile (the browser died): this webview is useless.
            // Whatever waits for one gets the next.
            controller::close(&wv);
            if !self.queue.borrow().is_empty() {
                self.wake();
            }
            return;
        }
        // The page's own zoom and mute outlive its webviews.
        let (zoom, muted) = {
            let s = self.status.borrow();
            (s.zoom, s.muted)
        };
        unsafe {
            let _ = wv.ctl.SetZoomFactor(zoom);
            if let Ok(v8) = wv.view.cast::<ICoreWebView2_8>() {
                let _ = v8.SetIsMuted(muted);
            }
        }
        self.host.apply(self, &wv);
        // Schemes registered until now; later ones are added live
        // (`scheme_added`), as the webview is now the page's.
        self.add_scheme_filters(&wv);
        self.intercept_file_choosers(&wv.view);
        *self.life.borrow_mut() = Life::Ready(wv.clone());
        self.visual_connected();
        // A fresh webview has WebView2's defaults (dialogs on, no focus
        // emulation).
        self.staged_settings.set(Some((false, false)));
        self.refresh_backstaged();
        if !self.queued_nav.replace(false) && !self.related {
            let restore = self.nav.borrow().current_url().map(str::to_owned);
            if let Some(url) = restore {
                view::navigate(self, &wv, &url);
            }
        }
        self.drain(Ok(&wv));
        self.profile.filters_changed();
    }

    fn drain(self: &Rc<Self>, wv: std::result::Result<&Wv, &EngineError>) {
        loop {
            let ops: Vec<Op> = std::mem::take(&mut *self.queue.borrow_mut());
            if ops.is_empty() {
                break;
            }
            for op in ops {
                op(self, wv);
            }
        }
    }

    /// Let go of the current webview (sleep, browser death, drop).
    pub fn release_webview(&self, next: Life) -> Option<Wv> {
        self.generation.set(self.generation.get() + 1);
        // Taken out, then dropped: dropping handler tokens calls into
        // WebView2, which must not happen under a borrow.
        let tokens = std::mem::take(&mut *self.tokens.borrow_mut());
        let frames = std::mem::take(&mut *self.frames.borrow_mut());
        drop(tokens);
        drop(frames);
        self.deny_pending_permissions();
        self.find.reset();
        self.schemes.released();
        // Its document goes with the webview.
        self.close_file_chooser(false);
        let old = std::mem::replace(&mut *self.life.borrow_mut(), next);
        self.nav.borrow_mut().detach_native();
        match old {
            Life::Ready(wv) => Some(wv),
            _ => None,
        }
    }

    pub fn deny_pending_permissions(&self) {
        // Taken out first: completing a deferral can raise events.
        let pending: Vec<PendingPermission> = self
            .permissions
            .borrow_mut()
            .drain()
            .map(|(_, p)| p)
            .collect();
        for p in pending {
            unsafe {
                let _ = p.args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY);
                let _ = p.deferral.Complete();
            }
        }
    }

    /// Whether the shell shows this page now (attached and visible).
    pub fn shown(&self) -> bool {
        self.host.shown()
    }

    /// Held by the backstage and not shown: popups, dialogs and permission
    /// prompts are swallowed.
    pub fn backstaged(&self) -> bool {
        self.held.get() && !self.shown()
    }
}

impl Drop for PageInner {
    fn drop(&mut self) {
        if let Some(wv) = self.release_webview(Life::Dead) {
            controller::close(&wv);
        }
        // Its filters' extensions stay until the profile's filters next
        // change: every page of a profile has the same filters (Copper's
        // shell gives them all one set), and a drop may run where no task
        // can be spawned (thread teardown).
    }
}

impl std::fmt::Debug for WebView2Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.inner.status.borrow();
        f.debug_struct("WebView2Page")
            .field("url", &s.url)
            .field("ready", &self.is_ready())
            .finish()
    }
}
