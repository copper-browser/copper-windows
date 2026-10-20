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

use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

use super::backstage::WebView2Backstage;
use super::env::{Env, EnvConfig};
use super::page::{PageInner, WebView2Page};
use super::profile::{ProfileInner, WebView2Profile};
use super::save_dialog::{AskOwner, SaveQueue};
use super::schemes::Schemes;
use super::window::{OwnedWindow, park_window};
use super::{com, paths};
use crate::appearance::{ColorScheme, Ground};
use crate::filter::{FilterStore, WebView2ContentFilter};
use crate::profile_home;

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
    /// The profile kept in WebView2's own default profile, which the browser
    /// process sets up anyway, so a fresh install initializes one profile
    /// rather than two (the first navigation is seconds sooner). It keeps
    /// its named WebView2 profile if it already has one. Default: `default`,
    /// the shell's shared profile; `None`: every profile is named.
    pub main_profile: Option<ProfileId>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            paths: None,
            user_data_dir: None,
            browser_folder: None,
            extra_arguments: Vec::new(),
            builtin_error_page: true,
            main_profile: ProfileId::new("default").ok(),
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

    /// Every live page of the engine.
    pub fn live_pages(&self) -> Vec<Rc<PageInner>> {
        self.pages
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }

    fn changed(&self) {
        for p in self.live_pages() {
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
    /// See [`EngineConfig::main_profile`].
    main_profile: Option<ProfileId>,
    pub backstage: WebView2Backstage,
    /// Custom-scheme handlers (`Engine::register_scheme`).
    pub schemes: Schemes,
    /// Downloads ask where to save, with the dialog owned by this window
    /// (`set_download_ask`); `None`: saved to the download folder.
    pub download_ask: Cell<Option<AskOwner>>,
    /// The Save As dialogs downloads ask with, one at a time.
    pub save_queue: SaveQueue,
    /// Where an open Save As dialog leaves its mark (`crate::partials`).
    pub asks_dir: PathBuf,
    /// What every webview shows where its page has painted nothing
    /// (`set_page_ground`).
    pub ground: Cell<Ground>,
    /// What pages are told about the colour scheme (`set_color_scheme`).
    pub color_scheme: Cell<ColorScheme>,
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
    /// Start the engine: the browser process starts on the next loop turn
    /// (or as soon as something needs it), in the background — so
    /// `set_preferred_languages` and `register_scheme` called right after
    /// this are part of it. Call on the UI thread after
    /// `copper_win_rt::init()`.
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
        let env = Env::new(EnvConfig {
            user_data_dir: config.user_data_dir.unwrap_or(default_udf),
            browser_folder: config.browser_folder,
            extra_arguments: extra,
        });
        let park = park_window().map_err(|e| com::other("host window", e))?;
        let asks_dir = paths.cache_root.join("download-asks");
        sweep_dead_asks(asks_dir.clone());
        let filters = FilterStore::new(paths.content_filter_dir());
        let inner = Rc::new(EngineInner {
            paths,
            env,
            park,
            profiles: RefCell::default(),
            filters,
            scripts: EngineScripts::default(),
            builtin_error_page: Cell::new(config.builtin_error_page),
            main_profile: config.main_profile,
            backstage: WebView2Backstage::new(),
            schemes: Schemes::default(),
            download_ask: Cell::new(None),
            save_queue: SaveQueue::default(),
            asks_dir,
            ground: Cell::new(Ground::WHITE),
            color_scheme: Cell::new(ColorScheme::Auto),
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

    /// Ask where to save every download before it writes anything (`true`),
    /// or save it to its profile's download folder under a free name
    /// (`false`, the default). Asking holds a starting download — a page's
    /// or `Profile::download`'s — while the system Save As dialog, owned by
    /// `owner` (disabled while it is open), offers the suggested name in the
    /// profile's download folder; it runs on a thread of its own, so the UI
    /// thread keeps running. Saved: the download goes to exactly that path
    /// and reports `Started` and `Destination` as usual. Cancelled or
    /// closed: it is cancelled before a byte is written (no file, no
    /// partial file) and reports only `Failed { cancelled: true }`.
    /// Applies to downloads starting from now on.
    pub fn set_download_ask(&self, ask: bool, owner: HWND) {
        let owner = ask.then_some(AskOwner(owner.0 as isize));
        self.inner.download_ask.set(owner);
    }

    /// The colour every webview shows where its page has painted nothing:
    /// before the first paint of a cold start or a new tab, on a blank or
    /// failed page, around a page smaller than its slot. Give it the
    /// window's ground, so the dark never flashes white. Applies at once to
    /// every page and warm spare, and to every webview made later.
    pub fn set_page_ground(&self, ground: Ground) {
        if self.inner.ground.replace(ground) == ground {
            return;
        }
        for p in self.inner.profiles() {
            p.ground_changed(ground);
        }
    }

    /// The colour scheme pages are told to use (`prefers-color-scheme`,
    /// form controls, scroll bars): `Auto` follows Windows, `Light` and
    /// `Dark` hold whatever Windows does — Copper's Look setting. Applies at
    /// once to every profile (InPrivate ones too), and to profiles made
    /// later.
    pub fn set_color_scheme(&self, scheme: ColorScheme) {
        if self.inner.color_scheme.replace(scheme) == scheme {
            return;
        }
        for p in self.inner.profiles() {
            p.color_scheme_changed(scheme);
        }
    }
}

/// Off the UI thread: remove the partial files of downloads whose Save As
/// dialog was open when Copper last ended without closing it.
fn sweep_dead_asks(dir: PathBuf) {
    let now = std::time::SystemTime::now();
    let spawned = std::thread::Builder::new()
        .name("copper-download-sweep".into())
        .spawn(move || {
            for f in crate::partials::sweep(&dir, now) {
                tracing::info!("removed a cut-off download's partial file {}", f.display());
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("download sweep: {e}");
    }
}

impl Engine for WebView2Engine {
    /// The environment's `Language` (the first entry: UI language and
    /// first `Accept-Language` entry) and `--accept-lang` (the whole list:
    /// `Accept-Language`, `navigator.languages`, the default `Intl`
    /// locale). Both are fixed when the browser process starts: called
    /// after the environment was made, the list applies from the next one
    /// (after a browser crash, or the next launch), with a warning.
    fn set_preferred_languages(&self, languages: &[String]) {
        let list = crate::languages::normalize(languages);
        let shown = list.join(",");
        if !self.inner.env.set_languages(list) {
            tracing::warn!(
                "set_preferred_languages({shown}) after the WebView2 environment was made: \
                 pages get these languages from the next browser process on"
            );
        }
    }

    /// A secure custom scheme with an authority (`copper://canvas/1` has
    /// origin `copper://canvas`), registered with the environment and
    /// answered through every webview's `WebResourceRequested`. The
    /// registration is part of the environment, so register before it is
    /// made (right after `WebView2Engine::new`); a later scheme still gets
    /// its handler and filters, with a warning, but WebView2 only routes
    /// its requests from the next browser process on.
    fn register_scheme(&self, scheme: &str, handler: copper_engine::SchemeHandler) -> Result<()> {
        let scheme = crate::scheme::normalize(scheme).map_err(EngineError::InvalidArgument)?;
        if !self.inner.schemes.insert(&scheme, handler) {
            // A new handler for a known scheme: nothing else changes.
            return Ok(());
        }
        if !self.inner.env.add_scheme(&scheme) {
            tracing::warn!(
                "register_scheme({scheme}) after the WebView2 environment was made: \
                 {scheme}: URLs reach the handler from the next browser process on"
            );
        }
        for page in self.inner.scripts.live_pages() {
            page.scheme_added(&scheme);
        }
        Ok(())
    }

    type Profile = WebView2Profile;
    type Filter = WebView2ContentFilter;
    type Page = WebView2Page;
    type Backstage = WebView2Backstage;

    fn paths(&self) -> &EnginePaths {
        &self.inner.paths
    }

    /// A WebView2 profile per id (`ControllerOptions.ProfileName`, or
    /// WebView2's default profile for [`EngineConfig::main_profile`]); an
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
        let udf = self.inner.env.user_data_dir();
        let name = profile_home::webview2_profile(
            id,
            ephemeral,
            self.inner.main_profile.as_ref(),
            |name| profile_home::kept(&profile_home::named_profile_dir(udf, name)),
        );
        if name.is_none() {
            tracing::debug!("profile {id}: in WebView2's default profile");
        }
        let inner = ProfileInner::new(
            self.inner.clone(),
            id.clone(),
            ephemeral,
            data_dir,
            name,
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
