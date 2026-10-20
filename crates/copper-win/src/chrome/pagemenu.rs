//! The page's right-click menu, drawn in Copper's own menu
//! (`ImageMenu.swift`, `Browser.swift` 2222–2340; spec §8), with
//! WebView2's menu kept as data (`WebView2Page::set_context_menu_handler`).
//!
//! - **An image** (`<img>` with a source): exactly the Mac's — Open Image in
//!   New Tab · Copy Image, Download Image · Copy Image Address — and, when
//!   the image is a link, the link's four after it.
//! - **A link**: Open Link in New Tab, Open Link in Background · Copy Link
//!   Address, Download Linked File, then whatever else WebView2 offers
//!   there (selection, spelling, Inspect).
//! - **The page**: Back, Forward, Reload · Copy Page Address, then
//!   WebView2's own (Save as, Print, Inspect…).
//! - **Selected text, a field, audio, video**: WebView2's own, as it has
//!   them.
//!
//! WebView2's items run through the reply (`ContextMenuReply::run`), so its
//! copy, paste, spelling and Inspect behave as they do in Edge; Copy Image
//! is its `copyImage` (the decoded picture, not the address). Opening and
//! downloading go through the shell and the engine, so a private tab's
//! links open private and downloads take the Save As question when it is
//! on. Submenus are not shown (Copper's menus are one level).

use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_engine::PageView;
use copper_engine_webview2::{
    ContextItemKind, ContextKind, ContextMenuItem, ContextMenuReply, ContextMenuRequest,
    WebView2Page,
};
use copper_shell_core::OpenTab;
use copper_win_ui::Menu;
use copper_win_ui::geom::Point;

/// The page menu's tag (the tab menu is 1).
pub const PAGE_MENU: u32 = 2;

/// Copper's own items; WebView2's are `NATIVE + index`.
mod item {
    pub const OPEN_LINK: u32 = 1;
    pub const OPEN_LINK_BACKGROUND: u32 = 2;
    pub const COPY_LINK: u32 = 3;
    pub const DOWNLOAD_LINK: u32 = 4;
    pub const OPEN_IMAGE: u32 = 5;
    pub const COPY_IMAGE: u32 = 6;
    pub const DOWNLOAD_IMAGE: u32 = 7;
    pub const COPY_IMAGE_ADDRESS: u32 = 8;
    pub const BACK: u32 = 9;
    pub const FORWARD: u32 = 10;
    pub const RELOAD: u32 = 11;
    pub const COPY_PAGE_ADDRESS: u32 = 12;
    pub const NATIVE: u32 = 1000;
}

/// WebView2's items Copper draws in its own words (or leaves out).
const OWN: &[&str] = &[
    "openLinkInNewWindow",
    "copyLinkLocation",
    "saveLinkAs",
    "openImageInNewWindow",
    "copyImage",
    "saveImageAs",
    "copyImageLocation",
    "back",
    "forward",
    "reload",
];

/// The open page menu: whose it is, what it was about, and WebView2's
/// waiting answer.
pub struct Pending {
    tab: TabId,
    link: Option<String>,
    source: Option<String>,
    page_url: String,
    /// WebView2's items by their place in the menu, and their commands.
    natives: Vec<(u32, i32)>,
    /// WebView2's `copyImage`, `back`, `forward`, `reload`.
    named: Vec<(&'static str, i32)>,
    reply: ContextMenuReply,
}

/// Show Copper's menu for `page`'s right clicks (called once per page).
pub fn watch(shell: &Shell, tab: &TabId, page: &WebView2Page) {
    let weak = shell.downgrade();
    let tab = tab.clone();
    page.set_context_menu_handler(Some(Box::new(move |req, reply| {
        // Called from inside WebView2's event: the menu's own loop starts
        // on a later turn.
        let weak = weak.clone();
        let tab = tab.clone();
        copper_win_rt::spawn_local(async move {
            if let Some(shell) = weak.upgrade() {
                show(&shell, &tab, req, reply);
            }
        });
    })));
}

/// One line of the menu being put together.
enum Line {
    Item {
        id: u32,
        label: String,
        shortcut: Option<String>,
        enabled: bool,
        checked: bool,
    },
    Rule,
}

fn own(id: u32, label: &str) -> Line {
    Line::Item {
        id,
        label: label.to_string(),
        shortcut: None,
        enabled: true,
        checked: false,
    }
}

/// A real picture: an image with a source that WebView2 can copy (it
/// cannot copy a broken or unloaded one, which keeps WebView2's menu).
fn is_image(req: &ContextMenuRequest) -> bool {
    req.kind == ContextKind::Image
        && req.source_url.as_deref().is_some_and(|s| !s.is_empty())
        && req.items.iter().any(|i| i.name == "copyImage" && i.enabled)
}

/// The lines for `req`; WebView2's are numbered into `natives`. `history`
/// is whether the page can go back and forward, for when WebView2 leaves
/// those items out (it lists Forward only when there is somewhere to go).
fn lines(
    req: &ContextMenuRequest,
    history: (bool, bool),
    natives: &mut Vec<(u32, i32)>,
) -> Vec<Line> {
    let mut out = Vec::new();
    let named = |n: &str| req.items.iter().find(|i| i.name == n);
    let image = is_image(req);
    let link = req.link_url.is_some();
    if image {
        out.push(own(item::OPEN_IMAGE, "Open Image in New Tab"));
        out.push(Line::Rule);
        out.push(own(item::COPY_IMAGE, "Copy Image"));
        out.push(own(item::DOWNLOAD_IMAGE, "Download Image"));
        out.push(Line::Rule);
        out.push(own(item::COPY_IMAGE_ADDRESS, "Copy Image Address"));
    }
    if link {
        if image {
            out.push(Line::Rule);
        }
        out.push(own(item::OPEN_LINK, "Open Link in New Tab"));
        out.push(own(item::OPEN_LINK_BACKGROUND, "Open Link in Background"));
        out.push(Line::Rule);
        out.push(own(item::COPY_LINK, "Copy Link Address"));
        out.push(own(item::DOWNLOAD_LINK, "Download Linked File"));
    }
    if image {
        return out;
    }
    let plain_page = req.kind == ContextKind::Page && !link && !req.editable;
    if plain_page {
        for (id, label, name, can) in [
            (item::BACK, "Back", "back", history.0),
            (item::FORWARD, "Forward", "forward", history.1),
            (item::RELOAD, "Reload", "reload", true),
        ] {
            let mut l = own(id, label);
            if let Line::Item { enabled, .. } = &mut l {
                // Both must agree: WebView2 has offered an enabled
                // Forward with nowhere to go.
                *enabled = can && named(name).is_none_or(|i| i.enabled);
            }
            out.push(l);
        }
        out.push(Line::Rule);
        out.push(own(item::COPY_PAGE_ADDRESS, "Copy Page Address"));
    }
    if link || plain_page {
        out.push(Line::Rule);
    }
    let theirs = req
        .items
        .iter()
        .filter(|i| !(link || plain_page) || !OWN.contains(&i.name.as_str()));
    for i in theirs {
        native(i, &mut out, natives);
    }
    out
}

/// WebView2's item `i` as a line.
fn native(i: &ContextMenuItem, out: &mut Vec<Line>, natives: &mut Vec<(u32, i32)>) {
    match i.kind {
        ContextItemKind::Separator => out.push(Line::Rule),
        // One level only.
        ContextItemKind::Submenu => {}
        ContextItemKind::Command | ContextItemKind::CheckBox | ContextItemKind::Radio => {
            let id = item::NATIVE + natives.len() as u32;
            natives.push((id, i.command));
            let label = if i.name == "inspectElement" {
                "&Inspect Element".to_string()
            } else {
                i.label.clone()
            };
            out.push(Line::Item {
                id,
                label,
                shortcut: i.shortcut.clone(),
                enabled: i.enabled,
                checked: i.checked,
            });
        }
    }
}

/// The lines as a menu: no rule first, last, or twice in a row.
fn menu(lines: Vec<Line>) -> Menu {
    let mut m = Menu::new();
    let mut rule = false;
    let mut any = false;
    for l in lines {
        match l {
            Line::Rule => rule = any,
            Line::Item {
                id,
                label,
                shortcut,
                enabled,
                checked,
            } => {
                if rule {
                    m = m.separator();
                    rule = false;
                }
                m = m.item(id, &label);
                if let Some(s) = shortcut {
                    m = m.shortcut(&s);
                }
                if !enabled {
                    m = m.disabled();
                }
                if checked {
                    m = m.checked(true);
                }
                any = true;
            }
        }
    }
    m
}

fn show(shell: &Shell, tab: &TabId, req: ContextMenuRequest, reply: ContextMenuReply) {
    let mut natives = Vec::new();
    let history = shell.slot(tab).map_or((false, false), |s| {
        (s.page.can_go_back(), s.page.can_go_forward())
    });
    let built = menu(lines(&req, history, &mut natives));
    if built.items.is_empty() {
        return;
    }
    let named = ["copyImage", "back", "forward", "reload"]
        .into_iter()
        .filter_map(|n| {
            req.items
                .iter()
                .find(|i| i.name == n)
                .map(|i| (n, i.command))
        })
        .collect();
    let pending = Pending {
        tab: tab.clone(),
        link: req.link_url.clone(),
        source: req.source_url.clone(),
        page_url: req.page_url.clone(),
        natives,
        named,
        reply,
    };
    let point = req.point;
    let tab = tab.clone();
    shell.view().chrome().edit(move |ui, st| {
        let Some(slot) = st.stage.slot_of(&tab) else {
            return;
        };
        let r = ui.tree.window_rect(slot);
        let s = ui.scale().max(0.01);
        let at = Point::new(r.x + point.0 as f32 / s, r.y + point.1 as f32 / s);
        // A menu still open is dismissed (its reply dropped) by this one.
        st.menus.page = Some(pending);
        ui.show_menu(PAGE_MENU, &built, at);
    });
}

/// The menu closed without a choice.
pub fn dismissed(st: &mut super::State) {
    // Dropping the reply tells WebView2 nothing was chosen.
    st.menus.page = None;
}

/// `chosen` from the page menu.
pub fn chosen(shell: &Shell, chosen: u32) {
    let Some(Some(p)) = shell.view().chrome().read(|_, st| st.menus.page.take()) else {
        return;
    };
    if chosen >= item::NATIVE {
        let command = p
            .natives
            .iter()
            .find(|(id, _)| *id == chosen)
            .map(|(_, c)| *c);
        p.reply.run(command);
        return;
    }
    let named = |n: &str| p.named.iter().find(|(m, _)| *m == n).map(|(_, c)| *c);
    let private = shell.browser().tab(&p.tab).is_some_and(|t| t.private);
    let open = |url: &str, foreground: bool| {
        shell.open(
            url,
            OpenTab {
                foreground,
                private,
                after: Some(p.tab.clone()),
                ..Default::default()
            },
        );
    };
    let page = shell.slot(&p.tab).map(|s| s.page.clone());
    let download = |url: &str| {
        if let Some(page) = &page
            && let Err(e) = page.download(url)
        {
            tracing::warn!("download {url}: {e}");
            shell.toast("Download failed");
        }
    };
    match chosen {
        item::COPY_IMAGE => {
            // WebView2's own: the decoded picture on the clipboard.
            match named("copyImage") {
                Some(c) => {
                    p.reply.run(Some(c));
                    shell.toast("Image copied");
                }
                None => shell.toast("Couldn't copy that image"),
            }
            return;
        }
        item::BACK | item::FORWARD | item::RELOAD => {
            let name = match chosen {
                item::BACK => "back",
                item::FORWARD => "forward",
                _ => "reload",
            };
            match named(name) {
                Some(c) => p.reply.run(Some(c)),
                // WebView2 did not offer it: the page itself.
                None => {
                    p.reply.run(None);
                    match chosen {
                        item::BACK => page.iter().for_each(|pg| pg.back()),
                        item::FORWARD => page.iter().for_each(|pg| pg.forward()),
                        _ => shell.reload(&p.tab),
                    }
                }
            }
            return;
        }
        _ => {}
    }
    // The rest are Copper's: WebView2 is told nothing was chosen first.
    let Pending {
        link,
        source,
        page_url,
        reply,
        ..
    } = p;
    reply.run(None);
    match chosen {
        item::OPEN_LINK => link.iter().for_each(|u| open(u, true)),
        item::OPEN_LINK_BACKGROUND => link.iter().for_each(|u| open(u, false)),
        item::COPY_LINK => {
            if let Some(u) = link {
                shell.view().copy_text(&u);
                shell.toast("Address copied");
            }
        }
        item::DOWNLOAD_LINK => link.iter().for_each(|u| download(u)),
        item::OPEN_IMAGE => source.iter().for_each(|u| open(u, true)),
        item::DOWNLOAD_IMAGE => source.iter().for_each(|u| download(u)),
        item::COPY_IMAGE_ADDRESS => {
            if let Some(u) = source {
                shell.view().copy_text(&u);
                shell.toast("Address copied");
            }
        }
        item::COPY_PAGE_ADDRESS => {
            shell.view().copy_text(&page_url);
            shell.toast("Address copied");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(name: &str, command: i32) -> ContextMenuItem {
        ContextMenuItem {
            command,
            name: name.to_string(),
            label: name.to_string(),
            shortcut: None,
            kind: ContextItemKind::Command,
            enabled: true,
            checked: false,
            children: Vec::new(),
        }
    }

    fn req(
        kind: ContextKind,
        link: bool,
        source: bool,
        items: Vec<ContextMenuItem>,
    ) -> ContextMenuRequest {
        ContextMenuRequest {
            kind,
            link_url: link.then(|| "https://example.com/a".to_string()),
            link_text: None,
            source_url: source.then(|| "https://example.com/i.png".to_string()),
            selection: None,
            editable: false,
            in_frame: false,
            page_url: "https://example.com/".to_string(),
            frame_url: None,
            point: (0, 0),
            items,
        }
    }

    fn labels(m: &Menu) -> Vec<String> {
        m.items
            .iter()
            .map(|i| match i {
                copper_win_ui::menu::Item::Action { label, .. } => label.clone(),
                copper_win_ui::menu::Item::Separator => "-".to_string(),
            })
            .collect()
    }

    #[test]
    fn an_image_has_the_macs_four() {
        let r = req(
            ContextKind::Image,
            false,
            true,
            vec![
                it("copyImage", 7),
                it("saveImageAs", 8),
                it("inspectElement", 9),
            ],
        );
        let m = menu(lines(&r, (false, false), &mut Vec::new()));
        assert_eq!(
            labels(&m),
            [
                "Open Image in New Tab",
                "-",
                "Copy Image",
                "Download Image",
                "-",
                "Copy Image Address"
            ]
        );
    }

    #[test]
    fn a_broken_image_keeps_webview2s_menu() {
        let mut copy = it("copyImage", 7);
        copy.enabled = false;
        let r = req(
            ContextKind::Image,
            false,
            true,
            vec![copy, it("copyImageLocation", 8)],
        );
        let m = menu(lines(&r, (false, false), &mut Vec::new()));
        assert_eq!(labels(&m), ["copyImage", "copyImageLocation"]);
    }

    #[test]
    fn a_linked_image_has_both() {
        let r = req(
            ContextKind::Image,
            true,
            true,
            vec![it("copyImage", 7), it("saveLinkAs", 8)],
        );
        let m = menu(lines(&r, (false, false), &mut Vec::new()));
        assert_eq!(
            labels(&m),
            [
                "Open Image in New Tab",
                "-",
                "Copy Image",
                "Download Image",
                "-",
                "Copy Image Address",
                "-",
                "Open Link in New Tab",
                "Open Link in Background",
                "-",
                "Copy Link Address",
                "Download Linked File"
            ]
        );
    }

    #[test]
    fn a_link_keeps_webview2s_rest() {
        let r = req(
            ContextKind::Page,
            true,
            false,
            vec![
                it("openLinkInNewWindow", 1),
                it("copyLinkLocation", 2),
                it("other", 0),
                it("inspectElement", 9),
            ],
        );
        let mut natives = Vec::new();
        let m = menu(lines(&r, (false, false), &mut natives));
        assert_eq!(
            labels(&m),
            [
                "Open Link in New Tab",
                "Open Link in Background",
                "-",
                "Copy Link Address",
                "Download Linked File",
                "-",
                "other",
                "&Inspect Element"
            ]
        );
        assert_eq!(natives, [(item::NATIVE, 0), (item::NATIVE + 1, 9)]);
    }

    #[test]
    fn the_page_leads_with_history() {
        let mut back = it("back", 3);
        back.enabled = false;
        let r = req(
            ContextKind::Page,
            false,
            false,
            vec![back, it("reload", 4), it("print", 5)],
        );
        let m = menu(lines(&r, (false, false), &mut Vec::new()));
        assert_eq!(
            labels(&m),
            [
                "Back",
                "Forward",
                "Reload",
                "-",
                "Copy Page Address",
                "-",
                "print"
            ]
        );
        assert!(matches!(
            &m.items[0],
            copper_win_ui::menu::Item::Action { enabled: false, .. }
        ));
        // Not listed by WebView2: Forward follows the page's history.
        assert!(matches!(
            &m.items[1],
            copper_win_ui::menu::Item::Action { enabled: false, .. }
        ));
        let m = menu(lines(&r, (false, true), &mut Vec::new()));
        assert!(matches!(
            &m.items[1],
            copper_win_ui::menu::Item::Action { enabled: true, .. }
        ));
    }
}
