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
    /// A press on a chip: where it went down (window DIPs, x), the
    /// chip's space and its place in the row.
    static PRESS: RefCell<Option<(f32, String, usize)>> = const { RefCell::new(None) };
    /// The space whose chip was dragged: the click the toolkit makes of
    /// the release is not a click.
    static DRAGGED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Where the chips are (client pixels: left, top, right, bottom), when
    /// it was read, and whether a name is up: a move far from them needs
    /// no look at the window.
    static ZONE: Cell<Option<([i32; 4], std::time::Instant)>> = const { Cell::new(None) };
    static NAMING: Cell<bool> = const { Cell::new(false) };
}

/// Note where the chips are now (client pixels), a little wider.
fn note_zone(ui: &copper_win_ui::win::Ui, st: &crate::chrome::State) {
    let r = ui.tree.visible_rect(st.column.chips_window());
    let k = ui.scale();
    let px = |v: f32| (v * k).round() as i32;
    let zone = [
        px(r.x) - 4,
        px(r.y) - 4,
        px(r.x + r.w) + 4,
        px(r.y + r.h) + 4,
    ];
    ZONE.with(|z| z.set(Some((zone, std::time::Instant::now()))));
}

/// Whether a move at (`x`, `y`) may be over the chips (or leaving them).
fn near_chips(x: i32, y: i32) -> bool {
    if NAMING.with(Cell::get) {
        return true;
    }
    match ZONE.with(Cell::get) {
        Some((z, at)) if at.elapsed() < Duration::from_millis(500) => {
            x >= z[0] && x < z[2] && y >= z[1] && y < z[3]
        }
        _ => true,
    }
}

/// A click on one of the column's spaces doors (the events keep every
/// other column hit for themselves).
pub fn column_hit(chrome: &Rc<Chrome>, shell: &Shell, h: Hit) {
    match h {
        Hit::Space(id) if DRAGGED.with(|d| d.borrow_mut().take_if(|d| *d == id).is_some()) => {}
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

/// The chips under the pointer (the toolkit has no drag, no hover and no
/// wheel for a plain row, so they are read from the window's messages
/// before it sees them): the hovered chip's name at once; a press that
/// travels [`DRAG_FROM`] picks the chip up, the others make way a step at
/// a time and the space moves with it (`SpaceStrip.reorder`); the wheel
/// slides an overflowing row. Nothing is swallowed but the wheel over the
/// chips, so the toolkit's own press and capture stay whole.
///
/// [`DRAG_FROM`]: crate::chrome::column::DRAG_FROM
pub fn pointer(shell: &Shell) {
    const WM_MOUSEMOVE: u32 = 0x0200;
    const WM_LBUTTONDOWN: u32 = 0x0201;
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_MOUSEWHEEL: u32 = 0x020A;
    const WM_MOUSEHWHEEL: u32 = 0x020E;
    const WM_MOUSELEAVE: u32 = 0x02A3;
    const MK_LBUTTON: usize = 0x0001;
    const NOTCH: f32 = 120.0;
    let weak = shell.downgrade();
    let hwnd = shell.view().chrome().window().hwnd();
    copper_win_rt::add_message_hook(move |m| {
        if m.hwnd != hwnd.0 as isize
            || !matches!(
                m.message,
                WM_MOUSEMOVE
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_MOUSEWHEEL
                    | WM_MOUSEHWHEEL
                    | WM_MOUSELEAVE
            )
        {
            return false;
        }
        let Some(shell) = weak.upgrade() else {
            return false;
        };
        let chrome = shell.view().chrome().clone();
        let words = m.lparam as usize;
        let (mut x, mut y) = (
            i32::from((words & 0xFFFF) as u16 as i16),
            i32::from(((words >> 16) & 0xFFFF) as u16 as i16),
        );
        if matches!(m.message, WM_MOUSEWHEEL | WM_MOUSEHWHEEL) {
            // The wheel says where on the screen.
            let mut p = windows::Win32::Foundation::POINT { x, y };
            // SAFETY: our own window and a live POINT.
            if !unsafe { windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p) }.as_bool() {
                return false;
            }
            (x, y) = (p.x, p.y);
        }
        let to_dips = |ui: &copper_win_ui::win::Ui| {
            let k = ui.scale().max(0.5);
            Point::new(x as f32 / k, y as f32 / k)
        };
        match m.message {
            WM_LBUTTONDOWN => {
                DRAGGED.with(|d| d.borrow_mut().take());
                let press = chrome
                    .read(|ui, st| {
                        // A chip still up from a release never seen.
                        st.column.drop_chip(ui);
                        let p = to_dips(ui);
                        st.column.chip_at(ui, p).map(|(i, id)| (p.x, id, i))
                    })
                    .flatten();
                PRESS.with(|s| *s.borrow_mut() = press);
                false
            }
            WM_MOUSEMOVE if m.wparam & MK_LBUTTON != 0 => {
                let Some((x0, id, from)) = PRESS.with(|s| s.borrow().clone()) else {
                    return false;
                };
                let moved = chrome
                    .read(|ui, st| {
                        let travel = to_dips(ui).x - x0;
                        if st.column.held().is_none() {
                            if travel.abs() < crate::chrome::column::DRAG_FROM {
                                return None;
                            }
                            st.column.hold_chip(ui, &id, from);
                        }
                        st.column.drag_chip(ui, travel)
                    })
                    .flatten();
                if let Some((id, to)) = moved {
                    // On the loop's next turn: moving a space reaches the
                    // pages, which may run messages (the release among
                    // them) where this hook does not see them.
                    let weak = shell.downgrade();
                    copper_win_rt::spawn_local(async move {
                        if let Some(shell) = weak.upgrade() {
                            shell.move_space(&id, to);
                        }
                    });
                }
                false
            }
            WM_MOUSEMOVE => {
                // The button came up where this hook did not see it.
                if PRESS.with(|s| s.borrow_mut().take()).is_some() {
                    chrome.read(|ui, st| st.column.drop_chip(ui));
                }
                if !near_chips(x, y) {
                    return false;
                }
                chrome.read(|ui, st| {
                    note_zone(ui, st);
                    let at = st.column.chip_at(ui, to_dips(ui)).map(|(_, id)| id);
                    NAMING.with(|n| n.set(at.is_some()));
                    st.column.name_chip(ui, at.as_deref());
                });
                false
            }
            WM_LBUTTONUP => {
                PRESS.with(|s| s.borrow_mut().take());
                let held = chrome
                    .read(|ui, st| {
                        let id = st.column.held().map(|h| h.id.clone());
                        st.column.drop_chip(ui);
                        id
                    })
                    .flatten();
                if held.is_some() {
                    DRAGGED.with(|d| *d.borrow_mut() = held);
                }
                false
            }
            WM_MOUSELEAVE => {
                NAMING.with(|n| n.set(false));
                chrome.read(|ui, st| st.column.name_chip(ui, None));
                false
            }
            _ => {
                let delta = f32::from((m.wparam >> 16) as u16 as i16) / NOTCH;
                // Down the wheel, or tilted right: further along the row.
                let lines = if m.message == WM_MOUSEHWHEEL {
                    -delta
                } else {
                    delta
                };
                chrome
                    .read(|ui, st| {
                        st.column.over_chips(ui, to_dips(ui)) && {
                            // The chip under the pointer is another one
                            // now: its name waits for the next move.
                            if st.column.wheel_chips(ui, lines) {
                                st.column.name_chip(ui, None);
                                NAMING.with(|n| n.set(false));
                            }
                            true
                        }
                    })
                    .unwrap_or(false)
            }
        }
    });
}

/// Keep Settings › Spaces true to the spaces: whenever the shell says a
/// space was switched to, added, changed, reordered or removed, or a tab
/// came or went (the page shows its count), the page redraws if it is up
/// (`SpacesSettingsPage` observes `Spaces.shared` the same way). A burst
/// of changes in one turn redraws once.
pub fn watch(shell: &Shell) {
    use copper_shell_core::control::ShellEvent;
    let mut events = shell.hub().subscribe();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let mut errors = 0u32;
        loop {
            let got = events.recv().await;
            // A lag is one error and then events again; a hub that has
            // gone is nothing but errors.
            errors = if got.is_err() { errors + 1 } else { 0 };
            if errors > 4 {
                break;
            }
            let Some(s) = weak.upgrade() else { break };
            let mut redraw = matches!(
                got,
                Ok(ShellEvent::SpaceSwitched(_)
                    | ShellEvent::SpacesChanged
                    | ShellEvent::TabOpened(_)
                    | ShellEvent::TabClosed(_))
            ) || got.is_err();
            // Whatever else is already waiting belongs to the same turn.
            while let Ok(e) = events.try_recv() {
                redraw |= matches!(
                    e,
                    ShellEvent::SpaceSwitched(_)
                        | ShellEvent::SpacesChanged
                        | ShellEvent::TabOpened(_)
                        | ShellEvent::TabClosed(_)
                );
            }
            if redraw {
                crate::settings::changed(&s, "spaces");
            }
        }
    });
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
