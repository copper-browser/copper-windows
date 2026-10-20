//! Canvas on Windows (Mac `Fork/Canvas`): the `copper://canvas` page and
//! its bridge are copper-shell-core's `canvas`; this is the native half
//! around it —
//!
//! * the Canvas door at the column's foot and its card ([`door`]): the
//!   list, new, rename, invite, members, delete, and the Share card
//!   ([`share`]);
//! * the invite and mention pills ([`pills`]) and the mention notice when
//!   Copper isn't in front ([`notify`]), fed by [`inbox`];
//! * the canvas tab's menu items and in-row rename ([`rename`]);
//! * the page, its tab and join links ([`serve`]);
//! * the commands: New Canvas (`Ctrl+Shift+E`), Personal Canvas
//!   (`Ctrl+Shift+O`), and the palette's Open / Join / Share / New Shared.
//!
//! [`copy`] (every word it says) builds and tests everywhere.

pub mod copy;

#[cfg(windows)]
pub mod door;
#[cfg(windows)]
mod inbox;
#[cfg(windows)]
mod look;
#[cfg(windows)]
mod notify;
#[cfg(windows)]
mod ops;
#[cfg(windows)]
mod pills;
#[cfg(windows)]
mod rename;
#[cfg(windows)]
mod serve;
#[cfg(windows)]
mod share;
#[cfg(windows)]
mod state;

#[cfg(windows)]
pub use self::windows::*;

/// The address label of a canvas tab: "Canvas · <name>".
pub fn address_label(url: &str) -> Option<String> {
    let id = copper_shell_core::canvas::id_from_url(url)?;
    Some(match copper_shell_core::canvas::registry::entry(&id) {
        Some(e) => copy::address_label(&e.name),
        None => "Canvas".to_string(),
    })
}

#[cfg(windows)]
mod windows {
    use super::state::{self, Act, FieldKind, Mode};
    use super::{copy, door, inbox, ops, pills, rename, serve, share};
    use crate::chrome::glyph;
    use crate::commands::Command;
    use crate::platform::{Shell, Win};
    use copper_core::canvas::PERSONAL_CANVAS_ID;
    use copper_shell_core::OpenTab;
    use copper_shell_core::canvas::{self as core, JoinLink, registry};
    use copper_win_ui::menu::{Item, Menu};
    use copper_win_ui::{Event, NodeId, Point};
    use std::cell::RefCell;

    /// The card's row menu.
    const ROW_MENU: u32 = 0x6361_FFFF;
    /// Our items, in the tab menu and the row menu.
    mod item {
        pub const RENAME: u32 = 0x6361_0001;
        pub const SHARE: u32 = 0x6361_0002;
        pub const DELETE: u32 = 0x6361_0003;
        pub const OPEN: u32 = 0x6361_0004;
        pub const OPEN_BEHIND: u32 = 0x6361_0005;
        pub const COPY_LINK: u32 = 0x6361_0006;
        pub const MEMBERS: u32 = 0x6361_0007;
        pub const INVITE: u32 = 0x6361_0008;
    }

    thread_local! {
        /// The canvas the row menu is about.
        static MENU_CANVAS: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    /// Turn canvas on: copper-shell-core's lane, then the Windows half.
    pub fn register(shell: &Shell) {
        core::start::<Win>(shell);
        serve::start(shell);
        inbox::start(shell);
        let r = shell.view().commands();
        r.add(Command::new(
            "new-canvas",
            "New Canvas",
            glyph::CANVAS,
            new_local,
        ));
        r.add(Command::new(
            "personal-canvas",
            "Personal Canvas",
            glyph::PERSON,
            |s| {
                core::show::<Win>(s, PERSONAL_CANVAS_ID, true);
            },
        ));
        r.add(
            Command::new("share-canvas", "Share Canvas", glyph::PEOPLE, |s| {
                if let Some(id) = core::active::<Win>(s) {
                    share::show(s, &id);
                }
            })
            .when(|s| {
                core::active::<Win>(s)
                    .and_then(|id| registry::entry(&id))
                    .is_some_and(|e| !registry::is_personal(&e))
            }),
        );
        r.add(
            Command::new(
                "new-shared-canvas",
                "New Shared Canvas…",
                glyph::PEOPLE,
                |s| door::open(s, Mode::New),
            )
            .when(|_| registry::cloud_ready()),
        );
        refresh_all(shell);
    }

    /// New Canvas: a canvas on this computer named "Untitled canvas", in
    /// front — in the tab in front when that one is blank.
    fn new_local(shell: &Shell) {
        let e = registry::create_local(copy::UNTITLED);
        core::recount::<Win>();
        let blank = shell.active_id().filter(|id| {
            shell.browser().tab(id).is_some_and(|t| {
                !t.private && (t.url.is_empty() || t.url.starts_with("about:blank"))
            })
        });
        match blank {
            Some(tab) => {
                shell.navigate(&tab, &core::url_for(&e.id));
                shell.set_saved(&tab, true);
            }
            None => {
                core::show::<Win>(shell, &e.id, true);
            }
        }
        refresh_all(shell);
    }

    /// Bring the window forward.
    pub fn raise(shell: &Shell) {
        crate::sys::window::bring_forward(shell.view().chrome().window().hwnd());
    }

    /// Something the door, the card, the pills or the palette show
    /// changed: draw them again.
    pub fn refresh_all(shell: &Shell) {
        door::refresh_door(shell);
        door::render(shell);
        palette_commands(shell);
    }

    /// Open Canvas · <name> per canvas, Join Canvas · <name> per invite;
    /// ones no longer there stay registered but are never offered.
    fn palette_commands(shell: &Shell) {
        let r = shell.view().commands();
        for e in registry::visible() {
            let id = format!("open-canvas:{}", e.id);
            let canvas = e.id.clone();
            let name = if registry::is_personal(&e) {
                "Open Canvas · Personal".to_string()
            } else {
                format!("Open Canvas · {}", e.name)
            };
            let glyph = if registry::is_shared(&e) {
                glyph::PEOPLE
            } else {
                glyph::CANVAS
            };
            let shown = canvas.clone();
            r.add(
                Command::new(&id, &name, glyph, move |s| {
                    core::show::<Win>(s, &canvas, true);
                })
                .when(move |_| {
                    let me = copper_shell_core::cloud::get()
                        .and_then(|c| c.client.account())
                        .map(|a| a.user_id.to_ascii_lowercase());
                    registry::entry(&shown).is_some_and(|e| {
                        !registry::is_shared(&e) || (me.is_some() && e.account == me)
                    })
                }),
            );
        }
        for i in inbox::invites() {
            let id = format!("join-canvas:{}", i.id);
            let name = format!(
                "Join Canvas · {}",
                i.canvas_name.clone().unwrap_or_else(|| "a canvas".into())
            );
            let invite = i.id.clone();
            let waiting = i.id.clone();
            r.add(
                Command::new(&id, &name, glyph::PEOPLE, move |s| {
                    ops::answer(s, &invite, true);
                })
                .when(move |_| inbox::invites().iter().any(|x| x.id == waiting)),
            );
        }
    }

    // MARK: hooks the chrome calls

    /// The card is up, or the rename field: Escape is the chrome's.
    pub fn overlay_open() -> bool {
        state::is_open() || rename::is_open()
    }

    /// Escape: the rename field, then the card. `true` if it was ours.
    pub fn escape(shell: &Shell) -> bool {
        rename::close(shell) || door::escape(shell)
    }

    /// A click the chrome did not take. `true` if it was ours.
    pub fn click(shell: &Shell, id: NodeId) -> bool {
        if pills::click(shell, id) || rename::click(shell, id) {
            return true;
        }
        if !state::is_open() {
            return false;
        }
        let (act, scrim) = state::with(|c| (c.hits.get(&id).cloned(), c.scrim));
        let Some(act) = act else {
            // Elsewhere in the card: nothing, but ours.
            return scrim.is_some_and(|s| {
                shell
                    .view()
                    .chrome()
                    .read(move |ui, _| ui.tree.is_within(id, s))
                    .unwrap_or(false)
            });
        };
        card_act(shell, act);
        true
    }

    fn card_act(shell: &Shell, act: Act) {
        match act {
            Act::Scrim => door::close(shell),
            Act::Back | Act::Cancel => {
                if state::with(|c| c.mode == Mode::List) {
                    door::close(shell);
                } else {
                    door::set_mode(shell, Mode::List);
                }
            }
            Act::Open(id) => {
                door::close(shell);
                core::show::<Win>(shell, &id, true);
            }
            Act::NewMode => door::set_mode(shell, Mode::New),
            Act::Settings => {
                door::close(shell);
                crate::settings::open(shell, "cloud");
            }
            Act::Join(i) => ops::answer(shell, &i, true),
            Act::Decline(i) => ops::answer(shell, &i, false),
            Act::Submit | Act::Confirm => ops::submit(shell),
            Act::Field(_) => {}
            other => {
                share::act(shell, &other);
            }
        }
    }

    /// A field event the chrome did not take. `true` if it was ours.
    pub fn field_event(shell: &Shell, id: NodeId, e: &Event) -> bool {
        if rename::field_event(shell, id, e) {
            return true;
        }
        let Some(kind) = state::with(|c| c.fields.get(&id).copied()) else {
            return false;
        };
        match e {
            Event::FieldChanged(_) => {
                let text = shell
                    .view()
                    .chrome()
                    .read(move |ui, _| ui.field_text(id).map(String::from))
                    .flatten()
                    .unwrap_or_default();
                state::with(|c| {
                    c.texts.insert(kind, text);
                    c.failure = None;
                });
                if kind == FieldKind::Query {
                    share::query_changed(shell);
                } else {
                    shell.view().chrome().edit(|ui, _| {
                        state::CARD.with(|c| door::sync_submit(ui, &c.borrow()));
                    });
                }
            }
            Event::FieldSubmit(_) => ops::submit(shell),
            Event::FieldCancel(_) => {
                door::escape(shell);
            }
            _ => {}
        }
        true
    }

    fn facts(id: &str) -> Option<(bool, bool, bool, bool)> {
        let e = registry::entry(id)?;
        let shared = registry::is_shared(&e);
        Some((
            registry::is_personal(&e),
            shared,
            e.is_owner(),
            !shared || registry::cloud_ready(),
        ))
    }

    /// The canvas items atop a canvas tab's menu (`CanvasTabMenu`): Rename,
    /// Share, Delete or Leave. None for Personal or another page.
    pub fn tab_menu_items(_shell: &Shell, tab: &copper_core::browser::Tab) -> Vec<Item> {
        if tab.private {
            return vec![];
        }
        let Some(id) = core::id_from_url(&tab.url) else {
            return vec![];
        };
        let Some((personal, shared, owner, reachable)) = facts(&id) else {
            return vec![];
        };
        if personal {
            return vec![];
        }
        let mut m = Menu::new();
        if owner {
            m = m.item(item::RENAME, "&Rename Canvas…");
            if !reachable {
                m = m.disabled();
            }
        }
        m = m.item(item::SHARE, "&Share…");
        m = m.item(
            item::DELETE,
            if shared && !owner {
                "&Leave Canvas…"
            } else {
                "De&lete Canvas…"
            },
        );
        if !reachable {
            m = m.disabled();
        }
        m.separator().items
    }

    /// The menu of a row in the card (right-click, Shift+F10, the menu
    /// key). `true` if `target` was one.
    pub fn context_menu(shell: &Shell, target: Option<NodeId>, at: Point) -> bool {
        let Some(t) = target else { return false };
        if !state::is_open() {
            return false;
        }
        let rows = state::with(|c| c.rows.clone());
        let canvas = shell
            .view()
            .chrome()
            .read(move |ui, _| {
                let mut cur = Some(t);
                while let Some(n) = cur {
                    if let Some(id) = rows.get(&n) {
                        return Some(id.clone());
                    }
                    cur = ui.tree.parent(n);
                }
                None
            })
            .flatten();
        let Some(canvas) = canvas else {
            return false;
        };
        let Some((personal, shared, owner, reachable)) = facts(&canvas) else {
            return true;
        };
        let mut m = Menu::new()
            .item(item::OPEN, "&Open")
            .item(item::OPEN_BEHIND, "Open in &Background")
            .item(item::COPY_LINK, "Copy &Link");
        if !personal {
            m = m.separator();
            if owner {
                m = m.item(item::RENAME, "&Rename…");
                if !reachable {
                    m = m.disabled();
                }
            }
            m = m.item(item::SHARE, "&Share…");
            if shared {
                if owner {
                    m = m.item(item::INVITE, "&Invite…");
                    if !reachable {
                        m = m.disabled();
                    }
                }
                m = m.item(item::MEMBERS, "&Members…");
                if !reachable {
                    m = m.disabled();
                }
            }
            m = m.separator().item(
                item::DELETE,
                if shared && !owner {
                    "&Leave…"
                } else {
                    "&Delete…"
                },
            );
            if !reachable {
                m = m.disabled();
            }
        }
        MENU_CANVAS.with(|c| *c.borrow_mut() = Some(canvas));
        shell
            .view()
            .chrome()
            .edit(move |ui, _| ui.show_menu(ROW_MENU, &m, at));
        true
    }

    /// A menu answered. `true` if it was one of ours.
    pub fn menu_chosen(shell: &Shell, menu: u32, chosen: u32) -> bool {
        if menu == ROW_MENU {
            let Some(id) = MENU_CANVAS.with(|c| c.borrow_mut().take()) else {
                return true;
            };
            match chosen {
                item::OPEN => {
                    door::close(shell);
                    core::show::<Win>(shell, &id, true);
                }
                item::OPEN_BEHIND => {
                    core::show::<Win>(shell, &id, false);
                    door::render(shell);
                }
                item::COPY_LINK => {
                    shell.view().copy_text(&core::url_for(&id));
                    shell.toast("Link copied");
                }
                item::RENAME => door::set_mode(shell, Mode::Rename(id)),
                item::SHARE => door::set_mode(shell, Mode::Share(id)),
                item::INVITE => door::set_mode(shell, Mode::Invite(id)),
                item::MEMBERS => door::set_mode(shell, Mode::Members(id)),
                item::DELETE => door::set_mode(shell, Mode::Delete(id)),
                _ => {}
            }
            return true;
        }
        if menu != crate::chrome::menus::TAB_MENU || chosen & 0xFFFF_0000 != 0x6361_0000 {
            return false;
        }
        let Some(Some(tab)) = shell.view().chrome().read(|_, st| st.menus.tab.take()) else {
            return true;
        };
        let Some(id) = shell
            .browser()
            .tab(&tab)
            .and_then(|t| core::id_from_url(&t.url))
        else {
            return true;
        };
        match chosen {
            item::RENAME => rename::start(shell, &tab, &id),
            item::SHARE => share::show(shell, &id),
            item::DELETE => {
                if state::is_open() {
                    door::set_mode(shell, Mode::Delete(id));
                } else {
                    door::open(shell, Mode::Delete(id));
                }
            }
            _ => {}
        }
        true
    }

    /// The palette is going to `url` (`in_place`: in the tab in front).
    /// Canvases and join links are ours: `true` if taken.
    pub fn go(shell: &Shell, url: &str, in_place: bool) -> bool {
        if let Some(link) = JoinLink::parse(url)
            && (link.copper || serve::same_cloud_web_link(url).is_some())
        {
            let s = shell.clone();
            copper_win_rt::spawn_local(async move { serve::join(&s, link, None).await });
            return true;
        }
        let active = shell.active_id();
        let front = active
            .as_ref()
            .and_then(|a| shell.browser().tab(a).cloned());
        if let Some(id) = core::id_from_url(url) {
            if core::tab_showing::<Win>(shell, &id).is_some() {
                core::show::<Win>(shell, &id, true);
                return true;
            }
            let blank = front
                .as_ref()
                .filter(|t| !t.private && (t.url.is_empty() || t.url.starts_with("about:blank")));
            if let Some(t) = blank.filter(|_| in_place) {
                // The blank tab in front becomes the canvas.
                shell.navigate(&t.id, &core::url_for(&id));
                shell.set_saved(&t.id, true);
                return true;
            }
            if !in_place || registry::entry(&id).is_some() {
                core::show::<Win>(shell, &id, true);
                return true;
            }
            // Not a canvas here: its page says so, beside a canvas in front
            // rather than in place of it.
            if let Some(t) = front.filter(|t| core::id_from_url(&t.url).is_some()) {
                shell.open(
                    url,
                    OpenTab {
                        foreground: true,
                        private: t.private,
                        after: Some(t.id.clone()),
                        ..Default::default()
                    },
                );
                return true;
            }
            return false;
        }
        // A web address typed on a canvas opens beside it.
        if in_place && let Some(t) = front.filter(|t| core::id_from_url(&t.url).is_some()) {
            shell.open(
                url,
                OpenTab {
                    foreground: true,
                    private: t.private,
                    after: Some(t.id.clone()),
                    ..Default::default()
                },
            );
            return true;
        }
        false
    }
}
