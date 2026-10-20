//! Downloads: WebView2's `DownloadStarting` taken over (no download bubble),
//! saved into the profile's folder under a unique name — or where the
//! Save As dialog says, when the engine asks — and reported as
//! `DownloadEvent`s on the profile (and the page that started them).
//!
//! `Profile::download`/`PageView::download` have no WebView2 API: they load
//! the URL in the profile's hidden *fetcher* webview with CDP `Fetch`
//! pausing the document response and adding `Content-Disposition:
//! attachment`, so the browser downloads it (same cookies, same network
//! stack). Fetches run one at a time; the id is handed out at once.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant, SystemTime};

use copper_engine::{DownloadEvent, DownloadId, DownloadState, EventSink, PageEvent};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::core::{Interface, PWSTR};

use super::com::{Tokens, get, get_string, hs};
use super::page::PageInner;
use super::profile::ProfileInner;
use super::save_dialog::SaveSpec;
use crate::convert::{download_failure, fraction, new_download_id};
use crate::partials::Mark;
use crate::util::{sanitize_filename, unique_path, unnumbered};

/// Minimum spacing of `Progress` events per download.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// How often running downloads are looked at for one Chromium gave up on.
const STALL_EVERY: Duration = Duration::from_secs(2);
/// Looks in a row at an operation left "in progress" with an interrupt
/// reason and no restart before the download counts as failed.
const STALL_LOOKS: u8 = 3;

struct Live {
    op: ICoreWebView2DownloadOperation,
    url: Option<String>,
    path: PathBuf,
    page: Option<Weak<PageInner>>,
    last_progress: Option<Instant>,
    /// Looks in a row that found the operation interrupted (see [`STALL_LOOKS`]).
    stalled: u8,
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
    /// The stall watch runs (while any download does).
    watching: Cell<bool>,
}

impl Downloads {
    /// The path `pick` chooses (given the paths running downloads hold),
    /// held until its download ends.
    fn reserve(&self, pick: impl FnOnce(&HashSet<PathBuf>) -> PathBuf) -> PathBuf {
        let mut reserved = self.reserved.borrow_mut();
        let path = pick(&reserved);
        reserved.insert(path.clone());
        path
    }

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

/// What a starting download says about itself.
struct Starting {
    op: ICoreWebView2DownloadOperation,
    url: Option<String>,
    /// The file name the browser suggests.
    suggested: Option<String>,
    total: Option<u64>,
}

impl Starting {
    fn read(args: &ICoreWebView2DownloadStartingEventArgs) -> windows::core::Result<Self> {
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
        Ok(Starting {
            op,
            url,
            suggested,
            total,
        })
    }
}

/// Take over a starting download (no download bubble), report it under
/// `id` (or a new id) and follow it to the end. It is saved under a unique
/// name in the profile's download folder — or, when the engine asks
/// (`WebView2Engine::set_download_ask`), where the Save As dialog says,
/// with the download held (a deferral) until it closes; cancelled there,
/// it is cancelled before a byte is written and reports only
/// `Failed { cancelled: true }`. `decided` runs once the download has a
/// destination or is cancelled: at once, or when the dialog closes.
pub(crate) fn take_over(
    profile: &Rc<ProfileInner>,
    args: &ICoreWebView2DownloadStartingEventArgs,
    page: Option<Weak<PageInner>>,
    id: Option<DownloadId>,
    decided: impl FnOnce() + 'static,
) -> windows::core::Result<()> {
    let mut start = Starting::read(args)?;
    // Chromium resumes an interrupted download on its own, from scratch
    // when the server cannot resume (a fresh `200`), and announces it
    // again: it is the same download, not a new one.
    if id.is_none()
        && let Some(was) = profile.downloads.restarted(&start)
    {
        unsafe { args.SetHandled(true)? };
        let r = profile.downloads.rebind(args, start, was);
        decided();
        return r;
    }
    let id = id.unwrap_or_else(new_download_id);
    let dir = profile.download_dir();
    // Chromium's default name is numbered against this folder (`a (1).pdf`);
    // the engine names its own way, from the page's name.
    start.suggested = start.suggested.map(|name| {
        let reserved = profile.downloads.reserved.borrow();
        unnumbered(&name, |plain| {
            let p = dir.join(plain);
            reserved.contains(&p) || std::fs::symlink_metadata(&p).is_ok()
        })
    });
    unsafe { args.SetHandled(true)? };
    let Some(owner) = profile.engine.download_ask.get() else {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("download folder {}: {e}", dir.display());
        }
        let path = profile.downloads.reserve(|reserved| {
            unique_path(
                &dir,
                start.suggested.as_deref().unwrap_or("download"),
                reserved,
            )
        });
        let r = destined(&profile.downloads, args, start, id, path, page);
        decided();
        return r;
    };
    let deferral = unsafe { args.GetDeferral()? };
    let args = args.clone();
    let engine = profile.engine.clone();
    let downloads = profile.downloads.clone();
    // The browser writes the download into this folder meanwhile.
    let mark = Mark {
        since: SystemTime::now(),
        dir: dir.clone(),
    };
    let spec = SaveSpec {
        name: sanitize_filename(start.suggested.as_deref().unwrap_or("download")),
        folder: dir,
        owner,
        mark: Some((engine.asks_dir.clone(), mark)),
    };
    copper_win_rt::spawn_local(async move {
        match engine.save_queue.ask(spec).await {
            Some(chosen) => {
                let path = downloads.reserve(|_| chosen);
                let page_then = page.clone();
                if let Err(e) = destined(&downloads, &args, start, id, path.clone(), page) {
                    tracing::warn!("download {id:?} to {}: {e}", path.display());
                    downloads.reserved.borrow_mut().remove(&path);
                    unsafe {
                        let _ = args.SetCancel(true);
                    }
                    let state = DownloadState::Failed {
                        error: e.message(),
                        cancelled: false,
                    };
                    downloads.emit(id, state, page_then.as_ref());
                }
            }
            None => {
                unsafe {
                    let _ = args.SetCancel(true);
                }
                let state = DownloadState::Failed {
                    error: "cancelled".into(),
                    cancelled: true,
                };
                downloads.emit(id, state, page.as_ref());
            }
        }
        unsafe {
            let _ = deferral.Complete();
        }
        decided();
    });
    Ok(())
}

/// Send the download to `path` (reserved), report its start and follow it.
fn destined(
    downloads: &Rc<Downloads>,
    args: &ICoreWebView2DownloadStartingEventArgs,
    start: Starting,
    id: DownloadId,
    path: PathBuf,
    page: Option<Weak<PageInner>>,
) -> windows::core::Result<()> {
    let Starting {
        op,
        url,
        suggested,
        total,
    } = start;
    unsafe { args.SetResultFilePath(&hs(&path.to_string_lossy()))? };
    downloads.emit(
        id,
        DownloadState::Started { url: url.clone() },
        page.as_ref(),
    );
    downloads.emit(
        id,
        DownloadState::Destination {
            path: path.clone(),
            suggested_filename: suggested,
            total,
        },
        page.as_ref(),
    );
    let tokens = follow(downloads, &op, id)?;
    downloads.live.borrow_mut().insert(
        id,
        Live {
            op,
            url,
            path,
            page,
            last_progress: None,
            stalled: 0,
            _tokens: tokens,
        },
    );
    downloads.watch_stalls();
    Ok(())
}

/// Report `op`'s progress and end as download `id`.
fn follow(
    downloads: &Rc<Downloads>,
    op: &ICoreWebView2DownloadOperation,
    id: DownloadId,
) -> windows::core::Result<Tokens> {
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
    Ok(tokens)
}

impl Downloads {
    /// The running download `start` is Chromium restarting: the same
    /// operation, or one for the same URL whose own operation was
    /// interrupted (whether or not its state says so).
    fn restarted(&self, start: &Starting) -> Option<DownloadId> {
        let me: windows::core::IUnknown = start.op.cast().ok()?;
        let live = self.live.borrow();
        let mut found = None;
        for (id, l) in live.iter() {
            let same =
                l.op.cast::<windows::core::IUnknown>()
                    .is_ok_and(|u| u == me);
            let same_url = l.url.is_some() && l.url == start.url;
            if !same && !same_url {
                continue;
            }
            let state = get(|p| unsafe { l.op.State(p) })
                .unwrap_or(COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS);
            let reason = get(|p| unsafe { l.op.InterruptReason(p) })
                .unwrap_or(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE);
            tracing::debug!(
                ?id,
                same,
                state = state.0,
                reason = reason.0,
                "download starting again? {:?}",
                start.url
            );
            // Chromium resuming by itself leaves the old operation "in
            // progress" with an interrupt reason (seen: 24, content length
            // mismatch), and starts a new one for the same URL.
            if same
                || state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED
                || reason != COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE
            {
                found = Some(*id);
            }
        }
        found
    }

    /// Follow `start` as download `id` (restarted), into the same file.
    fn rebind(
        self: &Rc<Self>,
        args: &ICoreWebView2DownloadStartingEventArgs,
        start: Starting,
        id: DownloadId,
    ) -> windows::core::Result<()> {
        let Some(path) = self.live.borrow().get(&id).map(|l| l.path.clone()) else {
            return Ok(());
        };
        unsafe { args.SetResultFilePath(&hs(&path.to_string_lossy()))? };
        let tokens = follow(self, &start.op, id)?;
        // The old operation's handlers go with its entry.
        if let Some(l) = self.live.borrow_mut().get_mut(&id) {
            l.op = start.op;
            l.last_progress = None;
            l.stalled = 0;
            l._tokens = tokens;
        }
        tracing::debug!(
            ?id,
            "download restarted by the browser: same row, same file"
        );
        Ok(())
    }

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

    /// Chromium restarts a download cut off by the server (content length
    /// mismatch) a few times by itself, then gives up without a word: the
    /// last operation stays "in progress" with an interrupt reason and no
    /// state change ever comes. Such a download is failed here.
    fn watch_stalls(self: &Rc<Self>) {
        if self.watching.replace(true) {
            return;
        }
        let me = Rc::downgrade(self);
        copper_win_rt::spawn_local(async move {
            loop {
                copper_win_rt::sleep(STALL_EVERY).await;
                let Some(d) = me.upgrade() else { return };
                let mut gone = Vec::new();
                {
                    let mut live = d.live.borrow_mut();
                    if live.is_empty() {
                        d.watching.set(false);
                        return;
                    }
                    for (id, l) in live.iter_mut() {
                        let state = get(|p| unsafe { l.op.State(p) })
                            .unwrap_or(COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS);
                        let reason = get(|p| unsafe { l.op.InterruptReason(p) })
                            .unwrap_or(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE);
                        if state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS
                            && reason != COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE
                            && reason != COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_PAUSED
                        {
                            l.stalled += 1;
                            if l.stalled >= STALL_LOOKS {
                                gone.push((*id, reason));
                            }
                        } else {
                            l.stalled = 0;
                        }
                    }
                }
                for (id, reason) in gone {
                    let Some(l) = d.live.borrow_mut().remove(&id) else {
                        continue;
                    };
                    tracing::debug!(?id, reason = reason.0, "download given up by the browser");
                    d.reserved.borrow_mut().remove(&l.path);
                    let (error, cancelled) = download_failure(reason.0);
                    d.emit(
                        id,
                        DownloadState::Failed { error, cancelled },
                        l.page.as_ref(),
                    );
                }
            }
        });
    }

    fn state_changed(&self, id: DownloadId, op: &ICoreWebView2DownloadOperation) {
        let state =
            get(|p| unsafe { op.State(p) }).unwrap_or(COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS);
        let reason = get(|p| unsafe { op.InterruptReason(p) })
            .unwrap_or(COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE);
        tracing::debug!(?id, state = state.0, reason = reason.0, "download state");
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
