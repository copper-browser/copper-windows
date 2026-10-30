//! The Settings panel (`SettingsPanel`, `SettingsFinder`): its pages, the
//! one shown, search across all of them, and what their controls do.
//!
//! A page describes itself as a [`Form`] ([`Page::draw`]) and hears its
//! controls back as actions ([`Page::act`]); a page that draws itself
//! (Cloud) takes the page column instead ([`Page::body`]) and hears its own
//! clicks and field events. Search indexes every page's lines lazily —
//! the first time a query is typed after Settings opens or a page says it
//! changed — so no page is described on every keystroke.
//!
//! Redrawing is cheap but not free: a page is redrawn only when what it
//! says changed, and a text box the keyboard is in keeps it (the same
//! field, caret at the end) when it is.

use super::draw::{Drawn, FieldHit, Hit};
use super::form::{Control, Form, act_of};
use super::frame::{self, Nodes, RailRow};
use super::index;
use super::look::metric;
use super::search::{Entry, Index, Results};
use crate::chrome::glyph;
use crate::commands::Command;
use crate::panels::plate::Plate;
use crate::panels::{self, Panel};
use crate::platform::Shell;
use copper_win_ui::input::{Key, Mods};
use copper_win_ui::tokens::motion;
use copper_win_ui::win::Ui;
use copper_win_ui::{Event, Menu, NodeId, Point, Prop};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

mod finding;
mod redraw;

/// One page of Settings.
///
/// Only the first six are required. The rest have defaults a page
/// overrides when it needs to:
///
/// - Search: a page is found by its title, blurb and [`Page::keywords`],
///   and its rows by [`Page::entries`] — by default one entry per titled
///   line of [`Page::draw`], landing on the line's anchor
///   ([`super::form::Section::anchor`], else its title) and also found by
///   its [`super::form::Section::keywords`]. A page that draws itself says
///   where its anchors are with [`Page::anchor`].
/// - Drawing itself ([`Page::body`], [`Page::click`], [`Page::field`],
///   [`Page::left`]): the Cloud page's way in, for what a form can't say.
pub trait Page: 'static {
    /// Its id: `settings::open(shell, id)` shows it, and the rail places it
    /// by it ([`index::GROUPS`]).
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    /// What the page is for, in a line under its title.
    fn blurb(&self) -> &'static str;
    fn glyph(&self) -> char;
    /// Describe the page from what the shell knows now.
    fn draw(&self, shell: &Shell) -> Form;
    /// A control was used: `action` is the control's, `value` the switch's
    /// new state (`"on"`/`"off"`), the option chosen, or a text box's text.
    fn act(&self, shell: &Shell, action: &str, value: &str);

    /// A page that draws itself: when `Some`, a refresh runs it with the
    /// page's scroll list instead of drawing [`Page::draw`]'s form. The
    /// closure owns the list's children (it may patch what it drew last
    /// time rather than rebuild it); the list is emptied for it whenever
    /// the page comes on screen.
    #[allow(clippy::type_complexity)]
    fn body(&self, _shell: &Shell) -> Option<Box<dyn FnOnce(&mut Ui, NodeId)>> {
        None
    }
    /// A click Settings' own controls didn't take. `true` if it was the
    /// page's.
    fn click(&self, _shell: &Shell, _id: NodeId) -> bool {
        false
    }
    /// A field event in the page that isn't one of its form's text boxes.
    /// `true` if it was the page's; else Escape closes Settings.
    fn field(&self, _shell: &Shell, _id: NodeId, _e: &Event) -> bool {
        false
    }
    /// The page went off screen: another page was picked, search results
    /// took its place, or Settings closed.
    fn left(&self) {}
    /// Other words the page as a whole is found by.
    fn keywords(&self) -> Vec<String> {
        index::page_keywords(self.id())
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    /// What search finds on the page, besides the page itself.
    fn entries(&self, shell: &Shell) -> Vec<Entry> {
        index::entries_of(self.id(), self.title(), &self.draw(shell))
    }
    /// For a page that draws itself: the node search lands on for `anchor`
    /// (one of its [`Page::entries`]), and whether it is a whole card.
    fn anchor(&self, _anchor: &str) -> Option<(NodeId, bool)> {
        None
    }
}

/// Settings' popup chooser (menus `40..=49` are Settings').
const PICK_MENU: u32 = 40;

/// The key `settings.page`: the page Settings opens at (`Browser.swift`
/// 54–59).
const PAGE_KEY: &str = "settings.page";

/// Who the page column belongs to now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Page(&'static str),
    Results,
}

/// What was last drawn, so an unchanged page is not drawn again.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    owner: Owner,
    /// The form, for a page drawn from one.
    form: Option<Form>,
    dark: bool,
    width: f32,
    rail: Vec<(&'static str, Option<usize>)>,
}

/// The search: what is typed, the index it ran against, and the results.
#[derive(Default)]
struct Finder {
    query: String,
    index: Option<Rc<Index>>,
    results: Results,
    selection: usize,
    /// The results are in the page's place (a page picked in the rail while
    /// searching hides them, keeping the query).
    showing: bool,
    /// Each result's row and its ↵ mark, in order.
    rows: Vec<NodeId>,
}

impl Finder {
    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }
}

/// Where a pick should land once its page is drawn.
#[derive(Clone, Debug)]
struct Landing {
    anchor: String,
    flash: bool,
}

/// The panel: its pages, the one shown, and what its controls do.
pub struct Settings {
    pages: RefCell<Vec<Rc<dyn Page>>>,
    page: Cell<&'static str>,
    nodes: Cell<Option<Nodes>>,
    drawn: RefCell<Drawn>,
    finder: RefCell<Finder>,
    shown: RefCell<Option<Shown>>,
    /// Bumped on every opening and closing: a timer from an older one stops.
    generation: Cell<u64>,
    landing: RefCell<Option<Landing>>,
    /// The popup chooser open now: its page and control.
    menu: RefCell<Option<(&'static str, Control)>>,
}

thread_local! {
    static SETTINGS: RefCell<Option<Rc<Settings>>> = const { RefCell::new(None) };
}

fn get() -> Option<Rc<Settings>> {
    SETTINGS.with(|s| s.borrow().clone())
}

/// Register Settings, its pages and its command.
pub fn register(shell: &Shell) {
    let s = Rc::new(Settings {
        pages: RefCell::new(Vec::new()),
        page: Cell::new(index::FIRST),
        nodes: Cell::new(None),
        drawn: RefCell::default(),
        finder: RefCell::default(),
        shown: RefCell::new(None),
        generation: Cell::new(0),
        landing: RefCell::new(None),
        menu: RefCell::new(None),
    });
    SETTINGS.with(|g| *g.borrow_mut() = Some(s.clone()));
    for p in super::pages::all() {
        add(p);
    }
    shell.view().panels().add(shell, s);
    // Passwords and Move in: plates of their own, opened from its pages.
    super::manager::register(shell);
    super::movein::register(shell);
    // What the pages put in place at launch (the downloads folder).
    super::pages::start(shell);
    shell
        .view()
        .commands()
        .add(Command::new("settings", "Settings", glyph::SETTINGS, |s| {
            let last = s.settings().string(PAGE_KEY).map(str::to_string);
            open(s, last.as_deref().unwrap_or(index::FIRST))
        }));
    // The greys are resolved for one appearance: draw again in the other.
    shell.on_dark_changed(|s| {
        if let Some(st) = get()
            && showing(s).is_some()
        {
            st.shown.borrow_mut().take();
            st.refresh(s);
        }
    });
}

/// Add (or replace) a page.
pub fn add(page: Rc<dyn Page>) {
    let Some(s) = get() else { return };
    {
        let mut pages = s.pages.borrow_mut();
        match pages.iter().position(|p| p.id() == page.id()) {
            Some(i) => pages[i] = page,
            None => pages.push(page),
        }
    }
    s.finder.borrow_mut().index = None;
}

/// Show Settings at `page` (the first page when there is no such page).
pub fn open(shell: &Shell, page: &str) {
    let Some(s) = get() else { return };
    let id = s
        .find(page)
        .or_else(|| s.find(index::FIRST))
        .map(|p| p.id());
    if let Some(id) = id {
        s.switch(shell, id);
    }
    // A doorway to a page shows the page, not results from before.
    s.finder.borrow_mut().showing = false;
    panels::show(shell, "settings");
}

/// Show Settings at `page`, scrolled to `anchor` and lit, as a picked
/// result lands (`openSettings(_:at:)`): Voice on Intelligence, Addons on
/// Extensions.
pub fn open_at(shell: &Shell, page: &str, anchor: &str) {
    open(shell, page);
    let Some(s) = get() else { return };
    super::fold::reveal(anchor);
    *s.landing.borrow_mut() = Some(Landing {
        anchor: anchor.to_string(),
        flash: true,
    });
    s.refresh(shell);
    s.land_soon(shell);
}

/// The page Settings is showing, if Settings is on screen.
pub fn showing(shell: &Shell) -> Option<&'static str> {
    let s = get()?;
    (shell.view().panels().showing() == Some("settings")).then(|| s.page.get())
}

/// What page `id` shows changed: redraw it if it is on screen, and search
/// it afresh next time.
pub fn changed(shell: &Shell, id: &str) {
    let Some(s) = get() else { return };
    s.finder.borrow_mut().index = None;
    if s.page.get() == id && showing(shell).is_some() && !s.finder.borrow().showing {
        s.refresh(shell);
    }
}

impl Settings {
    fn find(&self, id: &str) -> Option<Rc<dyn Page>> {
        self.pages.borrow().iter().find(|p| p.id() == id).cloned()
    }

    /// The page chosen (shown unless results are).
    fn current(&self) -> Option<Rc<dyn Page>> {
        self.find(self.page.get())
    }

    /// Choose page `id`, remembered for next time.
    fn switch(&self, shell: &Shell, id: &'static str) {
        if self.page.get() != id {
            self.page.set(id);
            shell.update_settings(|s| s.set(PAGE_KEY, id));
        }
    }

    fn ids(&self) -> Vec<&'static str> {
        self.pages.borrow().iter().map(|p| p.id()).collect()
    }

    /// The rail's rows, grouped, with counts while searching.
    fn rail_rows(&self) -> Vec<(&'static str, Vec<RailRow>)> {
        let f = self.finder.borrow();
        let searching = f.searching();
        let pages = self.pages.borrow();
        index::rail(&self.ids())
            .into_iter()
            .map(|(group, ids)| {
                let rows = ids
                    .into_iter()
                    .filter_map(|id| pages.iter().find(|p| p.id() == id))
                    .map(|p| RailRow {
                        id: p.id(),
                        title: p.title(),
                        glyph: p.glyph(),
                        count: searching
                            .then(|| f.results.counts.get(p.id()).copied().unwrap_or(0)),
                    })
                    .collect();
                (group, rows)
            })
            .collect()
    }

    /// The focused text box on the page, by its action.
    fn focused_field(&self, ui: &Ui) -> Option<String> {
        let id = ui.tree.focused()?;
        self.drawn
            .borrow()
            .fields
            .iter()
            .find(|(f, _)| *f == id)
            .map(|(_, h)| h.action.clone())
    }

    fn field_hit(&self, id: NodeId) -> Option<FieldHit> {
        self.drawn
            .borrow()
            .fields
            .iter()
            .find(|(f, _)| *f == id)
            .map(|(_, h)| h.clone())
    }

    /// Open the popup chooser under `at`.
    fn popup(&self, shell: &Shell, page: &'static str, control: Control, at: NodeId) {
        let Control::Pick {
            options, chosen, ..
        } = &control
        else {
            return;
        };
        let mut menu = Menu::new();
        for (i, (value, label)) in options.iter().enumerate() {
            menu = menu.item(i as u32, label).checked(value == chosen);
        }
        *self.menu.borrow_mut() = Some((page, control.clone()));
        shell.view().chrome().read(move |ui, _| {
            let r = ui.tree.window_rect(at);
            ui.show_menu(PICK_MENU, &menu, Point::new(r.x, r.y + r.h + 4.0));
        });
    }

    /// Run a page's action and draw what it changed.
    fn act(&self, shell: &Shell, page: &'static str, action: &str, value: &str) {
        if let Some(fold) = action.strip_prefix(super::fold::ACTION) {
            super::fold::toggle(fold);
            self.refresh(shell);
        } else if let Some(p) = self.find(page) {
            p.act(shell, action, value);
            self.finder.borrow_mut().index = None;
            self.refresh(shell);
        }
    }
}

impl Panel for Settings {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn title(&self) -> String {
        "Settings".into()
    }

    fn glyph(&self) -> char {
        glyph::SETTINGS
    }

    fn width(&self) -> f32 {
        920.0
    }

    fn build(&self, ui: &mut Ui, p: &Plate) {
        let nodes = frame::build(ui, p);
        // Arrive as the card does: the dimming fades in, the panel settles
        // from just smaller (`App.swift` 404–418).
        ui.tree.set_prop(nodes.scrim, Prop::Opacity, 0.0);
        ui.tree
            .animate(nodes.scrim, Prop::Opacity, 1.0, motion::QUICK);
        ui.tree.edit(nodes.plate, |n| n.flags.layer = true);
        ui.tree
            .set_prop(nodes.plate, Prop::Scale, metric::ENTER_SCALE);
        ui.tree
            .animate(nodes.plate, Prop::Scale, 1.0, motion::SETTLE);
        ui.focus(Some(nodes.search));
        self.nodes.set(Some(nodes));
        *self.shown.borrow_mut() = None;
        *self.finder.borrow_mut() = Finder::default();
        self.generation.set(self.generation.get() + 1);
    }

    fn refresh(&self, shell: &Shell) {
        self.redraw(shell);
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        let hit = self
            .drawn
            .borrow()
            .hits
            .iter()
            .find(|(n, _)| *n == id)
            .map(|(_, h)| h.clone());
        let Some(hit) = hit else {
            return self.current().is_some_and(|p| p.click(shell, id));
        };
        match hit {
            Hit::Page(p) => {
                self.finder.borrow_mut().showing = false;
                self.switch(shell, p);
                self.refresh(shell);
            }
            Hit::Act(page, control, option) => {
                if let Some((action, value)) = act_of(&control, option) {
                    self.act(shell, page, &action, &value);
                }
            }
            Hit::Pick(page, control) => self.popup(shell, page, control, id),
            Hit::Close => {
                shell.view().panels().hide(shell);
            }
            Hit::Clear => {
                self.clear(shell);
            }
            Hit::Result(i) => self.pick(shell, i),
            Hit::Suggest(word) => {
                if let Some(nodes) = self.nodes.get() {
                    shell.view().chrome().read(move |ui, _| {
                        ui.set_field_text(nodes.search, word, false);
                        ui.focus(Some(nodes.search));
                    });
                }
                self.search(shell, word.to_string());
            }
        }
        true
    }

    fn field(&self, shell: &Shell, id: NodeId, e: &Event) -> bool {
        let Some(nodes) = self.nodes.get() else {
            return false;
        };
        if id == nodes.search {
            return match e {
                Event::FieldChanged(_) => {
                    let text = shell
                        .view()
                        .chrome()
                        .read(move |ui, _| ui.field_text(id).unwrap_or("").to_string())
                        .unwrap_or_default();
                    self.search(shell, text);
                    true
                }
                Event::FieldSubmit(_) => {
                    let i = self.finder.borrow().selection;
                    if self.finder.borrow().showing {
                        self.pick(shell, i);
                    }
                    true
                }
                // Escape empties the search first, then closes.
                Event::FieldCancel(_) => self.clear(shell),
                _ => false,
            };
        }
        if let Some(h) = self.field_hit(id) {
            let text = shell
                .view()
                .chrome()
                .read(move |ui, _| ui.field_text(id).unwrap_or("").to_string())
                .unwrap_or_default();
            return match e {
                Event::FieldChanged(_) => {
                    // No redraw: the caret stays where the typing is.
                    if let Some(p) = self.find(h.page) {
                        p.act(shell, &h.action, &text);
                    }
                    true
                }
                Event::FieldSubmit(_) => {
                    self.act(shell, h.page, &h.submit, &text);
                    true
                }
                _ => false,
            };
        }
        self.current().is_some_and(|p| p.field(shell, id, e))
    }

    fn key(&self, shell: &Shell, key: Key, mods: Mods, _focused: Option<NodeId>) -> bool {
        let Some(nodes) = self.nodes.get() else {
            return false;
        };
        let plain = !mods.ctrl && !mods.alt && !mods.shift;
        match key {
            // Ctrl+F: to the search (`⌘F`), everything in it chosen.
            Key::Letter('F') if mods.ctrl && !mods.alt && !mods.shift => {
                shell.view().chrome().read(move |ui, _| {
                    let text = ui.field_text(nodes.search).unwrap_or("").to_string();
                    ui.set_field_text(nodes.search, &text, true);
                    ui.focus(Some(nodes.search));
                });
                true
            }
            Key::Down if plain && self.finder.borrow().showing => {
                self.step(shell, 1);
                true
            }
            Key::Up if plain && self.finder.borrow().showing => {
                self.step(shell, -1);
                true
            }
            _ => false,
        }
    }

    fn menu(&self, shell: &Shell, menu: u32, item: u32) -> bool {
        if menu != PICK_MENU {
            return false;
        }
        if let Some((page, control)) = self.menu.borrow_mut().take()
            && let Some((action, value)) = act_of(&control, Some(item as usize))
        {
            self.act(shell, page, &action, &value);
        }
        true
    }

    fn closed(&self) {
        if let Some(Shown {
            owner: Owner::Page(p),
            ..
        }) = self.shown.borrow_mut().take()
            && let Some(p) = self.find(p)
        {
            p.left();
        }
        self.generation.set(self.generation.get() + 1);
        self.nodes.set(None);
        *self.drawn.borrow_mut() = Drawn::default();
        *self.finder.borrow_mut() = Finder::default();
        self.landing.borrow_mut().take();
        self.menu.borrow_mut().take();
        super::fold::close_all();
    }
}
