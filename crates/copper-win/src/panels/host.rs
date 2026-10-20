//! The host: the panel on show, its plate, and the routing of clicks,
//! keys, menus and ticks to it (see the module docs in `panels`).

use super::plate::{self, Plate, Spot};
use super::{bookmarks, door, downloads, hidden, history};
use crate::commands::Command;
use crate::platform::Shell;
use copper_win_ui::input::{Key, Mods};
use copper_win_ui::win::Ui;
use copper_win_ui::{Event, NodeId, Point, Rect, Size};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// How often a panel that asked for it is ticked.
const TICK: Duration = Duration::from_millis(30);

/// The first id of the panels' menus (`Ui::show_menu`); `0x7061_0000..`
/// is the panels' block.
pub const MENU: u32 = 0x7061_0000;

/// Where a panel sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// Centred over a dimmed window.
    Sheet,
    /// Undimmed in the window's top-right corner (`App.swift` 419-431:
    /// 8 under the strip's 52, 14 from the right).
    Corner,
    /// Hanging from the door that opened it, `radius`-cornered, about
    /// `tall` high.
    Door { radius: f32, tall: f32 },
}

/// One panel.
pub trait Panel: 'static {
    /// Its id: `show-<id>` opens it.
    fn id(&self) -> &'static str;
    fn title(&self) -> String;
    fn glyph(&self) -> char;
    /// What the palette calls `show-<id>`; empty: not in the palette.
    fn name(&self) -> String {
        self.title()
    }
    /// How wide its plate is.
    fn width(&self) -> f32 {
        560.0
    }
    /// Where it sits.
    fn place(&self) -> Place {
        Place::Sheet
    }
    /// Build into a fresh plate (inside an edit: no shell calls).
    fn build(&self, ui: &mut Ui, plate: &Plate);
    /// Draw what the shell knows now (nothing borrowed).
    fn refresh(&self, shell: &Shell);
    /// A click inside the plate; `true` if it was the panel's.
    fn click(&self, shell: &Shell, id: NodeId) -> bool;
    /// A field event inside the plate; `true` if it was the panel's.
    fn field(&self, _shell: &Shell, _id: NodeId, _e: &Event) -> bool {
        false
    }
    /// A key the focused node did not use; `true` if it was the panel's.
    #[allow(dead_code)] // shared with Settings, whose key routing calls it
    fn key(&self, _shell: &Shell, _key: Key, _mods: Mods, _focused: Option<NodeId>) -> bool {
        false
    }
    /// A right-click on `target` inside the plate; `true` if it was the
    /// panel's (its own menus take ids from [`MENU`] up).
    fn context_menu(&self, _shell: &Shell, _target: NodeId, _at: Point) -> bool {
        false
    }
    /// Item `item` of menu `menu` was chosen; `true` if the menu was the
    /// panel's (panels' own menus use ids from [`MENU`] up).
    fn menu(&self, _shell: &Shell, _menu: u32, _item: u32) -> bool {
        false
    }
    /// Whether to be ticked every [`TICK`] while on show.
    fn ticks(&self) -> bool {
        false
    }
    /// Called every [`TICK`] while on show (if [`Panel::ticks`]).
    fn tick(&self, _shell: &Shell) {}
    /// The plate is about to go (nothing borrowed).
    fn closing(&self, _shell: &Shell) {}
    /// The plate went away (its nodes with it).
    fn closed(&self) {}
}

struct Open {
    panel: Rc<dyn Panel>,
    plate: Plate,
}

/// The panels, and the one on show.
#[derive(Default)]
pub struct Panels {
    list: RefCell<Vec<Rc<dyn Panel>>>,
    open: RefCell<Option<Open>>,
    /// Bumped whenever what is on show changes: a tick loop of an older
    /// showing stops.
    epoch: Cell<u64>,
    /// Refreshes asked for and not yet run, by panel id.
    pending: RefCell<Vec<&'static str>>,
}

impl Panels {
    /// Register `panel` and its `show-<id>` command.
    pub fn add(&self, shell: &Shell, panel: Rc<dyn Panel>) {
        let id = panel.id();
        let command = format!("show-{id}");
        shell.view().commands().add(Command::new(
            &command,
            &panel.name(),
            panel.glyph(),
            move |s| show(s, id),
        ));
        let mut l = self.list.borrow_mut();
        l.retain(|p| p.id() != id);
        l.push(panel);
    }

    fn get(&self, id: &str) -> Option<Rc<dyn Panel>> {
        self.list.borrow().iter().find(|p| p.id() == id).cloned()
    }

    /// The panel on show.
    pub fn showing(&self) -> Option<&'static str> {
        self.open.borrow().as_ref().map(|o| o.panel.id())
    }

    fn current(&self) -> Option<(Rc<dyn Panel>, Plate)> {
        self.open
            .borrow()
            .as_ref()
            .map(|o| (o.panel.clone(), o.plate))
    }

    /// Put the panel on show away; `false` if none was.
    pub fn hide(&self, shell: &Shell) -> bool {
        let Some((panel, _)) = self.current() else {
            return false;
        };
        panel.closing(shell);
        let Some(open) = self.open.borrow_mut().take() else {
            return false;
        };
        self.epoch.set(self.epoch.get() + 1);
        let scrim = open.plate.scrim;
        shell
            .view()
            .chrome()
            .edit(move |ui, _| ui.tree.remove(scrim));
        open.panel.closed();
        shell.view().overlay_changed();
        shell.view().focus_page();
        true
    }
}

/// Show panel `id` (another on show goes first; the same one is drawn
/// afresh, or — hanging from a door — put away).
pub fn show(shell: &Shell, id: &str) {
    show_from(shell, id, None);
}

/// Show panel `id` hanging from `door` (a window rect in DIPs); with
/// `None`, from the door under the pointer or with the keyboard's focus.
pub fn show_from(shell: &Shell, id: &str, door: Option<Rect>) {
    let panels = shell.view().panels();
    let Some(panel) = panels.get(id) else {
        tracing::debug!("no panel {id}");
        return;
    };
    if panels.showing() == Some(panel.id()) {
        if matches!(panel.place(), Place::Door { .. }) {
            panels.hide(shell);
        } else {
            panel.refresh(shell);
        }
        return;
    }
    panels.hide(shell);
    shell.view().chrome().watch("panel open");
    let started = std::time::Instant::now();
    let weak = shell.downgrade();
    let p = panel.clone();
    let title = panel.title();
    let width = panel.width();
    let place = panel.place();
    shell.view().chrome().edit(move |ui, st| {
        let plate = match place {
            Place::Sheet => plate::build(ui, st.ids.plates, &title, width),
            Place::Corner => plate::build_at(
                ui,
                st.ids.plates,
                &title,
                width,
                Spot::Corner {
                    top: copper_win_ui::tokens::metrics::STRIP + 8.0,
                    right: 14.0,
                },
            ),
            Place::Door { radius, tall } => {
                let anchor = door.unwrap_or_else(|| door_under(ui));
                plate::popover(ui, st.ids.plates, width, radius, anchor, tall)
            }
        };
        p.build(ui, &plate);
        // Only the view's own record of what is open: no shell call.
        if let Some(s) = weak.upgrade() {
            *s.view().panels().open.borrow_mut() = Some(Open { panel: p, plate });
        }
    });
    let epoch = panels.epoch.get() + 1;
    panels.epoch.set(epoch);
    // Drawn in the same frame as the plate when the edit ran at once;
    // else once the plate exists (the edit had to wait its turn).
    let built = panels.showing() == Some(panel.id());
    let plate_ms = started.elapsed().as_secs_f64() * 1000.0;
    if built {
        shell.view().overlay_changed();
        panel.refresh(shell);
    }
    tracing::debug!(
        target: "copper::perf",
        "panel {}: plate {plate_ms:.2} ms, drawn {:.2} ms (in its turn: {built})",
        panel.id(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    let weak = shell.downgrade();
    let ticks = panel.ticks();
    copper_win_rt::spawn_local(async move {
        let Some(s) = weak.upgrade() else { return };
        if !built {
            s.view().overlay_changed();
            refresh(&s, panel.id());
        }
        drop(s);
        if !ticks {
            return;
        }
        loop {
            copper_win_rt::sleep(TICK).await;
            let Some(s) = weak.upgrade() else { return };
            let panels = s.view().panels();
            if panels.epoch.get() != epoch {
                return;
            }
            if let Some((p, _)) = panels.current() {
                p.tick(&s);
            }
        }
    });
}

/// The door a popover was asked for from: the node under the pointer, or
/// with the keyboard's focus; the top middle of the window failing both.
fn door_under(ui: &Ui) -> Rect {
    let r = ui
        .tree
        .hovered()
        .or_else(|| ui.tree.focused())
        .map(|id| ui.tree.window_rect(id))
        .filter(|r| !r.is_empty());
    r.unwrap_or_else(|| {
        let Size { w, .. } = ui.size();
        Rect::new(w / 2.0, 0.0, 0.0, copper_win_ui::tokens::metrics::STRIP)
    })
}

/// Redraw the panel on show if it is `id`.
pub fn refresh(shell: &Shell, id: &str) {
    if let Some((panel, _)) = shell.view().panels().current()
        && panel.id() == id
    {
        panel.refresh(shell);
    }
}

/// Redraw the panel on show if it is `id`, once, on the loop's next turn
/// (however often it is asked before then).
pub fn refresh_soon(shell: &Shell, id: &'static str) {
    let panels = shell.view().panels();
    if panels.showing() != Some(id) {
        return;
    }
    {
        let mut p = panels.pending.borrow_mut();
        if p.contains(&id) {
            return;
        }
        p.push(id);
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        if let Some(s) = weak.upgrade() {
            s.view().panels().pending.borrow_mut().retain(|p| *p != id);
            refresh(&s, id);
        }
    });
}

/// A click the chrome did not take: a door of the panels', the plate's
/// cross, the window around it, or the panel's own. `true` if it was a
/// panel's.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    if door::click(shell, id) {
        return true;
    }
    let Some((panel, plate)) = shell.view().panels().current() else {
        return false;
    };
    if id == plate.close || id == plate.scrim {
        shell.view().panels().hide(shell);
        return true;
    }
    panel.click(shell, id)
}

/// A field event the chrome did not take.
pub fn field_event(shell: &Shell, id: NodeId, e: &Event) {
    if let Some((panel, _)) = shell.view().panels().current()
        && !panel.field(shell, id, e)
        && matches!(e, Event::FieldCancel(_))
    {
        shell.view().panels().hide(shell);
    }
}

/// A key the chrome's focused node did not use: the panel on show's, if
/// it wants it.
#[allow(dead_code)] // shared with Settings, whose key routing calls it
pub fn key(shell: &Shell, key: Key, mods: Mods, focused: Option<NodeId>) -> bool {
    match shell.view().panels().current() {
        Some((panel, _)) => panel.key(shell, key, mods, focused),
        None => false,
    }
}

/// A right-click the chrome did not take (not on a tab): the panel on
/// show's, if it is inside its plate. `true` if it was a panel's.
pub fn context_menu(shell: &Shell, target: Option<NodeId>, at: Point) -> bool {
    let Some(target) = target else { return false };
    let Some((panel, plate)) = shell.view().panels().current() else {
        return false;
    };
    let inside = shell
        .view()
        .chrome()
        .read(|ui, _| ui.tree.is_within(target, plate.scrim))
        .unwrap_or(false);
    inside && panel.context_menu(shell, target, at)
}

/// A menu was answered that is not the chrome's own.
pub fn menu_chosen(shell: &Shell, menu: u32, item: u32) {
    if let Some((panel, _)) = shell.view().panels().current()
        && panel.menu(shell, menu, item)
    {
        return;
    }
    tracing::debug!("menu {menu} item {item}: nobody asked");
}

/// Whether Ctrl is held right now (Ctrl+click opens a bookmark in a new
/// tab, as ⌘-click does on the Mac).
pub fn ctrl_down() -> bool {
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
        // SAFETY: reads the calling thread's key state.
        unsafe { GetKeyState(i32::from(VK_CONTROL.0)) < 0 }
    }
    #[cfg(not(windows))]
    false
}

/// Register the panels, their commands and the doors.
pub fn register(shell: &Shell) {
    let panels = shell.view().panels();
    panels.add(shell, Rc::new(history::History::default()));
    bookmarks::register(shell);
    downloads::register(shell);
    hidden::register(shell);
}

/// Go to `url` (Mac `Browser.visit`): in the selected tab, or in a new tab
/// in front when `apart` (Ctrl held) or there is no selected tab.
pub fn visit(shell: &Shell, url: &str, apart: bool) {
    match shell.active_id().filter(|_| !apart) {
        Some(tab) => shell.navigate(&tab, url),
        None => {
            shell.open(
                url,
                copper_shell_core::OpenTab {
                    foreground: true,
                    ..Default::default()
                },
            );
        }
    }
}
