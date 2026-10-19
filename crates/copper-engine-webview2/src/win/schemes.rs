//! The engine's custom-scheme handlers (`Engine::register_scheme`).
//!
//! A scheme is registered twice over: with the environment (a
//! `CustomSchemeRegistration`, secure, with an authority — this is what
//! makes `copper:` a secure context and lets WebView2 raise
//! `WebResourceRequested` for it), and as a `WebResourceRequested` filter on
//! every webview (`page::scheme`), whose requests the handler answers
//! synchronously on the UI thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use copper_engine::SchemeHandler;

/// Scheme (lowercase) → handler. A handler is handed out as an `Rc`, so it
/// can run with nothing borrowed (it may register schemes itself).
#[derive(Default)]
pub(crate) struct Schemes {
    handlers: RefCell<HashMap<String, Rc<SchemeHandler>>>,
}

impl Schemes {
    /// Set `scheme`'s handler; `true` if the scheme is new.
    pub fn insert(&self, scheme: &str, handler: SchemeHandler) -> bool {
        let old = self
            .handlers
            .borrow_mut()
            .insert(scheme.to_owned(), Rc::new(handler));
        // The replaced handler (and whatever it captured) goes after the
        // borrow ends.
        let fresh = old.is_none();
        drop(old);
        fresh
    }

    /// The handler for `scheme`, if one is registered.
    pub fn handler(&self, scheme: &str) -> Option<Rc<SchemeHandler>> {
        self.handlers.borrow().get(scheme).cloned()
    }

    /// Every registered scheme.
    pub fn names(&self) -> Vec<String> {
        self.handlers.borrow().keys().cloned().collect()
    }
}
