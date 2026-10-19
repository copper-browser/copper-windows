//! `Profile`: a WebView2 profile (`ControllerOptions.ProfileName`, InPrivate
//! for ephemeral ones), its cookies, website data, downloads and the
//! profile-wide content-filter extensions.
//!
//! WebView2 only hands out a profile object through a webview, so each
//! profile keeps one hidden *spare* webview: it anchors the profile for
//! cookie and data calls made while no page exists, and it is the next
//! page's controller (adopting it skips the ~100 ms creation). A new spare
//! is warmed whenever one is taken.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use copper_engine::{
    Cookie, DownloadEvent, DownloadId, EngineError, EventStream, LocalFuture, Profile, ProfileId,
    Result, WebsiteDataKinds,
};
use copper_win_rt::{OneshotTx, oneshot};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    BrowserExtensionRemoveCompletedHandler, ClearBrowsingDataCompletedHandler,
    GetCookiesCompletedHandler, ProfileAddBrowserExtensionCompletedHandler,
    ProfileGetBrowserExtensionsCompletedHandler,
};
use windows::core::Interface;

use super::cdp;
use super::com::{get, get_bool, get_string, hs, other};
use super::controller::{self, Wv};
use super::downloads::{Downloads, Job};
use super::engine::EngineInner;
use super::fetcher;
use super::page::PageInner;
use crate::convert;
use crate::dnr::EXTENSION_NAME_PREFIX;
use crate::filter::WebView2ContentFilter;
use crate::util::site_matches;

/// Changes to pages' filters are applied together after this pause.
const FILTER_DEBOUNCE: Duration = Duration::from_millis(50);

/// How long a cookie deletion may take to show (see
/// [`ProfileInner::until_deleted`]).
const COOKIE_DELETE_LIMIT: Duration = Duration::from_secs(2);

enum Spare {
    Empty,
    Creating,
    Ready(Wv),
}

/// The profile's content-filter extensions.
#[derive(Default)]
struct FilterSync {
    /// Filter id → (extension id, filter version) installed this session.
    installed: RefCell<HashMap<String, (String, u64)>>,
    /// Extensions left from an earlier run have been removed.
    swept: Cell<bool>,
    running: Cell<bool>,
    dirty: Cell<bool>,
}

pub(crate) struct ProfileInner {
    pub engine: Rc<EngineInner>,
    pub id: ProfileId,
    pub ephemeral: bool,
    data_dir: Option<PathBuf>,
    /// The WebView2 profile name.
    pub name: String,
    download_dir: RefCell<PathBuf>,
    pub downloads: Rc<Downloads>,
    /// The profile object of the browser process's first webview of this
    /// profile, once Copper's defaults are applied (`None`: no webview
    /// yet, or the browser process restarted). Only a marker and a last
    /// resort: see [`ProfileInner::core`].
    core: RefCell<Option<ICoreWebView2Profile>>,
    /// Waiting for the next webview of this profile.
    core_waiters: RefCell<Vec<OneshotTx<Result<ICoreWebView2Profile>>>>,
    spare: RefCell<Spare>,
    pub pages: RefCell<Vec<Weak<PageInner>>>,
    filters: FilterSync,
    pub fetcher: RefCell<fetcher::Fetcher>,
}

/// A WebView2 profile. Cheap to clone.
#[derive(Clone)]
pub struct WebView2Profile {
    pub(crate) inner: Rc<ProfileInner>,
}

impl ProfileInner {
    pub(crate) fn new(
        engine: Rc<EngineInner>,
        id: ProfileId,
        ephemeral: bool,
        data_dir: Option<PathBuf>,
        name: String,
        download_dir: PathBuf,
    ) -> Rc<Self> {
        let p = Rc::new(ProfileInner {
            engine,
            id,
            ephemeral,
            data_dir,
            name,
            download_dir: RefCell::new(download_dir),
            downloads: Rc::new(Downloads::default()),
            core: RefCell::new(None),
            core_waiters: RefCell::new(Vec::new()),
            spare: RefCell::new(Spare::Empty),
            pages: RefCell::new(Vec::new()),
            filters: FilterSync::default(),
            fetcher: RefCell::new(fetcher::Fetcher::default()),
        });
        p.refill();
        p
    }

    pub fn download_dir(&self) -> PathBuf {
        self.download_dir.borrow().clone()
    }

    /// A controller for a page: the warm spare if there is one.
    pub async fn new_controller(self: &Rc<Self>) -> Result<Wv> {
        // Taken first: a `borrow_mut()` in the match scrutinee would live
        // to the end of the match, and the `Creating` arm borrows again.
        let taken = std::mem::replace(&mut *self.spare.borrow_mut(), Spare::Empty);
        let spare = match taken {
            Spare::Ready(wv) => Some(wv),
            Spare::Creating => {
                *self.spare.borrow_mut() = Spare::Creating;
                None
            }
            Spare::Empty => None,
        };
        let wv = match spare {
            Some(wv) => wv,
            None => self.create().await?,
        };
        self.refill();
        Ok(wv)
    }

    /// A fresh hidden controller in this profile.
    pub async fn create(self: &Rc<Self>) -> Result<Wv> {
        let env = self.engine.env.get().await?;
        let wv = controller::create(&env, self.engine.park.0, &self.name, self.ephemeral).await?;
        self.saw(&wv);
        Ok(wv)
    }

    /// A new webview of this profile: the first (per browser process)
    /// gives the profile Copper's defaults; every one answers whoever waits
    /// for the profile object.
    fn saw(self: &Rc<Self>, wv: &Wv) {
        let Some(core) = controller::profile_of(wv) else {
            return;
        };
        let first = self.core.borrow().is_none();
        if first {
            controller::configure_profile(&core, &self.download_dir());
            *self.core.borrow_mut() = Some(core.clone());
        }
        for tx in self.core_waiters.take() {
            tx.send(Ok(core.clone()));
        }
        if first {
            self.filters_changed();
        }
    }

    /// Warm a spare unless one exists or is on its way.
    fn refill(self: &Rc<Self>) {
        {
            let mut spare = self.spare.borrow_mut();
            if !matches!(*spare, Spare::Empty) {
                return;
            }
            *spare = Spare::Creating;
        }
        let me = Rc::downgrade(self);
        copper_win_rt::spawn_local(async move {
            let Some(p) = me.upgrade() else { return };
            let r = p.create().await;
            // Decided under the borrow, done after it: closing a controller
            // and answering waiters call out.
            let unwanted = {
                let mut spare = p.spare.borrow_mut();
                match r {
                    Ok(wv) if matches!(*spare, Spare::Creating) => {
                        *spare = Spare::Ready(wv);
                        Ok(None)
                    }
                    Ok(wv) => Ok(Some(wv)),
                    Err(e) => {
                        *spare = Spare::Empty;
                        Err(e)
                    }
                }
            };
            match unwanted {
                Ok(Some(wv)) => controller::close(&wv),
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("profile {}: {e}", p.id);
                    for tx in p.core_waiters.take() {
                        tx.send(Err(e.clone()));
                    }
                }
            }
        });
    }

    /// The WebView2 profile object, taken from a webview that is open now.
    ///
    /// Never a profile object kept from an earlier webview: once the webview
    /// it came from is closed, WebView2 answers every call on it with
    /// `ERROR_INVALID_STATE` — and the first webview is usually the warm
    /// spare, which a page takes and closes with it. With no webview open
    /// this waits for the next one (the spare, made on demand).
    pub async fn core(self: &Rc<Self>) -> Result<ICoreWebView2Profile> {
        if let Some(c) = self.live_profile() {
            return Ok(c);
        }
        let (tx, rx) = oneshot();
        self.core_waiters.borrow_mut().push(tx);
        self.refill();
        rx.await
            .unwrap_or_else(|| Err(EngineError::Other("profile closed".into())))
    }

    /// The profile object of a webview open now, if there is one.
    fn live_profile(&self) -> Option<ICoreWebView2Profile> {
        let view = self.live_view()?;
        unsafe { view.cast::<ICoreWebView2_13>().ok()?.Profile().ok() }
    }

    /// A webview of this profile open now: the warm spare, a page's, or the
    /// download fetcher's.
    fn live_view(&self) -> Option<ICoreWebView2> {
        if let Spare::Ready(wv) = &*self.spare.borrow() {
            return Some(wv.view.clone());
        }
        let page = self
            .pages
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .find_map(|p| p.wv().map(|w| w.view));
        page.or_else(|| self.fetcher.borrow().view())
    }

    /// A webview of this profile to send profile-wide CDP calls through.
    async fn any_view(self: &Rc<Self>) -> Result<ICoreWebView2> {
        for _ in 0..2 {
            if let Some(view) = self.live_view() {
                return Ok(view);
            }
            self.core().await?;
            copper_win_rt::sleep(Duration::from_millis(50)).await;
        }
        Err(EngineError::Other("no webview for this profile".into()))
    }

    /// The browser process died: everything WebView2 gave us is gone.
    pub fn reset(&self) {
        // Taken out and dropped after each borrow: releasing WebView2
        // objects calls out.
        let core = self.core.borrow_mut().take();
        let spare = std::mem::replace(&mut *self.spare.borrow_mut(), Spare::Empty);
        let retired = self.fetcher.borrow_mut().reset();
        drop((core, spare, retired));
        self.filters.installed.borrow_mut().clear();
        self.filters.swept.set(false);
    }

    pub fn add_page(&self, page: &Rc<PageInner>) {
        let mut pages = self.pages.borrow_mut();
        pages.retain(|w| w.strong_count() > 0);
        pages.push(Rc::downgrade(page));
    }

    // -- content filters -------------------------------------------------

    /// The union of every live page's filters.
    fn desired_filters(&self) -> BTreeMap<String, WebView2ContentFilter> {
        let mut out = BTreeMap::new();
        let mut pages = self.pages.borrow_mut();
        pages.retain(|w| w.strong_count() > 0);
        for p in pages.iter().filter_map(Weak::upgrade) {
            for f in p.filters() {
                out.insert(copper_engine::ContentFilter::id(&f).to_owned(), f);
            }
        }
        out
    }

    /// The installed filters are exactly `desired`, at their versions.
    fn filters_in_step(&self, desired: &BTreeMap<String, WebView2ContentFilter>) -> bool {
        let installed = self.filters.installed.borrow();
        installed.len() == desired.len()
            && desired.iter().all(|(id, f)| {
                installed
                    .get(id)
                    .is_some_and(|(_, version)| *version == f.version())
            })
    }

    /// A page's filters changed (or a page went away): bring the profile's
    /// extensions in line, soon.
    pub fn filters_changed(self: &Rc<Self>) {
        self.filters.dirty.set(true);
        if self.filters.running.replace(true) {
            return;
        }
        let me = Rc::downgrade(self);
        copper_win_rt::spawn_local(async move {
            loop {
                copper_win_rt::sleep(FILTER_DEBOUNCE).await;
                let Some(p) = me.upgrade() else { return };
                if !p.filters.dirty.replace(false) {
                    p.filters.running.set(false);
                    return;
                }
                if p.core.borrow().is_none() {
                    // Applied when the first webview exists (`saw`).
                    p.filters.running.set(false);
                    return;
                }
                if let Err(e) = p.sync_filters().await {
                    tracing::warn!("profile {}: content filters: {e}", p.id);
                }
            }
        });
    }

    async fn sync_filters(self: &Rc<Self>) -> Result<()> {
        let core = self.core().await?;
        let Ok(p7) = core.cast::<ICoreWebView2Profile7>() else {
            return Err(EngineError::ContentFilter(
                "this WebView2 Runtime cannot load extensions".into(),
            ));
        };
        let desired = self.desired_filters();
        // Nothing to do (the usual case when a page merely got a webview):
        // no round trip to the browser.
        if self.filters.swept.get() && self.filters_in_step(&desired) {
            return Ok(());
        }
        let present = list_extensions(&p7).await?;
        if !self.filters.swept.replace(true) {
            for (ext, name) in &present {
                if name.starts_with(EXTENSION_NAME_PREFIX) {
                    remove_extension(ext).await;
                }
            }
            self.filters.installed.borrow_mut().clear();
        } else {
            let stale: Vec<(String, String)> = self
                .filters
                .installed
                .borrow()
                .iter()
                .filter(|(id, (_, version))| {
                    desired.get(*id).is_none_or(|f| f.version() != *version)
                })
                .map(|(id, (ext, _))| (id.clone(), ext.clone()))
                .collect();
            for (id, ext_id) in stale {
                if let Some((ext, _)) = present.iter().find(|(e, _)| extension_id(e) == ext_id) {
                    remove_extension(ext).await;
                }
                self.filters.installed.borrow_mut().remove(&id);
            }
        }
        for (id, f) in desired {
            if self.filters.installed.borrow().contains_key(&id) {
                continue;
            }
            match add_extension(&p7, f.dir()).await {
                Ok(ext) => {
                    let ext_id = extension_id(&ext);
                    self.filters
                        .installed
                        .borrow_mut()
                        .insert(id, (ext_id, f.version()));
                }
                Err(e) => tracing::warn!("content filter {id}: {e}"),
            }
        }
        tracing::debug!(
            "profile {}: content filters in step ({} installed, {} extension(s) found)",
            self.id,
            self.filters.installed.borrow().len(),
            present.len()
        );
        Ok(())
    }

    // -- cookies ---------------------------------------------------------

    async fn cookie_manager(self: &Rc<Self>) -> Result<ICoreWebView2CookieManager> {
        let core = self.core().await?;
        let p5: ICoreWebView2Profile5 = core.cast().map_err(|_| {
            EngineError::Cookies("this WebView2 Runtime has no cookie manager".into())
        })?;
        unsafe { p5.CookieManager() }.map_err(|e| EngineError::Cookies(e.to_string()))
    }

    async fn get_cookies(self: &Rc<Self>, url: &str) -> Result<Vec<Cookie>> {
        let mgr = self.cookie_manager().await?;
        let (tx, rx) = oneshot::<windows::core::Result<Vec<Cookie>>>();
        let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
            tx.send(hr.map(|()| list.map(|l| read_cookies(&l)).unwrap_or_default()));
            Ok(())
        }));
        unsafe { mgr.GetCookies(&hs(url), &handler) }
            .map_err(|e| EngineError::Cookies(e.to_string()))?;
        rx.await
            .ok_or_else(|| EngineError::Cookies("no reply".into()))?
            .map_err(|e| EngineError::Cookies(e.to_string()))
    }

    async fn put_cookie(self: &Rc<Self>, c: Cookie) -> Result<()> {
        convert::validate_cookie(&c)?;
        let mgr = self.cookie_manager().await?;
        let e = |e: windows::core::Error| EngineError::Cookies(e.to_string());
        unsafe {
            let ck = mgr
                .CreateCookie(&hs(&c.name), &hs(&c.value), &hs(&c.domain), &hs(&c.path))
                .map_err(e)?;
            ck.SetExpires(convert::expires_to(c.expires)).map_err(e)?;
            ck.SetIsSecure(c.secure).map_err(e)?;
            ck.SetIsHttpOnly(c.http_only).map_err(e)?;
            ck.SetSameSite(COREWEBVIEW2_COOKIE_SAME_SITE_KIND(convert::same_site_kind(
                c.same_site,
            )))
            .map_err(e)?;
            mgr.AddOrUpdateCookie(&ck).map_err(e)
        }
    }

    async fn drop_cookie(self: &Rc<Self>, c: Cookie) -> Result<()> {
        let mgr = self.cookie_manager().await?;
        request_delete(&mgr, &c)?;
        let url = format!("https://{}{}", c.domain.trim_start_matches('.'), c.path);
        self.until_deleted(&url, std::slice::from_ref(&c)).await
    }

    /// WebView2 deletes cookies in the background: a read right after the
    /// call still lists them (for some tens of ms). Resolve once `url`'s
    /// cookies (`""`: all) no longer include any of `gone`, so the caller's
    /// next read agrees — or after a short limit, as the deletion was asked
    /// for either way.
    async fn until_deleted(self: &Rc<Self>, url: &str, gone: &[Cookie]) -> Result<()> {
        let started = Instant::now();
        loop {
            let left = self.get_cookies(url).await?;
            if !left.iter().any(|k| gone.iter().any(|g| same_cookie(k, g))) {
                return Ok(());
            }
            if started.elapsed() >= COOKIE_DELETE_LIMIT {
                tracing::debug!(
                    "profile {}: deleted cookies still listed after {} ms",
                    self.id,
                    started.elapsed().as_millis()
                );
                return Ok(());
            }
            copper_win_rt::sleep(Duration::from_millis(10)).await;
        }
    }

    // -- website data ----------------------------------------------------

    pub(crate) async fn clear_data(
        self: &Rc<Self>,
        domain: Option<String>,
        kinds: WebsiteDataKinds,
    ) -> Result<Vec<String>> {
        let cookies = self.get_cookies("").await.unwrap_or_default();
        let hosts = |pred: &dyn Fn(&str) -> bool| -> BTreeSet<String> {
            cookies
                .iter()
                .map(|c| c.domain.trim_start_matches('.').to_ascii_lowercase())
                .filter(|h| !h.is_empty() && pred(h))
                .collect()
        };
        let Some(domain) = domain.map(|d| d.trim().trim_start_matches('.').to_ascii_lowercase())
        else {
            let names = hosts(&|_| true);
            self.clear_everything(kinds).await?;
            return Ok(names.into_iter().collect());
        };
        if domain.is_empty() {
            return Err(EngineError::InvalidArgument("empty domain".into()));
        }
        let mut names = hosts(&|h| site_matches(h, &domain));
        names.insert(domain.clone());
        if kinds.contains(WebsiteDataKinds::COOKIES) {
            let victims: Vec<Cookie> = cookies
                .iter()
                .filter(|c| site_matches(c.domain.trim_start_matches('.'), &domain))
                .cloned()
                .collect();
            if !victims.is_empty() {
                let mgr = self.cookie_manager().await?;
                for c in &victims {
                    request_delete(&mgr, c)?;
                }
                self.until_deleted("", &victims).await?;
            }
        }
        let view = self.any_view().await?;
        let types = convert::storage_types(kinds);
        if !types.is_empty() {
            for host in &names {
                for scheme in ["https", "http"] {
                    let origin = format!("{scheme}://{host}");
                    let params = serde_json::json!({ "origin": origin, "storageTypes": types });
                    if let Err(e) = cdp::call(&view, "Storage.clearDataForOrigin", params).await {
                        tracing::debug!("clear {origin}: {e}");
                    }
                }
            }
        }
        if kinds.contains(WebsiteDataKinds::MEMORY_CACHE)
            || kinds.contains(WebsiteDataKinds::DISK_CACHE)
        {
            // WebView2 has no per-site cache clearing: this empties the whole
            // HTTP cache of the profile (more than asked, never less).
            cdp::call(&view, "Network.clearBrowserCache", serde_json::json!({}))
                .await
                .map_err(|e| EngineError::WebsiteData(e.to_string()))?;
        }
        Ok(names.into_iter().collect())
    }

    async fn clear_everything(self: &Rc<Self>, kinds: WebsiteDataKinds) -> Result<()> {
        let bits = convert::browsing_data_kinds(kinds);
        if bits == 0 {
            return Ok(());
        }
        let core = self.core().await?;
        let p2: ICoreWebView2Profile2 = core.cast().map_err(|_| {
            EngineError::WebsiteData("this WebView2 Runtime cannot clear data".into())
        })?;
        let (tx, rx) = oneshot::<windows::core::Result<()>>();
        let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |hr| {
            tx.send(hr);
            Ok(())
        }));
        unsafe { p2.ClearBrowsingData(COREWEBVIEW2_BROWSING_DATA_KINDS(bits), &handler) }
            .map_err(|e| EngineError::WebsiteData(e.to_string()))?;
        rx.await
            .ok_or_else(|| EngineError::WebsiteData("no reply".into()))?
            .map_err(|e| EngineError::WebsiteData(e.to_string()))
    }

    // -- downloads -------------------------------------------------------

    /// Start a fetch-download of `url`, reported under the returned id.
    pub fn start_download(
        self: &Rc<Self>,
        url: &str,
        page: Option<Weak<PageInner>>,
    ) -> Result<DownloadId> {
        let parsed = url::Url::parse(url)
            .map_err(|e| EngineError::InvalidArgument(format!("download URL {url:?}: {e}")))?;
        if !matches!(parsed.scheme(), "http" | "https" | "data" | "blob" | "file") {
            return Err(EngineError::InvalidArgument(format!(
                "cannot download {} URLs",
                parsed.scheme()
            )));
        }
        let id = convert::new_download_id();
        self.downloads.jobs.borrow_mut().push_back(Job {
            id,
            url: url.to_owned(),
            page,
        });
        fetcher::pump(self);
        Ok(id)
    }
}

impl Drop for ProfileInner {
    fn drop(&mut self) {
        if let Spare::Ready(wv) = std::mem::replace(self.spare.get_mut(), Spare::Empty) {
            controller::close(&wv);
        }
        drop(self.fetcher.get_mut().reset());
    }
}

/// Ask WebView2 to delete the cookie matching `c`'s name, domain and path.
fn request_delete(mgr: &ICoreWebView2CookieManager, c: &Cookie) -> Result<()> {
    unsafe { mgr.DeleteCookiesWithDomainAndPath(&hs(&c.name), &hs(&c.domain), &hs(&c.path)) }
        .map_err(|e| EngineError::Cookies(e.to_string()))
}

/// The same cookie: name, domain (as given: `.example.com` is a different
/// cookie from host-only `example.com`) and path.
fn same_cookie(a: &Cookie, b: &Cookie) -> bool {
    a.name == b.name && a.path == b.path && a.domain.eq_ignore_ascii_case(&b.domain)
}

fn read_cookies(list: &ICoreWebView2CookieList) -> Vec<Cookie> {
    let n: u32 = get(|p| unsafe { list.Count(p) }).unwrap_or(0);
    (0..n)
        .filter_map(|i| unsafe { list.GetValueAtIndex(i) }.ok())
        .filter_map(|c| read_cookie(&c).ok())
        .collect()
}

fn read_cookie(c: &ICoreWebView2Cookie) -> windows::core::Result<Cookie> {
    unsafe {
        let session = get_bool(|p| c.IsSession(p))?;
        let expires: f64 = get(|p| c.Expires(p))?;
        let same_site: COREWEBVIEW2_COOKIE_SAME_SITE_KIND = get(|p| c.SameSite(p))?;
        Ok(Cookie {
            name: get_string(|p| c.Name(p))?,
            value: get_string(|p| c.Value(p))?,
            domain: get_string(|p| c.Domain(p))?,
            path: get_string(|p| c.Path(p))?,
            expires: convert::expires_from(expires, session),
            secure: get_bool(|p| c.IsSecure(p))?,
            http_only: get_bool(|p| c.IsHttpOnly(p))?,
            same_site: convert::same_site(same_site.0),
        })
    }
}

fn extension_id(ext: &ICoreWebView2BrowserExtension) -> String {
    get_string(|p| unsafe { ext.Id(p) }).unwrap_or_default()
}

async fn list_extensions(
    p7: &ICoreWebView2Profile7,
) -> Result<Vec<(ICoreWebView2BrowserExtension, String)>> {
    let (tx, rx) = oneshot::<windows::core::Result<Vec<(ICoreWebView2BrowserExtension, String)>>>();
    let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |hr, list| {
        tx.send(hr.map(|()| {
            let Some(list) = list else { return Vec::new() };
            let n: u32 = get(|p| unsafe { list.Count(p) }).unwrap_or(0);
            (0..n)
                .filter_map(|i| unsafe { list.GetValueAtIndex(i) }.ok())
                .map(|e| {
                    let name = get_string(|p| unsafe { e.Name(p) }).unwrap_or_default();
                    (e, name)
                })
                .collect()
        }));
        Ok(())
    }));
    unsafe { p7.GetBrowserExtensions(&handler) }.map_err(|e| other("list extensions", e))?;
    rx.await
        .ok_or_else(|| EngineError::ContentFilter("no reply".into()))?
        .map_err(|e| other("list extensions", e))
}

async fn add_extension(
    p7: &ICoreWebView2Profile7,
    dir: &Path,
) -> Result<ICoreWebView2BrowserExtension> {
    let (tx, rx) = oneshot::<windows::core::Result<ICoreWebView2BrowserExtension>>();
    let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |hr, ext| {
        tx.send(hr.and_then(|()| ext.ok_or_else(|| windows::Win32::Foundation::E_POINTER.into())));
        Ok(())
    }));
    unsafe { p7.AddBrowserExtension(&hs(&dir.to_string_lossy()), &handler) }
        .map_err(|e| other("add extension", e))?;
    rx.await
        .ok_or_else(|| EngineError::ContentFilter("no reply".into()))?
        .map_err(|e| EngineError::ContentFilter(format!("add extension: {e}")))
}

async fn remove_extension(ext: &ICoreWebView2BrowserExtension) {
    let (tx, rx) = oneshot::<windows::core::Result<()>>();
    let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |hr| {
        tx.send(hr);
        Ok(())
    }));
    if unsafe { ext.Remove(&handler) }.is_ok()
        && let Some(Err(e)) = rx.await
    {
        tracing::debug!("remove extension: {e}");
    }
}

impl Profile for WebView2Profile {
    fn id(&self) -> &ProfileId {
        &self.inner.id
    }

    fn is_ephemeral(&self) -> bool {
        self.inner.ephemeral
    }

    /// Copper's own folder for this profile. Web storage itself lives in
    /// the WebView2 user data folder, under the profile's WebView2 name.
    fn data_dir(&self) -> Option<&Path> {
        self.inner.data_dir.as_deref()
    }

    fn cookies(&self, url: &str) -> LocalFuture<Result<Vec<Cookie>>> {
        let p = self.inner.clone();
        let url = url.to_owned();
        Box::pin(async move { p.get_cookies(&url).await })
    }

    fn all_cookies(&self) -> LocalFuture<Result<Vec<Cookie>>> {
        let p = self.inner.clone();
        Box::pin(async move { p.get_cookies("").await })
    }

    fn set_cookie(&self, cookie: &Cookie) -> LocalFuture<Result<()>> {
        let p = self.inner.clone();
        let c = cookie.clone();
        Box::pin(async move { p.put_cookie(c).await })
    }

    fn delete_cookie(&self, cookie: &Cookie) -> LocalFuture<Result<()>> {
        let p = self.inner.clone();
        let c = cookie.clone();
        Box::pin(async move { p.drop_cookie(c).await })
    }

    fn clear_website_data(
        &self,
        domain: Option<&str>,
        kinds: WebsiteDataKinds,
    ) -> LocalFuture<Result<Vec<String>>> {
        let p = self.inner.clone();
        let domain = domain.map(str::to_owned);
        Box::pin(async move { p.clear_data(domain, kinds).await })
    }

    fn download(&self, url: &str) -> Result<DownloadId> {
        self.inner.start_download(url, None)
    }

    fn cancel_download(&self, id: DownloadId) -> bool {
        self.inner.downloads.cancel(id)
    }

    fn download_dir(&self) -> PathBuf {
        let dir = self.inner.download_dir();
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn set_download_dir(&self, dir: PathBuf) {
        *self.inner.download_dir.borrow_mut() = dir.clone();
        // A profile with no webview open gets it with its next one (`saw`
        // gives the first of each browser process the download folder).
        if let Some(core) = self.inner.live_profile() {
            unsafe {
                let _ = core.SetDefaultDownloadFolderPath(&hs(&dir.to_string_lossy()));
            }
        }
    }

    fn downloads(&self) -> EventStream<DownloadEvent> {
        self.inner.downloads.sink.subscribe()
    }
}
