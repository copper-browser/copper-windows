//! Settings' frame (`SettingsPanel.body`, `Settings.swift` 83–200): the
//! panel sized against the window, the rail on the left — title, search
//! field, grouped pages — and on the right the page's header with its
//! cross, the hairline that shows once the page scrolls, and the page.
//!
//! Built once into the host's plate (whose own head is hidden: Settings
//! has its header in the page column, as on the Mac), then restyled
//! whenever the window turns light or dark or changes size.

use super::controls;
use super::draw::{Drawn, Hit};
use super::look::{self, Inks, metric};
use crate::chrome::glyph;
use crate::panels::plate::Plate;
use copper_win_ui::tokens::Shadow;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Dim, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim, Weight};

/// The frame's nodes.
#[derive(Clone, Copy, Debug)]
pub struct Nodes {
    pub scrim: NodeId,
    pub plate: NodeId,
    pub rail_box: NodeId,
    /// The rail's scrolling list of groups.
    pub rail: NodeId,
    pub search_well: NodeId,
    pub search: NodeId,
    /// `Ctrl+F`, while the field is empty.
    pub hint: NodeId,
    /// The clear cross, while it is not.
    pub clear: NodeId,
    pub divider: NodeId,
    pub content: NodeId,
    pub title: NodeId,
    pub blurb: NodeId,
    pub close: NodeId,
    /// The hairline under the header, shown once the page has scrolled.
    pub rule: NodeId,
    /// The page (or the results): a scroll column.
    pub list: NodeId,
}

/// Room the panel leaves around itself in a roomy window: 80 short of the
/// window on each axis (`span`).
const MARGIN: f32 = 40.0;

/// Build the frame into the host's plate.
pub fn build(ui: &mut Ui, p: &Plate) -> Nodes {
    let plate = ui.tree.parent(p.body).expect("the body is in the plate");
    // The host's title row: Settings' title is in its rail, its cross in
    // the page's header.
    if let Some(head) = ui.tree.parent(p.close) {
        ui.tree.set_hidden(head, true);
    }
    // As big as reads well, never past the window (`span`): the layout
    // fills the scrim's padded box, clamped to the most and the least.
    ui.tree
        .edit(p.scrim, |n| n.style.padding = Insets::all(MARGIN));
    ui.tree.edit(plate, |n| {
        n.style.width = Dim::Fill;
        n.style.height = Dim::Fill;
        n.style.min_w = 600.0;
        n.style.max_w = 920.0;
        n.style.min_h = 440.0;
        n.style.max_h = 700.0;
        n.look.radius = metric::RADIUS;
        n.look.shadow = Some(Shadow {
            alpha: metric::SHADOW_ALPHA,
            radius: metric::SHADOW_RADIUS,
            y: metric::SHADOW_Y,
        });
        n.a11y.name = "Settings".into();
    });
    ui.tree.edit(p.body, |n| {
        n.style.padding = Insets::new(0.0, 0.0, 0.0, 0.0);
        n.style.gap = 0.0;
        n.style.grow = 1.0;
    });
    let row = ui
        .tree
        .add(p.body, Node::row().grow(1.0).fill_width().fill_height());

    // — the rail —
    let rail_box = ui.tree.add(
        row,
        Node::column()
            .width(220.0)
            .fill_height()
            .padding(Insets::hv(metric::RAIL_INSET, 0.0))
            .shrink(0.0),
    );
    ui.tree.add(
        rail_box,
        Node::text("Settings", metric::RAIL_TITLE)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .padding(Insets::new(6.0, 20.0, 6.0, 12.0))
            .role(Role::TitleBar),
    );
    let search_well = ui.tree.add(
        rail_box,
        Node::row()
            .height(metric::SEARCH_H)
            .padding(Insets::hv(10.0, 0.0))
            .gap(7.0)
            .radius(8.0)
            .align(Align::Center)
            .shrink(0.0),
    );
    ui.tree.add(
        search_well,
        Node::glyph(glyph::SEARCH, 12.0, 14.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let search = ui.tree.add(
        search_well,
        Node::field("Search settings", metric::SEARCH_TEXT)
            .fill_height()
            .grow(1.0)
            .name("Search settings"),
    );
    let hint = ui.tree.add(
        search_well,
        Node::text("Ctrl+F", 11.0)
            .weight(Weight::Medium)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let clear = ui.tree.add(
        search_well,
        Node::stack()
            .size(18.0, 18.0)
            .radius(9.0)
            .align(Align::Center)
            .justify(Justify::Center)
            .hover_fill(Token::Hover)
            .hidden(true)
            .button("Clear search")
            .tooltip("Clear  Esc"),
    );
    ui.tree.add(
        clear,
        Node::glyph(glyph::CLEAR, 12.0, 18.0)
            .ink(Swatch::Fade(Token::Muted, 0.8))
            .role(Role::None),
    );
    ui.tree
        .add(rail_box, Node::new().height(12.0).role(Role::None));
    let rail = ui.tree.add(
        rail_box,
        Node::scroll()
            .grow(1.0)
            .shrink(1.0)
            .gap(metric::RAIL_GROUPS)
            .padding(Insets::new(0.0, 0.0, 0.0, 28.0))
            .role(Role::List)
            .name("Settings pages"),
    );
    let divider = ui
        .tree
        .add(row, Node::new().width(1.0).fill_height().shrink(0.0));

    // — the page —
    let content = ui.tree.add(row, Node::column().grow(1.0).shrink(1.0));
    let head = ui.tree.add(
        content,
        Node::row()
            .padding(Insets::new(
                metric::PAGE_H,
                metric::HEAD_TOP,
                metric::PAGE_H - 6.0,
                metric::HEAD_BOTTOM,
            ))
            .gap(12.0)
            .align(Align::Start),
    );
    let words = ui
        .tree
        .add(head, Node::column().gap(4.0).grow(1.0).shrink(1.0));
    let title = ui.tree.add(
        words,
        Node::text("", metric::HEAD_TITLE)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .role(Role::Text),
    );
    let blurb = ui
        .tree
        .add(words, Node::text("", metric::HEAD_BLURB).trim(Trim::Wrap));
    let close = controls::door(ui, head, glyph::CLOSE, "Close Settings", "Close  Esc");
    let rule = ui.tree.add(
        content,
        Node::new().height(1.0).opacity(0.0).role(Role::None),
    );
    // A scroll fills what the header leaves and gives the rest back: its
    // content is as tall as it wants, so it has to shrink to the plate.
    let list = ui.tree.add(
        content,
        Node::scroll()
            .grow(1.0)
            .shrink(1.0)
            .padding(Insets::new(
                metric::PAGE_H,
                metric::PAGE_TOP,
                metric::PAGE_H,
                metric::PAGE_BOTTOM,
            ))
            .gap(metric::SECTIONS)
            .role(Role::Pane),
    );
    Nodes {
        scrim: p.scrim,
        plate,
        rail_box,
        rail,
        search_well,
        search,
        hint,
        clear,
        divider,
        content,
        title,
        blurb,
        close,
        rule,
        list,
    }
}

/// Colour the frame for the window's appearance and size the rail for the
/// panel `width` wide.
pub fn restyle(ui: &mut Ui, n: &Nodes, ink: &Inks, width: f32) {
    let fixed = Swatch::Fixed;
    ui.tree.repaint(n.plate, |p| {
        p.look.fill = fixed(ink.content);
        p.look.stroke = fixed(ink.card_edge);
    });
    ui.tree
        .repaint(n.rail_box, |p| p.look.fill = fixed(ink.rail));
    ui.tree
        .repaint(n.search_well, |p| p.look.fill = fixed(ink.field));
    ui.tree
        .repaint(n.divider, |p| p.look.fill = fixed(ink.card_edge));
    ui.tree
        .repaint(n.rule, |p| p.look.fill = fixed(ink.card_edge));
    ui.tree.repaint(n.blurb, |p| p.look.ink = fixed(ink.detail));
    ui.tree
        .repaint(n.content, |p| p.look.fill = fixed(ink.content));
    let rail = look::rail(width);
    if ui.tree.get(n.rail_box).map(|r| r.style.width) != Some(Dim::Px(rail)) {
        ui.tree.edit(n.rail_box, |r| r.style.width = Dim::Px(rail));
    }
}

/// The header's words.
pub fn header(ui: &mut Ui, n: &Nodes, title: &str, blurb: &str) {
    ui.tree.set_text(n.title, title);
    ui.tree.set_text(n.blurb, blurb);
}

/// The search field's trailing mark: `Ctrl+F` while it is empty, the
/// clear cross while it is not.
pub fn search_mark(ui: &mut Ui, n: &Nodes, empty: bool) {
    ui.tree.set_hidden(n.hint, !empty);
    ui.tree.set_hidden(n.clear, empty);
}

/// A page in the rail, with its match count while searching.
pub struct RailRow {
    pub id: &'static str,
    pub title: &'static str,
    pub glyph: char,
    /// While searching: how many of the page's rows match.
    pub count: Option<usize>,
}

/// Redraw the rail's groups, `chosen` lit.
pub fn rail(
    ui: &mut Ui,
    n: &Nodes,
    groups: &[(&'static str, Vec<RailRow>)],
    chosen: Option<&str>,
    ink: &Inks,
    drawn: &mut Drawn,
) {
    ui.tree.clear_children(n.rail);
    for (group, rows) in groups {
        let g = ui.tree.add(
            n.rail,
            Node::column().gap(1.0).role(Role::Group).name(group),
        );
        ui.tree.add(
            g,
            Node::text(group, metric::RAIL_GROUP_LABEL)
                .weight(Weight::Medium)
                .ink(Swatch::Fixed(ink.detail))
                .padding(Insets::new(10.0, 0.0, 10.0, 4.0))
                .role(Role::Text),
        );
        for r in rows {
            let id = rail_row(ui, g, r, chosen == Some(r.id), ink);
            drawn.hit(id, Hit::Page(r.id));
        }
    }
}

/// One page in the rail (`SettingsRailRow`).
fn rail_row(ui: &mut Ui, parent: NodeId, r: &RailRow, on: bool, ink: &Inks) -> NodeId {
    let name = match r.count {
        Some(c) => format!("{}, {c} match{}", r.title, if c == 1 { "" } else { "es" }),
        None => r.title.to_string(),
    };
    let mut row = Node::row()
        .height(metric::RAIL_ROW)
        .padding(Insets::hv(10.0, 0.0))
        .gap(9.0)
        .radius(metric::RAIL_ROW_RADIUS)
        .align(Align::Center)
        .fill(if on {
            Swatch::Fixed(ink.chosen)
        } else {
            Swatch::Clear
        })
        .hover_fill(if on {
            Swatch::Fixed(ink.chosen)
        } else {
            Swatch::Fade(Token::Ink, 0.05)
        })
        .press_fill(Swatch::Fade(Token::Ink, 0.08))
        .button(&name)
        .selected(on);
    if on {
        row = row.shadow(Shadow {
            alpha: 0.07,
            radius: 2.5,
            y: 1.0,
        });
    }
    if r.count == Some(0) {
        row = row.opacity(metric::DIMMED);
    }
    let text = if on {
        Swatch::Token(Token::Ink)
    } else {
        Swatch::Fade(Token::Ink, 0.78)
    };
    let b = ui.tree.add(parent, row);
    ui.tree.add(
        b,
        Node::glyph(r.glyph, 12.5, 18.0).ink(text).role(Role::None),
    );
    ui.tree.add(
        b,
        Node::text(r.title, metric::TITLE)
            .weight(if on { Weight::Medium } else { Weight::Regular })
            .ink(text)
            .grow(1.0)
            .shrink(1.0)
            .role(Role::None),
    );
    if let Some(c) = r.count.filter(|c| *c > 0) {
        let badge = ui.tree.add(
            b,
            Node::row()
                .height(17.0)
                .padding(Insets::hv(6.0, 0.0))
                .radius(8.5)
                .align(Align::Center)
                .fill(Swatch::Fade(Token::Ink, 0.06))
                .shrink(0.0)
                .role(Role::None),
        );
        ui.tree.add(
            badge,
            Node::text(&c.to_string(), 11.0)
                .weight(Weight::Medium)
                .ink(Swatch::Fixed(ink.detail))
                .role(Role::None),
        );
    }
    b
}
