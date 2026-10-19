//! Downloads: WebView2's `DownloadStarting` taken over (no download bubble),
//! saved into the profile's folder under a unique name, and reported as
//! `DownloadEvent`s on the profile (and the page that started them).
//!
//! `Profile::download`/`PageView::download` have no WebView2 API: they load
//! the URL in the profile's hidden *fetcher* webview with CDP `Fetch`
//! pausing the document response and adding `Content-Disposition:
//! attachment`, so the browser downloads it (same cookies, same network
//! stack). Fetches run one at a time; the id is handed out at once.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use copper_engine::{DownloadEvent, DownloadId, DownloadState, EventSink, PageEvent};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::core::PWSTR;

use super::com::{Tokens, get, get_string, hs};
use super::page::PageInner;
use crate::convert::{download_failure, fraction, new_download_id};
use crate::util::unique_path;

/// Minimum spacing of `Progress` events per download.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

struct Live {
    op: ICoreWebView2DownloadOperation,
    path: PathBuf,
    page: Option<Weak<PageInner>>,
    last_progress: Option<Instant>,
    _tokens: Tokens,
}

/// A fetch waiting for (or running in) the fetcher webview.
pub(crate) struct Job {
    pub id: DownloadId,
    pub url: String,
    pub page: Option<Weak<PageInner>>,
}

/// The downloads of one profile.
#[derive(Default)]
pub(crate) struct Downloads {
    pub sink: EventSink<DownloadEvent>,
    live: RefCell<HashMap<DownloadId, Live>>,
    reserved: RefCell<HashSet<PathBuf>>,
    pub jobs: RefCell<VecDeque<Job>>,
    /// The fetch whose download has not started yet.
    pub current: RefCell<Option<Job>>,
}

impl Downloads {
    pub fn emit(&self, id: DownloadId, state: DownloadState, page: Option<&Weak<PageInner>>) {
        let ev = DownloadEvent { id, state };
        if let Some(p) = page.and_then(Weak::upgrade) {
            p.emit(PageEvent::Download(ev.clone()));
        }
        self.sink.emit(ev);
    }

    /// Cancel a running download; `false` if unknown or over. A fetch that
    /// has not started is dropped (and reported cancelled).
    pub fn cancel(&self, id: DownloadId) -> bool {
        let op = self.live.borrow().get(&id).map(|l| l.op.clone());
        if let Some(op) = op {
            return unsafe { op.Cancel() }.is_ok();
        }
        let queued = {
            let mut jobs = self.jobs.borrow_mut();
            jobs.iter()
                .position(|j| j.id == id)
                .and_then(|i| jobs.remove(i))
        };
        match queued {
            Some(job) => {
                let state = DownloadState::Failed {
                    error: "cancelled".into(),
                    cancelled: true,
                };
                self.emit(id, state, job.page.as_ref());
                true
            }
            None => false,
        }
    }
}

/// Take over a starting download: save it to `dir`, report it under `id`
/// (or a new id), and follow it to the end.
pub(crate) fn take_over(
    downloads: &Rc<Downloads>,
    args: &ICoreWebView2DownloadStartingEventArgs,
    dir: &Path,
    page: Option<Weak<PageInner>>,
    id: Option<DownloadId>,
) -> windows::core::Result<()> {
    let op = unsafe { args.DownloadOperation()? };
    let url = get_string(|p| unsafe { op.Uri(p) })
        .ok()
        .filter(|u| !u.is_empty());
    let suggested_path = get_string(|p| unsafe { args.ResultFilePath(p) }).unwrap_or_default();
    let suggested = Path::new(&suggested_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty());
    let total = get(|p| unsafe { op.TotalBytesToReceive(p) })
        .ok()
        .and_then(|t: i64| u64::try_from(t).ok());
    let id = id.unwrap_or_else(new_download_id);
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!("download folder {}: {e}", dir.display());
    }
    let path = {
        let mut reserved = downloads.reserved.borrow_mut();
        let path = unique_path(dir, suggested.as_deref().unwrap_or("download"), &reserved);
        reserved.insert(path.clone());
        path
    };
    unsafe {
        args.SetResultFilePath(&hs(&path.to_string_lossy()))?;
        args.SetHandled(true)?;
    }
    downloads.emit(id, DownloadState::Started { url }, page.as_ref());
    downloads.emit(
        id,
        DownloadState::Destination {
            path: path.clone(),
            suggested_filename: suggested,
            total,
        },
        page.as_ref(),
    );
    let mut tokens = Tokens::default();
    let weak = Rc::downgrade(downloads);
    on!(
        tokens,
        op,
        BytesReceivedChanged,
        BytesReceivedChangedEventHandler,
        move |op: Option<ICoreWebView2DownloadOperation>, _| {
            if let (Some(d), Some(op)) = (weak.upgrade(), op) {
                d.progress(id, &op);
            }
            Ok(())
        }
    )?;
    let weak = Rc::downgrade(downloads);
    on!(
        tokens,
        op,
        StateChanged,
        StateChangedEventHandler,
        move |op: Option<ICoreWebView2DownloadOperation>, _| {
            if let (Some(d), Some(op)) = (weak.upgrade(), op) {
                d.state_changed(id, &op);
            }
            Ok(())
        }
    )?;
    downloads.live.borrow_mut().insert(
        id,
        Live {
            op,
            path,
            page,
            last_progress: None,
            _tokens: tokens,
        },
    );
    Ok(())
}

impl Downloads {
    fn progress(&self, id: DownloadId, op: &ICoreWebView2DownloadOperation) {
        let page = {
            let mut live = self.live.borrow_mut();
            let Some(l) = live.get_mut(&id) else { return };
            if l.last_progress
                .is_some_and(|t| t.elapsed() < PROGRESS_EVERY)
            {
                return;
            }
            l.last_progress = Some(Instant::now());
            l.page.clone()
        };
        let received = get(|p| unsafe { op.BytesReceived(p) })
            .unwrap_or(0i64)
            .max(0) as u64;
        let total = get(|p| unsafe { op.TotalBytesToReceive(p) })
            .ok()
            .and_then(|t: i64| u64::try_from(t).ok());
        let state = DownloadState::Progress {
            received,
            total,
            fraction: fraction(received, total),
        };
        self.emit(id, state, page.as_ref());
    }

    fn state_changed(&self, id: DownloadId, op: &ICoreWebView2DownloadOperation) {
        let state =
            get(|p| unsafe { op.State(p) }).unwrap_or(COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS);
        let reason = get(|p| unsafe { op.InterruptReason(p) })
            .unwrap_or(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE);
        let terminal = state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED
            || (state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED
                && reason != COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED);
        if !terminal {
            return;
        }
        let Some(live) = self.live.borrow_mut().remove(&id) else {
            return;
        };
        self.reserved.borrow_mut().remove(&live.path);
        let path = get_string(|p: *mut PWSTR| unsafe { op.ResultFilePath(p) })
            .ok()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or(live.path);
        let ev = if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
            let bytes = get(|p| unsafe { op.BytesReceived(p) })
                .unwrap_or(0i64)
                .max(0) as u64;
            DownloadState::Finished { path, bytes }
        } else {
            let (error, cancelled) = download_failure(reason.0);
            DownloadState::Failed { error, cancelled }
        };
        self.emit(id, ev, live.page.as_ref());
    }
}
