//! Tabs across the top (`TabsPosition::Top`, `TabBar.swift`): pinned
//! squares, then titled pills sharing the room, a plus, the empty strip
//! that drags the window, back / forward / reload, and the caption
//! buttons' corner.
//!
//! Clicking the live tab turns it into the address (`TabAddressField`):
//! it widens to take the field, the address comes up short and selected,
//! Return goes there, Escape or looking away puts the tab back. `Ctrl+L`
//! is the plain field over the page, as in the column (`Browser.edit`).

use super::build::{door, mark, tip};
use super::doors;
use super::glyph;
use super::images::Images;
use super::model::{Model, Nav};
use crate::platform::Icon;
use copper_core::browser::TabId;
use copper_shell_core::TabView;
use copper_win_ui::chrome::Caption;
use copper_win_ui::tokens::{metrics, side};
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Justify, Node, NodeId, Role, Swatch, Token, Weight};
use std::collections::HashMap;
use std::rc::Rc;

/// A strip tab's height and its title's size (`TabBar.swift` 410–441).
const TAB_HEIGHT: f32 = 28.0;
const TITLE: f32 = 12.5;
const RADIUS: f32 = 8.0;
const DOOR: f32 = 26.0;
const DOOR_GLYPH: f32 = 13.0;
/// The live tab as the address: as wide as this, room allowing
/// (`TabBar.content`: `min(340, strip - lights - 12)`).
const EDIT_WIDTH: f32 = 340.0;
/// The field's line in it.
const EDIT_LINE: f32 = 16.0;

/// What a click in the strip means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Back,
    Forward,
    Reload,
    NewTab,
    Tab(TabId),
    Close(TabId),
}

/// What the strip last drew for a tab.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    id: TabId,
    label: String,
    letter: String,
    icon: Option<usize>,
    live: bool,
    pinned: bool,
}

/// The strip's nodes.
pub struct Strip {
    pub strip: NodeId,
    /// The downloads door (the Downloads panel draws what it shows).
    pub downloads: doors::DownloadsDoor,
    /// The bookmarks door (the Bookmarks list hangs from it).
    pub bookmarks: NodeId,
    run: NodeId,
    back: NodeId,
    forward: NodeId,
    reload: NodeId,
    reload_glyph: NodeId,
    drawn: Vec<Drawn>,
    width: f32,
    titled: usize,
    targets: HashMap<NodeId, Hit>,
    nav: Nav,
    /// The tab being edited in place and the words in its field.
    editing: Option<(TabId, String)>,
    /// The field, while it shows; `fresh` until it has been given its
    /// address, selected, and the keyboard.
    field: Option<NodeId>,
    fresh: bool,
    /// The field had the keyboard before a redraw (it gets it back).
    keyboard: bool,
    /// Its selection before a redraw.
    picked: Option<std::ops::Range<usize>>,
}

pub fn build(ui: &mut Ui, parent: NodeId) -> Strip {
    let mut targets = HashMap::new();
    let strip = ui.tree.add(
        parent,
        Node::row()
            .height(metrics::STRIP)
            .shrink(0.0)
            .padding(Insets::new(side::INSET, 0.0, 0.0, 0.0))
            .gap(metrics::TAB_GAP)
            .align(Align::Center)
            .drag()
            .layer()
            .hidden(true)
            .role(Role::Tab)
            .name("Tabs"),
    );
    let run = ui.tree.add(
        strip,
        Node::row()
            .gap(metrics::TAB_GAP)
            .align(Align::Center)
            .shrink(1.0)
            .clip(),
    );
    let (plus, _) = door(
        ui,
        strip,
        glyph::PLUS,
        metrics::PLUS_WIDTH,
        DOOR_GLYPH,
        "New Tab",
        &tip("New Tab", "new-tab"),
    );
    targets.insert(plus, Hit::NewTab);
    ui.tree.add(strip, Node::new().grow(1.0).min_width(24.0));
    let helm = ui.tree.add(
        strip,
        Node::row()
            .gap(side::HELM_GAP)
            .align(Align::Center)
            .padding(Insets::new(0.0, 0.0, 8.0, 0.0)),
    );
    let (back, _) = door(
        ui,
        helm,
        glyph::BACK,
        DOOR,
        DOOR_GLYPH,
        "Back",
        &tip("Back", "back"),
    );
    let (forward, _) = door(
        ui,
        helm,
        glyph::FORWARD,
        DOOR,
        DOOR_GLYPH,
        "Forward",
        &tip("Forward", "forward"),
    );
    let (reload, reload_glyph) = door(
        ui,
        helm,
        glyph::RELOAD,
        DOOR,
        DOOR_GLYPH,
        "Reload",
        &tip("Reload", "reload"),
    );
    targets.insert(back, Hit::Back);
    targets.insert(forward, Hit::Forward);
    targets.insert(reload, Hit::Reload);
    // The panels' doors, after the helm (`TabBar.swift` 107–122).
    let downloads = doors::strip_downloads(ui, strip);
    let bookmarks = doors::bookmarks(ui, strip);
    // The caption buttons' corner.
    ui.tree
        .add(strip, Node::new().width(Caption::width()).shrink(0.0));
    Strip {
        strip,
        downloads,
        bookmarks,
        run,
        back,
        forward,
        reload,
        reload_glyph,
        drawn: Vec::new(),
        width: 0.0,
        titled: 0,
        targets,
        nav: Nav::default(),
        editing: None,
        field: None,
        fresh: false,
        keyboard: false,
        picked: None,
    }
}

impl Strip {
    pub fn hit(&self, id: NodeId) -> Option<Hit> {
        self.targets.get(&id).cloned()
    }

    /// The tab a strip node (or anything in it) belongs to.
    pub fn tab_at(&self, ui: &Ui, id: NodeId) -> Option<TabId> {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if let Some(Hit::Tab(t) | Hit::Close(t)) = self.targets.get(&c) {
                return Some(t.clone());
            }
            cur = ui.tree.parent(c);
        }
        None
    }

    /// Draw `m`, sized for a window `window_w` DIPs wide.
    pub fn sync(&mut self, ui: &mut Ui, images: &mut Images, m: &Model) {
        self.sync_nav(ui, m.nav);
        let tabs: Vec<&TabView<Icon>> = m.visible.all().collect();
        let drawn: Vec<Drawn> = tabs
            .iter()
            .map(|t| Drawn {
                id: t.id.clone(),
                label: t.label.clone(),
                letter: t.letter.clone(),
                icon: t.favicon.as_ref().map(|i| Rc::as_ptr(i) as usize),
                live: t.live,
                pinned: t.pin.is_some(),
            })
            .collect();
        let width = ui.size().w;
        let titled = drawn.iter().filter(|d| !d.pinned).count();
        // The edit ends when its tab is no longer the live, titled one.
        if let Some((id, _)) = &self.editing
            && !drawn.iter().any(|d| d.id == *id && d.live && !d.pinned)
        {
            self.editing = None;
        }
        if drawn == self.drawn
            && (width - self.width).abs() < 0.5
            && titled == self.titled
            && self.field.is_some() == self.editing.is_some()
        {
            return;
        }
        // What was typed so far, what is selected, and the keyboard
        // survive the redraw.
        if let (Some(f), Some((_, words))) = (self.field, self.editing.as_mut())
            && let Some(field) = ui.tree.get(f).and_then(|n| n.field_ref())
        {
            *words = field.edit.text().to_string();
            self.picked = Some(field.edit.selection());
        }
        let focused = self.fresh || (self.field.is_some() && ui.tree.focused() == self.field);
        self.keyboard = focused;
        self.field = None;
        self.width = width;
        self.titled = titled;
        let editing = self.editing.as_ref().map(|(id, _)| id.clone());
        let edit_w = self.edit_width(width);
        let tab_w = self.tab_width(width, &drawn, editing.as_ref().map(|_| edit_w));
        for c in ui.tree.children(self.run).to_vec() {
            self.forget(ui, c);
        }
        ui.tree.clear_children(self.run);
        for (t, d) in tabs.iter().zip(&drawn) {
            let edit = editing.as_ref() == Some(&d.id);
            let w = if d.pinned {
                metrics::PIN_WIDTH
            } else if edit {
                edit_w
            } else {
                tab_w
            };
            let icon = super::build::icon_id(ui, images, t.favicon.as_ref());
            self.add_tab(ui, t, d, w, icon, edit);
        }
        self.drawn = drawn;
    }

    /// The live tab `id` becomes the address, `text` in it, selected.
    pub fn begin_edit(&mut self, id: TabId, text: String) {
        self.editing = Some((id, text));
        self.fresh = true;
        self.picked = None;
        self.drawn.clear();
    }

    /// Put the tab back (`true` if it was being edited).
    pub fn end_edit(&mut self) -> bool {
        self.fresh = false;
        self.drawn.clear();
        self.editing.take().is_some()
    }

    /// The in-tab address field, while it shows.
    pub fn field(&self) -> Option<NodeId> {
        self.field
    }

    /// The width the edited tab takes in a window `window_w` wide.
    fn edit_width(&self, window_w: f32) -> f32 {
        let room = window_w
            - side::INSET
            - 12.0
            - metrics::PLUS_WIDTH
            - 24.0
            - metrics::HELM
            - Caption::width();
        room.clamp(metrics::TAB_WIDTH, EDIT_WIDTH)
    }

    /// Room for the titled tabs: what the strip leaves, shared, between
    /// the minimum and the fixed width.
    fn tab_width(&self, window_w: f32, drawn: &[Drawn], edit: Option<f32>) -> f32 {
        let pinned = drawn.iter().filter(|d| d.pinned).count() as f32;
        let edited = if edit.is_some() { 1.0 } else { 0.0 };
        let titled = drawn.len() as f32 - pinned - edited;
        let fixed = edit.map_or(0.0, |w| w + metrics::TAB_GAP)
            + side::INSET
            + pinned * (metrics::PIN_WIDTH + metrics::TAB_GAP)
            + metrics::PLUS_WIDTH
            + 24.0
            + metrics::HELM
            + Caption::width();
        if titled < 1.0 {
            return metrics::TAB_WIDTH;
        }
        let each = (window_w - fixed) / titled - metrics::TAB_GAP;
        each.clamp(metrics::TAB_MIN_WIDTH, metrics::TAB_WIDTH)
    }

    fn add_tab(
        &mut self,
        ui: &mut Ui,
        t: &TabView<Icon>,
        d: &Drawn,
        w: f32,
        icon: Option<u32>,
        edit: bool,
    ) {
        let mut n = Node::row()
            .size(w, TAB_HEIGHT)
            .shrink(0.0)
            .padding(Insets::hv(if d.pinned { 7.0 } else { 9.0 }, 0.0))
            .gap(7.0)
            .align(Align::Center)
            .justify(if d.pinned || w < metrics::TAB_TITLED {
                Justify::Center
            } else {
                Justify::Start
            })
            .radius(RADIUS)
            .hover_fill(Token::ColumnHover)
            .button(&t.label)
            .tooltip(&t.label)
            .role(Role::TabItem)
            .selected(d.live);
        if d.live {
            n = n
                .selected_fill(Token::Pill)
                .stroke(Token::Rim, 1.0)
                .shadow(copper_win_ui::tokens::Shadow::new(
                    1.0,
                    side::PILL_SHADOW_RADIUS,
                    side::PILL_SHADOW_Y,
                ))
                .shadow_ink(Token::Lift);
        }
        let node = ui.tree.add(self.run, n);
        if edit {
            // `TabAddressField`: 12.5 in the ink, a line of 16, no mark,
            // no cross.
            ui.tree.edit(node, |n| n.tooltip = None);
            let field = ui.tree.add(
                node,
                Node::field("", TITLE)
                    .height(EDIT_LINE)
                    .grow(1.0)
                    .ink(Token::ColumnInk)
                    .name("Address"),
            );
            let words = self
                .editing
                .as_ref()
                .map(|(_, w)| w.clone())
                .unwrap_or_default();
            ui.set_field_text(field, &words, self.fresh);
            if let Some(r) = self.picked.take().filter(|_| !self.fresh) {
                ui.edit_field(field, |e| e.select(r));
            }
            if self.keyboard {
                ui.focus(Some(field));
            }
            self.fresh = false;
            self.field = Some(field);
            return;
        }
        mark(ui, node, icon, &t.letter, side::MARK);
        self.targets.insert(node, Hit::Tab(t.id.clone()));
        if d.pinned || w < metrics::TAB_TITLED {
            return;
        }
        ui.tree.add(
            node,
            Node::text(&t.label, TITLE)
                .weight(if d.live {
                    Weight::Semibold
                } else {
                    Weight::Regular
                })
                .ink(Token::ColumnInk)
                .grow(1.0)
                .role(Role::None),
        );
        let close = ui.tree.add(
            node,
            Node::stack()
                .size(side::CLOSE, side::CLOSE)
                .shrink(0.0)
                .radius(side::CLOSE / 2.0)
                .fill(Swatch::Fade(Token::ColumnInk, side::CLOSE_FILL))
                .align(Align::Center)
                .justify(Justify::Center)
                .reveal()
                .button("Close tab")
                .tooltip(&tip("Close tab", "close-tab")),
        );
        ui.tree.add(
            close,
            Node::glyph(glyph::CLOSE, side::CLOSE_GLYPH, side::CLOSE)
                .ink(Token::ColumnMuted)
                .role(Role::None),
        );
        self.targets.insert(close, Hit::Close(t.id.clone()));
    }

    fn forget(&mut self, ui: &Ui, root: NodeId) {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            self.targets.remove(&n);
            stack.extend_from_slice(ui.tree.children(n));
        }
    }

    pub fn sync_nav(&mut self, ui: &mut Ui, nav: Nav) {
        if nav == self.nav {
            return;
        }
        self.nav = nav;
        for (id, on) in [
            (self.back, nav.back),
            (self.forward, nav.forward),
            (self.reload, nav.page),
        ] {
            ui.tree.edit(id, |n| {
                n.flags.disabled = !on;
                n.transform.opacity = if on { 1.0 } else { 0.3 };
            });
        }
        let g = if nav.loading {
            glyph::STOP
        } else {
            glyph::RELOAD
        };
        ui.tree.set_text(self.reload_glyph, &g.to_string());
    }
}
