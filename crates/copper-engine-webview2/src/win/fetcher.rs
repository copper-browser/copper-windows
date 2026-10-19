//! The profile's hidden fetcher webview, for downloads no page started
//! (`Profile::download`, `PageView::download`). See [`super::downloads`].

use std::rc::{Rc, Weak};
use std::time::Duration;

use copper_engine::DownloadState;
use serde_json::json;
use webview2_com::Microsoft::Web::WebView2::Win32::*;

use super::cdp;
use super::com::{Tokens, get, get_bool, hs};
use super::controller::{self, Wv};
use super::downloads::{Job, take_over};
use super::profile::ProfileInner;
use crate::convert::web_error_name;

/// After the fetcher's navigation ends without a download, how long to wait
/// for a late `DownloadStarting` before reporting the fetch failed.
const GRACE: Duration = Duration::from_millis(500);
/// An idle fetcher is closed after this long.
const IDLE_CLOSE: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct Fetcher {
    wv: Option<Wv>,
    tokens: Tokens,
    creating: bool,
    /// Bumped on every job start and reset (stale timers do nothing).
    generation: u64,
}

impl Fetcher {
    pub fn reset(&mut self) {
        self.tokens.clear();
        if let Some(wv) = self.wv.take() {
            controller::close(&wv);
        }
        self.creating = false;
        self.generation += 1;
    }
}

/// Start the next fetch if none is running.
pub(crate) fn pump(profile: &Rc<ProfileInner>) {
    if profile.downloads.current.borrow().is_some() {
        return;
    }
    let Some(job) = profile.downloads.jobs.borrow_mut().pop_front() else {
        schedule_idle_close(profile);
        return;
    };
    let url = job.url.clone();
    *profile.downloads.current.borrow_mut() = Some(job);
    let wv = {
        let mut f = profile.fetcher.borrow_mut();
        f.generation += 1;
        match &f.wv {
            Some(wv) => Some(wv.clone()),
            None if f.creating => return,
            None => {
                f.creating = true;
                None
            }
        }
    };
    match wv {
        Some(wv) => navigate(profile, &wv, &url),
        None => {
            let me = Rc::downgrade(profile);
            copper_win_rt::spawn_local(async move {
                let Some(p) = me.upgrade() else { return };
                let made = p.create().await;
                let made = match made {
                    Ok(wv) => wire(&p, &wv).await.map(|t| (wv, t)),
                    Err(e) => Err(e),
                };
                let url = p.downloads.current.borrow().as_ref().map(|j| j.url.clone());
                match made {
                    Ok((wv, tokens)) => {
                        {
                            let mut f = p.fetcher.borrow_mut();
                            f.creating = false;
                            f.wv = Some(wv.clone());
                            f.tokens = tokens;
                        }
                        if let Some(url) = url {
                            navigate(&p, &wv, &url);
                        }
                    }
                    Err(e) => {
                        p.fetcher.borrow_mut().creating = false;
                        let mut failed: Vec<Job> = p
                            .downloads
                            .current
                            .borrow_mut()
                            .take()
                            .into_iter()
                            .collect();
                        failed.extend(p.downloads.jobs.borrow_mut().drain(..));
                        for job in failed {
                            let state = DownloadState::Failed {
                                error: e.to_string(),
                                cancelled: false,
                            };
                            p.downloads.emit(job.id, state, job.page.as_ref());
                        }
                    }
                }
            });
        }
    }
}

fn navigate(profile: &Rc<ProfileInner>, wv: &Wv, url: &str) {
    if let Err(e) = unsafe { wv.view.Navigate(&hs(url)) } {
        finish_current(profile, Some(e.to_string()));
    }
}

/// End the current fetch: failed with `error`, or (`None`) handed to a
/// download. Then start the next.
fn finish_current(profile: &Rc<ProfileInner>, error: Option<String>) {
    let job = profile.downloads.current.borrow_mut().take();
    if let (Some(job), Some(error)) = (job, error) {
        let state = DownloadState::Failed {
            error,
            cancelled: false,
        };
        profile.downloads.emit(job.id, state, job.page.as_ref());
    }
    pump(profile);
}

async fn wire(profile: &Rc<ProfileInner>, wv: &Wv) -> copper_engine::Result<Tokens> {
    let mut tokens = Tokens::default();
    let fail = |e: windows::core::Error| copper_engine::EngineError::Download(e.to_string());
    let me = Rc::downgrade(profile);
    let view4: ICoreWebView2_4 = windows::core::Interface::cast(&wv.view).map_err(fail)?;
    on!(
        tokens,
        view4,
        DownloadStarting,
        DownloadStartingEventHandler,
        move |_, args: Option<ICoreWebView2DownloadStartingEventArgs>| {
            if let (Some(p), Some(args)) = (me.upgrade(), args) {
                let job = p.downloads.current.borrow_mut().take();
                let (id, page) = match job {
                    Some(j) => (Some(j.id), j.page),
                    None => (None, None),
                };
                take_over(&p.downloads, &args, &p.download_dir(), page, id)?;
                // The next fetch navigates this webview: not while the
                // navigation that became this download is still settling.
                copper_win_rt::spawn_local(async move { pump(&p) });
            }
            Ok(())
        }
    )
    .map_err(fail)?;
    let me = Rc::downgrade(profile);
    on!(
        tokens,
        wv.view,
        NavigationCompleted,
        NavigationCompletedEventHandler,
        move |_, args: Option<ICoreWebView2NavigationCompletedEventArgs>| {
            let (Some(p), Some(args)) = (me.upgrade(), args) else {
                return Ok(());
            };
            let ok = get_bool(|b| unsafe { args.IsSuccess(b) }).unwrap_or(false);
            let error = if ok {
                let status = windows::core::Interface::cast::<
                    ICoreWebView2NavigationCompletedEventArgs2,
                >(&args)
                .ok()
                .and_then(|a| get(|s| unsafe { a.HttpStatusCode(s) }).ok())
                .unwrap_or(200i32);
                format!("not a download (HTTP {status})")
            } else {
                let status = get(|s| unsafe { args.WebErrorStatus(s) })
                    .unwrap_or(COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN);
                web_error_name(status.0).to_owned()
            };
            let generation = p.fetcher.borrow().generation;
            let me: Weak<ProfileInner> = Rc::downgrade(&p);
            copper_win_rt::spawn_local(async move {
                copper_win_rt::sleep(GRACE).await;
                if let Some(p) = me.upgrade()
                    && p.fetcher.borrow().generation == generation
                    && p.downloads.current.borrow().is_some()
                {
                    finish_current(&p, Some(error));
                }
            });
            Ok(())
        }
    )
    .map_err(fail)?;
    let view = wv.view.clone();
    cdp::subscribe(
        &wv.view,
        "Fetch.requestPaused",
        &mut tokens,
        move |params| {
            if let Some((method, reply)) = crate::cdp::force_download(&params) {
                let view = view.clone();
                copper_win_rt::spawn_local(async move {
                    if let Err(e) = cdp::call(&view, method, reply).await {
                        tracing::debug!("{method}: {e}");
                    }
                });
            }
        },
    )
    .map_err(fail)?;
    let patterns = json!({ "patterns": [
        { "urlPattern": "*", "resourceType": "Document", "requestStage": "Response" }
    ]});
    cdp::call(&wv.view, "Fetch.enable", patterns).await?;
    Ok(tokens)
}

fn schedule_idle_close(profile: &Rc<ProfileInner>) {
    let generation = {
        let f = profile.fetcher.borrow();
        if f.wv.is_none() {
            return;
        }
        f.generation
    };
    let me = Rc::downgrade(profile);
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(IDLE_CLOSE).await;
        if let Some(p) = me.upgrade() {
            let mut f = p.fetcher.borrow_mut();
            if f.generation == generation && p.downloads.current.borrow().is_none() {
                f.reset();
            }
        }
    });
}
