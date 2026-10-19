//! A page's file chooser (`<input type=file>`).
//!
//! Every webview intercepts file choosers (CDP
//! `Page.setInterceptFileChooserDialog`), so WebView2 never shows its own:
//! that one could not be closed from here, outlived its document and opened
//! over the user's window for pages nobody sees. Instead:
//!
//! - a page held backstage (an agent's) is refused: the input hears
//!   `cancel`, and the backstage counts a swallowed popup;
//! - so is a page that is not on screen, or any page while another one has
//!   a chooser open (one at a time in the whole app);
//! - otherwise the system dialog opens over the Copper window
//!   (`win::file_dialog`), honouring `multiple`, `accept` and
//!   `webkitdirectory`. The choice goes to the input with CDP
//!   `DOM.setFileInputFiles` (which fires `input` and `change`); cancelling
//!   fires `cancel`.
//!
//! The chooser closes when the page navigates to a new document, crashes,
//! leaves its window or is dropped, on `cancel_file_chooser`, and on an
//! agent's Escape.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use copper_engine::JsonValue;
use serde_json::json;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use windows::Win32::Foundation::HWND;

use super::PageInner;
use crate::win::cdp;
use crate::win::file_dialog::{self, Dialog, Spec};

thread_local! {
    /// The page whose chooser is open: one at a time in the whole app, so a
    /// second request never stacks a dialog on the first.
    static CHOOSING: RefCell<Weak<PageInner>> = const { RefCell::new(Weak::new()) };
}

/// Reads what the dialog needs from the input (run on the input element).
const READ_INPUT: &str = "function () { return { accept: String(this.accept || ''), \
    multiple: !!this.multiple, folder: !!this.webkitdirectory }; }";
/// What the input does when its chooser is cancelled (HTML: a bubbling
/// `cancel` event).
const FIRE_CANCEL: &str =
    "function () { this.dispatchEvent(new Event('cancel', { bubbles: true })); }";

/// The input's wishes.
#[derive(Debug, Default, PartialEq, Eq)]
struct Wants {
    accept: String,
    multiple: bool,
    folder: bool,
}

/// An open chooser.
struct Open {
    id: u64,
    /// The input (`backendNodeId`).
    node: Option<i64>,
    /// The dialog, once it was asked for.
    dialog: Option<Dialog>,
}

/// `PageInner::chooser`.
#[derive(Default)]
pub(crate) struct PageChooser {
    open: RefCell<Option<Open>>,
    next_id: Cell<u64>,
}

impl PageChooser {
    pub fn is_open(&self) -> bool {
        self.open.borrow().is_some()
    }

    fn is_current(&self, id: u64) -> bool {
        self.open.borrow().as_ref().is_some_and(|o| o.id == id)
    }
}

impl PageInner {
    /// Intercept file choosers on this webview (once, when it is made).
    pub(super) fn intercept_file_choosers(self: &Rc<Self>, view: &ICoreWebView2) {
        let page = Rc::downgrade(self);
        let view = view.clone();
        copper_win_rt::spawn_local(async move {
            for (method, params) in [
                ("Page.enable", json!({})),
                (
                    "Page.setInterceptFileChooserDialog",
                    json!({ "enabled": true }),
                ),
            ] {
                let r = cdp::call(&view, method, params).await;
                // A page that let this webview go meanwhile (closed: the
                // call fails or never answers) has nothing left to set up.
                let current = page
                    .upgrade()
                    .and_then(|p| p.wv())
                    .is_some_and(|wv| wv.view == view);
                if !current {
                    return;
                }
                if let Err(e) = r {
                    tracing::warn!("file choosers: {method}: {e}");
                }
            }
        });
    }

    /// CDP `Page.fileChooserOpened`: refuse it or open the dialog (see the
    /// module docs).
    pub(super) fn file_chooser_opened(self: &Rc<Self>, params: &JsonValue) {
        let node = params.get("backendNodeId").and_then(JsonValue::as_i64);
        let multiple = params.get("mode").and_then(JsonValue::as_str) == Some("selectMultiple");
        if self.backstaged() {
            self.profile.engine.backstage.blocked();
            self.answer_cancel(node);
            return;
        }
        let busy = CHOOSING.with(|c| c.borrow().upgrade().is_some_and(|p| p.chooser.is_open()));
        let owner = self.host.window();
        let Some(owner) = owner.filter(|_| !busy && self.shown()) else {
            tracing::debug!(busy, shown = self.shown(), "file chooser refused");
            self.answer_cancel(node);
            return;
        };
        let id = self.chooser.next_id.get() + 1;
        self.chooser.next_id.set(id);
        *self.chooser.open.borrow_mut() = Some(Open {
            id,
            node,
            dialog: None,
        });
        CHOOSING.with(|c| *c.borrow_mut() = Rc::downgrade(self));
        let me = Rc::downgrade(self);
        copper_win_rt::spawn_local(choose(me, id, node, multiple, owner));
    }

    /// Close the chooser, if one is open; `notify`: the page hears `cancel`
    /// (not when its document is going anyway). `true` if one was open.
    pub(crate) fn close_file_chooser(&self, notify: bool) -> bool {
        let Some(open) = self.chooser.open.borrow_mut().take() else {
            return false;
        };
        CHOOSING.with(|c| {
            let mut c = c.borrow_mut();
            if std::ptr::eq(c.as_ptr(), self) {
                *c = Weak::new();
            }
        });
        if let Some(dialog) = &open.dialog {
            dialog.close();
        }
        if notify {
            self.answer_cancel(open.node);
        }
        true
    }

    /// The chooser `id` ended with `paths`: hand them to the input (or
    /// tell it nothing was chosen). Nothing if it was closed meanwhile.
    fn chosen(&self, id: u64, paths: Vec<PathBuf>) {
        if !self.chooser.is_current(id) {
            return;
        }
        let open = self.chooser.open.borrow_mut().take();
        CHOOSING.with(|c| *c.borrow_mut() = Weak::new());
        let Some(open) = open else { return };
        if paths.is_empty() {
            self.answer_cancel(open.node);
            return;
        }
        let (Some(wv), Some(node)) = (self.wv(), open.node) else {
            return;
        };
        let files: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        copper_win_rt::spawn_local(async move {
            let params = json!({ "files": files, "backendNodeId": node });
            if let Err(e) = cdp::call(&wv.view, "DOM.setFileInputFiles", params).await {
                tracing::warn!("file chooser: handing over the files: {e}");
            }
        });
    }

    /// Tell input `node` its chooser was cancelled.
    fn answer_cancel(&self, node: Option<i64>) {
        let (Some(wv), Some(node)) = (self.wv(), node) else {
            return;
        };
        copper_win_rt::spawn_local(async move {
            if let Err(e) = on_input(&wv.view, node, FIRE_CANCEL).await {
                tracing::debug!("file chooser: cancel: {e}");
            }
        });
    }
}

/// Read what the input wants, show the dialog, hand back the choice. Holds
/// the page only between awaits.
async fn choose(me: Weak<PageInner>, id: u64, node: Option<i64>, multiple: bool, owner: HWND) {
    let view = match me.upgrade().and_then(|p| p.wv()) {
        Some(wv) => wv.view,
        None => return,
    };
    let wants = match node {
        Some(node) => match on_input(&view, node, READ_INPUT).await {
            Ok(v) => wants(&v),
            Err(e) => {
                tracing::debug!("file chooser: reading the input: {e}");
                None
            }
        },
        None => None,
    }
    .unwrap_or(Wants {
        multiple,
        ..Wants::default()
    });
    let Some(page) = me.upgrade() else { return };
    if !page.chooser.is_current(id) {
        return;
    }
    let spec = Spec {
        title: title(&wants).into(),
        multiple: wants.multiple,
        folder: wants.folder,
        types: crate::accept::file_types(&wants.accept),
        owner: owner.0 as isize,
    };
    let (dialog, done) = file_dialog::open(spec);
    if let Some(open) = page.chooser.open.borrow_mut().as_mut() {
        open.dialog = Some(dialog);
    }
    drop(page);
    let paths = done.await;
    if let Some(page) = me.upgrade() {
        page.chosen(id, paths);
    }
}

/// The dialog's title, as the other platforms word it.
fn title(wants: &Wants) -> &'static str {
    if wants.folder {
        "Choose a Folder"
    } else if wants.multiple {
        "Choose Files"
    } else {
        "Choose a File"
    }
}

/// [`READ_INPUT`]'s answer.
fn wants(v: &JsonValue) -> Option<Wants> {
    let v = v.get("result")?.get("value")?;
    Some(Wants {
        accept: v.get("accept")?.as_str()?.to_owned(),
        multiple: v.get("multiple")?.as_bool()?,
        folder: v.get("folder")?.as_bool()?,
    })
}

/// Run `function` on input `node` (in the page's world), its value
/// returned; the handle is released after.
async fn on_input(
    view: &ICoreWebView2,
    node: i64,
    function: &str,
) -> copper_engine::Result<JsonValue> {
    let resolved = cdp::call(
        view,
        "DOM.resolveNode",
        json!({ "backendNodeId": node, "objectGroup": "copper-chooser" }),
    )
    .await?;
    let object = resolved
        .get("object")
        .and_then(|o| o.get("objectId"))
        .and_then(JsonValue::as_str)
        .ok_or_else(|| copper_engine::EngineError::Other("the input is gone".into()))?
        .to_owned();
    let r = cdp::call(
        view,
        "Runtime.callFunctionOn",
        json!({ "objectId": object, "functionDeclaration": function, "returnByValue": true }),
    )
    .await;
    let _ = cdp::call(
        view,
        "Runtime.releaseObjectGroup",
        json!({ "objectGroup": "copper-chooser" }),
    )
    .await;
    r
}
