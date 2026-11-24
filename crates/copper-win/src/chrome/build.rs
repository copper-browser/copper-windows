//! The window's skeleton, and the small builders every part uses.
//!
//! ```text
//! content
//! ├─ frame ········· one tinted ground under the column and the strip
//! │  ├─ strip ······ tabs across the top (`TabsPosition::Top`)
//! │  └─ body
//! │     ├─ side ···· the column (`TabsPosition::Left`)
//! │     └─ page
//! │        ├─ band · the caption buttons' row over the page (left layout)
//! │        └─ well · the column's hairline edge, then the stage
//! ├─ plates ········ a panel or Settings, floating over the page
//! ├─ toasts
//! ├─ peek ·········· the Calendar pin's card
//! ├─ card ·········· launch / edit / palette, over everything
//! └─ sheet ········· a question that needs an answer
//! ```

use super::{State, card, column, images, menus, sheet, stage, strip, toast};
use copper_win_ui::tokens::{metrics, side};
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Insets, Justify, Node, NodeId, Role, Swatch, Token};

/// The skeleton's nodes.
pub struct Ids {
    /// Over the page in the left layout: drags the window, holds the
    /// caption buttons' corner.
    pub band: NodeId,
    /// The column's edge beside the page.
    pub well: NodeId,
    /// The band's door that brings a folded column back.
    pub unfold: NodeId,
    /// Where a panel's plate goes (`crate::panels`).
    pub plates: NodeId,
}

/// Build the chrome into a fresh window.
pub fn build(ui: &mut Ui) -> State {
    let content = ui.content();
    let frame = ui.tree.add(
        content,
        Node::column().absolute(Abs::FILL).fill(Token::ColumnGround),
    );
    let strip = strip::build(ui, frame);
    // Shrinks to the window: its children may measure taller than it
    // (the column's long tab list), and the page, stage and Agent sidebar
    // take its height.
    let body = ui.tree.add(frame, Node::row().grow(1.0).shrink(1.0));
    let column = column::build(ui, body);
    let page = ui.tree.add(body, Node::column().grow(1.0));
    let band = ui.tree.add(
        page,
        Node::row()
            .height(metrics::BARE)
            .shrink(0.0)
            .padding(Insets::new(8.0, 0.0, 0.0, 0.0))
            .align(Align::Center)
            .drag()
            .role(Role::TitleBar),
    );
    // Folded away, the column comes back from here.
    let (unfold, _) = door(
        ui,
        band,
        super::glyph::SIDEBAR,
        side::HEAD_DOOR,
        side::HEAD_GLYPH,
        "Show sidebar",
        &tip("Show sidebar", "toggle-sidebar"),
    );
    ui.tree.set_hidden(unfold, true);
    let well = ui.tree.add(
        page,
        Node::row()
            .grow(1.0)
            .padding(Insets::new(1.0, 0.0, 0.0, 0.0))
            .fill(Swatch::Fade(Token::ColumnHairline, side::EDGE_ALPHA)),
    );
    let stage = stage::build(ui, well);
    super::pane::build(ui, well);
    let plates = ui.tree.add(content, Node::stack().absolute(Abs::FILL));
    let toast = toast::build(ui, content);
    let peek = crate::peek::Card::build(&mut ui.tree, content);
    let card = card::build(ui, content);
    let sheet = sheet::build(ui, toast.bars());
    State {
        ids: Ids {
            band,
            well,
            unfold,
            plates,
        },
        column,
        strip,
        stage,
        card,
        sheet,
        toast,
        peek,
        images: images::Images::default(),
        menus: menus::Menus::default(),
        side: true,
        folded: false,
        immersed: false,
        full: false,
        find: None,
        letter: None,
        welcome: None,
    }
}

/// A square glyph button in the column's ink, lighting on hover, with a
/// tooltip. Returns the button and its glyph.
pub fn door(
    ui: &mut Ui,
    parent: NodeId,
    glyph: char,
    size: f32,
    glyph_size: f32,
    name: &str,
    tip: &str,
) -> (NodeId, NodeId) {
    let d = ui.tree.add(
        parent,
        Node::stack()
            .size(size, size)
            .shrink(0.0)
            .radius(side::door_radius(size))
            .hover_fill(Token::ColumnHover)
            .press_fill(Token::ColumnHover)
            .align(Align::Center)
            .justify(Justify::Center)
            .button(name)
            .tooltip(tip),
    );
    let g = ui.tree.add(
        d,
        Node::glyph(glyph, glyph_size, size)
            .ink(Token::ColumnInk)
            .role(Role::None),
    );
    (d, g)
}

/// A tooltip with the command's shortcut: `Back (Alt+Left)`.
pub fn tip(text: &str, command: &str) -> String {
    match crate::keys::shortcut(command) {
        Some(k) => format!("{text} ({k})"),
        None => text.to_string(),
    }
}

/// The pin light (Mac `PinLightDot`, copper-shell-core `pinlight`): 6 DIPs
/// of copper in pin square `square`'s top-right corner, `inset` from its
/// edges, while the pin's title has an unread count and it isn't the live
/// tab. Clicks go through to the square.
pub fn pin_light(ui: &mut Ui, square: NodeId, lit: bool, inset: f32) {
    if lit {
        ui.tree.add(
            square,
            Node::new()
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(Token::Accent)
                .absolute(Abs {
                    top: Some(inset),
                    right: Some(inset),
                    ..Abs::default()
                })
                .inert()
                .role(Role::None),
        );
    }
}

/// A site's mark: its icon, or a plain globe until it lands — never a
/// letter (Mac `Mark`/`Globe`, copper-browser/Copper#181). Returns the
/// mark's box; its child is the picture or the globe.
pub fn mark(ui: &mut Ui, parent: NodeId, icon: Option<u32>, size: f32) -> NodeId {
    let m = ui.tree.add(
        parent,
        Node::stack()
            .size(size, size)
            .shrink(0.0)
            .align(Align::Center)
            .justify(Justify::Center),
    );
    fill_mark(ui, m, icon, size);
    m
}

/// Put an icon or the globe in mark `m` (replacing what it held).
pub fn fill_mark(ui: &mut Ui, m: NodeId, icon: Option<u32>, size: f32) {
    ui.tree.clear_children(m);
    let child = match icon {
        Some(id) => Node::image(id, size).role(Role::Image),
        None => Node::glyph(super::glyph::GLOBE, (size * 0.8).round(), size)
            .ink(Swatch::Fade(Token::ColumnInk, side::MARK_LETTER_INK))
            .role(Role::None),
    };
    ui.tree.add(m, child);
}

/// The picture id for a tab's favicon, registering it with the window.
pub fn icon_id(
    ui: &mut Ui,
    images: &mut images::Images,
    icon: Option<&crate::platform::Icon>,
) -> Option<u32> {
    icon.map(|i| images.id(ui, i))
}
