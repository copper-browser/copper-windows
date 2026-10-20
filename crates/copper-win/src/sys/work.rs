//! Slow work off the window's thread: a disk walk, a shell icon, a file
//! read. [`off_thread`] runs a closure on a thread of its own and hands its
//! result back to the UI thread through the loop (`copper_win_rt::post`),
//! so the awaiting task wakes the moment the work is done — no polling,
//! and no nested message pump.

use copper_win_rt::OneshotTx;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    /// The UI thread's waiting receivers, by ticket. A sender is `Rc`-based
    /// (UI thread only), so the worker carries the ticket, not the sender.
    static WAITING: RefCell<HashMap<u64, Box<dyn Any>>> = RefCell::default();
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// Run `f` on a thread of its own and wait (without blocking the UI) for
/// what it returns. `None` if `f` panicked. Call from a UI-thread task.
pub async fn off_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = copper_win_rt::oneshot::<T>();
    let ticket = NEXT.with(|n| {
        let t = n.get();
        n.set(t.wrapping_add(1));
        t
    });
    WAITING.with(|w| w.borrow_mut().insert(ticket, Box::new(tx)));
    std::thread::spawn(move || {
        let mut back = Deliver::<T> {
            ticket,
            value: None,
        };
        back.value = Some(f());
        // `back` drops here (or while unwinding) and posts what it holds.
    });
    rx.await
}

/// The worker's half: whatever it holds when it drops (a value, or nothing
/// after a panic) goes to the UI thread, where the waiting receiver gets
/// it — or `None`, its sender dropped.
struct Deliver<T: Send + 'static> {
    ticket: u64,
    value: Option<T>,
}

impl<T: Send + 'static> Drop for Deliver<T> {
    fn drop(&mut self) {
        let ticket = self.ticket;
        let value = self.value.take();
        copper_win_rt::post(move || {
            let Some(tx) = WAITING.with(|w| w.borrow_mut().remove(&ticket)) else {
                return;
            };
            let Ok(tx) = tx.downcast::<OneshotTx<T>>() else {
                return;
            };
            if let Some(v) = value {
                tx.send(v);
            }
        });
    }
}
