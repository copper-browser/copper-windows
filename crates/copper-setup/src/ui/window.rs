//! The setup window: borderless, DWM-backed (Mica on Windows 11, the
//! Copper ground on Windows 10), per-monitor-DPI aware, keyboard-operable,
//! hosting one [`Card`] driven by a [`Controller`].
//!
//! Frames are produced only while something moves: the loop waits in
//! `GetMessageW` when the card is still, and while it animates it drains
//! messages, draws one frame and waits for the compositor (`DwmFlush`), so
//! motion is paced to the display rather than to a 15.6 ms timer tick.

use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWMWINDOWATTRIBUTE, DwmExtendFrameIntoClientArea,
    DwmFlush, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ScreenToClient,
    ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetWindowLongPtrW, HCURSOR, HICON,
    HTCAPTION, HTCLIENT, IDC_ARROW, IDC_HAND, IMAGE_ICON, LR_DEFAULTCOLOR, LoadCursorW, LoadImageW,
    MSG, PM_REMOVE, PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassExW, SW_SHOW,
    SWP_NOACTIVATE, SWP_NOZORDER, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DPICHANGED,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCALCSIZE,
    WM_NCCREATE, WM_NCHITTEST, WM_PAINT, WM_QUIT, WM_SETCURSOR, WM_SETTINGCHANGE,
    WM_SYSCOLORCHANGE, WM_THEMECHANGED, WNDCLASSEXW, WS_CAPTION, WS_EX_APPWINDOW,
    WS_EX_NOREDIRECTIONBITMAP, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
};
use windows_core::{BOOL, HSTRING, PCWSTR};

use super::card::{Card, HEIGHT, Target, WIDTH};
use super::gfx::{APP_ICON_ID, Gfx};
use super::theme::Palette;
use crate::log::log;

/// The setup window's class name (distinct from the browser's
/// `CopperWindow`, so an upgrade never asks setup itself to close).
pub const CLASS: &str = "CopperSetupWindow";

/// Posted to wake the window when a background message is waiting.
const WM_WAKE: u32 = WM_APP + 1;

/// Sends messages from any thread to the window's controller.
pub struct Poster<M> {
    tx: Sender<M>,
    hwnd: isize,
}

impl<M> Clone for Poster<M> {
    fn clone(&self) -> Self {
        Poster {
            tx: self.tx.clone(),
            hwnd: self.hwnd,
        }
    }
}

impl<M: Send + 'static> Poster<M> {
    /// Queue `msg` for [`Controller::message`] on the UI thread.
    pub fn send(&self, msg: M) {
        if self.tx.send(msg).is_ok() {
            // SAFETY: posting to a window that may already be gone is fine.
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(self.hwnd as *mut _)),
                    WM_WAKE,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
    }
}

/// What a controller may do to the window.
pub struct Ctx<M> {
    poster: Poster<M>,
    close: bool,
}

impl<M: Send + 'static> Ctx<M> {
    /// Run `work` on a new thread; it reports back through the poster.
    pub fn spawn(&self, work: impl FnOnce(Poster<M>) + Send + 'static) {
        let poster = self.poster.clone();
        std::thread::spawn(move || work(poster));
    }

    /// Close the window (after the current message is handled).
    pub fn close(&mut self) {
        self.close = true;
    }
}

/// The logic behind a card.
pub trait Controller: 'static {
    type Msg: Send + 'static;
    /// Something on the card was clicked or activated from the keyboard.
    fn clicked(&mut self, target: Target, card: &mut Card, ctx: &mut Ctx<Self::Msg>);
    /// A message from a background thread arrived.
    fn message(&mut self, msg: Self::Msg, card: &mut Card, ctx: &mut Ctx<Self::Msg>);
    /// ×, Esc, Alt+F4 or the taskbar's Close.
    fn close_requested(&mut self, card: &mut Card, ctx: &mut Ctx<Self::Msg>);
    /// The process exit code once the window is gone.
    fn exit_code(&self) -> i32;
}

struct State<C: Controller> {
    controller: C,
    card: Card,
    ctx: Ctx<C::Msg>,
    rx: Receiver<C::Msg>,
    gfx: Option<Gfx>,
    palette: Palette,
    mica: bool,
    dpi: u32,
    moving: bool,
    last_frame: Instant,
    tracking_leave: bool,
}

fn scale(dpi: u32) -> f32 {
    dpi as f32 / 96.0
}

fn px(dips: f32, dpi: u32) -> i32 {
    (dips * scale(dpi)).round() as i32
}

fn set_attr<T>(hwnd: HWND, attr: DWMWINDOWATTRIBUTE, value: &T) -> bool {
    // SAFETY: `value` is the documented type for `attr`, by reference.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attr,
            (value as *const T).cast(),
            size_of::<T>() as u32,
        )
        .is_ok()
    }
}

/// Dark title-bar colours and a backdrop. Returns whether Mica is on.
fn apply_backdrop(hwnd: HWND, palette: &Palette) -> bool {
    set_attr(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &BOOL::from(palette.dark),
    );
    set_attr(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_ROUND);
    if palette.high_contrast {
        return false;
    }
    // Windows 11 22H2+: the documented system backdrop.
    if set_attr(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &DWMSBT_MAINWINDOW) {
        return true;
    }
    // Windows 11 21H2 knew Mica under an undocumented attribute.
    const DWMWA_MICA_EFFECT: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(1029);
    set_attr(hwnd, DWMWA_MICA_EFFECT, &BOOL(1))
}

impl<C: Controller> State<C> {
    fn ground(&self) -> D2D1_COLOR_F {
        if self.mica {
            D2D1_COLOR_F::default()
        } else {
            self.palette.ground
        }
    }

    fn client_size(&self) -> (u32, u32) {
        (px(WIDTH, self.dpi) as u32, px(HEIGHT, self.dpi) as u32)
    }

    fn render(&mut self, hwnd: HWND) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        self.moving = self.card.step(dt);

        if self.gfx.is_none() {
            match Gfx::new(hwnd, self.client_size(), self.dpi as f32) {
                Ok(g) => self.gfx = Some(g),
                Err(e) => {
                    log!("graphics init failed: {e:#}");
                    return;
                }
            }
        }
        let ground = self.ground();
        let palette = self.palette;
        let card = &self.card;
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let result = gfx.frame(ground, |p| card.draw(p, &palette));
        if let Err(e) = result {
            // Device lost (driver update, GPU reset): start over next frame.
            log!("frame failed, rebuilding graphics: {e:#}");
            self.gfx = None;
            self.moving = true;
        }
    }

    /// Something changed: make sure a frame follows.
    fn kick(&mut self) {
        if !self.moving {
            // Coming out of idle: pretend the last frame was one ago, so the
            // first step isn't the whole idle time.
            self.last_frame = Instant::now() - std::time::Duration::from_millis(16);
        }
        self.moving = true;
    }

    fn dip_point(&self, hwnd: HWND, lparam: LPARAM, screen: bool) -> (f32, f32) {
        let mut pt = POINT {
            x: (lparam.0 & 0xffff) as i16 as i32,
            y: ((lparam.0 >> 16) & 0xffff) as i16 as i32,
        };
        if screen {
            // SAFETY: valid window.
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }
        }
        let s = scale(self.dpi);
        (pt.x as f32 / s, pt.y as f32 / s)
    }

    fn hit(&self, x: f32, y: f32) -> Option<Target> {
        self.gfx.as_ref().and_then(|g| self.card.hit(g, x, y))
    }

    fn activate(&mut self, target: Target) {
        match target {
            Target::Close => self
                .controller
                .close_requested(&mut self.card, &mut self.ctx),
            t => self.controller.clicked(t, &mut self.card, &mut self.ctx),
        }
        self.kick();
    }
}

unsafe extern "system" fn wndproc<C: Controller>(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: lparam is the CREATESTRUCTW for this window; its
        // lpCreateParams is the &RefCell<State> passed to CreateWindowExW.
        unsafe {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
    }
    // SAFETY: the pointer stored at WM_NCCREATE; the State outlives the
    // window (it is dropped only after the message loop ends).
    let cell =
        unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State<C>>).as_ref() };
    let Some(cell) = cell else {
        // SAFETY: default handling.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    // A message sent while a handler is already running (a modal loop inside
    // a shell call) gets default handling rather than a second &mut.
    let Ok(mut st) = cell.try_borrow_mut() else {
        // SAFETY: default handling.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    let result = handle(&mut st, hwnd, msg, wparam, lparam);
    let close = std::mem::take(&mut st.ctx.close);
    drop(st);
    if close {
        // SAFETY: our window; WM_DESTROY re-enters with no borrow held.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    // SAFETY: default handling for whatever `handle` passed on.
    result.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) })
}

/// One message; `None` means "let DefWindowProc have it".
fn handle<C: Controller>(
    st: &mut State<C>,
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    match msg {
        // The whole window is client area: no frame, no caption to draw.
        WM_NCCALCSIZE if wparam.0 != 0 => Some(LRESULT(0)),
        WM_NCHITTEST => {
            let (x, y) = st.dip_point(hwnd, lparam, true);
            // Anything clickable is client; everything else drags the window.
            Some(LRESULT(if st.hit(x, y).is_some() {
                HTCLIENT
            } else {
                HTCAPTION
            } as isize))
        }
        WM_SETCURSOR => {
            let mut pt = POINT::default();
            // SAFETY: plain cursor queries.
            unsafe {
                let _ = GetCursorPos(&mut pt);
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            let s = scale(st.dpi);
            let link = st.hit(pt.x as f32 / s, pt.y as f32 / s) == Some(Target::Link);
            // SAFETY: system cursors.
            unsafe {
                let cursor: HCURSOR =
                    LoadCursorW(None, if link { IDC_HAND } else { IDC_ARROW }).unwrap_or_default();
                SetCursor(Some(cursor));
            }
            Some(LRESULT(1))
        }
        WM_MOUSEMOVE => {
            if !st.tracking_leave {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: sized struct for our window.
                st.tracking_leave = unsafe { TrackMouseEvent(&mut tme) }.is_ok();
            }
            let (x, y) = st.dip_point(hwnd, lparam, false);
            let t = st.hit(x, y);
            if st.card.set_hover(t) {
                st.kick();
            }
            Some(LRESULT(0))
        }
        WM_MOUSELEAVE => {
            st.tracking_leave = false;
            if st.card.set_hover(None) {
                st.kick();
            }
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            let (x, y) = st.dip_point(hwnd, lparam, false);
            st.card.pointer_used();
            st.card.set_pressed(st.hit(x, y));
            // SAFETY: our window.
            unsafe { SetCapture(hwnd) };
            st.kick();
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            // SAFETY: we hold the capture.
            unsafe {
                let _ = ReleaseCapture();
            }
            let (x, y) = st.dip_point(hwnd, lparam, false);
            let pressed = st.card.pressed();
            st.card.set_pressed(None);
            if let Some(t) = pressed
                && st.hit(x, y) == Some(t)
            {
                st.activate(t);
            }
            st.kick();
            Some(LRESULT(0))
        }
        WM_KEYDOWN => {
            let key = wparam.0 as u16;
            if key == VK_TAB.0 {
                // SAFETY: plain key-state query.
                let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                st.card.move_focus(shift);
                st.kick();
            } else if key == VK_RETURN.0 || key == VK_SPACE.0 {
                if let Some(t) = st.card.activation_target(key == VK_RETURN.0) {
                    st.activate(t);
                }
            } else if key == VK_ESCAPE.0 {
                st.activate(Target::Close);
            }
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            st.activate(Target::Close);
            Some(LRESULT(0))
        }
        WM_WAKE => {
            while let Ok(m) = st.rx.try_recv() {
                st.controller.message(m, &mut st.card, &mut st.ctx);
            }
            st.kick();
            Some(LRESULT(0))
        }
        WM_DPICHANGED => {
            st.dpi = (wparam.0 & 0xffff) as u32;
            // SAFETY: lparam is the suggested RECT, per WM_DPICHANGED.
            let r = unsafe { *(lparam.0 as *const RECT) };
            // SAFETY: our window.
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let size = st.client_size();
            let dpi = st.dpi as f32;
            if let Some(g) = st.gfx.as_mut()
                && g.resize(size, dpi).is_err()
            {
                st.gfx = None;
            }
            st.kick();
            Some(LRESULT(0))
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
            st.palette = Palette::current();
            st.mica = apply_backdrop(hwnd, &st.palette);
            st.kick();
            None
        }
        WM_PAINT => {
            st.render(hwnd);
            // SAFETY: our window; nothing is drawn through GDI.
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
            Some(LRESULT(0))
        }
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            Some(LRESULT(0))
        }
        _ => None,
    }
}

/// The DPI of the monitor under the pointer (where the window will open).
fn startup_monitor() -> (u32, RECT) {
    let mut pt = POINT::default();
    // SAFETY: plain queries with sized out-structs.
    unsafe {
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(mon, &mut info);
        (dx.max(96), info.rcWork)
    }
}

fn load_icon(px: i32) -> HICON {
    // SAFETY: our own module's icon resource.
    unsafe {
        let module = GetModuleHandleW(None).unwrap_or_default();
        LoadImageW(
            Some(module.into()),
            PCWSTR(APP_ICON_ID as usize as *const u16),
            IMAGE_ICON,
            px,
            px,
            LR_DEFAULTCOLOR,
        )
        .map(|h| HICON(h.0))
        .unwrap_or_default()
    }
}

/// Open the window with `card` and run it until the controller closes it;
/// the controller is handed back for its results. Fails, with nothing left
/// on screen, if the window can't draw at all.
pub fn run<C: Controller>(title: &str, card: Card, controller: C) -> anyhow::Result<C> {
    let (tx, rx) = channel();
    let (dpi, work) = startup_monitor();
    let palette = Palette::current();
    let cell = Box::new(RefCell::new(State {
        controller,
        card,
        ctx: Ctx {
            poster: Poster { tx, hwnd: 0 },
            close: false,
        },
        rx,
        gfx: None,
        palette,
        mica: false,
        dpi,
        moving: true,
        last_frame: Instant::now(),
        tracking_leave: false,
    }));

    // SAFETY: class registration and window creation; the State pointer
    // handed to the window lives in `cell` until after the loop ends.
    let hwnd = unsafe {
        let module = GetModuleHandleW(None)?;
        let class = HSTRING::from(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc::<C>),
            hInstance: module.into(),
            hIcon: load_icon(32),
            hIconSm: load_icon(16),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // A second registration in the same process fails harmlessly.
        RegisterClassExW(&wc);

        let (w, h) = (px(WIDTH, dpi), px(HEIGHT, dpi));
        let x = work.left + ((work.right - work.left) - w) / 2;
        let y = work.top + ((work.bottom - work.top) - h) / 2;
        // WS_CAPTION keeps DWM's shadow, snap animations and taskbar
        // behaviour; WM_NCCALCSIZE takes the visible caption away again.
        CreateWindowExW(
            WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP | WINDOW_EX_STYLE(0),
            &class,
            &HSTRING::from(title),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(module.into()),
            Some(&*cell as *const RefCell<State<C>> as *const _),
        )?
    };

    {
        let mut st = cell.borrow_mut();
        st.ctx.poster.hwnd = hwnd.0 as isize;
        // SAFETY: our window.
        st.dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        // Mica (or the ground) behind the whole client area.
        // SAFETY: our window; -1 margins = the whole window is "glass".
        unsafe {
            let _ = DwmExtendFrameIntoClientArea(
                hwnd,
                &MARGINS {
                    cxLeftWidth: -1,
                    cxRightWidth: -1,
                    cyTopHeight: -1,
                    cyBottomHeight: -1,
                },
            );
        }
        st.mica = apply_backdrop(hwnd, &st.palette);
        log!(
            "setup window: dpi {} mica {} dark {}",
            st.dpi,
            st.mica,
            st.palette.dark
        );
        // Draw before showing, so the window never appears empty.
        st.render(hwnd);
        if st.gfx.is_none() {
            drop(st);
            // SAFETY: our window, no borrow held.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            anyhow::bail!("the setup window couldn't draw");
        }
    }
    // SAFETY: our window.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }

    let mut msg = MSG::default();
    'outer: loop {
        let moving = cell.borrow().moving;
        if moving {
            // SAFETY: standard message pumping on this thread.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'outer;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if let Ok(mut st) = cell.try_borrow_mut() {
                st.render(hwnd);
            }
            // SAFETY: waits for the next composition pass.
            if unsafe { DwmFlush() }.is_err() {
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        } else {
            // SAFETY: standard blocking message loop.
            let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if got.0 <= 0 {
                break;
            }
            // SAFETY: as above.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    Ok(cell.into_inner().controller)
}
