//! The top-level window: creation, the message procedure, and the rules
//! that keep the app's handler from ever re-entering the window's state.
//!
//! The state lives in a `RefCell`. A message that arrives while it is
//! borrowed (Windows sends some synchronously from inside our own calls)
//! falls back to the default procedure, or, for a size or scale change,
//! is remembered and applied at the next chance. Anything the toolkit does
//! that sends messages back to the window (moving it, destroying it) is
//! queued as a command and run once the borrow is released, and the app's
//! handler runs only then too.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, Once};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{FILETIME, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, GetDpiForWindow,
    MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetCursorPos,
    GetWindowLongPtrW, IDC_ARROW, IsChild, LoadCursorW, MINMAXINFO, PostMessageW, RegisterClassExW,
    SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOWMAXIMIZED, SW_SHOWNORMAL, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, ShowWindow, WM_DPICHANGED, WM_GETMINMAXINFO, WM_GETOBJECT, WM_NCCALCSIZE,
    WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_SIZE, WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP,
    WS_OVERLAPPEDWINDOW,
};
use windows::core::{Result, w};

use super::frame::{self, Backdrop};
use super::gfx::{self, Gfx};
use super::text::{self, Fonts};
use super::ui::{Command, Parts, Ui};
use crate::event::Event;
use crate::geom::Size;
use crate::stats::{Lap, Phases};
use crate::theme::Appearance;
use crate::tokens::window as tokens;

const CLASS: windows::core::PCWSTR = w!("CopperWindow");

/// How a window opens.
#[derive(Clone, Debug)]
pub struct Options {
    pub title: String,
    /// The client area, in DIPs.
    pub size: Size,
    pub min_size: Size,
    pub appearance: Appearance,
    pub backdrop: Backdrop,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            title: "Copper".into(),
            size: Size::new(tokens::DEFAULT_W, tokens::DEFAULT_H),
            min_size: Size::new(tokens::MIN_W, tokens::MIN_H),
            appearance: Appearance::System,
            backdrop: Backdrop::Mica,
        }
    }
}

type Handler = Box<dyn FnMut(&mut Ui, Event)>;

/// What the procedure reaches through the window's user data.
pub(super) struct Host {
    ui: RefCell<Option<Ui>>,
    handler: RefCell<Option<Handler>>,
    flushing: Cell<bool>,
    pending_size: Cell<bool>,
    pending_dpi: Cell<Option<(u32, RECT)>>,
    min_size: Cell<Size>,
}

/// The host behind `hwnd`'s user data.
///
/// # Safety
///
/// `hwnd` must be one of this toolkit's windows: the main window keeps one
/// strong reference to its host in the user data until `WM_NCDESTROY`, and
/// popups hold a pointer to their owner's host and die before it.
pub(super) unsafe fn host_of<'a>(hwnd: HWND) -> Option<&'a Host> {
    // SAFETY: per the contract above.
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Host).as_ref() }
}

enum Step {
    Command(HWND, Command),
    Event(Event),
}

impl Host {
    /// Run `f` on the window's state, then the queued commands and events.
    /// `None` while the state is busy (a message sent from inside it) or
    /// gone.
    pub(super) fn with_ui<R>(&self, f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let r = {
            let mut g = self.ui.try_borrow_mut().ok()?;
            let ui = g.as_mut()?;
            self.catch_up(ui);
            f(ui)
        };
        self.flush();
        Some(r)
    }

    /// Run `f` on the state without running the app's handler afterwards
    /// (UI Automation reads). Queued work runs at the next frame.
    pub(super) fn peek<R>(&self, f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let mut g = self.ui.try_borrow_mut().ok()?;
        let ui = g.as_mut()?;
        let r = f(ui);
        if !ui.events.is_empty() || !ui.commands.is_empty() {
            ui.want_frame();
        }
        Some(r)
    }

    /// Apply a size or scale change that arrived while the state was busy.
    fn catch_up(&self, ui: &mut Ui) {
        if self.pending_size.take() {
            ui.sync_client_size();
        }
        if let Some((dpi, rect)) = self.pending_dpi.take() {
            ui.set_dpi(dpi);
            ui.commands.push(Command::SetPos(rect));
        }
    }

    /// Run queued commands and deliver queued events, one at a time, until
    /// both queues are empty. Nested calls (from messages the commands
    /// send) return at once; this loop picks up what they queue.
    fn flush(&self) {
        if self.flushing.replace(true) {
            return;
        }
        loop {
            let step = {
                let Ok(mut g) = self.ui.try_borrow_mut() else {
                    break;
                };
                let Some(ui) = g.as_mut() else {
                    break;
                };
                if !ui.commands.is_empty() {
                    Step::Command(ui.hwnd, ui.commands.remove(0))
                } else if let Some(e) = ui.events.pop_front() {
                    Step::Event(e)
                } else {
                    if ui.needs_frame() {
                        ui.want_frame();
                    }
                    break;
                }
            };
            match step {
                Step::Command(hwnd, c) => run_command(hwnd, c),
                Step::Event(e) => {
                    let Some(mut h) = self.handler.borrow_mut().take() else {
                        continue;
                    };
                    if let Ok(mut g) = self.ui.try_borrow_mut()
                        && let Some(ui) = g.as_mut()
                    {
                        h(ui, e);
                    }
                    let mut slot = self.handler.borrow_mut();
                    if slot.is_none() {
                        *slot = Some(h);
                    }
                }
            }
        }
        self.flushing.set(false);
    }

    /// Deliver the events still queued as the window goes away, whatever
    /// flush is in progress (it would find the state gone). Commands are
    /// dropped: there is no window left for them.
    fn deliver_last(&self) {
        let Some(mut h) = self.handler.borrow_mut().take() else {
            return;
        };
        loop {
            let Ok(mut g) = self.ui.try_borrow_mut() else {
                break;
            };
            let Some(ui) = g.as_mut() else {
                break;
            };
            ui.commands.clear();
            let Some(e) = ui.events.pop_front() else {
                break;
            };
            h(ui, e);
        }
        let mut slot = self.handler.borrow_mut();
        if slot.is_none() {
            *slot = Some(h);
        }
    }

    fn wndproc(&self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_NCCALCSIZE => {
                // SAFETY: a DPI query on our own window.
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                return frame::nccalcsize(hwnd, wp, lp, dpi);
            }
            WM_GETMINMAXINFO => {
                // SAFETY: as above; lParam points at a MINMAXINFO.
                unsafe {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    let s = dpi as f32 / 96.0;
                    let b = frame::border(dpi);
                    let m = self.min_size.get();
                    let info = &mut *(lp.0 as *mut MINMAXINFO);
                    info.ptMinTrackSize.x = (m.w * s).ceil() as i32 + 2 * b;
                    info.ptMinTrackSize.y = (m.h * s).ceil() as i32 + b;
                }
                return LRESULT(0);
            }
            WM_NCHITTEST => {
                if let Some(r) = self.with_ui(|ui| ui.hit_test(wp, lp)) {
                    return r;
                }
            }
            WM_SIZE => {
                if self.with_ui(|ui| ui.message(msg, wp, lp)).is_none() {
                    self.pending_size.set(true);
                }
                return LRESULT(0);
            }
            WM_DPICHANGED => {
                if self.with_ui(|ui| ui.message(msg, wp, lp)).is_none() {
                    // SAFETY: lParam points at the suggested rectangle.
                    let rect = unsafe { *(lp.0 as *const RECT) };
                    self.pending_dpi
                        .set(Some(((wp.0 >> 16) as u32 & 0xffff, rect)));
                }
                return LRESULT(0);
            }
            WM_GETOBJECT => {
                if let Some(Some(r)) = self.peek(|ui| ui.get_object(wp, lp)) {
                    return r;
                }
            }
            WM_NCDESTROY => {
                // Tell the app what is left (`Event::Closed` last), then
                // drop the state and the handler, which may hold the
                // window, while the window still exists.
                self.deliver_last();
                let ui = self.ui.try_borrow_mut().ok().and_then(|mut g| g.take());
                drop(ui);
                if let Ok(mut h) = self.handler.try_borrow_mut() {
                    *h = None;
                }
            }
            _ => {
                if let Some(Some(r)) = self.with_ui(|ui| ui.message(msg, wp, lp)) {
                    return r;
                }
            }
        }
        // SAFETY: default handling.
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }
}

fn run_command(hwnd: HWND, c: Command) {
    // SAFETY: plain window management on our own window.
    unsafe {
        match c {
            Command::SetPos(r) => {
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
            Command::Minimize => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            Command::ToggleMaximize => {
                let cmd = if frame::is_maximized(hwnd) {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                };
                let _ = ShowWindow(hwnd, cmd);
            }
            Command::FocusWindow => {
                let _ = SetFocus(Some(hwnd));
            }
            Command::FocusChild(child) => {
                // Only from this window: a click elsewhere since then wins.
                if GetFocus() == hwnd && IsChild(hwnd, child).as_bool() {
                    let _ = SetFocus(Some(child));
                }
            }
            Command::Destroy => {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: lParam is the CREATESTRUCTW whose parameter is the host
        // pointer `Window::new` passed; the window owns that reference now.
        unsafe {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        }
    }
    // SAFETY: the main window's user data is its host (see `host_of`).
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Host;
    // SAFETY: as above.
    let Some(host) = (unsafe { ptr.as_ref() }) else {
        // SAFETY: default handling.
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    let r = host.wndproc(hwnd, msg, wp, lp);
    if msg == WM_NCDESTROY {
        // SAFETY: release the window's reference, taken at creation; the
        // host is not touched after this.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Rc::from_raw(ptr));
        }
    }
    r
}

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: registering a class whose procedure lives for the process.
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
    });
}

/// Something a warm-up thread made, and how long it took (ms).
type Made<T> = (Result<T>, f32);

/// The warm-up threads: the font system; a WARP device to draw the first
/// frames on; the hardware device, made once the window is on screen.
struct Threads {
    d3d: Option<JoinHandle<Made<gfx::D3d>>>,
    warp: Option<JoinHandle<Made<gfx::D3d>>>,
    fonts: Option<JoinHandle<Made<text::Factory>>>,
    /// Sent or dropped: the hardware device may be made now.
    go: Option<Sender<()>>,
}

/// How long the hardware device waits for its window to be shown.
const HARDWARE_WAIT: Duration = Duration::from_secs(1);

/// What [`prewarm`] started, for the first window to take.
static PREWARM: Mutex<Option<Threads>> = Mutex::new(None);

/// Whether a window starts on WARP: only the process's first (later ones
/// find the GPU driver loaded, and a hardware device made in ~45 ms), and
/// not with `COPPER_GFX_WARP_FIRST=0`.
fn warp_first() -> bool {
    !gfx::HARDWARE_MADE.load(std::sync::atomic::Ordering::Relaxed)
        && std::env::var("COPPER_GFX_WARP_FIRST").map_or(true, |v| v != "0")
}

fn timed<T: Send + 'static>(
    name: &str,
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Option<JoinHandle<Made<T>>> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let at = Instant::now();
            let r = f();
            (r, at.elapsed().as_secs_f32() * 1000.0)
        })
        .ok()
}

fn spawn_warm() -> Threads {
    let fonts = timed("copper-fonts-warm", text::create_factory);
    if !warp_first() {
        return Threads {
            d3d: timed("copper-gfx-warm", gfx::create_d3d),
            warp: None,
            fonts,
            go: None,
        };
    }
    // WARP now, the hardware device once the window is shown: the GPU
    // driver's loading (~200 ms) holds the loader lock, which making the
    // WARP device and showing the window (input method DLLs) also take.
    let (go, wait) = mpsc::channel::<()>();
    let warp = timed("copper-warp-warm", gfx::create_warp);
    let d3d = timed("copper-gfx-warm", move || {
        let _ = wait.recv_timeout(HARDWARE_WAIT);
        gfx::create_d3d()
    });
    Threads {
        d3d,
        warp,
        fonts,
        go: Some(go),
    }
}

/// Start making the GPU devices and the font system (the slowest parts
/// of a window) now, on threads of their own, so the first [`Window::new`]
/// finds them ready. Call as early as the process knows it will open a
/// window (before reading its settings, say); without it the window starts
/// the same threads itself and waits for them after `CreateWindowExW`.
pub fn prewarm() {
    if let Ok(mut slot) = PREWARM.lock()
        && slot.is_none()
    {
        *slot = Some(spawn_warm());
    }
}

/// The warm-up a window waits for.
struct Warm(Threads);

/// The devices a window starts with.
struct Devices {
    /// The device to draw on now: the hardware one, or WARP while it is
    /// still being made (`None`: the window makes one itself).
    d3d: Option<gfx::D3d>,
    /// The hardware device still being made.
    hardware: Option<JoinHandle<Made<gfx::D3d>>>,
    /// Lets the hardware device be made (once the window is shown).
    go: Option<Sender<()>>,
    fonts: Option<text::Factory>,
}

fn join<T>(t: Option<JoinHandle<Made<T>>>, phases: &mut Phases, name: &'static str) -> Option<T> {
    let (r, ms) = t?.join().ok()?;
    phases.push(name, ms);
    r.ok()
}

impl Warm {
    /// [`prewarm`]'s threads, or new ones.
    fn start() -> Warm {
        let early = PREWARM.lock().ok().and_then(|mut s| s.take());
        Warm(early.unwrap_or_else(spawn_warm))
    }

    /// What they made, noting in `phases` how long each took on its
    /// thread: the hardware device if it is ready, else WARP's (the
    /// hardware device follows), else the hardware device waited for.
    fn take(self, phases: &mut Phases) -> Devices {
        let Threads {
            d3d,
            warp,
            fonts,
            go,
        } = self.0;
        let fonts = join(fonts, phases, "(thread: dwrite)");
        let ready = d3d.as_ref().is_some_and(|t| t.is_finished());
        if !ready && let Some(w) = join(warp, phases, "(thread: warp)") {
            return Devices {
                d3d: Some(w),
                hardware: d3d,
                go,
                fonts,
            };
        }
        // No WARP device: wait for the hardware one, made now.
        drop(go);
        let d3d = join(d3d, phases, "(thread: d3d)");
        if gfx::ON_WARP.load(std::sync::atomic::Ordering::Relaxed) {
            phases.push("(d3d on warp)", 0.0);
        }
        Devices {
            d3d,
            hardware: None,
            go: None,
            fonts,
        }
    }
}

/// Hand the hardware device to the window when it is ready: a thread
/// waits for it, leaves it in the slot and posts
/// [`super::clock::WM_GFX_READY`].
fn hand_over(hwnd: HWND, t: JoinHandle<Made<gfx::D3d>>) -> Option<Arc<Mutex<Option<gfx::D3d>>>> {
    let slot = Arc::new(Mutex::new(None));
    let to = slot.clone();
    let hwnd = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("copper-gfx-handover".into())
        .spawn(move || {
            let Ok((Ok(d3d), ms)) = t.join() else {
                return;
            };
            if let Ok(mut s) = to.lock() {
                *s = Some(d3d);
            }
            // SAFETY: posting to the window that waits for it (gone: the
            // post fails and the device is dropped with the slot).
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut _)),
                    super::clock::WM_GFX_READY,
                    WPARAM(ms as usize),
                    LPARAM(0),
                );
            }
        })
        .ok()?;
    Some(slot)
}

/// Milliseconds since this process was created.
pub fn since_process_ms() -> f64 {
    let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    let (mut created, mut exit, mut kernel, mut user) = Default::default();
    // SAFETY: querying our own process's times into locals.
    unsafe {
        if GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
        .is_err()
        {
            return 0.0;
        }
        let now = GetSystemTimePreciseAsFileTime();
        ticks(now).saturating_sub(ticks(created)) as f64 / 10_000.0
    }
}

/// Where a window of `size` DIPs opens: centred on the work area of the
/// monitor under the pointer, at that monitor's scale.
fn placement(size: Size) -> (RECT, u32) {
    // SAFETY: plain cursor and monitor queries.
    unsafe {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(mon, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let dpi = dx.max(96);
        let s = dpi as f32 / 96.0;
        let b = frame::border(dpi);
        let work = info.rcWork;
        let (ww, wh) = frame::rect_size(&work);
        // The invisible resize borders sit outside the visible window.
        let w = ((size.w * s).round() as i32 + 2 * b).min(ww + 2 * b);
        let h = ((size.h * s).round() as i32 + b).min(wh + b);
        let x = work.left + (ww - w) / 2;
        let y = work.top + ((wh - h) / 2).max(0);
        (
            RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            dpi,
        )
    }
}

/// A top-level Copper window. Clones share the window.
#[derive(Clone)]
pub struct Window {
    host: Rc<Host>,
    hwnd: HWND,
}

impl Window {
    /// Make the window, hidden. Build into [`Ui::content`] with
    /// [`Window::with`], then [`Window::show`]: the first frame is drawn
    /// before the window appears. `on_event` hears everything that
    /// happens, on the UI thread, with the state free to change.
    pub fn new(opts: Options, on_event: impl FnMut(&mut Ui, Event) + 'static) -> Result<Window> {
        let created = Instant::now();
        let mut phases = Phases::new();
        let mut lap = Lap::new(true);
        // SAFETY: process-wide setup on the UI thread; failures mean it is
        // already set (a manifest, or an earlier window).
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            // WebView2 and drag and drop want an OLE apartment.
            let _ = OleInitialize(None);
        }
        lap.mark(&mut phases, "window: dpi+ole");
        // The GPU device and the font system take the longest; make them
        // while the window is created (or take them from `prewarm`).
        let warm = Warm::start();
        lap.mark(&mut phases, "window: warm thread");

        register();
        let host = Rc::new(Host {
            ui: RefCell::new(None),
            handler: RefCell::new(Some(Box::new(on_event))),
            flushing: Cell::new(false),
            pending_size: Cell::new(false),
            pending_dpi: Cell::new(None),
            min_size: Cell::new(opts.min_size),
        });
        let (rect, _) = placement(opts.size);
        let title = text::wide(&opts.title);
        let param = Rc::into_raw(host.clone());
        // SAFETY: creating our own window; the procedure takes the host
        // reference in WM_NCCREATE.
        let made = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                CLASS,
                text::pcwstr(&title),
                WS_OVERLAPPEDWINDOW,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                Some(param.cast()),
            )
        };
        let hwnd = match made {
            Ok(h) => h,
            Err(e) => {
                // SAFETY: the window never took the reference.
                unsafe { drop(Rc::from_raw(param)) };
                return Err(e);
            }
        };
        lap.mark(&mut phases, "window: create");
        frame::init(hwnd);
        // SAFETY: re-run WM_NCCALCSIZE now that the procedure is in place.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }

        lap.mark(&mut phases, "window: frame");
        let devices = warm.take(&mut phases);
        lap.mark(&mut phases, "window: wait for d3d+dwrite");
        let factory = match devices.fonts {
            Some(f) => f,
            None => text::create_factory()?,
        };
        let gfx = Gfx::new(devices.d3d, 0)?;
        lap.mark(&mut phases, "window: d2d+dcomp");
        let fonts = Fonts::new(factory)?;
        lap.mark(&mut phases, "window: fonts");
        let mut ui = Ui::new(Parts {
            hwnd,
            host: Rc::downgrade(&host),
            gfx,
            fonts,
            appearance: opts.appearance,
            backdrop: opts.backdrop,
            created,
            phases: Phases::new(),
        });
        lap.mark(&mut phases, "window: state");
        ui.phases = phases;
        ui.hardware = devices.hardware.and_then(|t| hand_over(hwnd, t));
        ui.hardware_go = devices.go;
        *host.ui.borrow_mut() = Some(ui);
        Ok(Window { host, hwnd })
    }

    /// Draw the first frame and show the window.
    pub fn show(&self) {
        self.show_as(false);
    }

    /// [`Window::show`], maximised when `maximized` (a remembered place).
    pub fn show_as(&self, maximized: bool) {
        let first = self
            .host
            .with_ui(|ui| {
                let first = !ui.shown;
                ui.shown = true;
                ui.render();
                first
            })
            .unwrap_or(false);
        let shown = Instant::now();
        // SAFETY: showing our own window (outside the state's borrow: it
        // sends size, activation and focus messages).
        unsafe {
            let _ = ShowWindow(
                self.hwnd,
                if maximized {
                    SW_SHOWMAXIMIZED
                } else {
                    SW_SHOWNORMAL
                },
            );
        }
        if first {
            let process = since_process_ms();
            let show_ms = shown.elapsed().as_secs_f32() * 1000.0;
            self.host.with_ui(|ui| {
                // On screen: the hardware device may be made now.
                ui.hardware_go = None;
                ui.phases.push("show", show_ms);
                let create = ui.created.elapsed().as_secs_f64() * 1000.0;
                ui.events.push_back(Event::FirstFrame {
                    since_process_ms: process as f32,
                    since_create_ms: create as f32,
                });
            });
        }
    }

    /// Change the window's state. `None` while the window is busy handling
    /// a message that came from inside the state (never from the app's
    /// handler, tasks or timers) or after it closed.
    pub fn with<R>(&self, f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        self.host.with_ui(f)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Whether the window still exists.
    pub fn is_open(&self) -> bool {
        self.host.ui.try_borrow().map_or(true, |u| u.is_some())
    }

    /// Change the title Windows shows (taskbar, Alt+Tab).
    pub fn set_title(&self, title: &str) {
        let t = text::wide(title);
        // SAFETY: our own window; no state borrowed (it sends WM_SETTEXT).
        unsafe {
            let _ = SetWindowTextW(self.hwnd, text::pcwstr(&t));
        }
    }

    /// Replace the handler.
    pub fn on_event(&self, f: impl FnMut(&mut Ui, Event) + 'static) {
        if let Ok(mut h) = self.host.handler.try_borrow_mut() {
            *h = Some(Box::new(f));
        }
    }
}
