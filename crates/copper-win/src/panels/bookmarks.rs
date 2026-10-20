//! Bookmarks (`Bookmarks.swift`): folders and sites, in two places that
//! share one outline — the small list that hangs from a bookmarks door
//! (`BookmarksDropdown`) and the full manager (`BookmarksPanel`, a sheet).
//!
//! Folders open in place; a site opens in the selected tab (Ctrl+click: a
//! new tab in front). A row can be dragged into a folder or out to the top
//! level, or right-clicked: Open, Move to (the top level or any folder that
//! is not itself or inside it), Remove. `Ctrl+Shift+B` keeps the page in
//! front at the end of the list; the manager's foot brings in another
//! browser's bookmarks ([`super::take`]).
//!
//! The toolkit has no drag events: while a list is on show it is ticked,
//! and a press that wanders off its row is a drag — the row dims, the
//! folder under the pointer lights up (the list itself, for the top
//! level), and letting go there moves it.

use super::plate::{self, Plate};
use super::{MENU, Panel, Place, take};
use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::{Icon, Shell};
use copper_core::bookmarks::Bookmark;
use copper_import::Browser;
use copper_win_ui::tokens::motion;
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Align, Insets, Justify, Menu, Node, NodeId, Point, Prop, Role, Token, Trim, Weight,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The dropdown's id (its `show-` command is not in the palette).
pub const LIST: &str = "bookmarks-list";
/// The manager's id.
const MANAGER: &str = "bookmarks";
/// The menu a row's right-click opens.
const ROW_MENU: u32 = MENU + 0x100;
/// The sources that keep an import record (`Chromium.known` names).
const SOURCES: [&str; 6] = ["Chrome", "Edge", "Brave", "Vivaldi", "Chromium", "Arc"];

/// Register both lists, Add This Page and the column foot's door.
pub fn register(shell: &Shell) {
    let panels = shell.view().panels();
    panels.add(shell, Rc::new(Bookmarks::new(false)));
    panels.add(shell, Rc::new(Bookmarks::new(true)));
    let commands = shell.view().commands();
    commands.add(
        Command::new(
            "bookmark-page",
            "Bookmark This Page",
            glyph::STAR,
            bookmark_current,
        )
        .when(|s| page_to_keep(s).is_some()),
    );
    commands.add(Command::key("bookmarks-door", |s| {
        super::show_from(s, LIST, None);
    }));
}

/// The page in front, if it is one that can be kept.
fn page_to_keep(shell: &Shell) -> Option<(String, String)> {
    let id = shell.active_id()?;
    let b = shell.browser();
    let t = b.tab(&id)?;
    let ok =
        t.url.starts_with("http://") || t.url.starts_with("https://") || t.url.starts_with("file:");
    ok.then(|| (t.url.clone(), t.title.clone()))
}

/// `Ctrl+Shift+B` (Mac `bookmarkCurrent`): the page in front at the end of
/// the list, unless it is kept already.
pub fn bookmark_current(shell: &Shell) {
    let Some((url, title)) = page_to_keep(shell) else {
        return;
    };
    if shell.bookmarks().contains_url(&url) {
        shell.toast("Already a bookmark");
        return;
    }
    shell.bookmarks_mut().add(&url, &title);
    changed(shell);
    shell.toast("Bookmarked");
}

/// The tree changed: save it and redraw what shows it.
fn changed(shell: &Shell) {
    let roots = shell.bookmarks().roots().to_vec();
    shell.saver().schedule(roots);
    super::refresh_soon(shell, MANAGER);
    super::refresh_soon(shell, LIST);
}

/// A person's change under `ids`: the roots holding them are theirs now,
/// whatever an import brought (Mac `FlowBookmarks.relinquish`).
fn relinquish(shell: &Shell, ids: &[&str]) {
    let dir = shell.store().paths().data.clone();
    let sources: Vec<String> = SOURCES.iter().map(|s| s.to_string()).collect();
    let roots = shell.bookmarks().roots().to_vec();
    take::relinquish(&dir, &sources, ids, &roots);
}

/// Move `id` to the end of folder `into` (the top level for `None`).
fn move_to(shell: &Shell, id: &str, into: Option<&str>) {
    let mut ids = vec![id];
    ids.extend(into);
    relinquish(shell, &ids);
    if shell.bookmarks_mut().move_to(id, into) {
        changed(shell);
    }
}

/// Remove `id` and everything in it.
fn remove(shell: &Shell, id: &str) {
    relinquish(shell, &[id]);
    if shell.bookmarks_mut().remove(id) {
        changed(shell);
    }
}

/// Bring in `browser`'s bookmarks (Mac `takeBookmarks`): read off the UI
/// thread, then taken in at once.
fn bring_in(shell: &Shell, browser: Browser) {
    let dirs = browser.profile_dirs();
    let name = browser.name.clone();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let Some(read) = super::off_thread(move || copper_import::bookmarks::read(&dirs)).await
        else {
            return;
        };
        let Some(shell) = weak.upgrade() else { return };
        if !read.complete() {
            tracing::warn!(
                "{name} bookmarks: {} unreadable file(s); keeping its earlier roots",
                read.failed_files.len()
            );
        }
        let file = take::record(&shell.store().paths().data, &name);
        let previous = take::owned(&file);
        let roots = shell.bookmarks().roots().to_vec();
        let taken = take::taking(&read.nodes, read.complete(), &name, &roots, &previous);
        shell.bookmarks_mut().replace(taken.roots);
        take::keep(&file, &taken.imported);
        changed(&shell);
        let count = read.count();
        shell.toast(&if count == 0 {
            format!("No bookmarks in {name}")
        } else {
            format!("{count} bookmarks from {name}")
        });
    });
}

/// One line of the outline as drawn.
enum Line {
    Node {
        id: String,
        title: String,
        url: Option<String>,
        depth: usize,
        open: bool,
        /// Sites inside (folders only).
        count: usize,
        icon: Option<Icon>,
    },
    Empty(usize),
}

/// What a row is.
#[derive(Clone, Debug)]
struct RowOf {
    id: String,
    url: Option<String>,
}

/// What a click means.
#[derive(Clone, Debug)]
enum Hit {
    Row(RowOf),
    Add,
    Manage,
    Bring(usize),
}

/// What a right-click menu's items do.
#[derive(Clone, Debug)]
enum Act {
    Open(String),
    Move(String, Option<String>),
    Remove(String),
}

/// Where a drag would land.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    Nowhere,
    Top,
    Folder(String, NodeId),
}

/// A row being dragged.
#[derive(Clone, Debug)]
struct Drag {
    row: NodeId,
    id: String,
    target: Target,
}

#[derive(Default, Clone, Copy)]
struct Nodes {
    list: Option<NodeId>,
    /// The outline (the top level's drop target).
    outline: Option<NodeId>,
    count: Option<NodeId>,
    bring: Option<NodeId>,
}

/// The manager (`short == false`) or the dropdown.
pub struct Bookmarks {
    short: bool,
    nodes: Cell<Nodes>,
    hits: RefCell<HashMap<NodeId, Hit>>,
    open: RefCell<HashSet<String>>,
    sources: RefCell<Option<Vec<Browser>>>,
    drag: RefCell<Option<Drag>>,
    menu: RefCell<Vec<Act>>,
}

impl Bookmarks {
    fn new(short: bool) -> Bookmarks {
        Bookmarks {
            short,
            nodes: Cell::default(),
            hits: RefCell::default(),
            open: RefCell::default(),
            sources: RefCell::default(),
            drag: RefCell::default(),
            menu: RefCell::default(),
        }
    }

    /// The outline's lines: the tree with the open folders opened.
    fn lines(&self, shell: &Shell, roots: &[Bookmark]) -> Vec<Line> {
        fn walk(
            shell: &Shell,
            nodes: &[Bookmark],
            depth: usize,
            open: &HashSet<String>,
            out: &mut Vec<Line>,
        ) {
            for b in nodes {
                let is_open = b.is_folder() && open.contains(&b.id);
                out.push(Line::Node {
                    id: b.id.clone(),
                    title: b.title.clone(),
                    url: b.url.clone(),
                    depth,
                    open: is_open,
                    count: if b.is_folder() { count(b.kids()) } else { 0 },
                    icon: b.url.as_deref().and_then(|u| shell.cached_icon(u)),
                });
                if is_open {
                    if b.kids().is_empty() {
                        out.push(Line::Empty(depth + 1));
                    } else {
                        walk(shell, b.kids(), depth + 1, open, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(shell, roots, 0, &self.open.borrow(), &mut out);
        out
    }

    /// The row a node belongs to.
    fn row_of(&self, shell: &Shell, id: NodeId) -> Option<(NodeId, RowOf)> {
        let hits = self.hits.borrow();
        let rows: Vec<(NodeId, RowOf)> = hits
            .iter()
            .filter_map(|(n, h)| match h {
                Hit::Row(r) => Some((*n, r.clone())),
                _ => None,
            })
            .collect();
        drop(hits);
        shell
            .view()
            .chrome()
            .read(move |ui, _| rows.into_iter().find(|(n, _)| ui.tree.is_within(id, *n)))
            .flatten()
    }

    /// Put a drag's looks away.
    fn end_drag(&self, shell: &Shell) {
        let Some(d) = self.drag.borrow_mut().take() else {
            return;
        };
        let outline = self.nodes.get().outline;
        shell.view().chrome().edit(move |ui, _| {
            ui.tree.animate(d.row, Prop::Opacity, 1.0, motion::QUICK);
            light(ui, &d.target, outline, false);
        });
    }
}

/// How many sites, folders opened.
fn count(nodes: &[Bookmark]) -> usize {
    nodes
        .iter()
        .map(|b| if b.is_folder() { count(b.kids()) } else { 1 })
        .sum()
}

/// Light (or put out) where a drag would land.
fn light(ui: &mut Ui, t: &Target, outline: Option<NodeId>, on: bool) {
    match t {
        Target::Folder(_, row) => ui.tree.set_selected(*row, on),
        Target::Top => {
            if let Some(o) = outline {
                ui.tree.set_selected(o, on);
            }
        }
        Target::Nowhere => {}
    }
}

impl Panel for Bookmarks {
    fn id(&self) -> &'static str {
        if self.short { LIST } else { MANAGER }
    }

    fn title(&self) -> String {
        "Bookmarks".into()
    }

    fn name(&self) -> String {
        if self.short {
            String::new()
        } else {
            "Show Bookmarks".into()
        }
    }

    fn glyph(&self) -> char {
        glyph::BOOKMARKS
    }

    fn width(&self) -> f32 {
        if self.short { 280.0 } else { 600.0 }
    }

    fn place(&self) -> Place {
        if self.short {
            Place::Door {
                radius: 14.0,
                tall: 360.0 + 80.0,
            }
        } else {
            Place::Sheet
        }
    }

    fn build(&self, ui: &mut Ui, p: &Plate) {
        let mut hits = self.hits.borrow_mut();
        hits.clear();
        *self.drag.borrow_mut() = None;
        let list = ui.tree.add(p.body, Node::column().shrink(1.0));
        if self.short {
            plate::hairline(ui, p.body);
            let actions = ui
                .tree
                .add(p.body, Node::column().gap(1.0).padding(Insets::all(6.0)));
            let add = foot_row(ui, actions, Some(glyph::STAR), "Add This Page");
            let manage = foot_row(ui, actions, None, "Manage Bookmarks…");
            hits.insert(add, Hit::Add);
            hits.insert(manage, Hit::Manage);
            self.nodes.set(Nodes {
                list: Some(list),
                ..Nodes::default()
            });
        } else {
            let bring = ui.tree.add(
                p.foot,
                Node::row().gap(8.0).align(Align::Center).shrink(1.0),
            );
            ui.tree.add(p.foot, Node::new().grow(1.0));
            let count = ui
                .tree
                .add(p.foot, Node::text("", 12.0).ink(Token::Muted).shrink(0.0));
            p.show_foot(ui, true);
            self.nodes.set(Nodes {
                list: Some(list),
                count: Some(count),
                bring: Some(bring),
                ..Nodes::default()
            });
        }
    }

    fn refresh(&self, shell: &Shell) {
        let Nodes {
            list, count, bring, ..
        } = self.nodes.get();
        let Some(list) = list else { return };
        let roots = shell.bookmarks().roots().to_vec();
        let total = count_all(&roots);
        let lines = self.lines(shell, &roots);
        let short = self.short;
        // The browsers to bring bookmarks in from, found once per showing.
        let sources: Vec<String> = if short {
            Vec::new()
        } else {
            let mut s = self.sources.borrow_mut();
            s.get_or_insert_with(|| {
                copper_import::config_dir()
                    .map(|c| copper_import::detect(&c))
                    .unwrap_or_default()
            })
            .iter()
            .map(|b| b.name.clone())
            .collect()
        };
        let made = shell.view().chrome().read(move |ui, st| {
            let mut hits: Vec<(NodeId, Hit)> = Vec::new();
            ui.tree.clear_children(list);
            if let Some(c) = count {
                ui.tree.set_text(
                    c,
                    &if total == 1 {
                        "1 bookmark".to_string()
                    } else {
                        format!("{total} bookmarks")
                    },
                );
            }
            if let Some(b) = bring
                && ui.tree.children(b).is_empty()
                && !sources.is_empty()
            {
                ui.tree
                    .add(b, Node::text("Bring in from", 12.0).ink(Token::Muted));
                for (i, name) in sources.iter().enumerate() {
                    let pill = plate::pill(ui, b, name);
                    hits.push((pill, Hit::Bring(i)));
                }
            }
            if lines.is_empty() {
                if short {
                    ui.tree.add(
                        list,
                        Node::text("No bookmarks yet", 12.5)
                            .ink(Token::Muted)
                            .padding(Insets::all(14.0)),
                    );
                } else {
                    let card = plate::card(ui, list);
                    plate::nothing(
                        ui,
                        card,
                        "Nothing kept yet. Add this page with Ctrl+Shift+B, or bring yours in below.",
                    );
                }
                return (hits, None);
            }
            let outline_parent = if short {
                ui.tree.add(
                    list,
                    Node::scroll().max_height(360.0).shrink(1.0),
                )
            } else {
                let scroll = ui.tree.add(
                    list,
                    Node::scroll()
                        .max_height(440.0)
                        .padding(Insets::new(0.0, 0.0, 0.0, 2.0))
                        .shrink(1.0),
                );
                plate::card(ui, scroll)
            };
            // The outline: hit, so the space between rows is the top
            // level's drop target.
            let outline = ui.tree.add(
                outline_parent,
                Node::column()
                    .gap(1.0)
                    .padding(Insets::all(6.0))
                    .selected_fill(Token::Wash)
                    .hit()
                    .role(Role::List)
                    .name("Bookmarks"),
            );
            for line in &lines {
                match line {
                    Line::Empty(depth) => {
                        ui.tree.add(
                            outline,
                            Node::text("Empty", 12.0).ink(Token::Faint).padding(Insets::new(
                                *depth as f32 * 18.0 + 26.0,
                                5.0,
                                0.0,
                                5.0,
                            )),
                        );
                    }
                    Line::Node {
                        id,
                        title,
                        url,
                        depth,
                        open,
                        count,
                        icon,
                    } => {
                        let r = ui.tree.add(
                            outline,
                            Node::row()
                                .gap(8.0)
                                .padding(Insets::new(
                                    *depth as f32 * 18.0 + 10.0,
                                    6.0,
                                    10.0,
                                    6.0,
                                ))
                                .radius(8.0)
                                .hover_fill(Token::Wash)
                                .selected_fill(Token::Hover)
                                .align(Align::Center)
                                .button(title),
                        );
                        if url.is_none() {
                            ui.tree.add(
                                r,
                                Node::glyph(
                                    if *open { glyph::CHEVRON_DOWN } else { glyph::CHEVRON },
                                    9.0,
                                    10.0,
                                )
                                .weight(Weight::Semibold)
                                .ink(Token::Faint)
                                .role(Role::None),
                            );
                            let m = plate::mark(ui, r, None, "", 15.0);
                            ui.tree.add(
                                m,
                                Node::glyph(glyph::FOLDER_FILL, 9.0, 15.0)
                                    .ink(Token::Muted)
                                    .role(Role::None),
                            );
                        } else {
                            ui.tree.add(r, Node::new().width(10.0).shrink(0.0));
                            let host = url
                                .as_deref()
                                .map(copper_shell_core::shell::host_of)
                                .unwrap_or_default();
                            let picture = icon.as_ref().map(|i| st.images.id(ui, i));
                            plate::mark(ui, r, picture, &plate::letter(&host), 15.0);
                        }
                        ui.tree.add(
                            r,
                            Node::text(title, 12.5)
                                .ink(Token::Ink)
                                .trim(Trim::End)
                                .shrink(1.0)
                                .role(Role::None),
                        );
                        ui.tree.add(r, Node::new().grow(1.0).min_width(8.0));
                        if url.is_none() && *count > 0 {
                            ui.tree.add(
                                r,
                                Node::text(&count.to_string(), 11.0)
                                    .ink(Token::Faint)
                                    .role(Role::None),
                            );
                        }
                        hits.push((
                            r,
                            Hit::Row(RowOf {
                                id: id.clone(),
                                url: url.clone(),
                            }),
                        ));
                    }
                }
            }
            (hits, Some(outline))
        });
        let Some((made, outline)) = made else {
            super::refresh_soon(shell, self.id());
            return;
        };
        let mut n = self.nodes.get();
        n.outline = outline;
        self.nodes.set(n);
        let mut h = self.hits.borrow_mut();
        h.retain(|_, v| matches!(v, Hit::Add | Hit::Manage | Hit::Bring(_)));
        h.extend(made);
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        if self.drag.borrow().is_some() {
            // A drag let go where it began: nothing to do.
            self.end_drag(shell);
            return true;
        }
        let Some(hit) = self.hits.borrow().get(&id).cloned() else {
            return false;
        };
        match hit {
            Hit::Row(RowOf { id, url: None }) => {
                {
                    let mut open = self.open.borrow_mut();
                    if !open.remove(&id) {
                        open.insert(id);
                    }
                }
                self.refresh(shell);
            }
            Hit::Row(RowOf { url: Some(url), .. }) => {
                let apart = super::ctrl_down();
                shell.view().panels().hide(shell);
                super::visit(shell, &url, apart);
            }
            Hit::Add => bookmark_current(shell),
            Hit::Manage => super::show(shell, MANAGER),
            Hit::Bring(i) => {
                let b = self
                    .sources
                    .borrow()
                    .as_ref()
                    .and_then(|s| s.get(i).cloned());
                if let Some(b) = b {
                    bring_in(shell, b);
                }
            }
        }
        true
    }

    fn context_menu(&self, shell: &Shell, target: NodeId, at: Point) -> bool {
        let Some((_, row)) = self.row_of(shell, target) else {
            return false;
        };
        let b = shell.bookmarks();
        let Some(node) = b.find(&row.id).cloned() else {
            return false;
        };
        let folders: Vec<(String, String, usize)> = b
            .folders()
            .into_iter()
            .filter(|(f, _)| !node.holds(&f.id))
            .map(|(f, d)| (f.id.clone(), f.title.clone(), d))
            .collect();
        drop(b);
        let mut acts = Vec::new();
        let mut menu = Menu::new();
        if let Some(url) = &node.url {
            menu = menu.item(acts.len() as u32, "Open");
            acts.push(Act::Open(url.clone()));
            menu = menu.separator();
        }
        // No submenus here: "Move to" heads the places it can go.
        menu = menu.item(u32::MAX, "Move to").disabled();
        menu = menu.item(acts.len() as u32, "    Top Level");
        acts.push(Act::Move(node.id.clone(), None));
        for (id, title, depth) in folders {
            let label = format!("    {}{title}", "   ".repeat(depth));
            menu = menu.item(acts.len() as u32, &label);
            acts.push(Act::Move(node.id.clone(), Some(id)));
        }
        menu = menu.separator();
        menu = menu.item(acts.len() as u32, "Remove");
        acts.push(Act::Remove(node.id.clone()));
        *self.menu.borrow_mut() = acts;
        shell
            .view()
            .chrome()
            .edit(move |ui, _| ui.show_menu(ROW_MENU, &menu, at));
        true
    }

    fn menu(&self, shell: &Shell, menu: u32, item: u32) -> bool {
        if menu != ROW_MENU {
            return false;
        }
        let act = self.menu.borrow().get(item as usize).cloned();
        match act {
            Some(Act::Open(url)) => {
                shell.view().panels().hide(shell);
                super::visit(shell, &url, false);
            }
            Some(Act::Move(id, into)) => move_to(shell, &id, into.as_deref()),
            Some(Act::Remove(id)) => remove(shell, &id),
            None => {}
        }
        true
    }

    fn ticks(&self) -> bool {
        true
    }

    /// Follow a drag (see the module docs).
    fn tick(&self, shell: &Shell) {
        let Some((pressed, hovered)) = shell
            .view()
            .chrome()
            .read(|ui, _| (ui.tree.pressed(), ui.tree.hovered()))
        else {
            return;
        };
        let outline = self.nodes.get().outline;
        let dragging = self.drag.borrow().clone();
        match (dragging, pressed) {
            (None, Some(p)) if hovered != Some(p) => {
                let Some(Hit::Row(row)) = self.hits.borrow().get(&p).cloned() else {
                    return;
                };
                *self.drag.borrow_mut() = Some(Drag {
                    row: p,
                    id: row.id,
                    target: Target::Nowhere,
                });
                shell
                    .view()
                    .chrome()
                    .edit(move |ui, _| ui.tree.animate(p, Prop::Opacity, 0.35, motion::QUICK));
                self.tick(shell);
            }
            (Some(d), Some(_)) => {
                let target = match hovered {
                    Some(h) if h != d.row => match self.hits.borrow().get(&h) {
                        Some(Hit::Row(RowOf { id, url: None })) => Target::Folder(id.clone(), h),
                        Some(Hit::Row(_)) => Target::Nowhere,
                        _ if Some(h) == outline => Target::Top,
                        _ => Target::Nowhere,
                    },
                    _ => Target::Nowhere,
                };
                if target != d.target {
                    let old = d.target.clone();
                    let new = target.clone();
                    shell.view().chrome().edit(move |ui, _| {
                        light(ui, &old, outline, false);
                        light(ui, &new, outline, true);
                    });
                    if let Some(d) = self.drag.borrow_mut().as_mut() {
                        d.target = target;
                    }
                }
            }
            (Some(d), None) => {
                self.end_drag(shell);
                match d.target {
                    Target::Folder(into, _) => move_to(shell, &d.id, Some(&into)),
                    Target::Top => move_to(shell, &d.id, None),
                    Target::Nowhere => {}
                }
            }
            _ => {}
        }
    }

    fn closed(&self) {
        self.nodes.set(Nodes::default());
        self.hits.borrow_mut().clear();
        *self.sources.borrow_mut() = None;
        *self.drag.borrow_mut() = None;
        self.menu.borrow_mut().clear();
    }
}

/// How many sites in the whole tree.
fn count_all(roots: &[Bookmark]) -> usize {
    count(roots)
}

/// One of the dropdown's two actions (`Foot`).
fn foot_row(ui: &mut Ui, parent: NodeId, symbol: Option<char>, title: &str) -> NodeId {
    let r = ui.tree.add(
        parent,
        Node::row()
            .gap(8.0)
            .padding(Insets::hv(10.0, 6.0))
            .radius(8.0)
            .hover_fill(Token::Wash)
            .align(Align::Center)
            .justify(Justify::Start)
            .button(title),
    );
    match symbol {
        Some(g) => {
            ui.tree.add(
                r,
                Node::glyph(g, 11.0, 14.0)
                    .ink(Token::Muted)
                    .role(Role::None),
            );
        }
        None => {
            ui.tree.add(r, Node::new().width(14.0).shrink(0.0));
        }
    }
    ui.tree
        .add(r, Node::text(title, 12.5).ink(Token::Ink).role(Role::None));
    r
}
