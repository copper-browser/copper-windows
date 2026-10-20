//! `copper-addon://<id>/<path>` (CONTRACT §5.4; the Mac's `AddonFiles`;
//! Linux `addons/scheme.rs`): files only from the bundle of the addon in
//! the slot, and only to the slot's own page. What a path may name, the
//! type it is served as and the CSP `<meta>` every HTML file gets are the
//! core's (`copper_core::addons::scheme::serve`).
//!
//! WebView2 asks on the UI thread and wants the answer there and then
//! (`Engine::register_scheme`), so a bundle is read ahead, off the UI
//! thread, as soon as its page is made ([`preload`]); a file not read yet
//! (a developer folder that changed) is read when it is asked for. The
//! built-in agent's files are in the binary ([`super::builtin::serve`]).

use super::glue::Active;
use super::policy;
use crate::platform::Shell;
use copper_core::addons::scheme::{SCHEME, SchemeResponse};
use copper_engine::{Engine, SchemeBody, SchemeRequest};
use std::cell::RefCell;
use std::collections::HashMap;

/// The most files read ahead, and the largest one.
const PRELOAD_FILES: usize = 256;
const PRELOAD_BYTES: u64 = 4 * 1024 * 1024;

/// One read-ahead answer.
#[derive(Clone)]
struct Served {
    status: u16,
    mime: &'static str,
    body: Vec<u8>,
}

impl From<SchemeResponse> for Served {
    fn from(r: SchemeResponse) -> Served {
        Served {
            status: r.status,
            mime: r.mime,
            body: r.body,
        }
    }
}

thread_local! {
    /// The read-ahead bundle: (sha256 or the developer folder, URI) → answer.
    static CACHE: RefCell<(String, HashMap<String, Served>)> = RefCell::new((String::new(), HashMap::new()));
}

/// Register the scheme with the engine (before any addon page exists).
pub fn install(shell: &Shell) {
    match shell.engine().register_scheme(SCHEME, Box::new(answer)) {
        Ok(()) => tracing::debug!("copper-addon: registered"),
        Err(e) => tracing::error!(error = %e, "copper-addon: scheme not registered"),
    }
}

/// The bundle changed: what was read ahead no longer counts.
pub fn forget() {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.0.clear();
        c.1.clear();
    });
}

fn key_of(a: &Active) -> String {
    if a.dev {
        format!("dev:{}", a.dir.display())
    } else {
        a.sha256.clone()
    }
}

fn answer(req: &SchemeRequest) -> copper_engine::SchemeResponse {
    let Some(view) = super::view::live() else {
        return Err("no addon in the Agent sidebar".into());
    };
    if req.page != Some(copper_engine::PageView::page_id(view.page())) {
        tracing::warn!("copper-addon: refused a request from another page");
        return Err("not the addon's page".into());
    }
    let active = view.addon();
    if super::builtin::is(&active) {
        // Copper's own agent: compiled in, nothing to read.
        let served = super::builtin::serve(&req.uri);
        tracing::debug!(uri = %req.uri, status = served.status, "copper-addon: built-in");
        if served.status != 200 {
            return Err(format!("{} Not found", served.status));
        }
        return Ok(SchemeBody {
            data: served.body,
            mime: policy::bare_mime(served.mime).to_string(),
        });
    }
    let key = key_of(&active);
    let cached = CACHE.with(|c| {
        let c = c.borrow();
        (c.0 == key && !active.dev)
            .then(|| c.1.get(&req.uri).cloned())
            .flatten()
    });
    let cached_hit = cached.is_some();
    let served =
        cached.unwrap_or_else(|| copper_core::addons::scheme::serve(&active.core, &req.uri).into());
    tracing::debug!(
        uri = %req.uri,
        status = served.status,
        cached = cached_hit,
        "copper-addon: request"
    );
    if served.status != 200 {
        tracing::debug!(status = served.status, "copper-addon: not served");
        return Err(format!("{} Not found", served.status));
    }
    Ok(SchemeBody {
        data: served.body,
        mime: policy::bare_mime(served.mime).to_string(),
    })
}

/// Read `active`'s bundle ahead, off the UI thread.
pub fn preload(active: &Active) {
    if super::builtin::is(active) {
        // Compiled in: served straight from the binary.
        return;
    }
    if active.dev {
        // A developer folder changes under us: always read it fresh.
        return;
    }
    let key = key_of(active);
    if CACHE.with(|c| c.borrow().0 == key) {
        return;
    }
    let core = active.core.clone();
    let id = active.id.clone();
    let dir = active.dir.clone();
    copper_win_rt::spawn_local(async move {
        let started = std::time::Instant::now();
        let read = copper_shell_core::main_loop::spawn_blocking(move || {
            let mut out = HashMap::new();
            let mut stack = vec![(dir.clone(), String::new())];
            while let Some((d, prefix)) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&d) else {
                    continue;
                };
                for e in entries.flatten() {
                    if out.len() >= PRELOAD_FILES {
                        return out;
                    }
                    let Ok(meta) = e.metadata() else { continue };
                    let name = e.file_name().to_string_lossy().to_string();
                    let rel = if prefix.is_empty() {
                        name
                    } else {
                        format!("{prefix}/{name}")
                    };
                    if meta.is_dir() {
                        stack.push((e.path(), rel));
                    } else if meta.is_file() && meta.len() <= PRELOAD_BYTES {
                        let uri = copper_core::addons::scheme::addon_url(&id, &rel);
                        let r = copper_core::addons::scheme::serve(&core, &uri);
                        if r.status == 200 {
                            out.insert(uri, Served::from(r));
                        }
                    }
                }
            }
            // The root is the entry.
            let root = copper_core::addons::scheme::root_url(&id);
            let r = copper_core::addons::scheme::serve(&core, &root);
            if r.status == 200 {
                out.insert(root, Served::from(r));
            }
            out
        })
        .await;
        let Some(files) = read else { return };
        tracing::debug!(
            files = files.len(),
            ms = started.elapsed().as_millis() as u64,
            "copper-addon: bundle read ahead"
        );
        CACHE.with(|c| *c.borrow_mut() = (key, files));
    });
}
