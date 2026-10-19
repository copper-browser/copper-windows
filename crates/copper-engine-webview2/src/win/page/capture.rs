//! Snapshots, wake covers and find-in-page.
//!
//! A page Chromium is rendering (shown, or in the backstage stage) is
//! captured with `CapturePreview` — the composited frame, exactly what is on
//! screen. Anything else, and whole documents, go through CDP
//! `Page.captureScreenshot`, which renders on demand (bounded, since a page
//! that is hidden and throttled may never produce a frame).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use copper_engine::{
    Bitmap, EngineError, FindOptions, Image, LocalFuture, Result, Snapshot, SnapshotRegion,
};
use copper_win_rt::{OneshotTx, oneshot};
use serde_json::json;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{CapturePreviewCompletedHandler, FindStartCompletedHandler};
use windows::core::Interface;

use super::PageInner;
use crate::cdp as build;
use crate::hosting::{Cover, jpeg_size};
use crate::util::png_size;
use crate::win::cdp;
use crate::win::com::{Tokens, get, hs, memory_stream, read_stream, within};
use crate::win::controller::Wv;

/// A hidden page may take a while to produce a frame for CDP.
const SCREENSHOT_LIMIT: Duration = Duration::from_secs(10);
/// How long hiding waits for the cover frame.
const COVER_LIMIT: Duration = Duration::from_millis(150);
/// How long `find` waits for WebView2 to count matches.
const COUNT_LIMIT: Duration = Duration::from_millis(1000);
/// How long a cover for an already hidden page may take.
const COVER_NOW_LIMIT: Duration = Duration::from_secs(1);
/// JPEG quality of covers (they are shown for a moment, scaled).
const COVER_QUALITY: u8 = 70;

fn snap_err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Snapshot(e.to_string())
}

/// `CapturePreview` of the composited frame.
async fn capture_preview(
    wv: &Wv,
    format: COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT,
    limit: Duration,
) -> Result<Vec<u8>> {
    let stream = memory_stream().map_err(snap_err)?;
    let (tx, rx) = oneshot::<windows::core::Result<()>>();
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |hr| {
        tx.send(hr);
        Ok(())
    }));
    unsafe { wv.view.CapturePreview(format, &stream, &handler) }.map_err(snap_err)?;
    within(rx, limit)
        .await
        .ok_or_else(|| snap_err("the page produced no frame"))?
        .map_err(snap_err)?;
    read_stream(&stream).map_err(snap_err)
}

/// CDP `Page.captureScreenshot` (`full`: the whole document).
async fn screenshot(
    wv: &Wv,
    format: &str,
    quality: Option<u8>,
    full: bool,
    limit: Duration,
) -> Result<Vec<u8>> {
    let size = if full {
        let metrics = cdp::call(&wv.view, "Page.getLayoutMetrics", json!({}))
            .await
            .map_err(snap_err)?;
        Some(build::content_size(&metrics).ok_or_else(|| snap_err("the page has no size"))?)
    } else {
        None
    };
    let params = build::screenshot(format, quality, size);
    let reply = cdp::call_within(&wv.view, "Page.captureScreenshot", params, limit)
        .await
        .map_err(snap_err)?;
    build::screenshot_bytes(&reply)
}

pub(super) fn snapshot(
    page: &Rc<PageInner>,
    region: SnapshotRegion,
) -> LocalFuture<Result<Snapshot>> {
    let page = page.clone();
    Box::pin(async move {
        let wv = page.view().await?;
        let png = match region {
            SnapshotRegion::Visible if page.host.rendering() => {
                capture_preview(
                    &wv,
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    SCREENSHOT_LIMIT,
                )
                .await?
            }
            SnapshotRegion::Visible => {
                screenshot(&wv, "png", None, false, SCREENSHOT_LIMIT).await?
            }
            SnapshotRegion::FullDocument => {
                screenshot(&wv, "png", None, true, SCREENSHOT_LIMIT).await?
            }
        };
        let (width, height) = png_size(&png).ok_or_else(|| snap_err("not a PNG"))?;
        Ok(Image { png, width, height })
    })
}

/// `snapshot(Visible)`'s frame as raw pixels (`PageView::snapshot_bitmap`):
/// the PNG is made in the browser process and decoded on a worker thread.
pub(super) fn snapshot_bitmap(page: &Rc<PageInner>) -> LocalFuture<Result<Bitmap>> {
    let visible = snapshot(page, SnapshotRegion::Visible);
    Box::pin(async move {
        let image = visible.await?;
        crate::win::bitmap::decode_png(image.png).await
    })
}

/// The page is being hidden: keep its last frame as the cover, then hide
/// it (unless it was shown again meanwhile). Chromium keeps rendering for
/// at most [`COVER_LIMIT`] longer.
pub(super) fn hide_with_cover(page: &Rc<PageInner>, wv: &Wv) {
    let me = Rc::downgrade(page);
    let wv = wv.clone();
    let generation = page.generation.get();
    copper_win_rt::spawn_local(async move {
        let jpeg = capture_preview(
            &wv,
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG,
            COVER_LIMIT,
        )
        .await;
        let Some(p) = me.upgrade() else { return };
        if p.generation.get() != generation {
            return;
        }
        match jpeg {
            Ok(jpeg) => {
                if let Some((width, height)) = jpeg_size(&jpeg) {
                    *p.cover.borrow_mut() = Some(Cover {
                        jpeg,
                        width,
                        height,
                    });
                }
            }
            Err(e) => tracing::debug!("cover: {e}"),
        }
        if !p.host.rendering() {
            unsafe {
                let _ = wv.ctl.SetIsVisible(false);
            }
        }
    });
}

/// A JPEG of the page as it renders now, at [`COVER_QUALITY`] (CDP, so it
/// works for hidden pages as far as Chromium can draw them).
pub(super) async fn cover_now(wv: &Wv) -> Result<Cover> {
    let jpeg = screenshot(wv, "jpeg", Some(COVER_QUALITY), false, COVER_NOW_LIMIT).await?;
    let (width, height) = jpeg_size(&jpeg).ok_or_else(|| snap_err("not a JPEG"))?;
    Ok(Cover {
        jpeg,
        width,
        height,
    })
}

// -- find ------------------------------------------------------------------------

/// The page's find session (`ICoreWebView2Find`, runtime 1.0.2849+).
#[derive(Default)]
pub(crate) struct Find {
    find: RefCell<Option<ICoreWebView2Find>>,
    tokens: RefCell<Tokens>,
    counted: Rc<RefCell<Option<OneshotTx<()>>>>,
}

impl Find {
    pub fn reset(&self) {
        // Dropping the tokens removes handlers (a WebView2 call): not under
        // the borrow.
        let tokens = std::mem::take(&mut *self.tokens.borrow_mut());
        drop(tokens);
        // Taken before Stop(): it can raise find events synchronously.
        let find = self.find.borrow_mut().take();
        if let Some(f) = find {
            unsafe {
                let _ = f.Stop();
            }
        }
    }

    /// The find object of `wv`, its match-count listener registered once.
    fn object(&self, wv: &Wv) -> Result<ICoreWebView2Find> {
        if let Some(f) = self.find.borrow().clone() {
            return Ok(f);
        }
        let v28 = wv.view.cast::<ICoreWebView2_28>().map_err(|_| {
            EngineError::Other("find needs WebView2 Runtime 1.0.2849 or newer".into())
        })?;
        let f = unsafe { v28.Find() }.map_err(|e| EngineError::Other(format!("find: {e}")))?;
        let counted = self.counted.clone();
        let r = on!(
            self.tokens.borrow_mut(),
            f,
            FindMatchCountChanged,
            FindMatchCountChangedEventHandler,
            move |_, _| {
                if let Some(tx) = counted.borrow_mut().take() {
                    tx.send(());
                }
                Ok(())
            }
        );
        if let Err(e) = r {
            tracing::debug!("find: match count: {e}");
        }
        *self.find.borrow_mut() = Some(f.clone());
        Ok(f)
    }
}

/// `at_word_starts` uses WebView2's whole-word matching (the closest it
/// has); WebView2's find always wraps around.
pub(super) fn find(
    page: &Rc<PageInner>,
    query: String,
    opts: FindOptions,
) -> LocalFuture<Result<u32>> {
    let page = page.clone();
    Box::pin(async move {
        if query.is_empty() {
            find_finish(&page);
            return Ok(0);
        }
        let wv = page.view().await?;
        let env15 = page
            .profile
            .engine
            .env
            .get()
            .await?
            .cast::<ICoreWebView2Environment15>()
            .map_err(|_| {
                EngineError::Other("find needs WebView2 Runtime 1.0.2849 or newer".into())
            })?;
        let fail = |e: windows::core::Error| EngineError::Other(format!("find: {e}"));
        let options = unsafe { env15.CreateFindOptions() }.map_err(fail)?;
        unsafe {
            options.SetFindTerm(&hs(&query)).map_err(fail)?;
            options
                .SetIsCaseSensitive(opts.case_sensitive)
                .map_err(fail)?;
            options
                .SetShouldMatchWord(opts.at_word_starts)
                .map_err(fail)?;
            options.SetShouldHighlightAllMatches(true).map_err(fail)?;
            options.SetSuppressDefaultFindDialog(true).map_err(fail)?;
        }
        let f = page.find.object(&wv)?;
        // Ends the last session, so its count is not read as this one's.
        unsafe {
            let _ = f.Stop();
        }
        let (count_tx, count_rx) = oneshot::<()>();
        *page.find.counted.borrow_mut() = Some(count_tx);
        let (tx, rx) = oneshot::<windows::core::Result<()>>();
        let handler = FindStartCompletedHandler::create(Box::new(move |hr| {
            tx.send(hr);
            Ok(())
        }));
        unsafe { f.Start(&options, &handler) }.map_err(fail)?;
        within(rx, COUNT_LIMIT * 5)
            .await
            .ok_or_else(|| EngineError::Other("find did not start".into()))?
            .map_err(fail)?;
        let mut count: i32 = get(|c| unsafe { f.MatchCount(c) }).unwrap_or(-1);
        if count < 0 {
            let _ = within(count_rx, COUNT_LIMIT).await;
            count = get(|c| unsafe { f.MatchCount(c) }).unwrap_or(0);
        }
        if opts.backwards && count > 0 {
            unsafe {
                let _ = f.FindPrevious();
            }
        }
        Ok(u32::try_from(count.max(0))
            .unwrap_or(0)
            .min(opts.max_matches))
    })
}

// The find object is cloned out before each call: WebView2 can raise find
// events (which reach `Find`) synchronously from inside them.

pub(super) fn find_step(page: &PageInner, next: bool) {
    let find = page.find.find.borrow().clone();
    if let Some(f) = find {
        unsafe {
            let _ = if next { f.FindNext() } else { f.FindPrevious() };
        }
    }
}

pub(super) fn find_finish(page: &PageInner) {
    let find = page.find.find.borrow().clone();
    if let Some(f) = find {
        unsafe {
            let _ = f.Stop();
        }
    }
}
