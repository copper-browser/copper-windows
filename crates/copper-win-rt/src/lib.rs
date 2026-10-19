//! UI-thread runtime for Copper on Windows.
//!
//! Everything in Copper's shell runs on one UI thread (WebView2 requires it,
//! and the shared `copper-engine` traits are `!Send` by design). This crate is
//! the small runtime that thread needs:
//!
//! * [`init`] once on the UI thread, before anything else.
//! * [`spawn_local`] runs a `!Send` future on the UI thread.
//! * [`sleep`] is a timer future backed by `SetTimer`.
//! * [`post`] runs a closure on the UI thread from any thread.
//! * [`run`] is the standard message loop; [`quit`] ends it.
//! * [`add_message_hook`] lets the UI toolkit see each message before
//!   dispatch (keyboard accelerators, IME, dialog navigation).
//!
//! Wakeups go through a hidden message-only window, so futures resume inside
//! the normal message loop and never re-enter a COM callback.
//!
//! This crate is a seam shared by `copper-engine-webview2`, `copper-win-ui`
//! and `copper-win`. Keep its API small and stable.

#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

type LocalTask = Pin<Box<dyn Future<Output = ()>>>;
type Posted = Box<dyn FnOnce() + Send>;
type Hook = Box<dyn FnMut(&RawMsg) -> bool>;

/// Shared between threads: ready task ids, posted closures, the wake window.
struct Shared {
    ready: Mutex<VecDeque<u64>>,
    posted: Mutex<VecDeque<Posted>>,
    hwnd: isize,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

thread_local! {
    static TASKS: RefCell<HashMap<u64, LocalTask>> = RefCell::new(HashMap::new());
    static NEXT_TASK: Cell<u64> = const { Cell::new(1) };
    static TIMERS: RefCell<HashMap<usize, TimerSlot>> = RefCell::new(HashMap::new());
    static NEXT_TIMER: Cell<usize> = const { Cell::new(1) };
    static HOOKS: RefCell<Vec<Hook>> = const { RefCell::new(Vec::new()) };
    static ON_UI: Cell<bool> = const { Cell::new(false) };
}

/// A message as seen by [`add_message_hook`]: the fields of `MSG`.
#[derive(Clone, Copy, Debug)]
pub struct RawMsg {
    pub hwnd: isize,
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
}

struct TimerSlot {
    fired: bool,
    waker: Option<Waker>,
}

const WM_APP_WAKE: u32 = 0x8000 + 0x0C01; // WM_APP + n
const WM_APP_POSTED: u32 = 0x8000 + 0x0C02;

/// True when called on the thread that ran [`init`].
pub fn on_ui_thread() -> bool {
    ON_UI.with(Cell::get)
}

struct TaskWaker {
    id: u64,
    shared: Arc<Shared>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.shared.ready.lock().unwrap().push_back(self.id);
        notify(self.shared.hwnd, WM_APP_WAKE);
    }
}

/// Spawn a `!Send` future on the UI thread. Must be called on the UI thread.
pub fn spawn_local(fut: impl Future<Output = ()> + 'static) {
    debug_assert!(on_ui_thread(), "spawn_local off the UI thread");
    let id = NEXT_TASK.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    TASKS.with(|t| t.borrow_mut().insert(id, Box::pin(fut)));
    if let Some(shared) = SHARED.get() {
        shared.ready.lock().unwrap().push_back(id);
        notify(shared.hwnd, WM_APP_WAKE);
    }
}

/// Run a closure on the UI thread, from any thread. Order is preserved.
pub fn post(f: impl FnOnce() + Send + 'static) {
    let Some(shared) = SHARED.get() else {
        return;
    };
    shared.posted.lock().unwrap().push_back(Box::new(f));
    notify(shared.hwnd, WM_APP_POSTED);
}

/// A future that resolves after `d` on the UI thread (`SetTimer`; ~10–16 ms
/// resolution). Zero duration yields once.
pub fn sleep(d: Duration) -> Sleep {
    Sleep { dur: d, id: None }
}

/// Future returned by [`sleep`].
pub struct Sleep {
    dur: Duration,
    id: Option<usize>,
}

impl Future for Sleep {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match self.id {
            None => {
                let id = NEXT_TIMER.with(|n| {
                    let id = n.get();
                    n.set(id + 1);
                    id
                });
                TIMERS.with(|t| {
                    t.borrow_mut().insert(
                        id,
                        TimerSlot {
                            fired: false,
                            waker: Some(cx.waker().clone()),
                        },
                    )
                });
                start_timer(id, self.dur);
                self.id = Some(id);
                Poll::Pending
            }
            Some(id) => TIMERS.with(|t| {
                let mut t = t.borrow_mut();
                match t.get_mut(&id) {
                    Some(slot) if slot.fired => {
                        t.remove(&id);
                        Poll::Ready(())
                    }
                    Some(slot) => {
                        slot.waker = Some(cx.waker().clone());
                        Poll::Pending
                    }
                    None => Poll::Ready(()),
                }
            }),
        }
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let live = TIMERS.with(|t| t.borrow_mut().remove(&id).is_some_and(|s| !s.fired));
            if live {
                stop_timer(id);
            }
        }
    }
}

/// See every message before it is translated and dispatched. Return `true`
/// to swallow it. Hooks run in registration order. UI thread only.
pub fn add_message_hook(hook: impl FnMut(&RawMsg) -> bool + 'static) {
    HOOKS.with(|h| h.borrow_mut().push(Box::new(hook)));
}

fn run_hooks(msg: &RawMsg) -> bool {
    // Take the hooks out while they run so a hook may add another hook.
    let mut hooks = HOOKS.with(|h| std::mem::take(&mut *h.borrow_mut()));
    let mut swallowed = false;
    for hook in hooks.iter_mut() {
        if hook(msg) {
            swallowed = true;
            break;
        }
    }
    HOOKS.with(|h| {
        let mut cur = h.borrow_mut();
        hooks.append(&mut cur);
        *cur = hooks;
    });
    swallowed
}

fn poll_ready() {
    let Some(shared) = SHARED.get() else {
        return;
    };
    loop {
        let next = shared.ready.lock().unwrap().pop_front();
        let Some(id) = next else {
            break;
        };
        let Some(mut task) = TASKS.with(|t| t.borrow_mut().remove(&id)) else {
            continue; // finished, or a duplicate wake
        };
        let waker = Waker::from(Arc::new(TaskWaker {
            id,
            shared: shared.clone(),
        }));
        let mut cx = Context::from_waker(&waker);
        if task.as_mut().poll(&mut cx).is_pending() {
            TASKS.with(|t| t.borrow_mut().insert(id, task));
        }
    }
}

fn run_posted() {
    let Some(shared) = SHARED.get() else {
        return;
    };
    loop {
        let next = shared.posted.lock().unwrap().pop_front();
        let Some(f) = next else {
            break;
        };
        f();
    }
}

fn timer_fired(id: usize) {
    stop_timer(id);
    let waker = TIMERS.with(|t| {
        t.borrow_mut().get_mut(&id).and_then(|s| {
            s.fired = true;
            s.waker.take()
        })
    });
    if let Some(w) = waker {
        w.wake();
    }
}

#[cfg(windows)]
mod sys {
    use super::*;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, KillTimer,
        MSG, PostMessageW, PostQuitMessage, RegisterClassW, SetTimer, TranslateMessage,
        WINDOW_EX_STYLE, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
    };
    use windows::core::w;

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_APP_WAKE => {
                poll_ready();
                LRESULT(0)
            }
            WM_APP_POSTED => {
                run_posted();
                poll_ready();
                LRESULT(0)
            }
            WM_TIMER => {
                timer_fired(wp.0);
                poll_ready();
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        }
    }

    pub fn init() -> windows::core::Result<()> {
        if SHARED.get().is_some() {
            return Ok(());
        }
        unsafe {
            let hinst = GetModuleHandleW(None)?;
            let class = w!("CopperRtWake");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!(""),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(hinst.into()),
                None,
            )?;
            let _ = SHARED.set(Arc::new(Shared {
                ready: Mutex::new(VecDeque::new()),
                posted: Mutex::new(VecDeque::new()),
                hwnd: hwnd.0 as isize,
            }));
        }
        ON_UI.with(|u| u.set(true));
        Ok(())
    }

    pub fn notify(hwnd: isize, msg: u32) {
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(0), LPARAM(0));
        }
    }

    pub fn start_timer(id: usize, d: Duration) {
        if let Some(shared) = SHARED.get() {
            let ms = d.as_millis().clamp(1, u32::MAX as u128) as u32;
            unsafe {
                SetTimer(Some(HWND(shared.hwnd as *mut _)), id, ms, None);
            }
        }
    }

    pub fn stop_timer(id: usize) {
        if let Some(shared) = SHARED.get() {
            unsafe {
                let _ = KillTimer(Some(HWND(shared.hwnd as *mut _)), id);
            }
        }
    }

    pub fn run() -> i32 {
        let mut msg = MSG::default();
        unsafe {
            loop {
                let r = GetMessageW(&mut msg, None, 0, 0);
                if r.0 == 0 {
                    return msg.wParam.0 as i32;
                }
                if r.0 == -1 {
                    return -1;
                }
                let raw = RawMsg {
                    hwnd: msg.hwnd.0 as isize,
                    message: msg.message,
                    wparam: msg.wParam.0,
                    lparam: msg.lParam.0,
                };
                if run_hooks(&raw) {
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    pub fn quit(code: i32) {
        unsafe { PostQuitMessage(code) }
    }
}

#[cfg(not(windows))]
mod sys {
    use super::*;
    pub fn init() -> Result<(), std::io::Error> {
        Err(std::io::Error::other("copper-win-rt runs on Windows only"))
    }
    pub fn notify(_hwnd: isize, _msg: u32) {}
    pub fn start_timer(_id: usize, _d: Duration) {}
    pub fn stop_timer(_id: usize) {}
    pub fn run() -> i32 {
        0
    }
    pub fn quit(_code: i32) {}
}

use sys::{notify, start_timer, stop_timer};

/// Initialise the runtime on the current (UI) thread. Idempotent.
#[cfg(windows)]
pub fn init() -> windows::core::Result<()> {
    sys::init()
}

/// Initialise the runtime (fails off Windows).
#[cfg(not(windows))]
pub fn init() -> Result<(), std::io::Error> {
    sys::init()
}

/// Run the message loop until [`quit`]. Returns the exit code.
pub fn run() -> i32 {
    sys::run()
}

/// Post `WM_QUIT` to end [`run`].
pub fn quit(code: i32) {
    sys::quit(code)
}

/// Await a value delivered later by a callback (e.g. a WebView2 completion
/// handler). [`oneshot`] returns the sending half and the future.
pub fn oneshot<T>() -> (OneshotTx<T>, OneshotRx<T>) {
    let cell = std::rc::Rc::new(RefCell::new(OneshotState {
        value: None,
        waker: None,
        closed: false,
    }));
    (OneshotTx { cell: cell.clone() }, OneshotRx { cell })
}

struct OneshotState<T> {
    value: Option<T>,
    waker: Option<Waker>,
    closed: bool,
}

/// Sending half of [`oneshot`] (UI thread only).
pub struct OneshotTx<T> {
    cell: std::rc::Rc<RefCell<OneshotState<T>>>,
}

impl<T> OneshotTx<T> {
    pub fn send(self, value: T) {
        let waker = {
            let mut s = self.cell.borrow_mut();
            s.value = Some(value);
            s.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    }
}

impl<T> Drop for OneshotTx<T> {
    fn drop(&mut self) {
        let waker = {
            let mut s = self.cell.borrow_mut();
            s.closed = true;
            s.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    }
}

/// Receiving half of [`oneshot`]; resolves to `None` if the sender dropped.
pub struct OneshotRx<T> {
    cell: std::rc::Rc<RefCell<OneshotState<T>>>,
}

impl<T> Future for OneshotRx<T> {
    type Output = Option<T>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let mut s = self.cell.borrow_mut();
        if let Some(v) = s.value.take() {
            return Poll::Ready(Some(v));
        }
        if s.closed {
            return Poll::Ready(None);
        }
        s.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}
