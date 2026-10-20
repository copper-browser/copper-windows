//! The column down the left (`Side.swift`): the head (the column door,
//! back / forward / reload), the address, the favourites, the space's rows
//! with the live one on a sliding pill, and the foot (doors, the space
//! chips, a new space).
//!
//! Rows are keyed by tab: a refresh updates the rows that changed, adds
//! and removes the rest and puts them in order, so a long column costs
//! what its changes do.

use super::build::{door, fill_mark, mark, tip};
use super::glyph;
use super::images::Images;
use super::model::{Model, Nav, SpaceView};
use crate::palette::symbols;
use crate::platform::Icon;
use copper_core::browser::TabId;
use copper_shell_core::TabView;
use copper_win_ui::tokens::{metrics, motion, side};
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Align, Insets, Justify, Node, NodeId, Point, Prop, Role, Swatch, TextAlign, Token, Trim, Weight,
};
use std::collections::HashMap;
use std::rc::Rc;

/// What a click in the column means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Fold,
    Back,
    Forward,
    Reload,
    Address,
    NewTab,
    Tab(TabId),
    Close(TabId),
    Settings,
    Bookmarks,
    Canvas,
    /// A chip: another space's switches to it, the current one's opens
    /// its page.
    Space(String),
    NewSpace,
    /// The header's name: the space menu.
    SpaceMenu,
    /// The header's mark: Settings › Spaces.
    SpaceMark,
}

/// One tab's row.
struct Row {
    node: NodeId,
    mark: NodeId,
    label: NodeId,
    close: NodeId,
    audio: NodeId,
    drawn: Drawn,
}

/// What a row last drew, to redraw only what changed.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    label: String,
    letter: String,
    icon: Option<usize>,
    live: bool,
    loading: bool,
    audio: bool,
    crashed: bool,
    private: bool,
}

impl Drawn {
    fn of(t: &TabView<Icon>) -> Drawn {
        Drawn {
            label: t.label.clone(),
            letter: t.letter.clone(),
            icon: t.favicon.as_ref().map(|i| Rc::as_ptr(i) as usize),
            live: t.live,
            loading: t.loading,
            audio: t.audio,
            crashed: t.crashed,
            private: t.private,
        }
    }
}

/// The column's nodes and what they show.
pub struct Column {
    pub side: NodeId,
    pub address: NodeId,
    address_text: NodeId,
    back: NodeId,
    forward: NodeId,
    reload: NodeId,
    reload_glyph: NodeId,
    pins_box: NodeId,
    pins_drawn: Vec<(TabId, Drawn)>,
    scroll: NodeId,
    list: NodeId,
    pill: NodeId,
    pill_placed: bool,
    /// Where the pill was put last (list DIPs).
    pill_y: f32,
    header: NodeId,
    header_name: NodeId,
    header_mark: NodeId,
    header_count: NodeId,
    header_drawn: Option<SpaceView>,
    divider: NodeId,
    new_tab: NodeId,
    chips_row: NodeId,
    chips_drawn: Vec<SpaceView>,
    rows: HashMap<TabId, Row>,
    /// Every pointer target in the column and what it means.
    targets: HashMap<NodeId, Hit>,
    nav: Nav,
}

/// The rows' scroll: the space's name, then the rows. The header keeps
/// the rows' rhythm (`GroupedRows.row`).
const HEADER: f32 = side::ROW;
/// The header's mark door: 24 square, 6 round, 6 before the name.
const HEADER_MARK: f32 = 24.0;
const HEADER_MARK_RADIUS: f32 = 6.0;
const HEADER_MARK_GAP: f32 = 6.0;
/// The mark drawn in it.
const HEADER_GLYPH: f32 = 16.0;
/// The count and the ellipsis: 11, the ellipsis in a 16 frame, 10 apart.
const HEADER_SMALL: f32 = 11.0;
const HEADER_MORE: f32 = 16.0;
const HEADER_GAP: f32 = 10.0;
/// A chip's mark (`SpaceGlyph(size: 17, bare: true)`).
const CHIP_GLYPH: f32 = 17.0;
/// The plus at the foot.
const PLUS_GLYPH: f32 = 15.0;
/// A resting chip's mark, and the second copy that lifts it to
/// [`side::CHIP_HOVER`] under the pointer (the two compose).
const CHIP_LIFT: f32 = 1.0 - (1.0 - side::CHIP_HOVER) / (1.0 - side::CHIP_RESTING);
/// The divider: a hairline with room above and below.
const DIVIDER: f32 = 1.0 + 2.0 * side::DIVIDER_PAD;

/// Build the column into `parent`.
pub fn build(ui: &mut Ui, parent: NodeId) -> Column {
    let mut targets = HashMap::new();
    let side_n = ui.tree.add(
        parent,
        Node::column()
            .width(metrics::SIDE)
            .shrink(0.0)
            .layer()
            .role(Role::Pane)
            .name("Tabs"),
    );

    // The head: the column door at the left (Windows has no lights there),
    // back / forward / reload at the far end. Empty space drags the window.
    let head = ui.tree.add(
        side_n,
        Node::row()
            .height(side::HEAD)
            .shrink(0.0)
            .padding(Insets::new(8.0, 0.0, 6.0, 0.0))
            .gap(side::HELM_GAP)
            .align(Align::Center)
            .drag(),
    );
    let (fold, _) = door(
        ui,
        head,
        glyph::SIDEBAR,
        side::HEAD_DOOR,
        side::HEAD_GLYPH,
        "Hide sidebar",
        &tip("Hide sidebar", "toggle-sidebar"),
    );
    targets.insert(fold, Hit::Fold);
    ui.tree.add(head, Node::new().grow(1.0));
    let (back, _) = door(
        ui,
        head,
        glyph::BACK,
        side::HELM_DOOR,
        side::HEAD_GLYPH,
        "Back",
        &tip("Back", "back"),
    );
    let (forward, _) = door(
        ui,
        head,
        glyph::FORWARD,
        side::HELM_DOOR,
        side::HEAD_GLYPH,
        "Forward",
        &tip("Forward", "forward"),
    );
    let (reload, reload_glyph) = door(
        ui,
        head,
        glyph::RELOAD,
        side::HELM_DOOR,
        side::HEAD_GLYPH,
        "Reload",
        &tip("Reload", "reload"),
    );
    targets.insert(back, Hit::Back);
    targets.insert(forward, Hit::Forward);
    targets.insert(reload, Hit::Reload);

    // The address: a button showing the host; clicking it opens the card
    // to edit the address (`SideAddress`).
    let bar = ui.tree.add(
        side_n,
        Node::row().shrink(0.0).padding(Insets::new(
            side::INSET,
            0.0,
            side::INSET,
            side::ADDRESS_BELOW,
        )),
    );
    let address = ui.tree.add(
        bar,
        Node::row()
            .height(side::ADDRESS_HEIGHT)
            .grow(1.0)
            .radius(side::ADDRESS_RADIUS)
            .fill(Token::Square)
            .hover_fill(Token::ColumnHover)
            .padding(Insets::new(
                side::ADDRESS_LEADING,
                0.0,
                side::ADDRESS_TRAILING,
                0.0,
            ))
            .align(Align::Center)
            .button("Address")
            .tooltip(&format!(
                "Change the address   {}",
                crate::keys::shortcut("focus-field").unwrap_or_default()
            )),
    );
    let address_text = ui.tree.add(
        address,
        Node::text(PLACEHOLDER, side::ADDRESS_TEXT)
            .ink(Token::ColumnFaint)
            .trim(Trim::Middle)
            .grow(1.0)
            .role(Role::None),
    );
    targets.insert(address, Hit::Address);

    // The favourites: a grid, only while there are any.
    let pins_box = ui.tree.add(
        side_n,
        Node::column()
            .shrink(0.0)
            .padding(Insets::new(side::INSET, 0.0, side::INSET, side::PINS_BELOW))
            .gap(side::PIN_GAP)
            .hidden(true)
            .role(Role::List)
            .name("Favourites"),
    );

    // The space's rows, in one scroll.
    let scroll = ui.tree.add(
        side_n,
        Node::scroll().grow(1.0).name("Open tabs").role(Role::List),
    );
    // The pill is a layer of its own (it glides), beneath the rows' layer:
    // a layer composes over its parent's painted content, so inside the
    // list it would cover the row it marks.
    let pill = ui.tree.add(
        scroll,
        Node::new()
            .absolute(copper_win_ui::Abs {
                left: Some(side::INSET),
                top: Some(side::SCROLL_TOP),
                right: Some(side::INSET),
                bottom: None,
            })
            .height(side::ROW)
            .fill(Token::Pill)
            .radius(side::ROW_RADIUS)
            // A one-point ring and a soft lift (`Side.swift` 711–716).
            .stroke(Token::Rim, 1.0)
            .shadow(copper_win_ui::tokens::Shadow::new(
                1.0,
                side::PILL_SHADOW_RADIUS,
                side::PILL_SHADOW_Y,
            ))
            .shadow_ink(Token::Lift)
            .layer()
            .opacity(0.0),
    );
    let list = ui.tree.add(
        scroll,
        Node::column()
            .padding(Insets::new(
                side::INSET,
                side::SCROLL_TOP,
                side::INSET,
                side::SCROLL_BOTTOM,
            ))
            .gap(side::GAP)
            .layer(),
    );
    // The header (`SpaceHeader`): the space's mark, its own door to
    // Settings › Spaces; the rest of the row is the space menu, with the
    // tab count and an ellipsis under the pointer.
    let header = ui.tree.add(
        list,
        Node::row()
            .height(HEADER)
            .shrink(0.0)
            .padding(Insets::new(
                side::ROW_INSET - 4.0,
                0.0,
                side::ROW_TRAILING,
                0.0,
            ))
            .align(Align::Center)
            .radius(side::ROW_RADIUS)
            .hover_fill(Token::ColumnHover)
            .button("Space"),
    );
    let header_mark = ui.tree.add(
        header,
        Node::stack()
            .size(HEADER_MARK, HEADER_MARK)
            .shrink(0.0)
            .radius(HEADER_MARK_RADIUS)
            .hover_fill(Token::ColumnHover)
            .align(Align::Center)
            .justify(Justify::Center)
            .button("Customize space"),
    );
    targets.insert(header_mark, Hit::SpaceMark);
    ui.tree
        .add(header, Node::new().width(HEADER_MARK_GAP).shrink(0.0));
    let header_name = ui.tree.add(
        header,
        Node::text("", side::SPACE_NAME)
            .weight(Weight::Semibold)
            .ink(Token::ColumnMuted)
            .shrink(1.0)
            .role(Role::None),
    );
    ui.tree
        .add(header, Node::new().grow(1.0).min_width(HEADER_GAP + 4.0));
    let header_count = ui.tree.add(
        header,
        Node::text("", HEADER_SMALL)
            .ink(Token::ColumnFaint)
            .shrink(0.0)
            .reveal()
            .role(Role::None),
    );
    ui.tree
        .add(header, Node::new().width(HEADER_GAP).shrink(0.0));
    ui.tree.add(
        header,
        Node::glyph(glyph::MORE, HEADER_SMALL, HEADER_MORE)
            .weight(Weight::Semibold)
            .ink(Token::ColumnMuted)
            .shrink(0.0)
            .reveal()
            .role(Role::None),
    );
    targets.insert(header, Hit::SpaceMenu);
    let divider = ui.tree.add(
        list,
        Node::column()
            .height(DIVIDER)
            .shrink(0.0)
            .padding(Insets::new(
                side::ROW_INSET,
                side::DIVIDER_PAD,
                side::ROW_INSET,
                side::DIVIDER_PAD,
            ))
            .hidden(true),
    );
    ui.tree
        .add(divider, Node::new().height(1.0).fill(Token::ColumnHairline));
    let new_tab = ui.tree.add(
        list,
        Node::row()
            .height(side::ROW)
            .shrink(0.0)
            .padding(Insets::new(side::ROW_INSET, 0.0, side::ROW_TRAILING, 0.0))
            .gap(side::MARK_GAP)
            .align(Align::Center)
            .radius(side::ROW_RADIUS)
            .hover_fill(Token::ColumnHover)
            .button("New tab")
            .tooltip(&tip("New tab", "new-tab"))
            .opacity(side::QUIET_REST),
    );
    ui.tree.add(
        new_tab,
        Node::glyph(glyph::PLUS, side::QUIET_GLYPH, side::MARK)
            .ink(Token::ColumnMuted)
            .role(Role::None),
    );
    ui.tree.add(
        new_tab,
        Node::text("New tab", side::TITLE_SIZE)
            .ink(Token::ColumnMuted)
            .grow(1.0)
            .role(Role::None),
    );
    targets.insert(new_tab, Hit::NewTab);

    // The foot: the doors, the space chips, a new space.
    let foot = ui.tree.add(
        side_n,
        Node::row()
            .shrink(0.0)
            .padding(Insets::new(
                side::FOOT_PAD_H,
                side::FOOT_PAD_TOP,
                side::FOOT_PAD_H,
                side::FOOT_PAD_BOTTOM,
            ))
            .gap(2.0)
            .align(Align::Center),
    );
    for (g, name, command, hit) in [
        (glyph::SETTINGS, "Settings", "settings", Hit::Settings),
        (
            glyph::BOOKMARKS,
            "Bookmarks",
            "bookmarks-door",
            Hit::Bookmarks,
        ),
        (glyph::CANVAS, "New canvas", "new-canvas", Hit::Canvas),
    ] {
        let (d, _) = door(
            ui,
            foot,
            g,
            side::FOOT_DOOR,
            side::FOOT_GLYPH,
            name,
            &tip(name, command),
        );
        targets.insert(d, hit);
    }
    ui.tree.add(foot, Node::new().grow(1.0).min_width(4.0));
    // The chips scroll sideways rather than shrink: the row is clipped and
    // slid so the current one stays in view.
    let chips = ui.tree.add(
        foot,
        Node::row()
            .shrink(1.0)
            .clip()
            .align(Align::Center)
            .role(Role::List)
            .name("Spaces"),
    );
    let chips_row = ui.tree.add(
        chips,
        Node::row()
            .shrink(0.0)
            .gap(side::CHIP_GAP)
            .padding(Insets::hv(0.0, 1.0))
            .align(Align::Center)
            .layer(),
    );
    ui.tree.add(foot, Node::new().grow(1.0).min_width(4.0));
    let (plus, _) = door(
        ui,
        foot,
        glyph::PLUS,
        side::FOOT_DOOR,
        PLUS_GLYPH,
        "New Space",
        "New Space \u{2014} right-click for New Canvas",
    );
    targets.insert(plus, Hit::NewSpace);

    Column {
        side: side_n,
        address,
        address_text,
        back,
        forward,
        reload,
        reload_glyph,
        pins_box,
        pins_drawn: Vec::new(),
        scroll,
        list,
        pill,
        pill_placed: false,
        pill_y: -1.0,
        header,
        header_name,
        header_mark,
        header_count,
        header_drawn: None,
        divider,
        new_tab,
        chips_row,
        chips_drawn: Vec::new(),
        rows: HashMap::new(),
        targets,
        nav: Nav::default(),
    }
}

/// The address field's words when there is no page (`Side.swift` 886).
pub const PLACEHOLDER: &str = "Search or Enter URL\u{2026}";

impl Column {
    /// What clicking `id` means, if it is the column's.
    pub fn hit(&self, id: NodeId) -> Option<Hit> {
        self.targets.get(&id).cloned()
    }

    /// The tab a row node (or anything in it) belongs to.
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

    /// Draw `m`.
    pub fn sync(&mut self, ui: &mut Ui, images: &mut Images, m: &Model) {
        self.sync_nav(ui, m.nav);
        self.sync_address(ui, &m.host);
        self.sync_pins(ui, images, &m.visible.pins);
        self.sync_header(ui, &m.space);
        self.sync_rows(ui, images, &m.visible.saved, &m.visible.today);
        self.sync_chips(ui, &m.spaces);
    }

    /// Back, forward, reload / stop.
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
        let (g, name) = if nav.loading {
            (glyph::STOP, tip("Stop", "escape"))
        } else {
            (glyph::RELOAD, tip("Reload", "reload"))
        };
        ui.tree.set_text(self.reload_glyph, &g.to_string());
        ui.tree.edit(self.reload, |n| n.tooltip = Some(name));
    }

    fn sync_address(&mut self, ui: &mut Ui, host: &str) {
        let (text, ink) = if host.is_empty() {
            (PLACEHOLDER, Swatch::Token(Token::ColumnFaint))
        } else {
            (host, Swatch::Fade(Token::ColumnInk, side::ADDRESS_INK))
        };
        let same = ui
            .tree
            .get(self.address_text)
            .is_some_and(|n| n.a11y.name == text && n.look.ink == ink);
        if !same {
            ui.tree.set_text(self.address_text, text);
            ui.tree.repaint(self.address_text, |n| n.look.ink = ink);
        }
    }

    fn sync_header(&mut self, ui: &mut Ui, space: &SpaceView) {
        if self.header_drawn.as_ref() == Some(space) {
            return;
        }
        let title = symbols::title(space.icon.as_deref(), &space.title);
        ui.tree.set_text(self.header_name, &title);
        ui.tree
            .set_text(self.header_count, &symbols::tabs(space.count));
        ui.tree.edit(self.header, |n| {
            n.a11y.name = title.clone();
            n.tooltip = Some(format!("{title} \u{2014} click for the space menu"));
        });
        ui.tree.edit(self.header_mark, |n| {
            n.a11y.name = format!("Customize {title}\u{2026}");
            n.tooltip = Some(format!("Customize {title}\u{2026}"));
        });
        ui.tree.clear_children(self.header_mark);
        space_mark(
            ui,
            self.header_mark,
            space,
            HEADER_GLYPH,
            Swatch::Token(Token::ColumnMuted),
            1.0,
        );
        self.header_drawn = Some(space.clone());
    }

    /// Where the space menu opens from the header: under its name.
    pub fn header_menu_point(&self, ui: &Ui) -> Point {
        let r = ui.tree.window_rect(self.header_name);
        Point::new(r.x, ui.tree.window_rect(self.header).bottom())
    }

    /// The spaces door `id` is part of (a chip, the header, its mark, the
    /// plus), for a right-click.
    pub fn space_hit(&self, ui: &Ui, id: NodeId) -> Option<Hit> {
        let mut cur = Some(id);
        while let Some(c) = cur {
            match self.targets.get(&c) {
                Some(h @ (Hit::Space(_) | Hit::SpaceMenu | Hit::SpaceMark | Hit::NewSpace)) => {
                    return Some(h.clone());
                }
                Some(_) => return None,
                None => cur = ui.tree.parent(c),
            }
        }
        None
    }

    fn sync_pins(&mut self, ui: &mut Ui, images: &mut Images, pins: &[TabView<Icon>]) {
        let drawn: Vec<(TabId, Drawn)> =
            pins.iter().map(|t| (t.id.clone(), Drawn::of(t))).collect();
        if drawn == self.pins_drawn {
            return;
        }
        for c in ui.tree.children(self.pins_box).to_vec() {
            self.forget_targets(ui, c);
        }
        ui.tree.clear_children(self.pins_box);
        ui.tree.set_hidden(self.pins_box, pins.is_empty());
        let cols = side::pin_columns(metrics::SIDE);
        let w = side::pin_width(metrics::SIDE);
        let h = side::pin_height(w);
        for chunk in pins.chunks(cols) {
            let line = ui.tree.add(self.pins_box, Node::row().gap(side::PIN_GAP));
            for t in chunk {
                let mut pin = Node::stack()
                    .size(w, h)
                    .shrink(0.0)
                    .radius(side::pin_radius(w, h))
                    .fill(Token::Square)
                    .hover_fill(Token::ColumnHover)
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .button(&t.label)
                    .tooltip(&t.label)
                    .selected(t.live);
                if t.live {
                    pin = pin
                        .selected_fill(Token::Pill)
                        .stroke(Token::Rim, 1.0)
                        .shadow(copper_win_ui::tokens::Shadow::new(
                            1.0,
                            side::PIN_SHADOW_RADIUS,
                            side::PIN_SHADOW_Y,
                        ))
                        .shadow_ink(Token::Lift);
                }
                let p = ui.tree.add(line, pin);
                let icon = super::build::icon_id(ui, images, t.favicon.as_ref());
                mark(ui, p, icon, &t.letter, side::pin_mark(h));
                self.targets.insert(p, Hit::Tab(t.id.clone()));
            }
        }
        self.pins_drawn = drawn;
    }

    fn forget_targets(&mut self, ui: &Ui, root: NodeId) {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            self.targets.remove(&n);
            stack.extend_from_slice(ui.tree.children(n));
        }
    }

    /// The rows: keep, update, add, remove, then order them and move the
    /// pill to the live one.
    fn sync_rows(
        &mut self,
        ui: &mut Ui,
        images: &mut Images,
        saved: &[TabView<Icon>],
        today: &[TabView<Icon>],
    ) {
        let wanted: Vec<&TabView<Icon>> = saved.iter().chain(today.iter()).collect();
        // Gone.
        let gone: Vec<TabId> = self
            .rows
            .keys()
            .filter(|id| !wanted.iter().any(|t| &t.id == *id))
            .cloned()
            .collect();
        for id in gone {
            if let Some(r) = self.rows.remove(&id) {
                self.forget_targets(ui, r.node);
                ui.tree.remove(r.node);
            }
        }
        // New and changed.
        for t in &wanted {
            let d = Drawn::of(t);
            match self.rows.get_mut(&t.id) {
                Some(r) if r.drawn == d => {}
                Some(r) => {
                    let icon = super::build::icon_id(ui, images, t.favicon.as_ref());
                    update_row(ui, r, &d, icon);
                    r.drawn = d;
                }
                None => {
                    let icon = super::build::icon_id(ui, images, t.favicon.as_ref());
                    let r = new_row(ui, self.list, t, icon);
                    self.targets.insert(r.node, Hit::Tab(t.id.clone()));
                    self.targets.insert(r.close, Hit::Close(t.id.clone()));
                    self.rows.insert(t.id.clone(), r);
                }
            }
        }
        // Order: header, saved, divider, new tab, today.
        let mut order: Vec<NodeId> = Vec::with_capacity(wanted.len() + 3);
        let header = ui.tree.parent(self.header_name).unwrap_or(self.header_name);
        order.push(header);
        order.extend(
            saved
                .iter()
                .filter_map(|t| self.rows.get(&t.id).map(|r| r.node)),
        );
        order.push(self.divider);
        order.push(self.new_tab);
        order.extend(
            today
                .iter()
                .filter_map(|t| self.rows.get(&t.id).map(|r| r.node)),
        );
        if ui.tree.children(self.list) != order.as_slice() {
            for (i, id) in order.iter().enumerate() {
                if ui.tree.children(self.list).get(i) != Some(id) {
                    ui.tree.reorder(*id, i);
                }
            }
        }
        ui.tree
            .set_hidden(self.divider, saved.is_empty() || today.is_empty());

        // The pill rides under the live row.
        let mut y = 0.0;
        let mut at = None;
        let step = side::ROW + side::GAP;
        y += HEADER + side::GAP;
        for t in saved {
            if t.live {
                at = Some(y);
            }
            y += step;
        }
        if !saved.is_empty() && !today.is_empty() {
            y += DIVIDER + side::GAP;
        }
        y += step; // New tab.
        for t in today {
            if t.live {
                at = Some(y);
            }
            y += step;
        }
        if let Some(y) = at
            && y != self.pill_y
        {
            self.pill_y = y;
            self.keep_in_view(ui, y);
        }
        match at {
            Some(y) if self.pill_placed => {
                ui.tree.animate(self.pill, Prop::Y, y, motion::GLIDE);
                ui.tree
                    .animate(self.pill, Prop::Opacity, 1.0, motion::QUICK);
            }
            Some(y) => {
                ui.tree.set_prop(self.pill, Prop::Y, y);
                ui.tree.set_prop(self.pill, Prop::Opacity, 1.0);
                self.pill_placed = true;
            }
            None => {
                ui.tree
                    .animate(self.pill, Prop::Opacity, 0.0, motion::QUICK);
            }
        }
    }

    /// Glide the rows so the live one (at `y` in the list) is in view.
    fn keep_in_view(&self, ui: &mut Ui, y: f32) {
        let pad = side::SCROLL_TOP;
        let view = ui.tree.get(self.scroll).map_or(0.0, |n| n.frame.h);
        if view <= 0.0 {
            return;
        }
        let top = pad + y;
        let off = ui.tree.scroll_offset(self.scroll);
        let to = if top - pad < off {
            top - pad
        } else if top + side::ROW + pad > off + view {
            top + side::ROW + pad - view
        } else {
            return;
        };
        ui.tree.scroll(self.scroll, |s| s.scroll_to(to, true));
    }

    /// The space chips (`SpaceStrip`): rebuilt when the spaces change. One
    /// space still shows its chip — the way to its page.
    fn sync_chips(&mut self, ui: &mut Ui, spaces: &[SpaceView]) {
        if spaces == self.chips_drawn.as_slice() {
            return;
        }
        for c in ui.tree.children(self.chips_row).to_vec() {
            self.forget_targets(ui, c);
        }
        ui.tree.clear_children(self.chips_row);
        for s in spaces {
            let title = symbols::title(s.icon.as_deref(), &s.title);
            let help = if s.current {
                format!("{title} \u{2014} click to customize")
            } else {
                title.clone()
            };
            let mut chip = Node::stack()
                .size(side::CHIP, side::CHIP)
                .shrink(0.0)
                .radius(side::CHIP_RADIUS)
                .align(Align::Center)
                .justify(Justify::Center)
                .button(&title)
                .tooltip(&help);
            if !s.current {
                chip = chip.hover_fill(Token::ColumnHover);
            }
            let chip = ui.tree.add(self.chips_row, chip);
            ui.tree.edit(chip, |n| n.flags.selected = s.current);
            let ink = Swatch::Token(Token::ColumnInk);
            if s.current {
                space_mark(ui, chip, s, CHIP_GLYPH, ink, 1.0);
            } else {
                // Thinned at rest; a second copy lifts it under the pointer.
                space_mark(ui, chip, s, CHIP_GLYPH, ink, side::CHIP_RESTING);
                let lift = space_mark(ui, chip, s, CHIP_GLYPH, ink, CHIP_LIFT);
                ui.tree.edit(lift, |n| n.flags.reveal = true);
            }
            self.targets.insert(chip, Hit::Space(s.id.clone()));
        }
        // More chips than room: slide the row so the current one is in
        // the middle of what shows.
        let n = spaces.len() as f32;
        let whole = n * side::CHIP + (n - 1.0).max(0.0) * side::CHIP_GAP;
        let room = chip_room();
        let at = spaces.iter().position(|s| s.current).unwrap_or(0) as f32;
        let centre = at * (side::CHIP + side::CHIP_GAP) + side::CHIP / 2.0;
        let slide = if whole > room {
            (centre - room / 2.0).clamp(0.0, whole - room)
        } else {
            0.0
        };
        ui.tree
            .animate(self.chips_row, Prop::X, -slide.round(), motion::GLIDE);
        self.chips_drawn = spaces.to_vec();
    }
}

/// How wide the chips' window is: the foot less its padding, its three
/// doors, the plus and the two spacers either side of the chips.
fn chip_room() -> f32 {
    metrics::SIDE - 2.0 * side::FOOT_PAD_H - 4.0 * side::FOOT_DOOR - 3.0 * 2.0 - 2.0 * (4.0 + 2.0)
}

/// Draw `space`'s mark (`SpaceGlyph`, bare) into `parent`, `size` square:
/// its emoji, its symbol, or a dot — never a letter. Returns the mark.
fn space_mark(
    ui: &mut Ui,
    parent: NodeId,
    space: &SpaceView,
    size: f32,
    ink: Swatch,
    opacity: f32,
) -> NodeId {
    let node = match symbols::mark(space.icon.as_deref(), &space.title) {
        symbols::Mark::Emoji(e) => Node::text(&e, (size * 0.9).round())
            .text_align(TextAlign::Center)
            .size(size, size),
        symbols::Mark::Symbol(g) => Node::glyph(g, (size * 0.78).round(), size)
            .weight(Weight::Semibold)
            .ink(ink),
        symbols::Mark::Dot => {
            let d = size * 0.42;
            Node::new().size(d, d).radius(d / 2.0).fill(ink)
        }
    };
    let node = node.opacity(opacity).role(Role::None);
    let holder = ui.tree.add(
        parent,
        Node::stack()
            .absolute(copper_win_ui::Abs::FILL)
            .align(Align::Center)
            .justify(Justify::Center)
            .role(Role::None),
    );
    ui.tree.add(holder, node);
    holder
}

/// A new row for `t`, appended to `list` (ordered afterwards).
fn new_row(ui: &mut Ui, list: NodeId, t: &TabView<Icon>, icon: Option<u32>) -> Row {
    let d = Drawn::of(t);
    let node = ui.tree.add(
        list,
        Node::row()
            .height(side::ROW)
            .shrink(0.0)
            .padding(Insets::new(side::ROW_INSET, 0.0, side::ROW_TRAILING, 0.0))
            .gap(side::MARK_GAP)
            .align(Align::Center)
            .radius(side::ROW_RADIUS)
            .hover_fill(Token::ColumnHover)
            .button(&t.label)
            .role(Role::ListItem),
    );
    let m = mark(ui, node, icon, &t.letter, side::MARK);
    let label = ui.tree.add(
        node,
        Node::text(&t.label, side::TITLE_SIZE)
            .ink(Token::ColumnInk)
            .grow(1.0)
            .role(Role::None),
    );
    let audio = ui.tree.add(
        node,
        Node::glyph(glyph::AUDIO, side::CLOSE_GLYPH + 1.0, side::CLOSE)
            .ink(Token::ColumnMuted)
            .hidden(true)
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
    let mut r = Row {
        node,
        mark: m,
        label,
        close,
        audio,
        drawn: d.clone(),
    };
    style_row(ui, &mut r, &d);
    r
}

/// Redraw what changed in a row.
fn update_row(ui: &mut Ui, r: &mut Row, d: &Drawn, icon: Option<u32>) {
    if r.drawn.label != d.label {
        ui.tree.set_text(r.label, &d.label);
        let label = d.label.clone();
        ui.tree.edit(r.node, |n| n.a11y.name = label);
    }
    if r.drawn.icon != d.icon || r.drawn.letter != d.letter {
        fill_mark(ui, r.mark, icon, &d.letter, side::MARK);
    }
    style_row(ui, r, d);
}

/// The row's state: live (semibold, selected), loading (a quieter mark),
/// playing audio, crashed.
fn style_row(ui: &mut Ui, r: &mut Row, d: &Drawn) {
    ui.tree.set_weight(
        r.label,
        if d.live {
            Weight::Semibold
        } else {
            Weight::Regular
        },
    );
    ui.tree.set_selected(r.node, d.live);
    let mark_alpha = if d.loading {
        0.45
    } else if d.live {
        1.0
    } else {
        side::MARK_RESTING
    };
    ui.tree.set_prop(r.mark, Prop::Opacity, mark_alpha);
    ui.tree.set_hidden(r.audio, !d.audio);
    let ink = if d.crashed {
        Swatch::Token(Token::ColumnMuted)
    } else {
        Swatch::Token(Token::ColumnInk)
    };
    ui.tree.repaint(r.label, |n| n.look.ink = ink);
}
