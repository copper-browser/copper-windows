//! The one `CoreWebView2Environment` of the process.
//!
//! Created when the engine is made (so the browser process starts while the
//! shell builds its window) and shared by every profile and page. If the
//! browser process dies, every webview dies with it; the environment is then
//! made again once the old process has exited (WebView2 refuses a second
//! environment on a user data folder still in use).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use copper_engine::{EngineError, Result};
use copper_win_rt::{OneshotTx, oneshot};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{CoreWebView2EnvironmentOptions, CreateCoreWebView2EnvironmentCompletedHandler};
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
    pub arguments: String,
}

enum State {
    Creating(Vec<OneshotTx<Result<ICoreWebView2Environment>>>),
    Ready(ICoreWebView2Environment),
    Failed(String),
}

pub(crate) struct Env {
    config: EnvConfig,
    state: RefCell<State>,
    /// Bumped each time an environment is made; stale browser-exit events
    /// are ignored.
    generation: Cell<u64>,
    tokens: RefCell<Tokens>,
    /// Signalled when the current browser process exits.
    exited: RefCell<Option<OneshotTx<()>>>,
    started: Cell<Option<Instant>>,
    ready_after: Cell<Option<Duration>>,
}

impl Env {
    /// Start making the environment.
    pub fn start(config: EnvConfig) -> Rc<Env> {
        let env = Rc::new(Env {
            config,
            state: RefCell::new(State::Creating(Vec::new())),
            generation: Cell::new(0),
            tokens: RefCell::new(Tokens::default()),
            exited: RefCell::new(None),
            started: Cell::new(None),
            ready_after: Cell::new(None),
        });
        env.create();
        env
    }

    /// How long the first environment took to come up (for the smoke log).
    pub fn ready_after(&self) -> Option<Duration> {
        self.ready_after.get()
    }

    pub fn user_data_dir(&self) -> &PathBuf {
        &self.config.user_data_dir
    }

    /// The environment, once it exists.
    pub async fn get(self: &Rc<Self>) -> Result<ICoreWebView2Environment> {
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
            if matches!(*state, State::Creating(_)) {
                return;
            }
            *state = State::Creating(Vec::new());
            let (tx, rx) = oneshot();
            *self.exited.borrow_mut() = Some(tx);
            rx
        };
        self.tokens.borrow_mut().clear();
        tracing::warn!("WebView2 browser process exited; recreating the environment");
        let weak = Rc::downgrade(self);
        copper_win_rt::spawn_local(async move {
            let _ = within(exit_rx, EXIT_WAIT).await;
            if let Some(env) = weak.upgrade() {
                env.create();
            }
        });
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
        let options = CoreWebView2EnvironmentOptions::default();
        unsafe {
            options.set_additional_browser_arguments(self.config.arguments.clone());
            options.set_are_browser_extensions_enabled(true);
            options.set_enable_tracking_prevention(true);
            options.set_is_custom_crash_reporting_enabled(true);
            options.set_allow_single_sign_on_using_os_primary_account(false);
            options.set_exclusive_user_data_folder_access(false);
            // Only the installed (stable, Evergreen) runtime: never a
            // Beta/Dev/Canary Edge the user happens to have.
            options.set_release_channels(COREWEBVIEW2_RELEASE_CHANNELS_STABLE);
        }
        let options: ICoreWebView2EnvironmentOptions = options.into();
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
            move |_, _| {
                if let Some(this) = weak.upgrade()
                    && this.generation.get() == generation
                {
                    if let Some(tx) = this.exited.borrow_mut().take() {
                        tx.send(());
                    }
                    // Exited without a ProcessFailed (e.g. killed with the
                    // last webview gone): the next page needs a new one.
                    if this.now().is_some() {
                        this.invalidate();
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
