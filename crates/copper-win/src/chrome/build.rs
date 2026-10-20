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
//! ├─ card ·········· launch / edit / palette, over everything
//! └─ sheet ········· a question that needs an answer
//! ```

use super::{State, card, column, images, menus, sheet, stage, strip, toast};
use copper_win_ui::tokens::{metrics, side};
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Abs, Align, Insets, Justify, Node, NodeId, Role, Swatch, TextAlign, Token, Weight,
};

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
    let body = ui.tree.add(frame, Node::row().grow(1.0));
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
    let plates = ui.tree.add(content, Node::stack().absolute(Abs::FILL));
    let toast = toast::build(ui, content);
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

/// A site's mark: its icon, or its letter on a rounded square. Returns the
/// mark's box; its child is the picture or the letter.
pub fn mark(ui: &mut Ui, parent: NodeId, icon: Option<u32>, letter: &str, size: f32) -> NodeId {
    let m = ui.tree.add(
        parent,
        Node::stack()
            .size(size, size)
            .shrink(0.0)
            .align(Align::Center)
            .justify(Justify::Center),
    );
    fill_mark(ui, m, icon, letter, size);
    m
}

/// Put an icon or a letter in mark `m` (replacing what it held).
pub fn fill_mark(ui: &mut Ui, m: NodeId, icon: Option<u32>, letter: &str, size: f32) {
    ui.tree.clear_children(m);
    match icon {
        Some(id) => {
            ui.tree.repaint(m, |n| {
                n.look.fill = Swatch::Clear;
                n.look.radius = 0.0;
            });
            ui.tree.add(m, Node::image(id, size).role(Role::Image));
        }
        None => {
            let radius = side::mark_radius(size);
            ui.tree.repaint(m, |n| {
                n.look.fill = Swatch::Token(Token::Square);
                n.look.radius = radius;
            });
            ui.tree.add(
                m,
                Node::text(letter, side::mark_letter(size))
                    .weight(Weight::Semibold)
                    .ink(Swatch::Fade(Token::ColumnInk, side::MARK_LETTER_INK))
                    .text_align(TextAlign::Center)
                    .role(Role::None),
            );
        }
    }
}

/// The picture id for a tab's favicon, registering it with the window.
pub fn icon_id(
    ui: &mut Ui,
    images: &mut images::Images,
    icon: Option<&crate::platform::Icon>,
) -> Option<u32> {
    icon.map(|i| images.id(ui, i))
}
