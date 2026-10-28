//! The one `CoreWebView2Environment` of the process.
//!
//! Made on the loop turn after the engine is (or by the first page that
//! needs it, if sooner), so what the shell sets up synchronously right
//! after `WebView2Engine::new` — preferred languages, custom schemes —
//! is part of it; the browser process still starts while the shell builds
//! its window. Shared by every profile and page. If the browser process
//! dies, every webview dies with it; the environment is then made again
//! (with everything set up so far) once the old process has exited
//! (WebView2 refuses a second environment on a user data folder still in
//! use). WebView2 also ends the browser process, normally, when its last
//! webview closes (every tab closed, no warm spare); that is no failure:
//! the environment starts a new browser process for its next webview (and
//! is made again only if that does not work).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use copper_engine::{EngineError, Result};
use copper_win_rt::{OneshotTx, oneshot};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    CoreWebView2CustomSchemeRegistration, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2EnvironmentCompletedHandler,
};
use windows::core::{Interface, PCWSTR};

use super::com::{Tokens, hs, within};
use super::loader;

/// How long to wait for a dead browser process to exit before making a new
/// environment anyway.
const EXIT_WAIT: Duration = Duration::from_secs(3);

/// What the environment is made with.
#[derive(Clone, Debug)]
pub(crate) struct EnvConfig {
    pub user_data_dir: PathBuf,
    pub browser_folder: Option<PathBuf>,
    /// Switches after Copper's own (`COPPER_WEBVIEW2_ARGS`, then
    /// `EngineConfig::extra_arguments`).
    pub extra_arguments: Vec<String>,
}

/// What the shell set up that only the environment can carry: read each
/// time one is made.
#[derive(Default)]
struct Setup {
    /// Custom schemes (lowercase), in registration order.
    schemes: Vec<String>,
    /// Preferred languages, normalized, best first.
    languages: Vec<String>,
}

enum State {
    /// Not made yet: made on the loop turn after the engine was, or by the
    /// first `get`.
    Idle,
    Creating(Vec<OneshotTx<Result<ICoreWebView2Environment>>>),
    Ready(ICoreWebView2Environment),
    Failed(String),
}

pub(crate) struct Env {
    config: EnvConfig,
    setup: RefCell<Setup>,
    state: RefCell<State>,
    /// Bumped each time an environment is made; stale browser-exit events
    /// are ignored.
    generation: Cell<u64>,
    tokens: RefCell<Tokens>,
    /// Signalled when the current browser process exits.
    exited: RefCell<Option<OneshotTx<()>>>,
    started: Cell<Option<Instant>>,
    ready_after: Cell<Option<Duration>>,
    /// See [`Env::ended`].
    ended: Cell<bool>,
    /// Told when the browser process failed with no page to hear it
    /// ([`Env::on_lost`]).
    lost: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Env {
    /// The environment-to-be; it is made on the next loop turn (see the
    /// module docs).
    pub fn new(config: EnvConfig) -> Rc<Env> {
        let env = Rc::new(Env {
            config,
            setup: RefCell::default(),
            state: RefCell::new(State::Idle),
            generation: Cell::new(0),
            tokens: RefCell::new(Tokens::default()),
            exited: RefCell::new(None),
            started: Cell::new(None),
            ready_after: Cell::new(None),
            ended: Cell::new(false),
            lost: RefCell::new(None),
        });
        let weak = Rc::downgrade(&env);
        copper_win_rt::spawn_local(async move {
            if let Some(env) = weak.upgrade() {
                env.start();
            }
        });
        env
    }

    /// Make the environment now, unless it is made (or being made).
    pub fn start(self: &Rc<Self>) {
        let idle = {
            let mut state = self.state.borrow_mut();
            let idle = matches!(*state, State::Idle);
            if idle {
                *state = State::Creating(Vec::new());
            }
            idle
        };
        if idle {
            self.create();
        }
    }

    /// Whether an environment was made (or is being made): set-up changes
    /// now only reach the next one.
    fn begun(&self) -> bool {
        !matches!(*self.state.borrow(), State::Idle)
    }

    /// Register `scheme` (lowercase) as a secure custom scheme. `false` if
    /// the environment already exists, so only the next one has it.
    pub fn add_scheme(&self, scheme: &str) -> bool {
        let mut setup = self.setup.borrow_mut();
        if !setup.schemes.iter().any(|s| s == scheme) {
            setup.schemes.push(scheme.to_owned());
        }
        !self.begun()
    }

    /// Set the preferred languages (normalized). `false` if the environment
    /// already exists, so only the next one has them.
    pub fn set_languages(&self, languages: Vec<String>) -> bool {
        self.setup.borrow_mut().languages = languages;
        !self.begun()
    }

    /// How long the first environment took to come up (for the smoke log).
    pub fn ready_after(&self) -> Option<Duration> {
        self.ready_after.get()
    }

    pub fn user_data_dir(&self) -> &PathBuf {
        &self.config.user_data_dir
    }

    /// The environment, once it exists (made now if it was not yet).
    pub async fn get(self: &Rc<Self>) -> Result<ICoreWebView2Environment> {
        self.start();
        let rx = {
            let mut state = self.state.borrow_mut();
            match &mut *state {
                State::Ready(e) => return Ok(e.clone()),
                State::Failed(why) => return Err(EngineError::Other(why.clone())),
                State::Creating(waiters) => {
                    let (tx, rx) = oneshot();
                    waiters.push(tx);
                    rx
                }
                State::Idle => {
                    return Err(EngineError::Other(
                        "WebView2 environment was not started".into(),
                    ));
                }
            }
        };
        rx.await
            .unwrap_or_else(|| Err(EngineError::Other("WebView2 environment dropped".into())))
    }

    /// The environment if it is ready now.
    pub fn now(&self) -> Option<ICoreWebView2Environment> {
        match &*self.state.borrow() {
            State::Ready(e) => Some(e.clone()),
            _ => None,
        }
    }

    /// The browser process died: make a new environment after it exited.
    pub fn invalidate(self: &Rc<Self>) {
        let exit_rx = {
            let mut state = self.state.borrow_mut();
            if matches!(*state, State::Creating(_) | State::Idle) {
                return;
            }
            *state = State::Creating(Vec::new());
            let (tx, rx) = oneshot();
            *self.exited.borrow_mut() = Some(tx);
            rx
        };
        tracing::warn!("WebView2 browser process failed; a new environment follows");
        let weak = Rc::downgrade(self);
        let t = Instant::now();
        copper_win_rt::spawn_local(async move {
            // The exit handler stays until the old process has exited (it
            // is what says so; without it this always waited it all out).
            let heard = within(exit_rx, EXIT_WAIT).await.is_some();
            if let Some(env) = weak.upgrade() {
                tracing::debug!(
                    "the old browser process {} after {:.0} ms",
                    if heard { "exited" } else { "had not exited" },
                    t.elapsed().as_secs_f64() * 1000.0
                );
                // Removing the handlers calls into WebView2: not under the
                // borrow.
                let old = std::mem::take(&mut *env.tokens.borrow_mut());
                drop(old);
                env.create();
            }
        });
    }

    /// The browser process ended normally (its last webview closed) since
    /// the last webview was made: if making one now fails, a new
    /// environment is the cure ([`Env::renew`]).
    pub fn ended(&self) -> bool {
        self.ended.get()
    }

    /// `f` runs when the browser process fails while no page has a webview
    /// (so no ProcessFailed tells the engine): what it held (a warm spare)
    /// went with it.
    pub fn on_lost(&self, f: Rc<dyn Fn()>) {
        *self.lost.borrow_mut() = Some(f);
    }

    /// A webview was made: the environment's browser process is up.
    pub fn alive(&self) {
        self.ended.set(false);
    }

    /// Make a new environment now (its browser process has exited already).
    pub fn renew(self: &Rc<Self>) {
        self.ended.set(false);
        {
            let mut state = self.state.borrow_mut();
            if matches!(*state, State::Creating(_) | State::Idle) {
                return;
            }
            *state = State::Creating(Vec::new());
        }
        // Removing the handlers calls into WebView2: not under the borrow.
        let old = std::mem::take(&mut *self.tokens.borrow_mut());
        drop(old);
        tracing::info!("WebView2 browser process gone; a new environment");
        self.create();
    }

    /// The options for a new environment, from the config and the set-up.
    fn options(&self) -> ICoreWebView2EnvironmentOptions {
        let setup = self.setup.borrow();
        let options = CoreWebView2EnvironmentOptions::default();
        let arguments =
            crate::args::browser_arguments(&self.config.extra_arguments, &setup.languages);
        let registrations: Vec<Option<ICoreWebView2CustomSchemeRegistration>> = setup
            .schemes
            .iter()
            .map(|s| {
                let r = CoreWebView2CustomSchemeRegistration::new(s.clone());
                unsafe {
                    // A secure context like `https:` (crypto.randomUUID,
                    // service workers, clipboard), with a host part
                    // (`copper://canvas/<id>` has origin `copper://canvas`).
                    r.set_treat_as_secure(true);
                    r.set_has_authority_component(true);
                }
                Some(r.into())
            })
            .collect();
        unsafe {
            options.set_additional_browser_arguments(arguments);
            if let Some(first) = setup.languages.first() {
                options.set_language(first.clone());
            }
            if !registrations.is_empty() {
                options.set_scheme_registrations(registrations);
            }
            options.set_are_browser_extensions_enabled(true);
            options.set_enable_tracking_prevention(true);
            options.set_is_custom_crash_reporting_enabled(true);
            options.set_allow_single_sign_on_using_os_primary_account(false);
            options.set_exclusive_user_data_folder_access(false);
            // Only the installed (stable, Evergreen) runtime: never a
            // Beta/Dev/Canary Edge the user happens to have.
            options.set_release_channels(COREWEBVIEW2_RELEASE_CHANNELS_STABLE);
        }
        options.into()
    }

    fn create(self: &Rc<Self>) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.started.set(Some(Instant::now()));
        if let Err(e) = std::fs::create_dir_all(&self.config.user_data_dir) {
            self.finish(Err(EngineError::Io(format!(
                "WebView2 user data folder {}: {e}",
                self.config.user_data_dir.display()
            ))));
            return;
        }
        let browser = self
            .config
            .browser_folder
            .as_ref()
            .map(|p| hs(&p.to_string_lossy()));
        let browser_ptr = browser
            .as_ref()
            .map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr()));
        if loader::runtime_version(browser_ptr).is_none() {
            self.finish(Err(EngineError::Other(
                "the Microsoft Edge WebView2 Runtime is not installed".into(),
            )));
            return;
        }
        let options = self.options();
        let weak: Weak<Env> = Rc::downgrade(self);
        let handler =
            CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |hr, env| {
                if let Some(this) = weak.upgrade()
                    && this.generation.get() == generation
                {
                    let result = hr
                        .and_then(|()| {
                            env.ok_or_else(|| {
                                windows::core::Error::from(windows::Win32::Foundation::E_POINTER)
                            })
                        })
                        .map_err(|e| EngineError::Other(format!("WebView2 environment: {e}")));
                    if let Ok(env) = &result {
                        this.watch_exit(env, generation);
                    }
                    this.finish(result);
                }
                Ok(())
            }));
        let udf = hs(&self.config.user_data_dir.to_string_lossy());
        let r = unsafe {
            loader::create_environment(
                browser_ptr,
                PCWSTR(udf.as_ptr()),
                options.as_raw(),
                handler.as_raw(),
            )
        };
        if let Err(e) = r {
            self.finish(Err(EngineError::Other(format!(
                "WebView2 environment: {e}"
            ))));
        }
    }

    fn watch_exit(self: &Rc<Self>, env: &ICoreWebView2Environment, generation: u64) {
        let Ok(env5) = env.cast::<ICoreWebView2Environment5>() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let r = on!(
            self.tokens.borrow_mut(),
            env5,
            BrowserProcessExited,
            BrowserProcessExitedEventHandler,
            move |_, a: Option<ICoreWebView2BrowserProcessExitedEventArgs>| {
                if let Some(this) = weak.upgrade()
                    && this.generation.get() == generation
                {
                    let (normal, pid) = a.as_ref().map_or((false, 0), exit_of);
                    let exited = this.exited.borrow_mut().take();
                    if let Some(tx) = exited {
                        tx.send(());
                    }
                    if normal {
                        // WebView2 ends its browser process once the last
                        // webview is closed: not a failure, and the same
                        // environment starts a new one for the next webview.
                        tracing::debug!(pid, "WebView2 browser process ended (no webview left)");
                        this.ended.set(true);
                    } else if this.now().is_some() {
                        // Failed with no page to hear a ProcessFailed (no
                        // page had a webview): the next page needs a new one.
                        tracing::warn!(pid, "WebView2 browser process failed");
                        let lost = this.lost.borrow().clone();
                        if let Some(f) = lost {
                            f();
                        }
                        this.renew();
                    }
                }
                Ok(())
            }
        );
        if let Err(e) = r {
            tracing::debug!("BrowserProcessExited: {e}");
        }
    }

    fn finish(&self, result: Result<ICoreWebView2Environment>) {
        if self.ready_after.get().is_none()
            && let Some(t) = self.started.get()
        {
            self.ready_after.set(Some(t.elapsed()));
            tracing::debug!(target: "copper::perf", "webview2 environment ready in {:?}", t.elapsed());
        }
        let waiters = {
            let mut state = self.state.borrow_mut();
            let next = match &result {
                Ok(e) => State::Ready(e.clone()),
                Err(e) => {
                    tracing::error!("{e}");
                    State::Failed(e.to_string())
                }
            };
            match std::mem::replace(&mut *state, next) {
                State::Creating(w) => w,
                _ => Vec::new(),
            }
        };
        for tx in waiters {
            tx.send(result.clone());
        }
    }
}

/// Whether the browser process ended normally (its last webview closed)
/// rather than failing, and its process id.
fn exit_of(a: &ICoreWebView2BrowserProcessExitedEventArgs) -> (bool, u32) {
    let mut kind = COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_FAILED;
    let mut pid = 0u32;
    // SAFETY: out-parameters on a live event argument.
    unsafe {
        let _ = a.BrowserProcessExitKind(&mut kind);
        let _ = a.BrowserProcessId(&mut pid);
    }
    (kind == COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND_NORMAL, pid)
}
