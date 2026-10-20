//! The one door between this lane and the shared addon core
//! (`copper_core::addons`, `copper_cloud::addons`; CONTRACT §8) — the
//! Windows twin of Linux's `addons/core_glue.rs`. Every core call the
//! Windows side makes is here.
//!
//! * [`start`] makes the app's `AddonSync` over the cloud client, starts it
//!   on the cloud's tokio runtime, and follows its status watch and change
//!   stream on the UI thread (→ [`super::changed`]).
//! * [`active`], [`list`], [`pending`], [`status`]: what the UI draws.
//! * [`set_enabled`], [`remove`], [`check_now`], [`install_pending`]: what
//!   the person does. File work runs on the cloud runtime, never on the UI
//!   thread.
//! * [`dispatch`]: one bridge message, run by the core on the cloud runtime
//!   with this lane's [`Host`]; the reply's JS comes back on the UI thread.
//!
//! No cloud client (an unreadable `cloud.json`) still runs a developer
//! folder (`COPPER_ADDONS_DEV_DIR`), from the store alone.

use super::host::Host;
use super::oauth::Outcome;
use crate::platform::{Shell, WinLoop};
use copper_cloud::addons::{AddonSync, HttpFetcher, SyncStatus};
use copper_core::addons::bridge::{
    AgentCall, BridgeError, Fetcher, Identity, PageInfo, agent_unavailable_state,
};
use copper_core::addons::{
    ActiveAddon, AddonState, AddonStore, BRIDGE_JS, BridgeHost, Capability, Context, Platform,
    SlotId, handle_message,
};
use copper_shell_core::cloud;
use serde_json::Value;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// The slot this lane shows.
pub const SLOT: SlotId = SlotId::AgentSidebar;

/// The addon the slot shows, ready to serve and run.
#[derive(Debug, Clone, PartialEq)]
pub struct Active {
    pub id: String,
    pub name: String,
    pub version: String,
    pub sha256: String,
    /// Resolved settings (`copper.settings()`).
    pub settings: Value,
    /// The unpacked bundle (or the developer folder).
    pub dir: PathBuf,
    /// From `COPPER_ADDONS_DEV_DIR`.
    pub dev: bool,
    pub(super) core: ActiveAddon,
}

impl Active {
    fn of(a: ActiveAddon) -> Active {
        Active {
            id: a.installed.id.clone(),
            name: a.installed.display_name().to_string(),
            version: a.installed.version.clone(),
            sha256: a.installed.sha256.clone(),
            settings: a.installed.settings.clone(),
            dir: a.dir.clone(),
            dev: a.dev,
            core: a,
        }
    }
}

/// One addon for Settings › Addons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub version: String,
    /// "Agent sidebar".
    pub slot_name: String,
    /// The cloud it came from (`host:port`).
    pub source: String,
    /// Not turned off here.
    pub enabled: bool,
    /// The staged version waiting for Install.
    pub pending: Option<String>,
    pub installed: bool,
    pub dev: bool,
}

/// A staged update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: String,
    pub name: String,
    pub version: String,
    pub slot_name: String,
}

/// Where the cloud check stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// The cloud lane runs (a readable `cloud.json`).
    pub cloud: bool,
    /// Linked and signed in.
    pub linked: bool,
    pub checking: bool,
    pub error: Option<String>,
    /// "3 minutes ago".
    pub checked: Option<String>,
}

struct Glue {
    sync: Option<AddonSync>,
    store: AddonStore,
    /// Without a cloud client: the state as loaded (the developer folder).
    local: RefCell<AddonState>,
    fetcher: Arc<dyn Fetcher>,
}

thread_local! {
    static GLUE: RefCell<Option<Rc<Glue>>> = const { RefCell::new(None) };
}

fn glue() -> Option<Rc<Glue>> {
    GLUE.with(|g| g.borrow().clone())
}

/// This run is the addons self-test (which stages its own fixture and must
/// not have an unlinked check take it away again).
pub fn selftest_run() -> bool {
    std::env::var("COPPER_SELFTEST").is_ok_and(|v| v == "addons")
}

/// Make and start the app's addon sync; `on_change` runs on the UI thread
/// whenever its status or a slot changes.
pub fn start(shell: &Shell, on_change: fn(&Shell)) {
    if glue().is_some() {
        return;
    }
    let rt = cloud::runtime();
    let fetcher: Arc<dyn Fetcher> = Arc::new(HttpFetcher::with_runtime(Some(rt.handle().clone())));
    let sync = cloud::get().map(|c| {
        let _rt = rt.enter();
        AddonSync::new(c.client.clone())
    });
    let store = match &sync {
        Some(s) => s.store().clone(),
        None => AddonStore::new(shell.store().paths()),
    };
    let local = if sync.is_some() {
        AddonState::default()
    } else {
        store.load()
    };
    GLUE.with(|g| {
        *g.borrow_mut() = Some(Rc::new(Glue {
            sync: sync.clone(),
            store,
            local: RefCell::new(local),
            fetcher,
        }))
    });
    let Some(sync) = sync else {
        tracing::info!("no Copper Cloud client: addons from a developer folder only");
        return;
    };
    if selftest_run() {
        tracing::info!("addons self-test: the cloud check is not started");
    } else {
        let _rt = rt.enter();
        sync.start();
    }
    let mut status = sync.watch();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        while status.changed().await.is_ok() {
            let Some(s) = weak.upgrade() else { break };
            on_change(&s);
        }
    });
    let mut changes = sync.subscribe();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match changes.recv().await {
                Ok(c) => tracing::info!(change = ?c, "addon change"),
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
            let Some(s) = weak.upgrade() else { break };
            on_change(&s);
        }
    });
}

fn sync_status() -> SyncStatus {
    glue()
        .and_then(|g| g.sync.as_ref().map(AddonSync::status))
        .unwrap_or_default()
}

/// What the Agent sidebar shows now (`None`: Copper's placeholder).
pub fn active() -> Option<Active> {
    let g = glue()?;
    let a = match &g.sync {
        Some(s) => s.active(SLOT),
        None => g.store.active(&g.local.borrow(), SLOT),
    }?;
    Some(Active::of(a))
}

/// The installed, pending and developer addons, one per slot.
pub fn list() -> Vec<Listed> {
    let st = sync_status();
    let source = st.cloud.clone().unwrap_or_default();
    let mut out: Vec<Listed> = Vec::new();
    if let Some(d) = active().filter(|a| a.dev) {
        out.push(Listed {
            id: d.id.clone(),
            name: d.name.clone(),
            version: d.version.clone(),
            slot_name: SLOT.name().to_string(),
            source: String::new(),
            enabled: true,
            pending: None,
            installed: false,
            dev: true,
        });
    }
    for i in &st.installed {
        out.push(Listed {
            id: i.id.clone(),
            name: i.name.clone(),
            version: i.version.clone(),
            slot_name: i.slot.name().to_string(),
            source: source.clone(),
            enabled: i.enabled,
            pending: None,
            installed: true,
            dev: false,
        });
    }
    for p in &st.pending {
        match out.iter_mut().find(|l| l.id == p.id && !l.dev) {
            Some(l) => l.pending = Some(p.version.clone()),
            None => out.push(Listed {
                id: p.id.clone(),
                name: p.name.clone(),
                version: p.version.clone(),
                slot_name: p.slot.name().to_string(),
                source: source.clone(),
                enabled: p.enabled,
                pending: Some(p.version.clone()),
                installed: false,
                dev: false,
            }),
        }
    }
    out
}

/// The staged update for the Agent sidebar, if any.
pub fn pending() -> Option<Pending> {
    sync_status()
        .pending
        .into_iter()
        .find(|p| p.slot == SLOT)
        .map(|p| Pending {
            id: p.id,
            name: p.name,
            version: p.version,
            slot_name: p.slot.name().to_string(),
        })
}

/// Linked, checking, the last error and when it last asked.
pub fn status() -> Status {
    let st = sync_status();
    Status {
        cloud: glue().is_some_and(|g| g.sync.is_some()),
        linked: st.linked,
        checking: st.checking,
        error: st.last_error,
        checked: st.checked_at.map(|d| cloud::ago(d.unix() as f64)),
    }
}

/// When the sync last finished asking the cloud (the cloud self-test
/// tells its own Check now from an earlier one).
pub fn checked_at() -> Option<i64> {
    sync_status().checked_at.map(|d| d.unix())
}

/// The developer folder in use, if any.
pub fn dev_dir() -> Option<PathBuf> {
    active().filter(|a| a.dev).map(|a| a.dir)
}

/// "Use in Agent sidebar" for addon `id`.
pub fn set_enabled(id: &str, on: bool) {
    let Some(sync) = glue().and_then(|g| g.sync.clone()) else {
        return;
    };
    let id = id.to_string();
    cloud::run_then::<WinLoop, _, _>(async move { sync.set_enabled(&id, on) }, |()| {});
}

/// "Remove": addon `id` leaves this Copper until the cloud changes.
pub fn remove(id: &str) {
    let Some(sync) = glue().and_then(|g| g.sync.clone()) else {
        return;
    };
    let id = id.to_string();
    cloud::run_then::<WinLoop, _, _>(async move { sync.remove(&id) }, |()| {});
}

/// "Check for addon updates" (and every Copper update check).
pub fn check_now() {
    if selftest_run() {
        return;
    }
    if let Some(sync) = glue().and_then(|g| g.sync.clone()) {
        sync.kick();
    }
}

/// Install the staged update; `done` gets the "<name> <version>" of what
/// now runs.
pub fn install_pending(done: impl FnOnce(Result<Vec<String>, String>) + 'static) {
    let Some(sync) = glue().and_then(|g| g.sync.clone()) else {
        done(Err("no Copper Cloud".into()));
        return;
    };
    let names: Vec<(String, String)> = sync_status()
        .pending
        .iter()
        .map(|p| (p.id.clone(), format!("{} {}", p.name, p.version)))
        .collect();
    cloud::run_then::<WinLoop, _, _>(
        async move {
            sync.install_pending(Some(&[SLOT]))
                .map_err(|e| e.to_string())
        },
        move |result| {
            done(result.map(|changes| {
                changes
                    .iter()
                    .filter_map(|c| match c {
                        copper_core::addons::Change::Installed { id, .. } => {
                            names.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone())
                        }
                        _ => None,
                    })
                    .collect()
            }))
        },
    );
}

/// `window.__copperAddon.emit(…)` for an event, safe to evaluate.
pub fn emit_js(name: &str, payload: &Value) -> String {
    copper_core::addons::emit_js(name, payload)
}

/// The bridge shim, injected before any addon script.
pub fn bridge_js() -> &'static str {
    BRIDGE_JS
}

/// The addon asked for capability `cap`.
pub fn granted(active: &Active, cap: Capability) -> bool {
    active.core.manifest.permissions.grants(cap)
}

/// One message from the addon page: the core dispatches it on the cloud
/// runtime with `host`; `done` gets the reply's JS on the UI thread
/// (nothing for a message that can't be answered).
pub fn dispatch(
    active: &Active,
    text: String,
    dark: bool,
    host: Host,
    done: impl FnOnce(String) + 'static,
) {
    let Some(g) = glue() else { return };
    let mut ctx = Context::new(
        &active.core,
        Platform::Windows,
        env!("CARGO_PKG_VERSION"),
        g.store.clone(),
    );
    ctx.theme_dark = dark;
    ctx.fetcher = Some(g.fetcher.clone());
    let host = CoreHost(host);
    cloud::run_then::<WinLoop, _, _>(
        async move { handle_message(&ctx, &host, &text).await.map(|r| r.to_js()) },
        move |js| {
            if let Some(js) = js {
                done(js);
            }
        },
    );
}

/// The core's `BridgeHost`, over this lane's [`Host`].
struct CoreHost(Host);

impl BridgeHost for CoreHost {
    fn identity(&self) -> impl std::future::Future<Output = Option<Identity>> {
        let id = cloud::get().and_then(|c| copper_cloud::addons::identity(&c.client));
        async move { id }
    }

    async fn open_url(&self, url: Url) -> Result<(), BridgeError> {
        if self.0.open_url(url.to_string()).await {
            Ok(())
        } else {
            Err(BridgeError::internal("the window has closed"))
        }
    }

    async fn page_current(&self) -> Option<PageInfo> {
        let v = self.0.page_current().await;
        Some(PageInfo {
            url: v.get("url")?.as_str()?.to_string(),
            title: v
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
    }

    async fn close_slot(&self) {
        self.0.close_slot().await;
    }

    async fn oauth_authorize(
        &self,
        url: Url,
        redirect_prefix: String,
        timeout: Duration,
    ) -> Result<String, BridgeError> {
        match self
            .0
            .oauth_authorize(url.to_string(), redirect_prefix, timeout)
            .await
        {
            Outcome::Redirect(u) => Ok(u),
            Outcome::Cancelled => Err(BridgeError::cancelled("the sign-in tab was closed")),
            Outcome::Timeout => Err(BridgeError::timeout("the sign-in took too long")),
            Outcome::Failed(e) => Err(BridgeError::internal(e)),
        }
    }

    async fn agent_state(&self) -> Value {
        agent_unavailable_state()
    }

    async fn agent_call(&self, _call: AgentCall) -> Result<Value, BridgeError> {
        Err(BridgeError::unavailable(
            "Copper for Windows has no native agent yet",
        ))
    }
}

/// The self-test's stand-in for the cloud: pack `files` (a bundle's
/// files, `manifest.json` among them) in the served format, then verify
/// and stage it exactly as a cloud answer would be (so Settings › Updates
/// shows it and Install installs it).
pub fn selftest_stage(refs: &[(&str, &[u8])]) -> Result<(), String> {
    use copper_core::addons::testkit;
    let sync = glue()
        .and_then(|g| g.sync.clone())
        .ok_or("no addon sync (no cloud client)")?;
    let manifest: Value = serde_json::from_slice(
        refs.iter()
            .find(|(n, _)| *n == "manifest.json")
            .map(|(_, b)| *b)
            .ok_or("no manifest.json")?,
    )
    .map_err(|e| e.to_string())?;
    let bytes = testkit::served_bundle(&manifest, refs);
    let resolved = testkit::resolved_for(&manifest, &bytes);
    let set = copper_core::addons::ResolvedSet::parse(&testkit::resolved_body(
        "r_selftest",
        std::slice::from_ref(&resolved),
    ))
    .map_err(|e| e.to_string())?;
    let outcome = cloud::runtime().block_on(sync.apply_with(set, move |addon| {
        let bytes = bytes.clone();
        async move {
            copper_core::addons::verify_bundle(&bytes, &addon.sha256, &addon)
                .map(|b| (bytes, b))
                .map_err(|e| copper_cloud::addons::bundle_failure(&e))
        }
    }));
    match outcome {
        copper_cloud::addons::CheckOutcome::Applied { staged: 1 } => Ok(()),
        other => Err(format!("{other:?}")),
    }
}
