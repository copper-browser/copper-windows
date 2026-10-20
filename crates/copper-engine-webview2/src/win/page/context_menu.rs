//! App-rendered context menus (`WebView2Page::set_context_menu_handler`):
//! `ContextMenuRequested` read into a [`ContextMenuRequest`], WebView2's
//! own menu held back (`Handled` plus a deferral), and the app's choice
//! sent back through a [`ContextMenuReply`].

use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND, COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND,
    ICoreWebView2ContextMenuItem, ICoreWebView2ContextMenuItemCollection,
    ICoreWebView2ContextMenuRequestedEventArgs, ICoreWebView2Deferral,
};
use windows::Win32::Foundation::POINT;

use super::{PageInner, WebView2Page};
use crate::context_menu::{
    ContextItemKind, ContextKind, ContextMenuItem, ContextMenuRequest, non_empty,
};
use crate::win::com::{get, get_bool, get_string};

/// What the app does with a page's context-menu request.
pub(crate) type ContextMenuHandler = Rc<dyn Fn(ContextMenuRequest, ContextMenuReply)>;

/// Deeper submenus than this are not read (WebView2's go two deep).
const MAX_DEPTH: usize = 8;

/// The answer to one [`ContextMenuRequest`]. One-shot: [`run`] it with the
/// chosen command, or drop it — the menu was dismissed. Until then the
/// page waits for the menu (WebView2 holds the request open). UI thread
/// only.
///
/// [`run`]: ContextMenuReply::run
pub struct ContextMenuReply {
    open: Option<(
        ICoreWebView2ContextMenuRequestedEventArgs,
        ICoreWebView2Deferral,
    )>,
}

impl ContextMenuReply {
    /// Run WebView2's command `command` (a [`ContextMenuItem::command`] of
    /// the request) — `None`: nothing chosen, as when the menu is
    /// dismissed. Commands of the app's own items are the app's to run;
    /// answer those with `None`.
    pub fn run(mut self, command: Option<i32>) {
        if let Some((args, deferral)) = self.open.take() {
            if let Some(id) = command
                && let Err(e) = unsafe { args.SetSelectedCommandId(id) }
            {
                tracing::debug!("context menu command {id}: {e}");
            }
            unsafe {
                let _ = deferral.Complete();
            }
        }
    }
}

impl Drop for ContextMenuReply {
    fn drop(&mut self) {
        if let Some((_, deferral)) = self.open.take() {
            unsafe {
                let _ = deferral.Complete();
            }
        }
    }
}

impl std::fmt::Debug for ContextMenuReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextMenuReply")
            .field("open", &self.open.is_some())
            .finish()
    }
}

impl WebView2Page {
    /// Render this page's context menus in the app. With a handler set, a
    /// right click, the Menu key or Shift+F10 on the page (not while the
    /// backstage holds it) calls `f` — from inside WebView2's event, so the
    /// app should show its menu on a later turn — with what was clicked and
    /// WebView2's own menu as data, instead of WebView2 showing that menu;
    /// the page waits until the [`ContextMenuReply`] is run or dropped.
    /// `None` (the default) lets WebView2 show its menu. Survives the
    /// page's webview being rebuilt.
    pub fn set_context_menu_handler(
        &self,
        f: Option<Box<dyn Fn(ContextMenuRequest, ContextMenuReply)>>,
    ) {
        *self.inner.context_menu.borrow_mut() = f.map(Rc::from);
    }
}

impl PageInner {
    /// `ContextMenuRequested` on a page that is not backstaged: the app's
    /// menu if it has a handler; otherwise WebView2's own.
    pub(super) fn context_menu_requested(
        &self,
        args: &ICoreWebView2ContextMenuRequestedEventArgs,
    ) -> windows::core::Result<()> {
        // Out of the cell before the app runs: it may replace the handler.
        let Some(handler) = self.context_menu.borrow().clone() else {
            return Ok(());
        };
        // Unreadable: WebView2 shows its own menu rather than none.
        let request = match read_request(args) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("context menu: {e}");
                return Ok(());
            }
        };
        let deferral = unsafe {
            args.SetHandled(true)?;
            args.GetDeferral()?
        };
        let reply = ContextMenuReply {
            open: Some((args.clone(), deferral)),
        };
        handler(request, reply);
        Ok(())
    }
}

fn read_request(
    args: &ICoreWebView2ContextMenuRequestedEventArgs,
) -> windows::core::Result<ContextMenuRequest> {
    unsafe {
        let target = args.ContextMenuTarget()?;
        let kind = get(|k: *mut COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND| target.Kind(k))?;
        // Runtime 155 answers `Has*` false (and `IsRequestedForMainFrame`
        // false) whatever was clicked, so the values themselves decide.
        let text = |value: windows::core::Result<String>| value.ok().and_then(non_empty);
        let link_url = text(get_string(|s| target.LinkUri(s)));
        let link_text = text(get_string(|s| target.LinkText(s)));
        let source_url = text(get_string(|s| target.SourceUri(s)));
        let selection = text(get_string(|s| target.SelectionText(s)));
        let page_url = get_string(|s| target.PageUri(s)).unwrap_or_default();
        let frame = text(get_string(|s| target.FrameUri(s)));
        let main = get_bool(|b| target.IsRequestedForMainFrame(b)).unwrap_or(false)
            || frame.as_ref().is_none_or(|f| *f == page_url);
        let point = get(|p: *mut POINT| args.Location(p))?;
        Ok(ContextMenuRequest {
            kind: ContextKind::from_webview2(kind.0),
            link_url,
            link_text,
            source_url,
            selection,
            editable: get_bool(|b| target.IsEditable(b)).unwrap_or(false),
            in_frame: !main,
            page_url,
            frame_url: if main { None } else { frame },
            point: (point.x, point.y),
            items: read_items(&args.MenuItems()?, 0),
        })
    }
}

/// The items of `menu`, in order; an item that cannot be read is left out.
fn read_items(menu: &ICoreWebView2ContextMenuItemCollection, depth: usize) -> Vec<ContextMenuItem> {
    let count = get(|n: *mut u32| unsafe { menu.Count(n) }).unwrap_or(0);
    (0..count)
        .filter_map(|i| unsafe { menu.GetValueAtIndex(i) }.ok())
        .filter_map(|item| read_item(&item, depth).ok())
        .collect()
}

fn read_item(
    item: &ICoreWebView2ContextMenuItem,
    depth: usize,
) -> windows::core::Result<ContextMenuItem> {
    unsafe {
        let kind = ContextItemKind::from_webview2(
            get(|k: *mut COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND| item.Kind(k))?.0,
        );
        let children = match (kind, depth < MAX_DEPTH) {
            (ContextItemKind::Submenu, true) => item
                .Children()
                .map(|c| read_items(&c, depth + 1))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        Ok(ContextMenuItem {
            command: get(|c: *mut i32| item.CommandId(c))?,
            name: get_string(|s| item.Name(s)).unwrap_or_default(),
            label: get_string(|s| item.Label(s)).unwrap_or_default(),
            shortcut: get_string(|s| item.ShortcutKeyDescription(s))
                .ok()
                .and_then(non_empty),
            kind,
            enabled: get_bool(|b| item.IsEnabled(b)).unwrap_or(true),
            checked: get_bool(|b| item.IsChecked(b)).unwrap_or(false),
            children,
        })
    }
}
