//! The Canvas card's pieces (`CanvasUI.swift`): section headings, the muted
//! explanation line, the subject a form is about, the card's capsule
//! buttons, the glyph tile a row leads with, and the bordered name field.

use copper_win_ui::win::Ui;
use copper_win_ui::{
    Align, Color, Insets, Justify, Node, NodeId, Prop, Role, Swatch, Token, Trim, Weight,
};

/// Where a failure is said (SwiftUI `.orange`).
pub const ORANGE: Color = Color::hex(0xD9730D);
/// Someone live (SwiftUI `.green`).
pub const GREEN: Color = Color::hex(0x30A14E);
/// The destructive button (`systemRed`).
pub const RED: Color = Color::hex(0xE5372B);

/// The card's insets (`.padding(.horizontal, 14)`).
pub const INSET: f32 = 14.0;

/// A button's kind (`CanvasButtonStyle.Kind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Plain,
    Primary,
    Destructive,
}

/// "INVITATIONS": 10 semibold, muted, 14 / top 10 / bottom 3.
pub fn section(ui: &mut Ui, parent: NodeId, title: &str) -> NodeId {
    ui.tree.add(
        parent,
        Node::text(&title.to_uppercase(), 10.0)
            .weight(Weight::Semibold)
            .ink(Token::Muted)
            .padding(Insets::new(INSET, 10.0, INSET, 3.0))
            .name(title),
    )
}

/// One wrapping line in the muted voice (`CanvasExplain`).
pub fn explain(ui: &mut Ui, parent: NodeId, text: &str) -> NodeId {
    ui.tree.add(
        parent,
        Node::text(text, 11.0)
            .ink(Token::Muted)
            .trim(Trim::Wrap)
            .padding(Insets::new(INSET, 0.0, INSET, 4.0)),
    )
}

/// A short line, muted or (`failed`) orange, wrapping.
pub fn note(ui: &mut Ui, parent: NodeId, text: &str, failed: bool) -> NodeId {
    let ink: Swatch = if failed {
        Swatch::Fixed(ORANGE)
    } else {
        Swatch::Token(Token::Muted)
    };
    ui.tree
        .add(parent, Node::text(text, 11.0).ink(ink).trim(Trim::Wrap))
}

/// A form's short title (`CanvasFormTitle`): 12 medium.
pub fn form_title(ui: &mut Ui, parent: NodeId, text: &str) -> NodeId {
    ui.tree.add(
        parent,
        Node::text(text, 12.0)
            .weight(Weight::Medium)
            .ink(Token::Ink),
    )
}

/// The canvas a form is about, whole (`CanvasSubject`).
pub fn subject(ui: &mut Ui, parent: NodeId, name: &str) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::column()
            .padding(Insets::hv(9.0, 6.0))
            .radius(7.0)
            .fill(Token::Wash)
            .name(&format!("Canvas: {name}")),
    );
    ui.tree.add(
        b,
        Node::text(name, 12.5)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .trim(Trim::Wrap),
    );
    b
}

/// A capsule button (`CanvasButtonStyle`): 11.5, 11 / 4.5.
pub fn button(ui: &mut Ui, parent: NodeId, label: &str, kind: Kind, enabled: bool) -> NodeId {
    let (fill, ink, weight): (Swatch, Swatch, Weight) = match kind {
        Kind::Plain => (Token::Ground.into(), Token::Ink.into(), Weight::Regular),
        Kind::Primary => (Token::Ink.into(), Token::Ground.into(), Weight::Semibold),
        Kind::Destructive => (RED.into(), Color::WHITE.into(), Weight::Semibold),
    };
    let mut n = Node::row()
        .padding(Insets::hv(11.0, 4.5))
        .radius(12.0)
        .shrink(0.0)
        .align(Align::Center)
        .justify(Justify::Center)
        .fill(fill)
        .button(label);
    if kind == Kind::Plain {
        n = n.stroke(Token::Faint, 1.0).hover_fill(Token::Hover);
    }
    if !enabled {
        n = n.opacity(0.45);
        n.flags.disabled = true;
    }
    let b = ui.tree.add(parent, n);
    ui.tree.add(
        b,
        Node::text(label, 11.5)
            .weight(weight)
            .ink(ink)
            .role(Role::None),
    );
    b
}

/// Turn a button on or off where it stands.
pub fn set_enabled(ui: &mut Ui, b: NodeId, enabled: bool) {
    ui.tree.edit(b, |n| n.flags.disabled = !enabled);
    ui.tree
        .set_prop(b, Prop::Opacity, if enabled { 1.0 } else { 0.45 });
}

/// A 26×26 tile with a glyph (a row's mark), radius 7.
pub fn tile(ui: &mut Ui, parent: NodeId, glyph: char, front: bool) -> NodeId {
    let t = ui.tree.add(
        parent,
        Node::stack()
            .size(26.0, 26.0)
            .shrink(0.0)
            .radius(7.0)
            .fill(if front { Token::Ground } else { Token::Wash })
            .align(Align::Center)
            .justify(Justify::Center)
            .role(Role::None),
    );
    ui.tree.add(
        t,
        Node::glyph(glyph, 12.0, 26.0)
            .ink(Swatch::Fade(Token::Ink, 0.8))
            .role(Role::None),
    );
    t
}

/// A round face: initials on the person's colour.
pub fn face(ui: &mut Ui, parent: NodeId, name: &str, colour: u32, size: f32) -> NodeId {
    let f = ui.tree.add(
        parent,
        Node::stack()
            .size(size, size)
            .shrink(0.0)
            .radius(size / 2.0)
            .fill(Color::hex(colour))
            .align(Align::Center)
            .justify(Justify::Center)
            .tooltip(name)
            .role(Role::Image)
            .name(name),
    );
    ui.tree.add(
        f,
        Node::text(&super::copy::initials(name), size * 0.45)
            .weight(Weight::Semibold)
            .ink(Color::WHITE)
            .role(Role::None),
    );
    f
}

/// A rounded, bordered text field (`.roundedBorder`); returns the field.
pub fn field(ui: &mut Ui, parent: NodeId, placeholder: &str) -> NodeId {
    let row = ui.tree.add(
        parent,
        Node::row()
            .height(26.0)
            .shrink(0.0)
            .padding(Insets::hv(8.0, 0.0))
            .radius(6.0)
            .fill(Token::Ground)
            .stroke(Token::Faint, 1.0)
            .align(Align::Center),
    );
    ui.tree.add(
        row,
        Node::field(placeholder, 12.5)
            .fill_height()
            .grow(1.0)
            .name(placeholder),
    )
}

/// A one-DIP rule across the card.
pub fn hairline(ui: &mut Ui, parent: NodeId) -> NodeId {
    ui.tree.add(
        parent,
        Node::new().height(1.0).shrink(0.0).fill(Token::Hairline),
    )
}

/// The `@` badge: unread mentions of you on a row.
pub fn mention_badge(ui: &mut Ui, parent: NodeId, count: usize) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::row()
            .min_width(16.0)
            .min_height(16.0)
            .shrink(0.0)
            .padding(Insets::hv(5.0, 0.0))
            .radius(8.0)
            .fill(Token::Accent)
            .align(Align::Center)
            .justify(Justify::Center)
            .tooltip(&format!(
                "{count} mention{} of you in its chat",
                if count == 1 { "" } else { "s" }
            )),
    );
    let text = if count > 1 {
        format!("@{count}")
    } else {
        "@".into()
    };
    ui.tree.add(
        b,
        Node::text(&text, 10.5)
            .weight(Weight::Semibold)
            .ink(Color::WHITE)
            .role(Role::None),
    );
    b
}

/// A 6×6 dot.
pub fn dot(colour: impl Into<Swatch>) -> Node {
    Node::new()
        .size(6.0, 6.0)
        .shrink(0.0)
        .radius(3.0)
        .fill(colour)
        .role(Role::None)
}
