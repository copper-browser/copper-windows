//! The window: Copper's chrome drawn with copper-win-ui, the pages hosted
//! in its slots.
//!
//! The Mac's `Side.swift` and `Stage.swift` are the model: one tinted
//! column down the left (the lights' band with the column door and back /
//! forward / reload, the address, the favourites, the space's rows with the
//! live one on a pill, the space strip at the foot), the page beside it,
//! and the card (launch, edit, palette) dropped over both. On Windows the
//! caption buttons take the top-right corner the Mac leaves to the page.
//!
//! # Threading and re-entrancy
//!
//! Everything here runs on the UI thread. The toolkit calls its event
//! handler with its state borrowed, and [`Window::with`] answers `None`
//! while it is; the shell, meanwhile, calls the view from inside its own
//! operations. So the two never call each other directly:
//!
//! * the toolkit's handler only queues the [`Event`] and schedules a drain
//!   task ([`Chrome::schedule`]); the drain runs on the loop's next turn,
//!   with nothing borrowed, and turns each event into shell calls
//!   ([`events`]);
//! * the view's changes go through [`Chrome::edit`], which applies them at
//!   once when the window is free and otherwise queues them, in order, for
//!   the next turn — never dropped, never reordered.
//!
//! An edit never calls the shell; an event handler never holds the state
//! across a shell call.

pub mod bring;
pub mod build;
pub mod card;
pub mod column;
pub mod events;
pub mod glyph;
pub mod images;
pub mod letter;
pub mod menus;
pub mod model;
pub mod pagemenu;
pub mod permit;
pub mod sheet;
pub mod stage;
pub mod strip;
pub mod toast;
pub mod welcome;

use crate::perf;
use crate::platform::{Shell, WeakShell};
use copper_win_ui::Appearance;
use copper_win_ui::Event;
use copper_win_ui::win::{Backdrop, Options, Ui, Window};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::time::Instant;

/// A change to the window: run with the toolkit's state and the chrome's.
pub type Edit = Box<dyn FnOnce(&mut Ui, &mut State)>;

/// The chrome's nodes and what they show.
pub struct State {
    pub ids: build::Ids,
    pub column: column::Column,
    pub strip: strip::Strip,
    pub stage: stage::Stage,
    pub card: card::Card,
    pub sheet: sheet::Sheet,
    pub toast: toast::Toasts,
    pub images: images::Images,
    pub menus: menus::Menus,
    /// Tabs down the left (`true`) or across the top.
    pub side: bool,
    /// The column folded away (`Ctrl+S`, the Mac's `⌘S`).
    pub folded: bool,
    /// A page is full screen: the chrome steps away.
    pub immersed: bool,
    /// F11: the window fills the monitor, its chrome kept (but no band for
    /// caption buttons it no longer has).
    pub full: bool,
    /// The find bar, once it has been wanted.
    pub find: Option<crate::find::Bar>,
    /// The pin-letter pill, once it has been wanted.
    pub letter: Option<letter::Bar>,
    /// The first start's welcome, while it is up.
    pub welcome: Option<welcome::Welcome>,
}

/// The window and everything drawn in it.
pub struct Chrome {
    window: Window,
    state: RefCell<Option<State>>,
    /// Edits that could not run at once, oldest first.
    deferred: RefCell<VecDeque<Edit>>,
    /// Toolkit events waiting for the drain.
    events: RefCell<VecDeque<Event>>,
    scheduled: Cell<bool>,
    /// Light or dark right now (the toolkit says so as it changes).
    dark: Cell<bool>,
    /// Something of the chrome's is open over the page (the card, the
    /// sheet, a menu): Escape from the page is the chrome's then.
    overlay: Cell<bool>,
    shell: RefCell<Option<WeakShell>>,
    /// A latency being measured to the next frame: what, and since when.
    watch: Cell<Option<(&'static str, Instant)>>,
    /// Frames drawn so far, up to a few (the first ones are logged).
    frames: Cell<u32>,
}

impl Chrome {
    /// Make the window, hidden, and build the chrome in it. The page and
    /// the rows come later, when the shell refreshes it.
    pub fn new(title: &str) -> windows::core::Result<Rc<Chrome>> {
        let me: Rc<OnceCell<Weak<Chrome>>> = Rc::new(OnceCell::new());
        let handler_me = me.clone();
        let window = Window::new(
            Options {
                title: title.to_string(),
                // The settings' look is applied before the first frame.
                appearance: Appearance::Light,
                backdrop: Backdrop::Mica,
                ..Options::default()
            },
            move |ui: &mut Ui, e: Event| {
                if matches!(e, Event::FirstFrame { .. }) {
                    tracing::debug!(target: "copper::perf", "window: {}", ui.startup_phases().line());
                }
                if let Some(c) = handler_me.get().and_then(Weak::upgrade) {
                    c.heard(e);
                }
            },
        )?;
        let chrome = Rc::new(Chrome {
            window,
            state: RefCell::new(None),
            deferred: RefCell::default(),
            events: RefCell::default(),
            scheduled: Cell::new(false),
            dark: Cell::new(false),
            overlay: Cell::new(false),
            shell: RefCell::new(None),
            watch: Cell::new(None),
            frames: Cell::new(0),
        });
        let _ = me.set(Rc::downgrade(&chrome));
        let state = chrome.window.with(build::build);
        *chrome.state.borrow_mut() = state;
        if let Some(dark) = chrome.window.with(|ui| ui.is_dark()) {
            chrome.dark.set(dark);
        }
        Ok(chrome)
    }

    /// Wire the chrome to the shell it draws (once the shell exists).
    pub fn attach(&self, shell: &Shell) {
        *self.shell.borrow_mut() = Some(shell.downgrade());
    }

    /// The shell, while it lives.
    pub fn shell(&self) -> Option<Shell> {
        self.shell.borrow().as_ref().and_then(WeakShell::upgrade)
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn is_dark(&self) -> bool {
        self.dark.get()
    }

    /// Whether the chrome has something open over the page.
    pub fn has_overlay(&self) -> bool {
        self.overlay.get()
    }

    pub(crate) fn set_overlay(&self, on: bool) {
        self.overlay.set(on);
    }

    pub(crate) fn set_dark(&self, dark: bool) {
        self.dark.set(dark);
    }

    /// Change the window. Runs now when the window is free, else on the
    /// loop's next turn after the edits already waiting.
    pub fn edit(self: &Rc<Self>, f: impl FnOnce(&mut Ui, &mut State) + 'static) {
        let f: Edit = Box::new(f);
        if !self.deferred.borrow().is_empty() {
            self.deferred.borrow_mut().push_back(f);
            self.schedule();
        } else if !self.try_edit(f) {
            self.schedule();
        }
    }

    /// Run `f` now if the window and the state are free; else queue it.
    /// `true` if it ran.
    fn try_edit(&self, f: Edit) -> bool {
        let mut slot = Some(f);
        let ran = self.window.with(|ui| {
            let Ok(mut st) = self.state.try_borrow_mut() else {
                return false;
            };
            let Some(st) = st.as_mut() else {
                // Built but gone (closing): nothing to change.
                slot = None;
                return true;
            };
            if let Some(f) = slot.take() {
                f(ui, st);
            }
            true
        });
        match (ran, slot) {
            (Some(true), _) => true,
            (_, Some(f)) if self.window.is_open() => {
                self.deferred.borrow_mut().push_back(f);
                false
            }
            _ => true,
        }
    }

    /// Read from the window now; `None` while it is busy (callers only
    /// read from the drain, where it never is).
    pub fn read<R>(&self, f: impl FnOnce(&mut Ui, &mut State) -> R) -> Option<R> {
        self.window.with(|ui| {
            let mut st = self.state.try_borrow_mut().ok()?;
            Some(f(ui, st.as_mut()?))
        })?
    }

    /// The toolkit's handler: note what must be noted at once, queue the
    /// rest for the drain.
    fn heard(self: &Rc<Self>, e: Event) {
        match e {
            Event::Frame {
                cpu_ms,
                input_to_screen_ms,
                layers,
                ..
            } => {
                // The first frames, and the early ones that redraw much
                // (the move to the hardware device).
                let n = self.frames.get();
                if n < 6 || (n < 120 && layers >= 5) {
                    tracing::debug!(
                        target: "copper::perf",
                        "frame {}: {layers} layers, {cpu_ms:.1} ms CPU, at {:.1} ms from process start",
                        n + 1,
                        perf::since_start_ms()
                    );
                }
                if n < 120 {
                    self.frames.set(n + 1);
                }
                if let Some((what, since)) = self.watch.take() {
                    let ms = since.elapsed().as_secs_f64() * 1000.0;
                    match input_to_screen_ms {
                        Some(screen) => tracing::info!(
                            target: "copper::perf",
                            "{what}: {ms:.1} ms to its frame ({cpu_ms:.1} ms CPU), input to screen {screen:.1} ms"
                        ),
                        None => tracing::info!(
                            target: "copper::perf",
                            "{what}: {ms:.1} ms to its frame ({cpu_ms:.1} ms CPU)"
                        ),
                    }
                }
                return;
            }
            Event::FirstFrame {
                since_process_ms, ..
            } => {
                perf::mark_at(perf::FIRST_FRAME, f64::from(since_process_ms));
                return;
            }
            Event::Appearance { dark } => self.dark.set(dark),
            Event::FieldChanged(_) => self.watch("key to frame"),
            _ => {}
        }
        self.events.borrow_mut().push_back(e);
        self.schedule();
    }

    /// Measure from now to the next frame (logged under `copper::perf`).
    pub fn watch(&self, what: &'static str) {
        if self.watch.get().is_none() {
            self.watch.set(Some((what, Instant::now())));
        }
    }

    /// Drain on the loop's next turn (once, however often it is asked).
    fn schedule(self: &Rc<Self>) {
        if self.scheduled.replace(true) {
            return;
        }
        let me = Rc::downgrade(self);
        copper_win_rt::spawn_local(async move {
            if let Some(c) = me.upgrade() {
                c.scheduled.set(false);
                c.drain();
            }
        });
    }

    /// Run the deferred edits, then the queued events.
    fn drain(self: &Rc<Self>) {
        loop {
            let Some(f) = self.deferred.borrow_mut().pop_front() else {
                break;
            };
            if !self.try_edit_front(f) {
                // Still busy: try again next turn, keeping the order.
                self.schedule();
                return;
            }
        }
        loop {
            let Some(e) = self.events.borrow_mut().pop_front() else {
                break;
            };
            events::handle(self, e);
        }
    }

    /// Like [`Chrome::try_edit`], but a failure goes back to the front.
    fn try_edit_front(&self, f: Edit) -> bool {
        let before = self.deferred.borrow().len();
        if self.try_edit(f) {
            return true;
        }
        // `try_edit` queued it at the back; it belongs at the front.
        let mut q = self.deferred.borrow_mut();
        if q.len() > before
            && let Some(f) = q.pop_back()
        {
            q.push_front(f);
        }
        false
    }
}
