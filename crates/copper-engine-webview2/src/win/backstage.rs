//! `WebView2Backstage`: pages an agent works in while nobody is looking at
//! them. See `copper_engine::Backstage` for the contract.
//!
//! Held pages that are not shown live in one *stage*: an unowned,
//! off-screen, non-activating tool window with a DirectComposition target
//! (`window::stage_window`). Each staged page is visible to WebView2 at
//! stage size under its own child visual, so Chromium renders it at full
//! rate (the native occlusion calculation that would otherwise mark the
//! off-screen window covered is off; see `args`). Being unowned, the stage
//! is unaffected by shell windows minimizing or closing; the `Host` a page
//! is held for gives only its DPI scale, so the page renders at the scale
//! it will be shown at.
//!
//! The shell takes a held page by `hand_off` + `attach` in one turn; when
//! it `detach`es the page again the page goes back to the stage.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use copper_engine::{Backstage, BackstageMode, BackstageStatus, EngineError, Result};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionTarget,
    IDCompositionVisual, IDCompositionVisual2,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;
use windows::core::IUnknown;

use super::page::{PageInner, WebView2Page};
use super::window::{OwnedWindow, resize_stage, stage_window};

/// The stage size (logical px) until the shell sets one: a laptop's worth.
const DEFAULT_STAGE: (i32, i32) = (1280, 800);

/// The stage window and its composition tree.
struct Stage {
    window: OwnedWindow,
    device: IDCompositionDesktopDevice,
    _target: IDCompositionTarget,
    root: IDCompositionVisual2,
    /// Window size (physical px): the largest staged page so far.
    size: Cell<(i32, i32)>,
}

impl Stage {
    fn new(width: i32, height: i32) -> windows::core::Result<Self> {
        let window = stage_window(width, height)?;
        let device: IDCompositionDesktopDevice =
            unsafe { DCompositionCreateDevice2(None::<&IUnknown>)? };
        let target = unsafe { device.CreateTargetForHwnd(window.0, true)? };
        let root = unsafe { device.CreateVisual()? };
        unsafe {
            target.SetRoot(&root)?;
            device.Commit()?;
        }
        Ok(Stage {
            window,
            device,
            _target: target,
            root,
            size: Cell::new((width, height)),
        })
    }

    /// A new child visual for one page.
    fn slot(&self) -> windows::core::Result<IDCompositionVisual2> {
        unsafe {
            let v = self.device.CreateVisual()?;
            self.root
                .AddVisual(&v, false, None::<&IDCompositionVisual>)?;
            self.device.Commit()?;
            Ok(v)
        }
    }

    fn remove(&self, v: &IDCompositionVisual2) {
        unsafe {
            let _ = self.root.RemoveVisual(v);
            let _ = self.device.Commit();
        }
    }

    /// Grow the window to hold a page `w` x `h` (physical px).
    fn fit(&self, w: i32, h: i32) {
        let (cw, ch) = self.size.get();
        if w > cw || h > ch {
            let size = (w.max(cw), h.max(ch));
            self.size.set(size);
            resize_stage(&self.window, size.0, size.1);
        }
    }
}

struct Held {
    page: Weak<PageInner>,
    /// The shell window it is held for (its DPI scale is the page's).
    host: HWND,
    /// Its slot in the stage, while staged.
    slot: Option<IDCompositionVisual2>,
}

struct Inner {
    size: Cell<(i32, i32)>,
    stage: RefCell<Option<Stage>>,
    held: RefCell<Vec<Held>>,
    blocked: Cell<u64>,
    page_focus: Cell<bool>,
}

/// Keeps held pages rendering at full rate in an off-screen stage window.
/// Cheap to clone (shared handle). UI thread only.
#[derive(Clone)]
pub struct WebView2Backstage {
    inner: Rc<Inner>,
}

impl Default for WebView2Backstage {
    fn default() -> Self {
        Self::new()
    }
}

/// The DPI scale of `hwnd` (1.0 = 96 DPI).
fn scale_of(hwnd: HWND) -> f64 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
}

fn alive(hwnd: HWND) -> bool {
    unsafe { IsWindow(Some(hwnd)).as_bool() }
}

impl WebView2Backstage {
    pub(crate) fn new() -> Self {
        WebView2Backstage {
            inner: Rc::new(Inner {
                size: Cell::new(DEFAULT_STAGE),
                stage: RefCell::default(),
                held: RefCell::default(),
                blocked: Cell::new(0),
                page_focus: Cell::new(true),
            }),
        }
    }

    /// Lay staged pages out at `width` x `height` logical px (the size the
    /// shell's tab area has). Default 1280 x 800.
    pub fn set_stage_size(&self, width: i32, height: i32) {
        self.inner.size.set((width.max(1), height.max(1)));
        let staged: Vec<Rc<PageInner>> = self
            .inner
            .held
            .borrow()
            .iter()
            .filter(|h| h.slot.is_some())
            .filter_map(|h| h.page.upgrade())
            .collect();
        for p in staged {
            self.inner.stage_page(&p);
        }
    }

    pub fn stage_size(&self) -> (i32, i32) {
        self.inner.size.get()
    }

    /// Give staged pages page focus (`document.hasFocus()`, focus events,
    /// the caret) without taking keyboard focus from the shell, as the
    /// macOS backstage window does (CDP `Emulation.setFocusEmulationEnabled`).
    /// On by default.
    pub fn set_page_focus(&self, on: bool) {
        if self.inner.page_focus.replace(on) == on {
            return;
        }
        for p in self.inner.pages() {
            p.refresh_backstaged();
        }
    }

    pub(crate) fn page_focus(&self) -> bool {
        self.inner.page_focus.get()
    }

    /// Count one popup / dialog / prompt swallowed for a held page.
    pub(crate) fn blocked(&self) {
        self.inner.blocked.set(self.inner.blocked.get() + 1);
    }

    /// The shell attached a staged page to its own window: it is no longer
    /// in the stage (it stays held).
    pub(crate) fn lost_to_shell(&self, page: &Rc<PageInner>) {
        self.inner.unstage(page, false);
    }

    /// The shell detached a held page: back to the stage.
    pub(crate) fn take_back(&self, page: &Rc<PageInner>) {
        self.inner.stage_page(page);
    }
}

impl Inner {
    fn pages(&self) -> Vec<Rc<PageInner>> {
        self.held
            .borrow()
            .iter()
            .filter_map(|h| h.page.upgrade())
            .collect()
    }

    fn index(&self, page: &Rc<PageInner>) -> Option<usize> {
        self.held
            .borrow()
            .iter()
            .position(|h| h.page.as_ptr() == Rc::as_ptr(page))
    }

    /// Forget pages that are gone (their slots with them).
    fn prune(&self) {
        let stage = self.stage.borrow();
        self.held.borrow_mut().retain(|h| {
            let keep = h.page.strong_count() > 0;
            if !keep && let (Some(stage), Some(slot)) = (stage.as_ref(), &h.slot) {
                stage.remove(slot);
            }
            keep
        });
    }

    fn ensure_stage(&self, w: i32, h: i32) -> Result<()> {
        if self.stage.borrow().is_none() {
            let stage = Stage::new(w, h)
                .map_err(|e| EngineError::Other(format!("backstage stage: {e}")))?;
            *self.stage.borrow_mut() = Some(stage);
        }
        Ok(())
    }

    /// Put a held page in the stage (or re-lay it out there).
    fn stage_page(&self, page: &Rc<PageInner>) {
        if let Err(e) = self.try_stage(page) {
            tracing::warn!("backstage: {e}");
        }
    }

    fn try_stage(&self, page: &Rc<PageInner>) -> Result<()> {
        let Some(i) = self.index(page) else {
            return Ok(());
        };
        let host = self.held.borrow()[i].host;
        let scale = if alive(host) {
            scale_of(host)
        } else {
            page.host.scale()
        };
        let (lw, lh) = self.size.get();
        let (w, h) = (
            (f64::from(lw) * scale).round() as i32,
            (f64::from(lh) * scale).round() as i32,
        );
        self.ensure_stage(w, h)?;
        let stage = self.stage.borrow();
        let stage = stage.as_ref().expect("made above");
        stage.fit(w, h);
        let held = self.held.borrow()[i].slot.clone();
        let slot = match held {
            Some(s) => s,
            None => stage
                .slot()
                .map_err(|e| EngineError::Other(format!("backstage slot: {e}")))?,
        };
        self.held.borrow_mut()[i].slot = Some(slot.clone());
        let rect = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        page.attach_stage(stage.window.0, &slot, rect, scale);
        page.refresh_backstaged();
        Ok(())
    }

    /// Take a page out of the stage. `hide`: it becomes an ordinary hidden
    /// page (release); otherwise the shell is taking it.
    fn unstage(&self, page: &Rc<PageInner>, hide: bool) {
        let Some(i) = self.index(page) else { return };
        let slot = self.held.borrow_mut()[i].slot.take();
        if let Some(slot) = slot
            && let Some(stage) = self.stage.borrow().as_ref()
        {
            stage.remove(&slot);
        }
        if hide {
            page.detach_stage();
        }
        page.refresh_backstaged();
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        for h in self.held.get_mut().drain(..) {
            if let Some(p) = h.page.upgrade() {
                p.held.set(false);
                p.detach_stage();
            }
        }
    }
}

impl Backstage for WebView2Backstage {
    type Page = WebView2Page;
    /// The shell's top-level window.
    type Host = HWND;

    fn hold(&self, page: &WebView2Page, host: &HWND) -> Result<()> {
        let inner = &self.inner;
        inner.prune();
        let p = &page.inner;
        if p.host.in_shell() && !p.shown() {
            return Err(EngineError::InvalidArgument(
                "the page is attached to a shell window that hides it; detach it first".into(),
            ));
        }
        match inner.index(p) {
            Some(i) => inner.held.borrow_mut()[i].host = *host,
            None => inner.held.borrow_mut().push(Held {
                page: Rc::downgrade(p),
                host: *host,
                slot: None,
            }),
        }
        p.held.set(true);
        if p.shown() {
            // The shell shows it: left where it is.
            p.refresh_backstaged();
            return Ok(());
        }
        inner.try_stage(p)
    }

    fn release(&self, page: &WebView2Page) {
        let p = &page.inner;
        if self.inner.index(p).is_none() {
            return;
        }
        self.inner.unstage(p, true);
        self.inner
            .held
            .borrow_mut()
            .retain(|h| h.page.as_ptr() != Rc::as_ptr(p));
        p.held.set(false);
        p.refresh_backstaged();
    }

    fn holds(&self, page: &WebView2Page) -> bool {
        self.inner.index(&page.inner).is_some()
    }

    /// Takes the page out of the stage *hidden*; the shell's `attach` and
    /// `set_visible(true)` (same turn) show it.
    fn hand_off(&self, page: &WebView2Page) {
        let p = &page.inner;
        if p.host.in_stage() {
            self.inner.unstage(p, true);
        }
    }

    #[allow(clippy::field_reassign_with_default)] // `BackstageStatus` is non-exhaustive
    fn status(&self) -> BackstageStatus {
        let inner = &self.inner;
        inner.prune();
        let (mut hosted, mut shown, mut ok) = (0, 0, true);
        let mut hosts: Vec<HWND> = Vec::new();
        for h in inner.held.borrow().iter() {
            let Some(p) = h.page.upgrade() else { continue };
            if p.host.in_stage() {
                hosted += 1;
            } else if p.shown() {
                shown += 1;
            } else {
                ok = false;
            }
            if alive(h.host) && !hosts.contains(&h.host) {
                hosts.push(h.host);
            }
        }
        let held = inner.held.borrow().len();
        let (w, h) = inner.size.get();
        let mut status = BackstageStatus::default();
        status.mode = if held == 0 {
            BackstageMode::Idle
        } else if ok {
            BackstageMode::InWindow
        } else {
            BackstageMode::Degraded
        };
        status.held = held;
        status.hosted = hosted;
        status.shown = shown;
        status.stage = (w as u32, h as u32);
        status.hosts = hosts.len();
        status.popups_blocked = inner.blocked.get();
        status
    }
}

impl std::fmt::Debug for WebView2Backstage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebView2Backstage")
            .field("held", &self.inner.held.borrow().len())
            .field("stage", &self.inner.size.get())
            .finish()
    }
}
