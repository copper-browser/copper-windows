//! Work that must not run on the UI thread (image decoding, the system file
//! dialog's modal loop), run on a thread of its own and awaited from the UI
//! thread.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// Where the worker leaves its result for the awaiting task.
struct Slot<T> {
    value: Option<T>,
    done: bool,
    waker: Option<Waker>,
}

/// The result of [`run`]: `None` if the worker panicked (or never started).
pub(crate) struct Done<T> {
    slot: Arc<Mutex<Slot<T>>>,
}

impl<T> Future for Done<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if slot.done {
            return Poll::Ready(slot.value.take());
        }
        slot.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// Run `f` on a new thread called `name`. The UI thread's task wakes when it
/// is done (`copper_win_rt`'s wakers may be woken from any thread).
pub(crate) fn run<T: Send + 'static>(
    name: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> Done<T> {
    let slot = Arc::new(Mutex::new(Slot {
        value: None,
        done: false,
        waker: None,
    }));
    let out = slot.clone();
    let spawned = std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let value = catch_unwind(AssertUnwindSafe(f)).ok();
            let waker = {
                let mut slot = out.lock().unwrap_or_else(|e| e.into_inner());
                slot.value = value;
                slot.done = true;
                slot.waker.take()
            };
            if let Some(w) = waker {
                w.wake();
            }
        });
    if let Err(e) = spawned {
        tracing::error!("{name}: cannot start a thread: {e}");
        slot.lock().unwrap_or_else(|e| e.into_inner()).done = true;
    }
    Done { slot }
}
