//! Panels (`docs/APP.md` › Panels): History, Bookmarks, Downloads and the
//! rest float over the page on a plate (`App.swift` `sheet`), one at a
//! time, each opened by a command (`show-<id>`) and closed by its cross,
//! a click on the dimmed window around it, or Escape.
//!
//! A panel implements [`Panel`]; [`Panels::add`] registers it (and its
//! `show-<id>` command). The host builds the plate, lets the panel build
//! its body into it, then asks it to draw what the shell knows — the same
//! read-then-edit split as the rest of the chrome: [`Panel::build`] and
//! the closures a panel passes to `Chrome::edit` never call the shell;
//! [`Panel::refresh`] and [`Panel::click`] run with nothing borrowed.
//!
//! History is the worked example ([`history`]).

pub mod history;
pub mod plate;

use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::Shell;
use copper_win_ui::win::Ui;
use copper_win_ui::{Event, NodeId};
use plate::Plate;
use std::cell::RefCell;
use std::rc::Rc;

/// One panel.
pub trait Panel: 'static {
    /// Its id: `show-<id>` opens it.
    fn id(&self) -> &'static str;
    fn title(&self) -> String;
    fn glyph(&self) -> char;
    /// How wide its plate is.
    fn width(&self) -> f32 {
        560.0
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
}

impl Panels {
    /// Register `panel` and its `show-<id>` command.
    pub fn add(&self, shell: &Shell, panel: Rc<dyn Panel>) {
        let id = panel.id();
        let command = format!("show-{id}");
        shell.view().commands().add(Command::new(
            &command,
            &panel.title(),
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
        let Some(open) = self.open.borrow_mut().take() else {
            return false;
        };
        let scrim = open.plate.scrim;
        shell
            .view()
            .chrome()
            .edit(move |ui, _| ui.tree.remove(scrim));
        open.panel.closed();
        shell.view().focus_page();
        true
    }
}

/// Show panel `id` (another on show goes first; the same one is drawn
/// afresh).
pub fn show(shell: &Shell, id: &str) {
    let panels = shell.view().panels();
    let Some(panel) = panels.get(id) else {
        tracing::debug!("no panel {id}");
        return;
    };
    if panels.showing() == Some(panel.id()) {
        panel.refresh(shell);
        return;
    }
    panels.hide(shell);
    let weak = shell.downgrade();
    let p = panel.clone();
    let title = panel.title();
    let width = panel.width();
    shell.view().chrome().edit(move |ui, st| {
        let plate = plate::build(ui, st.ids.plates, &title, width);
        p.build(ui, &plate);
        // Only the view's own record of what is open: no shell call.
        if let Some(s) = weak.upgrade() {
            *s.view().panels().open.borrow_mut() = Some(Open { panel: p, plate });
        }
    });
    // Drawn once the plate exists (the edit may have had to wait its turn).
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        if let Some(s) = weak.upgrade() {
            refresh(&s, panel.id());
        }
    });
}

/// Redraw the panel on show if it is `id`.
pub fn refresh(shell: &Shell, id: &str) {
    if let Some((panel, _)) = shell.view().panels().current()
        && panel.id() == id
    {
        panel.refresh(shell);
    }
}

/// A click the chrome did not take: the plate's cross, the dimmed window
/// around it, or the panel's own. `true` if it was a panel's.
pub fn click(shell: &Shell, id: NodeId) -> bool {
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

/// A menu a panel opened was answered (no panel opens one yet).
pub fn menu_chosen(_shell: &Shell, menu: u32, item: u32) {
    tracing::debug!("menu {menu} item {item}: nobody asked");
}

/// Register the panels and their commands.
pub fn register(shell: &Shell) {
    let panels = shell.view().panels();
    panels.add(shell, Rc::new(history::History::default()));
    // The column's foot door and Ctrl+Shift+O open the bookmarks: until
    // Bookmarks has a panel of its own, the palette lists them.
    shell.view().commands().add(Command::new(
        "show-bookmarks",
        "Bookmarks",
        glyph::BOOKMARKS,
        |s| {
            crate::palette::open(
                s,
                copper_shell_core::palette::complete::Purpose::Palette,
                false,
            )
        },
    ));
}
