//! The pieces every panel is made of (`Plate.swift`), so Settings,
//! History and the rest read as one kind of thing: the plate — a rounded
//! card with a title and a cross, the panel's body, and a foot under a
//! hairline — over a lightly dimmed window; hairline cards with a rule
//! between lines; one line per thing to set or do; the quiet pill button.

use copper_win_ui::tokens::Shadow;
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim, Weight};

/// The plate's nodes.
#[derive(Clone, Copy, Debug)]
pub struct Plate {
    pub scrim: NodeId,
    pub close: NodeId,
    pub body: NodeId,
    /// The foot's row; show it with [`Plate::show_foot`].
    pub foot: NodeId,
    foot_box: NodeId,
}

impl Plate {
    /// Show the foot (and the hairline over it), or not.
    pub fn show_foot(&self, ui: &mut Ui, on: bool) {
        ui.tree.set_hidden(self.foot_box, !on);
        ui.tree.edit(self.body, |n| {
            n.style.padding.bottom = if on { 18.0 } else { 20.0 };
        });
    }
}

const RADIUS: f32 = 16.0;
const PAD_H: f32 = 22.0;
const TITLE: f32 = 17.0;
const SCRIM: f32 = 0.10;
const CARD_RADIUS: f32 = 11.0;
const LINE_INSET: f32 = 14.0;
const LINE_PAD_V: f32 = 10.0;

/// Build a plate `width` wide titled `title` into `parent`.
pub fn build(ui: &mut Ui, parent: NodeId, title: &str, width: f32) -> Plate {
    let scrim = ui.tree.add(
        parent,
        Node::column()
            .absolute(Abs::FILL)
            .align(Align::Center)
            .justify(Justify::Center)
            .padding(Insets::all(24.0))
            .fill(Swatch::Fade(Token::Black, SCRIM))
            .layer()
            .hit()
            .modal(),
    );
    let plate = ui.tree.add(
        scrim,
        Node::column()
            .width(width)
            .shrink(1.0)
            .fill(Token::Ground)
            .radius(RADIUS)
            .stroke(Token::Hairline, 1.0)
            .shadow(Shadow {
                alpha: 0.16,
                radius: 34.0,
                y: 12.0,
            })
            .clip()
            .layer()
            .hit()
            .role(Role::Pane)
            .name(title),
    );
    let head = ui.tree.add(
        plate,
        Node::row()
            .padding(Insets::new(PAD_H, 18.0, PAD_H - 6.0, 14.0))
            .gap(10.0)
            .align(Align::Center),
    );
    ui.tree.add(
        head,
        Node::text(title, TITLE)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .grow(1.0),
    );
    let close = ui.tree.add(
        head,
        Node::stack()
            .size(28.0, 28.0)
            .radius(8.0)
            .hover_fill(Token::Hover)
            .press_fill(Token::Hover)
            .align(Align::Center)
            .justify(Justify::Center)
            .button("Done")
            .tooltip("Done (Esc)"),
    );
    ui.tree.add(
        close,
        Node::glyph(crate::chrome::glyph::CLOSE, 11.0, 28.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let body = ui.tree.add(
        plate,
        Node::column()
            .padding(Insets::new(PAD_H, 0.0, PAD_H, 20.0))
            .gap(14.0)
            .shrink(1.0),
    );
    let foot_box = ui.tree.add(plate, Node::column().hidden(true));
    ui.tree
        .add(foot_box, Node::new().height(1.0).fill(Token::Hairline));
    let foot = ui.tree.add(
        foot_box,
        Node::row()
            .padding(Insets::hv(PAD_H, 14.0))
            .gap(10.0)
            .align(Align::Center),
    );
    Plate {
        scrim,
        close,
        body,
        foot,
        foot_box,
    }
}

/// A hairline card holding lines.
pub fn card(ui: &mut Ui, parent: NodeId) -> NodeId {
    ui.tree.add(
        parent,
        Node::column()
            .radius(CARD_RADIUS)
            .stroke(Token::Hairline, 1.0)
            .clip(),
    )
}

/// The hairline between two lines of a card.
pub fn rule(ui: &mut Ui, card: NodeId) -> NodeId {
    let r = ui.tree.add(
        card,
        Node::row()
            .height(1.0)
            .padding(Insets::new(LINE_INSET, 0.0, 0.0, 0.0)),
    );
    ui.tree
        .add(r, Node::new().height(1.0).grow(1.0).fill(Token::Hairline));
    r
}

/// One thing to set or do: its words on the left, a row for its control on
/// the right (returned).
pub fn line(ui: &mut Ui, card: NodeId, title: &str, detail: Option<&str>) -> NodeId {
    let row = ui.tree.add(
        card,
        Node::row()
            .padding(Insets::hv(LINE_INSET, LINE_PAD_V))
            .gap(16.0)
            .align(Align::Center),
    );
    let words = ui
        .tree
        .add(row, Node::column().gap(3.0).grow(1.0).shrink(1.0));
    ui.tree.add(words, Node::text(title, 13.0).ink(Token::Ink));
    if let Some(d) = detail {
        ui.tree.add(
            words,
            Node::text(d, 11.5).ink(Token::Muted).trim(Trim::Wrap),
        );
    }
    ui.tree
        .add(row, Node::row().gap(8.0).align(Align::Center).shrink(0.0))
}

/// A small heading over a card.
pub fn caption(ui: &mut Ui, parent: NodeId, text: &str) -> NodeId {
    ui.tree.add(
        parent,
        Node::text(text, 11.5)
            .weight(Weight::Semibold)
            .ink(Token::Muted)
            .padding(Insets::new(4.0, 0.0, 0.0, 0.0)),
    )
}

/// The quiet capsule button (`Pill`). Returns the button.
pub fn pill(ui: &mut Ui, parent: NodeId, label: &str) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::row()
            .height(26.0)
            .padding(Insets::hv(12.0, 0.0))
            .radius(13.0)
            .fill(Token::Wash)
            .hover_fill(Token::Hover)
            .press_fill(Token::Hover)
            .align(Align::Center)
            .shrink(0.0)
            .button(label),
    );
    ui.tree
        .add(b, Node::text(label, 12.0).ink(Token::Ink).role(Role::None));
    b
}

/// A line of muted words where there is nothing to list.
pub fn nothing(ui: &mut Ui, parent: NodeId, text: &str) -> NodeId {
    ui.tree.add(
        parent,
        Node::text(text, 12.5)
            .ink(Token::Muted)
            .padding(Insets::hv(LINE_INSET, 16.0)),
    )
}

/// The search field panels open with (`Hunt`). Returns the field.
pub fn hunt(ui: &mut Ui, parent: NodeId, prompt: &str) -> NodeId {
    let row = ui.tree.add(
        parent,
        Node::row()
            .height(34.0)
            .padding(Insets::hv(12.0, 0.0))
            .gap(8.0)
            .radius(10.0)
            .fill(Token::Wash)
            .align(Align::Center),
    );
    ui.tree.add(
        row,
        Node::glyph(crate::chrome::glyph::SEARCH, 12.0, 16.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    ui.tree
        .add(row, Node::field(prompt, 13.0).fill_height().grow(1.0))
}
