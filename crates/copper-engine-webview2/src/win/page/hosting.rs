//! Visual hosting (CONTRACT §2): the page's DirectComposition visual in a
//! shell window, raw mouse/pointer input the toolkit hit-tested into the
//! page's slot, focus, cursor, UI Automation, and keys offered to the shell
//! first.
//!
//! Hosting is state, not a queue: the target, bounds, visibility and scale
//! are remembered and applied to every webview the page gets (the first,
//! and any made after sleep or a browser crash). Input is dropped while the
//! page has no webview.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::DirectComposition::IDCompositionVisual;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Input::Pointer::{
    GetPointerDeviceRects, GetPointerPenInfo, GetPointerTouchInfo, POINTER_INFO, POINTER_PEN_INFO,
    POINTER_TOUCH_INFO,
};
use windows::Win32::UI::WindowsAndMessaging::{HCURSOR, IDC_ARROW, LoadCursorW, PT_PEN, PT_TOUCH};
use windows::core::{IUnknown, Interface};

use super::{PageInner, WebView2Page, capture};
use crate::hosting::{
    AcceleratorKey, FocusExit, FocusReason, KeyEventKind, MouseEventKind, PageChange,
    PointerEventKind, VirtualKeys,
};
use crate::win::controller::Wv;

/// The DComp visual a page draws into, and the window it lives in.
#[derive(Clone)]
struct Target {
    hwnd: HWND,
    visual: IDCompositionVisual,
    /// The backstage's stage, not a shell window.
    stage: bool,
}

type AccelHandler = Rc<dyn Fn(&AcceleratorKey) -> bool>;
type FocusExitHandler = Rc<dyn Fn(FocusExit)>;
type CursorHandler = Rc<dyn Fn(HCURSOR)>;

#[derive(Default)]
pub(crate) struct Host {
    target: RefCell<Option<Target>>,
    bounds: Cell<Option<RECT>>,
    visible: Cell<bool>,
    scale: Cell<Option<f64>>,
    cursor: Cell<Option<HCURSOR>>,
    cursor_handlers: RefCell<Vec<CursorHandler>>,
    accelerator: RefCell<Option<AccelHandler>>,
    focus_exit: RefCell<Option<FocusExitHandler>>,
}

impl Host {
    /// Attached to a shell window and visible.
    pub fn shown(&self) -> bool {
        self.visible.get() && self.target.borrow().as_ref().is_some_and(|t| !t.stage)
    }

    /// Attached (shell or stage) and visible: Chromium renders it.
    pub fn rendering(&self) -> bool {
        self.visible.get() && self.target.borrow().is_some()
    }

    pub fn in_stage(&self) -> bool {
        self.target.borrow().as_ref().is_some_and(|t| t.stage)
    }

    pub fn in_shell(&self) -> bool {
        self.target.borrow().as_ref().is_some_and(|t| !t.stage)
    }

    pub fn scale(&self) -> f64 {
        self.scale.get().unwrap_or(1.0)
    }

    /// Give a (new) webview the remembered hosting.
    pub fn apply(&self, page: &PageInner, wv: &Wv) {
        let target = self.target.borrow().clone();
        unsafe {
            match &target {
                Some(t) => {
                    let _ = wv.ctl.SetParentWindow(t.hwnd);
                    let _ = wv.comp.SetRootVisualTarget(&t.visual);
                }
                None => {
                    let _ = wv.comp.SetRootVisualTarget(None::<&IUnknown>);
                    let _ = wv.ctl.SetParentWindow(page.profile.engine.park.0);
                }
            }
            if let Some(r) = self.bounds.get() {
                let _ = wv.ctl.SetBounds(r);
            }
            if let (Some(s), Ok(c3)) = (self.scale.get(), wv.ctl.cast::<ICoreWebView2Controller3>())
            {
                let _ = c3.SetRasterizationScale(s);
            }
            let _ = wv.ctl.SetIsVisible(self.rendering());
        }
    }

    pub fn cursor_changed(&self, cursor: HCURSOR) {
        self.cursor.set(Some(cursor));
        let handlers: Vec<CursorHandler> = self.cursor_handlers.borrow().clone();
        for h in handlers {
            h(cursor);
        }
    }

    /// Offer a key to the shell; `true` if it took it.
    pub fn accelerator(&self, key: &AcceleratorKey) -> bool {
        let h = self.accelerator.borrow().clone();
        h.is_some_and(|h| h(key))
    }

    /// Tab left the page; `true` if the shell moves focus on.
    pub fn focus_exit(&self, exit: FocusExit) -> bool {
        let h = self.focus_exit.borrow().clone();
        match h {
            Some(h) => {
                h(exit);
                true
            }
            None => false,
        }
    }
}

impl PageInner {
    /// Point the page at `target` (or nowhere) and apply it.
    fn set_target(self: &Rc<Self>, target: Option<Target>) {
        *self.host.target.borrow_mut() = target;
        if let Some(wv) = self.wv() {
            self.host.apply(self, &wv);
        } else if self.host.target.borrow().is_some() {
            self.wake();
        }
    }

    /// Visibility changed: showing applies at once; hiding first keeps the
    /// last frame as the page's cover (see `capture::hide_with_cover`).
    fn set_shown(self: &Rc<Self>, visible: bool) {
        let was = self.host.rendering();
        self.host.visible.set(visible);
        let now = self.host.rendering();
        let Some(wv) = self.wv() else { return };
        if now {
            // Showing resumes a suspended page.
            let suspended = wv
                .view
                .cast::<ICoreWebView2_3>()
                .ok()
                .and_then(|v3| crate::win::com::get_bool(|b| unsafe { v3.IsSuspended(b) }).ok())
                .unwrap_or(false);
            life_visible(&wv);
            unsafe {
                let _ = wv.ctl.SetIsVisible(true);
            }
            if suspended {
                self.change(PageChange::Suspended(false));
            }
        } else if was {
            capture::hide_with_cover(self, &wv);
        }
    }

    /// Host the page in the backstage's stage window.
    pub(crate) fn attach_stage(
        self: &Rc<Self>,
        hwnd: HWND,
        visual: &IDCompositionVisual,
        rect: RECT,
        scale: f64,
    ) {
        self.host.bounds.set(Some(rect));
        self.host.scale.set(Some(scale));
        self.host.visible.set(true);
        self.set_target(Some(Target {
            hwnd,
            visual: visual.clone(),
            stage: true,
        }));
    }

    /// Take the page out of the stage (it becomes an ordinary hidden page).
    pub(crate) fn detach_stage(self: &Rc<Self>) {
        if self.host.in_stage() {
            self.host.visible.set(false);
            self.set_target(None);
        }
    }
}

/// A shown page runs at its normal memory level again.
fn life_visible(wv: &Wv) {
    if let Ok(v19) = wv.view.cast::<ICoreWebView2_19>() {
        unsafe {
            let _ = v19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
        }
    }
}

/// When a Ctrl/Shift/middle click went in last; a popup right after it is
/// a "new tab" request.
pub(super) const MODIFIED_CLICK_WINDOW: Duration = Duration::from_millis(1000);

impl WebView2Page {
    /// Host this page's visual under `visual` (the toolkit's slot visual) in
    /// top-level window `hwnd`. Makes the webview if it does not exist yet
    /// (or wakes a slept page). Idempotent. A page the backstage holds is
    /// taken out of its stage.
    pub fn attach(&self, hwnd: HWND, visual: &IDCompositionVisual) {
        let p = &self.inner;
        if p.host.in_stage() {
            p.profile.engine.backstage.lost_to_shell(p);
        }
        p.set_target(Some(Target {
            hwnd,
            visual: visual.clone(),
            stage: false,
        }));
        p.refresh_backstaged();
    }

    /// Take the page out of the shell's window. A page the backstage holds
    /// goes back to its stage.
    pub fn detach(&self) {
        let p = &self.inner;
        if !p.host.in_shell() {
            return;
        }
        if p.host.rendering()
            && let Some(wv) = p.wv()
        {
            capture::hide_with_cover(p, &wv);
        }
        *p.host.target.borrow_mut() = None;
        if let Some(wv) = p.wv() {
            unsafe {
                let _ = wv.comp.SetRootVisualTarget(None::<&IUnknown>);
                let _ = wv.ctl.SetParentWindow(p.profile.engine.park.0);
            }
        }
        if p.held.get() {
            p.profile.engine.backstage.take_back(p);
        }
        p.refresh_backstaged();
    }

    /// Bounds in physical pixels, relative to the window's client area.
    pub fn set_bounds(&self, rect: RECT) {
        let p = &self.inner;
        if p.host.in_stage() {
            return; // the backstage sizes staged pages
        }
        p.host.bounds.set(Some(rect));
        if let Some(wv) = p.wv() {
            unsafe {
                let _ = wv.ctl.SetBounds(rect);
            }
        }
    }

    /// Show or hide the page (an attached page starts hidden). Hidden pages
    /// are throttled by Chromium and may be suspended.
    pub fn set_visible(&self, visible: bool) {
        if self.inner.host.in_stage() {
            return;
        }
        self.inner.set_shown(visible);
        self.inner.refresh_backstaged();
    }

    /// The monitor scale of the page's slot (1.0 = 96 DPI).
    pub fn set_rasterization_scale(&self, scale: f64) {
        let p = &self.inner;
        if !scale.is_finite() || scale <= 0.0 || p.host.in_stage() {
            return;
        }
        p.host.scale.set(Some(scale));
        if let Some(wv) = p.wv()
            && let Ok(c3) = wv.ctl.cast::<ICoreWebView2Controller3>()
        {
            unsafe {
                let _ = c3.SetRasterizationScale(scale);
            }
        }
    }

    /// Tell WebView2 the top-level window moved (so its popups, `<select>`
    /// menus and IME windows follow).
    pub fn notify_parent_window_moved(&self) {
        if let Some(wv) = self.inner.wv() {
            unsafe {
                let _ = wv.ctl.NotifyParentWindowPositionChanged();
            }
        }
    }

    /// Forward a mouse message the toolkit hit-tested into the slot. `point`
    /// is slot-local, physical pixels; `mouse_data` the wheel delta or X
    /// button.
    pub fn send_mouse_input(
        &self,
        kind: MouseEventKind,
        keys: VirtualKeys,
        mouse_data: u32,
        point: POINT,
    ) {
        let p = &self.inner;
        let modified = matches!(kind, MouseEventKind::MiddleButtonDown)
            || (matches!(kind, MouseEventKind::LeftButtonDown)
                && (keys.contains(VirtualKeys::CONTROL) || keys.contains(VirtualKeys::SHIFT)));
        if modified {
            p.modified_click.set(Some(Instant::now()));
        }
        if let Some(wv) = p.wv() {
            unsafe {
                let _ = wv.comp.SendMouseInput(
                    COREWEBVIEW2_MOUSE_EVENT_KIND(kind as i32),
                    COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS(keys.0),
                    mouse_data,
                    point,
                );
            }
        }
    }

    /// Forward a pointer (touch or pen) message. `info` is the message's
    /// `GetPointerInfo`, in screen coordinates; it is moved into the slot's
    /// space here, with the pen or touch details read for its pointer id.
    pub fn send_pointer_input(&self, kind: PointerEventKind, info: &POINTER_INFO) {
        let p = &self.inner;
        let Some(wv) = p.wv() else { return };
        let Some(env3) = p
            .profile
            .engine
            .env
            .now()
            .and_then(|e| e.cast::<ICoreWebView2Environment3>().ok())
        else {
            return;
        };
        let Ok(pi) = (unsafe { env3.CreateCoreWebView2PointerInfo() }) else {
            return;
        };
        let origin = self.slot_origin();
        if let Err(e) = unsafe { fill_pointer_info(&pi, info, origin, p.host.scale()) } {
            tracing::debug!("pointer info: {e}");
            return;
        }
        unsafe {
            let _ = wv
                .comp
                .SendPointerInput(COREWEBVIEW2_POINTER_EVENT_KIND(kind as i32), &pi);
        }
    }

    /// The slot's top-left in screen coordinates.
    fn slot_origin(&self) -> POINT {
        let host = &self.inner.host;
        let mut pt = POINT::default();
        if let Some(t) = host.target.borrow().as_ref() {
            let mut origin = POINT::default();
            unsafe {
                let _ = ScreenToClient(t.hwnd, &mut origin);
            }
            // ScreenToClient of (0,0) is minus the client origin.
            pt = POINT {
                x: -origin.x,
                y: -origin.y,
            };
        }
        if let Some(r) = host.bounds.get() {
            pt.x += r.left;
            pt.y += r.top;
        }
        pt
    }

    /// Give the page keyboard focus (`Next`/`Previous`: its first/last
    /// focusable element, as when tabbing in).
    pub fn move_focus(&self, reason: FocusReason) {
        self.inner.with_view(move |_, wv| unsafe {
            let _ = wv
                .ctl
                .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON(reason as i32));
        });
    }

    /// The cursor the page wants over its slot (the toolkit sets it).
    pub fn cursor(&self) -> HCURSOR {
        if let Some(c) = self.inner.host.cursor.get() {
            return c;
        }
        let from_view = self.inner.wv().and_then(|wv| {
            let mut c = HCURSOR::default();
            unsafe { wv.comp.Cursor(&mut c) }.ok().map(|()| c)
        });
        from_view
            .filter(|c| !c.is_invalid())
            .or_else(|| unsafe { LoadCursorW(None, IDC_ARROW) }.ok())
            .unwrap_or_default()
    }

    /// Called with the new cursor whenever the page changes it.
    pub fn on_cursor_changed(&self, f: impl Fn(HCURSOR) + 'static) {
        self.inner
            .host
            .cursor_handlers
            .borrow_mut()
            .push(Rc::new(f));
    }

    /// The page's UI Automation provider (`IRawElementProviderSimple`), for
    /// the toolkit's slot to return from `WM_GETOBJECT`.
    pub fn automation_provider(&self) -> Option<IUnknown> {
        let wv = self.inner.wv()?;
        let c2 = wv.comp.cast::<ICoreWebView2CompositionController2>().ok()?;
        unsafe { c2.AutomationProvider() }.ok()
    }

    /// Keys pressed while the page has focus are offered to `f` first;
    /// returning `true` takes the key (the page never sees it). This is how
    /// Ctrl+L / Ctrl+T / Ctrl+K / Escape reach the shell.
    pub fn on_accelerator_key(&self, f: impl Fn(&AcceleratorKey) -> bool + 'static) {
        *self.inner.host.accelerator.borrow_mut() = Some(Rc::new(f));
    }

    /// Called when Tab (or Shift+Tab) moves focus out of the page; the
    /// shell focuses its next (previous) control.
    pub fn on_focus_exit(&self, f: impl Fn(FocusExit) + 'static) {
        *self.inner.host.focus_exit.borrow_mut() = Some(Rc::new(f));
    }
}

/// Copy a Win32 pointer message into WebView2's pointer info, in the
/// webview's own coordinates.
///
/// # Safety
/// `info` must come from `GetPointerInfo` for the message being handled
/// (pen and touch details are read for its pointer id).
unsafe fn fill_pointer_info(
    pi: &ICoreWebView2PointerInfo,
    info: &POINTER_INFO,
    origin: POINT,
    scale: f64,
) -> windows::core::Result<()> {
    let local = |p: POINT| POINT {
        x: p.x - origin.x,
        y: p.y - origin.y,
    };
    let himetric_per_px = 2540.0 / (96.0 * scale.max(0.1));
    let himetric = |p: POINT| POINT {
        x: (f64::from(p.x) * himetric_per_px).round() as i32,
        y: (f64::from(p.y) * himetric_per_px).round() as i32,
    };
    let at = local(info.ptPixelLocation);
    let raw = local(info.ptPixelLocationRaw);
    unsafe {
        pi.SetPointerKind(info.pointerType.0 as u32)?;
        pi.SetPointerId(info.pointerId)?;
        pi.SetFrameId(info.frameId)?;
        pi.SetPointerFlags(info.pointerFlags.0)?;
        pi.SetPixelLocation(at)?;
        pi.SetHimetricLocation(himetric(at))?;
        pi.SetPixelLocationRaw(raw)?;
        pi.SetHimetricLocationRaw(himetric(raw))?;
        pi.SetTime(info.dwTime)?;
        pi.SetHistoryCount(info.historyCount)?;
        pi.SetInputData(info.InputData)?;
        pi.SetKeyStates(info.dwKeyStates)?;
        pi.SetPerformanceCount(info.PerformanceCount)?;
        pi.SetButtonChangeKind(info.ButtonChangeType.0)?;
        let mut device = RECT::default();
        let mut display = RECT::default();
        if GetPointerDeviceRects(info.sourceDevice, &mut device, &mut display).is_ok() {
            pi.SetPointerDeviceRect(device)?;
            pi.SetDisplayRect(display)?;
        }
        if info.pointerType == PT_PEN {
            let mut pen = POINTER_PEN_INFO::default();
            if GetPointerPenInfo(info.pointerId, &mut pen).is_ok() {
                pi.SetPenFlags(pen.penFlags)?;
                pi.SetPenMask(pen.penMask)?;
                pi.SetPenPressure(pen.pressure)?;
                pi.SetPenRotation(pen.rotation)?;
                pi.SetPenTiltX(pen.tiltX)?;
                pi.SetPenTiltY(pen.tiltY)?;
            }
        } else if info.pointerType == PT_TOUCH {
            let mut touch = POINTER_TOUCH_INFO::default();
            if GetPointerTouchInfo(info.pointerId, &mut touch).is_ok() {
                let rect = |r: RECT| RECT {
                    left: r.left - origin.x,
                    top: r.top - origin.y,
                    right: r.right - origin.x,
                    bottom: r.bottom - origin.y,
                };
                pi.SetTouchFlags(touch.touchFlags)?;
                pi.SetTouchMask(touch.touchMask)?;
                pi.SetTouchContact(rect(touch.rcContact))?;
                pi.SetTouchContactRaw(rect(touch.rcContactRaw))?;
                pi.SetTouchOrientation(touch.orientation)?;
                pi.SetTouchPressure(touch.pressure)?;
            }
        }
    }
    Ok(())
}

/// `AcceleratorKeyPressed` arguments as the shell sees them.
pub(super) fn accelerator_key(
    args: &ICoreWebView2AcceleratorKeyPressedEventArgs,
) -> windows::core::Result<AcceleratorKey> {
    use crate::win::com::get;
    let kind: COREWEBVIEW2_KEY_EVENT_KIND = get(|k| unsafe { args.KeyEventKind(k) })?;
    let vk: u32 = get(|v| unsafe { args.VirtualKey(v) })?;
    let lparam: i32 = get(|l| unsafe { args.KeyEventLParam(l) })?;
    let st: COREWEBVIEW2_PHYSICAL_KEY_STATUS = get(|s| unsafe { args.PhysicalKeyStatus(s) })?;
    let down = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| unsafe { GetKeyState(i32::from(vk.0)) } < 0;
    Ok(AcceleratorKey {
        kind: match kind {
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP => KeyEventKind::KeyUp,
            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN => KeyEventKind::SystemKeyDown,
            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP => KeyEventKind::SystemKeyUp,
            _ => KeyEventKind::KeyDown,
        },
        virtual_key: vk,
        lparam,
        repeat_count: st.RepeatCount,
        scan_code: st.ScanCode,
        is_extended_key: st.IsExtendedKey.as_bool(),
        is_menu_key_down: st.IsMenuKeyDown.as_bool(),
        was_key_down: st.WasKeyDown.as_bool(),
        is_key_released: st.IsKeyReleased.as_bool(),
        ctrl: down(VK_CONTROL),
        shift: down(VK_SHIFT),
        alt: down(VK_MENU),
        win: down(VK_LWIN) || down(VK_RWIN),
    })
}

/// Focus moved into or out of the page's webview.
pub(super) fn focus_changed(page: &PageInner, focused: bool) {
    page.status.borrow_mut().focused = focused;
    page.change(PageChange::Focus(focused));
}
