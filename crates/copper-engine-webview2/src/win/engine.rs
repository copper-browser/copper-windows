//! `Engine`: the process's WebView2 environment, its profiles, pages,
//! content-filter store, engine-wide page scripts and backstage.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use copper_engine::{
    Capabilities, Engine, EngineError, EnginePaths, LocalFuture, PageOptions, ProfileId, Result,
    ScriptId, UserScript,
};

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

use super::backstage::WebView2Backstage;
use super::env::{Env, EnvConfig};
use super::page::{PageInner, WebView2Page};
use super::profile::{ProfileInner, WebView2Profile};
use super::window::{OwnedWindow, park_window};
use super::{com, paths};
use crate::filter::{FilterStore, WebView2ContentFilter};
use crate::util::webview2_profile_name;

/// Extra browser switches, space-separated (diagnostics; see `args.rs`).
pub const ARGS_ENV: &str = "COPPER_WEBVIEW2_ARGS";

/// How the engine is set up. `Default` is Copper's standard layout.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Copper's own folders (profile data, caches, downloads); `None` = the
    /// standard ones under `%LOCALAPPDATA%\Copper`.
    pub paths: Option<EnginePaths>,
    /// The WebView2 user data folder (all web storage of every profile);
    /// `None` = `%LOCALAPPDATA%\Copper\WebView2`.
    pub user_data_dir: Option<PathBuf>,
    /// A fixed-version runtime folder; `None` = the installed Evergreen
    /// runtime.
    pub browser_folder: Option<PathBuf>,
    /// Extra browser switches, after Copper's (and after [`ARGS_ENV`]).
    pub extra_arguments: Vec<String>,
    /// Keep WebView2's own error page for failed loads (see
    /// [`WebView2Engine::set_builtin_error_page`]).
    pub builtin_error_page: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            paths: None,
            user_data_dir: None,
            browser_folder: None,
            extra_arguments: Vec::new(),
            builtin_error_page: true,
        }
    }
}

/// Scripts every page of the engine gets (`Engine::add_page_script`).
#[derive(Default)]
pub(crate) struct EngineScripts {
    pub scripts: RefCell<Vec<(ScriptId, UserScript)>>,
    pages: RefCell<Vec<Weak<PageInner>>>,
}

impl EngineScripts {
    pub fn register(&self, page: &Rc<PageInner>) {
        let mut pages = self.pages.borrow_mut();
        pages.retain(|w| w.strong_count() > 0);
        pages.push(Rc::downgrade(page));
    }

    fn changed(&self) {
        let pages: Vec<Rc<PageInner>> = self
            .pages
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for p in pages {
            p.scripts_changed();
        }
    }
}

pub(crate) struct EngineInner {
    pub paths: EnginePaths,
    pub env: Rc<Env>,
    /// Parent of every controller no shell window hosts.
    pub park: OwnedWindow,
    profiles: RefCell<HashMap<(ProfileId, bool), Weak<ProfileInner>>>,
    filters: FilterStore,
    pub scripts: EngineScripts,
    pub builtin_error_page: Cell<bool>,
    pub backstage: WebView2Backstage,
}

impl EngineInner {
    /// Every live profile.
    pub fn profiles(&self) -> Vec<Rc<ProfileInner>> {
        self.profiles
            .borrow()
            .values()
            .filter_map(Weak::upgrade)
            .collect()
    }

    /// The browser process died: everything WebView2 gave us is gone.
    /// Pages are rebuilt (with their URL stacks) when next used.
    pub fn browser_died(&self) {
        self.env.invalidate();
        for p in self.profiles() {
            p.reset();
        }
    }
}

/// The WebView2 engine. Cheap to clone (shared handle). UI thread only.
#[derive(Clone)]
pub struct WebView2Engine {
    pub(crate) inner: Rc<EngineInner>,
}

impl WebView2Engine {
    /// Start the engine: the browser process starts now, in the background.
    /// Call on the UI thread after `copper_win_rt::init()`.
    pub fn new(config: EngineConfig) -> Result<Self> {
        // WebView2 needs a single-threaded apartment on the UI thread. A
        // thread already in one (the app's `OleInitialize`) is fine.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            return Err(EngineError::Other(
                "the UI thread is in a multithreaded COM apartment; WebView2 needs an STA".into(),
            ));
        }
        let (default_paths, default_udf) = paths::default_paths()?;
        let paths = config.paths.unwrap_or(default_paths);
        let mut extra: Vec<String> = std::env::var(ARGS_ENV).ok().into_iter().collect();
        extra.extend(config.extra_arguments);
        let env = Env::start(EnvConfig {
            user_data_dir: config.user_data_dir.unwrap_or(default_udf),
            browser_folder: config.browser_folder,
            arguments: crate::args::browser_arguments(&extra),
        });
        let park = park_window().map_err(|e| com::other("host window", e))?;
        let filters = FilterStore::new(paths.content_filter_dir());
        let inner = Rc::new(EngineInner {
            paths,
            env,
            park,
            profiles: RefCell::default(),
            filters,
            scripts: EngineScripts::default(),
            builtin_error_page: Cell::new(config.builtin_error_page),
            backstage: WebView2Backstage::new(),
        });
        Ok(WebView2Engine { inner })
    }

    /// The standard engine (`EngineConfig::default()`).
    pub fn with_defaults() -> Result<Self> {
        Self::new(EngineConfig::default())
    }

    /// Wait until the WebView2 environment is up (or failed to come up:
    /// e.g. no runtime installed).
    pub async fn ready(&self) -> Result<()> {
        self.inner.env.get().await.map(drop)
    }

    /// How long the environment took to come up, once it has.
    pub fn ready_after(&self) -> Option<Duration> {
        self.inner.env.ready_after()
    }

    /// The installed WebView2 Runtime's version, if any.
    pub fn runtime_version() -> Option<String> {
        super::loader::runtime_version(windows::core::PCWSTR::null())
    }

    /// The WebView2 user data folder.
    pub fn user_data_dir(&self) -> PathBuf {
        self.inner.env.user_data_dir().clone()
    }

    /// Use WebView2's own error page for failed loads (default), or show
    /// nothing so the shell draws its own from `LoadState::Failed`.
    /// Applies to pages made from now on.
    pub fn set_builtin_error_page(&self, enabled: bool) {
        self.inner.builtin_error_page.set(enabled);
    }
}

impl Engine for WebView2Engine {
    // TODO(engine-shell): Accept-Language / navigator.languages and custom
    // schemes (`copper://`), registered with the environment.
    fn set_preferred_languages(&self, _languages: &[String]) {}

    fn register_scheme(&self, scheme: &str, _handler: copper_engine::SchemeHandler) -> Result<()> {
        Err(EngineError::Other(format!(
            "{scheme}: custom schemes are not wired yet"
        )))
    }

    type Profile = WebView2Profile;
    type Filter = WebView2ContentFilter;
    type Page = WebView2Page;
    type Backstage = WebView2Backstage;

    fn paths(&self) -> &EnginePaths {
        &self.inner.paths
    }

    /// A WebView2 profile per id (`ControllerOptions.ProfileName`); an
    /// ephemeral one is InPrivate (nothing written to disk).
    fn profile(&self, id: &ProfileId, ephemeral: bool) -> Result<WebView2Profile> {
        let key = (id.clone(), ephemeral);
        if let Some(inner) = self
            .inner
            .profiles
            .borrow()
            .get(&key)
            .and_then(Weak::upgrade)
        {
            return Ok(WebView2Profile { inner });
        }
        let data_dir = if ephemeral {
            None
        } else {
            let dir = self.inner.paths.profile_data_dir(id);
            std::fs::create_dir_all(&dir)?;
            Some(dir)
        };
        let inner = ProfileInner::new(
            self.inner.clone(),
            id.clone(),
            ephemeral,
            data_dir,
            webview2_profile_name(id.as_str()),
            self.inner.paths.downloads_dir.clone(),
        );
        let mut cache = self.inner.profiles.borrow_mut();
        cache.retain(|_, w| w.strong_count() > 0);
        cache.insert(key, Rc::downgrade(&inner));
        Ok(WebView2Profile { inner })
    }

    /// The page comes back at once; its webview follows (a warm spare
    /// usually, so within a frame or two). Everything done to the page
    /// meanwhile is queued and applied in order.
    fn new_page(&self, profile: &WebView2Profile, opts: PageOptions) -> WebView2Page {
        WebView2Page::create(&profile.inner, opts, false)
    }

    fn compile_content_filter(
        &self,
        id: &str,
        rules_json: &str,
    ) -> LocalFuture<Result<WebView2ContentFilter>> {
        com::ready(self.inner.filters.compile(id, rules_json))
    }

    fn load_content_filter(&self, id: &str) -> LocalFuture<Result<Option<WebView2ContentFilter>>> {
        com::ready(self.inner.filters.load(id))
    }

    fn remove_content_filter(&self, id: &str) -> LocalFuture<Result<()>> {
        com::ready(self.inner.filters.remove(id))
    }

    fn backstage(&self) -> &WebView2Backstage {
        &self.inner.backstage
    }

    /// WebView2 (Chromium) has WebRTC, camera/microphone and WebAuthn
    /// (through Windows Hello). Widevine is not part of the WebView2
    /// Runtime, so DRM video is reported unavailable.
    #[allow(clippy::field_reassign_with_default)] // `Capabilities` is non-exhaustive
    fn capabilities(&self) -> Capabilities {
        let mut c = Capabilities::default();
        c.webrtc = true;
        c.media_stream = true;
        c.web_authn = true;
        c.encrypted_media = false;
        c
    }

    fn add_page_script(&self, script: &UserScript) -> ScriptId {
        let id = ScriptId(crate::util::next_id());
        self.inner
            .scripts
            .scripts
            .borrow_mut()
            .push((id, script.clone()));
        self.inner.scripts.changed();
        id
    }

    fn remove_page_script(&self, id: ScriptId) -> bool {
        let removed = {
            let mut s = self.inner.scripts.scripts.borrow_mut();
            let before = s.len();
            s.retain(|(i, _)| *i != id);
            s.len() != before
        };
        if removed {
            self.inner.scripts.changed();
        }
        removed
    }
}

impl std::fmt::Debug for WebView2Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebView2Engine")
            .field("paths", &self.inner.paths)
            .field("user_data_dir", self.inner.env.user_data_dir())
            .finish()
    }
}
