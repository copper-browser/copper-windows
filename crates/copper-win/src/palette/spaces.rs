//! The Spaces doors around the column (`Fork/SpacesUI.swift`): the header
//! (its mark opens Settings › Spaces, its name the space's menu), the
//! chips at the foot (another space's takes you there, the current one's
//! opens its page), the plus (a new space; right-click for a new canvas),
//! the space menu, and the delete question (`SpaceDelete`).
//!
//! The toolkit's menus have no submenus, so the menu's Colour and Profile
//! open a second menu where the first was.

use super::symbols;
use crate::chrome::Chrome;
use crate::chrome::column::Hit;
use crate::platform::Shell;
use copper_shell_core::spaces::colour::SpaceColour;
use copper_win_ui::{Menu, NodeId, Point};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// The spaces' menus: their own block of ids.
pub const SPACE_MENU: u32 = 0x7370_0001;
pub const COLOUR_MENU: u32 = 0x7370_0002;
pub const PROFILE_MENU: u32 = 0x7370_0003;
pub const PLUS_MENU: u32 = 0x7370_0004;

/// The space menu's items.
mod item {
    pub const EDIT: u32 = 1;
    pub const NEW_TAB: u32 = 2;
    pub const NEW_CANVAS: u32 = 3;
    pub const MOVE_TAB: u32 = 4;
    pub const LEFT: u32 = 5;
    pub const RIGHT: u32 = 6;
    pub const COLOUR: u32 = 7;
    pub const PROFILE: u32 = 8;
    pub const DELETE: u32 = 9;
    /// The plus's menu.
    pub const NEW_SPACE: u32 = 1;
    /// The profile menu: Shared, each name from 1, then New Profile….
    pub const SHARED: u32 = 0;
    pub const NEW_PROFILE: u32 = 999;
}

/// What the delete question's buttons do, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Close,
    Move,
    Cancel,
}

thread_local! {
    /// The space the menu on show is for, and where it opened.
    static MENU: RefCell<Option<(String, Point)>> = const { RefCell::new(None) };
    /// The space whose delete question is up, and its answers.
    static ASKING: RefCell<Option<(String, Vec<Answer>)>> = const { RefCell::new(None) };
    /// The space Settings › Spaces shows; `None` is the one you are in.
    static EDITING: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The profile names the profile menu offered, by item.
    static PROFILES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static NEW_SPACE_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// A click on one of the column's spaces doors (the events keep every
/// other column hit for themselves).
pub fn column_hit(chrome: &Rc<Chrome>, shell: &Shell, h: Hit) {
    match h {
        Hit::Space(id) => {
            // Arc's rule: another space's chip takes you there; the one
            // you are in opens its page.
            if shell.browser().current_space_id() == id {
                edit(shell, &id);
            } else {
                shell.switch_space(&id);
            }
        }
        Hit::SpaceMark => {
            let id = shell.browser().current_space_id().to_string();
            edit(shell, &id);
        }
        Hit::SpaceMenu => {
            let id = shell.browser().current_space_id().to_string();
            let at = chrome
                .read(|ui, st| st.column.header_menu_point(ui))
                .unwrap_or_default();
            show_space_menu(chrome, shell, &id, at);
        }
        Hit::NewSpace => new_space(shell),
        _ => {}
    }
}

/// A right-click: the space menu on the header or a chip, the plus's own.
/// `false`: not ours.
pub fn context_menu(chrome: &Rc<Chrome>, shell: &Shell, target: Option<NodeId>, at: Point) -> bool {
    let Some(t) = target else { return false };
    let Some(hit) = chrome.read(|ui, st| st.column.space_hit(ui, t)).flatten() else {
        return false;
    };
    match hit {
        Hit::Space(id) => show_space_menu(chrome, shell, &id, at),
        Hit::SpaceMenu | Hit::SpaceMark => {
            let id = shell.browser().current_space_id().to_string();
            show_space_menu(chrome, shell, &id, at);
        }
        Hit::NewSpace => {
            let menu = Menu::new()
                .item(item::NEW_SPACE, "New Space")
                .shortcut(&crate::keys::shortcut("new-space").unwrap_or_default())
                .item(item::NEW_CANVAS, "New Canvas")
                .shortcut(&crate::keys::shortcut("new-canvas").unwrap_or_default());
            chrome.edit(move |ui, _| ui.show_menu(PLUS_MENU, &menu, at));
        }
        _ => return false,
    }
    true
}

/// The space menu for `id` (`SpaceMenu`).
fn show_space_menu(chrome: &Rc<Chrome>, shell: &Shell, id: &str, at: Point) {
    let (index, count, current, movable) = {
        let b = shell.browser();
        let index = b.spaces().position(|s| s.id == id).unwrap_or(0);
        let count = b.spaces().count();
        let current = b.current_space_id() == id;
        // A pin is already in every space: it has nowhere to move to.
        let movable = b.active().is_some_and(|t| !t.is_pinned());
        (index, count, current, movable)
    };
    let mut menu = Menu::new()
        .item(item::EDIT, "Edit Space…")
        .item(item::NEW_TAB, "New Tab in Space")
        .item(item::NEW_CANVAS, "New Canvas in Space");
    if movable && !current {
        menu = menu.item(item::MOVE_TAB, "Move Current Tab Here");
    }
    menu = menu.separator().item(item::LEFT, "Move Left");
    if index == 0 {
        menu = menu.disabled();
    }
    menu = menu.item(item::RIGHT, "Move Right");
    if index + 1 >= count {
        menu = menu.disabled();
    }
    menu = menu
        .separator()
        .item(item::COLOUR, "Colour…")
        .item(item::PROFILE, "Profile…");
    if count > 1 {
        menu = menu.separator().item(item::DELETE, "Delete Space…");
    }
    MENU.with(|m| *m.borrow_mut() = Some((id.to_string(), at)));
    chrome.edit(move |ui, _| ui.show_menu(SPACE_MENU, &menu, at));
}

/// A menu item chosen. `false`: not one of ours.
pub fn menu_chosen(shell: &Shell, menu: u32, chosen: u32) -> bool {
    if !matches!(menu, SPACE_MENU | COLOUR_MENU | PROFILE_MENU | PLUS_MENU) {
        return false;
    }
    if menu == PLUS_MENU {
        match chosen {
            item::NEW_SPACE => new_space(shell),
            item::NEW_CANVAS => {
                crate::commands::run(shell, "new-canvas");
            }
            _ => {}
        }
        return true;
    }
    let Some((id, at)) = MENU.with(|m| m.borrow().clone()) else {
        return true;
    };
    if shell.browser().space(&id).is_none() {
        return true;
    }
    let chrome = shell.view().chrome().clone();
    match (menu, chosen) {
        (SPACE_MENU, item::EDIT) => edit(shell, &id),
        (SPACE_MENU, item::NEW_TAB) => {
            enter(shell, &id);
            super::open(
                shell,
                copper_shell_core::palette::complete::Purpose::Launch,
                false,
            );
        }
        (SPACE_MENU, item::NEW_CANVAS) => {
            enter(shell, &id);
            crate::commands::run(shell, "new-canvas");
        }
        (SPACE_MENU, item::MOVE_TAB) => {
            if let Some(tab) = shell.active_id() {
                shell.move_to_space(&tab, &id);
            }
        }
        (SPACE_MENU, item::LEFT) => shell.nudge_space(&id, -1),
        (SPACE_MENU, item::RIGHT) => shell.nudge_space(&id, 1),
        (SPACE_MENU, item::COLOUR) => {
            let now = {
                let b = shell.browser();
                b.space(&id).map(SpaceColour::of)
            };
            let mut m = Menu::new();
            for (i, c) in SpaceColour::ALL.iter().enumerate() {
                m = m.item(i as u32, c.name()).checked(Some(*c) == now);
            }
            chrome.edit(move |ui, _| ui.show_menu(COLOUR_MENU, &m, at));
        }
        (SPACE_MENU, item::PROFILE) => {
            let (names, mine) = {
                let b = shell.browser();
                (b.profiles(), b.space(&id).and_then(|s| s.profile.clone()))
            };
            let mut m = Menu::new()
                .item(item::SHARED, "Shared")
                .checked(mine.is_none());
            for (i, n) in names.iter().enumerate() {
                m = m
                    .item(i as u32 + 1, n)
                    .checked(mine.as_deref() == Some(n.as_str()));
            }
            m = m.separator().item(item::NEW_PROFILE, "New Profile…");
            PROFILES.with(|p| *p.borrow_mut() = names);
            chrome.edit(move |ui, _| ui.show_menu(PROFILE_MENU, &m, at));
        }
        (SPACE_MENU, item::DELETE) => ask_delete(shell, &id),
        (COLOUR_MENU, i) => {
            if let Some(c) = SpaceColour::ALL.get(i as usize) {
                shell.set_space_colour(&id, *c);
            }
        }
        (PROFILE_MENU, item::SHARED) => shell.set_space_profile(&id, None),
        (PROFILE_MENU, item::NEW_PROFILE) => edit(shell, &id),
        (PROFILE_MENU, i) => {
            let name = PROFILES.with(|p| p.borrow().get(i as usize - 1).cloned());
            if let Some(n) = name {
                shell.set_space_profile(&id, Some(&n));
            }
        }
        _ => {}
    }
    crate::settings::changed(shell, "spaces");
    true
}

/// Into space `id` if not there already.
fn enter(shell: &Shell, id: &str) {
    if shell.browser().current_space_id() != id {
        shell.switch_space(id);
    }
}

/// Settings › Spaces, showing space `id` (`SpaceEditing.open`).
pub fn edit(shell: &Shell, id: &str) {
    EDITING.with(|e| *e.borrow_mut() = Some(id.to_string()));
    crate::settings::open(shell, "spaces");
}

/// The space Settings › Spaces shows: the one picked there, else the one
/// you are in.
pub fn editing(shell: &Shell) -> String {
    let b = shell.browser();
    EDITING
        .with(|e| e.borrow().clone())
        .filter(|id| b.space(id).is_some())
        .unwrap_or_else(|| b.current_space_id().to_string())
}

/// Pick space `id` on Settings › Spaces (without going there).
pub fn pick(id: &str) {
    EDITING.with(|e| *e.borrow_mut() = Some(id.to_string()));
}

/// A new space (`Spaces.add`): "Space <n>", a colour nobody wears, and its
/// page open a beat later so it gets a name and an icon.
pub fn new_space(shell: &Shell) {
    if NEW_SPACE_PENDING.with(Cell::get) {
        return;
    }
    let id = shell.add_space("");
    NEW_SPACE_PENDING.with(|p| p.set(true));
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(200)).await;
        NEW_SPACE_PENDING.with(|p| p.set(false));
        if let Some(s) = weak.upgrade() {
            edit(&s, &id);
        }
    });
}

/// Ask before deleting space `id` (`SpaceDelete.ask`): with no tabs,
/// Delete or Cancel; with some, close them, move them to the neighbour,
/// or Cancel. Pins live in every space and are never touched.
pub fn ask_delete(shell: &Shell, id: &str) {
    let (title, count) = {
        let b = shell.browser();
        if b.spaces().count() < 2 {
            return;
        }
        let Some(sp) = b.space(id) else { return };
        (
            symbols::title(sp.icon.as_deref(), &sp.name),
            b.row(id).len(),
        )
    };
    let neighbour = shell
        .neighbour_space(id)
        .and_then(|n| {
            shell
                .browser()
                .space(&n)
                .map(|s| symbols::title(s.icon.as_deref(), &s.name))
        })
        .unwrap_or_default();
    let detail = match count {
        0 => "It has no tabs.".to_string(),
        1 => "Its one tab will close.".to_string(),
        n => format!("Its {n} tabs will close."),
    };
    let (buttons, answers): (Vec<(String, bool)>, Vec<Answer>) = if count == 0 {
        (
            vec![("Delete".into(), true), ("Cancel".into(), false)],
            vec![Answer::Close, Answer::Cancel],
        )
    } else {
        (
            vec![
                ("Close Tabs & Delete".into(), true),
                (
                    format!("Move Tabs to \u{201C}{neighbour}\u{201D} & Delete"),
                    false,
                ),
                ("Cancel".into(), false),
            ],
            vec![Answer::Close, Answer::Move, Answer::Cancel],
        )
    };
    ASKING.with(|a| *a.borrow_mut() = Some((id.to_string(), answers)));
    let title = format!("Delete \u{201C}{title}\u{201D}?");
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, st| st.card.ask(ui, &title, &detail, &buttons));
    shell.view().overlay_changed();
}

/// A click on one of the delete question's buttons. `false`: not one.
pub fn answered(shell: &Shell, id: NodeId) -> bool {
    let chrome = shell.view().chrome().clone();
    let Some(Some(i)) = chrome.read(|ui, st| st.card.answer(ui, id)) else {
        return false;
    };
    shell.view().overlay_changed();
    let Some((space, answers)) = ASKING.with(|a| a.borrow_mut().take()) else {
        return true;
    };
    match answers.get(i) {
        Some(Answer::Close) => {
            shell.remove_space(&space, false);
        }
        Some(Answer::Move) => {
            shell.remove_space(&space, true);
        }
        _ => {}
    }
    crate::settings::changed(shell, "spaces");
    shell.view().focus_page();
    true
}
