//! The pieces every panel is made of (`Plate.swift`), so Settings,
//! History and the rest read as one kind of thing: the plate — a rounded
//! card with a title and a cross, the panel's body, and a foot under a
//! hairline — over a lightly dimmed window; hairline cards with a rule
//! between lines; one line per thing to set or do; the quiet pill button.
//!
//! Besides the centred sheet ([`build`]) a plate can sit undimmed in the
//! window's top-right corner ([`build_at`] with [`Spot::Corner`], the
//! Hidden list) or hang from a door as a popover ([`popover`], the
//! bookmarks dropdown and the downloads list).

use copper_win_ui::tokens::{Shadow, motion};
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Abs, Align, Color, Insets, Justify, Node, NodeId, Prop, Rect, Role, Swatch, Token, Trim, Weight,
};

/// The plate's nodes.
#[derive(Clone, Copy, Debug)]
pub struct Plate {
    /// The window-wide layer under the plate: a click on it closes.
    pub scrim: NodeId,
    /// The cross (a popover has none: it is the scrim then).
    pub close: NodeId,
    pub body: NodeId,
    /// The foot's row; show it with [`Plate::show_foot`].
    pub foot: NodeId,
    foot_box: NodeId,
    /// The title's text (a popover has none): a panel whose subject moves
    /// while it is open (Hidden, with the site in front) retitles it.
    pub title: Option<NodeId>,
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

/// Where a plate sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spot {
    /// Centred over a dimmed window (`App.swift` `sheet`).
    Center,
    /// Undimmed, this far from the window's top and right edges (`App.swift`
    /// `reviewing`: the page stays in view).
    Corner { top: f32, right: f32 },
}

const RADIUS: f32 = 16.0;
const PAD_H: f32 = 22.0;
const TITLE: f32 = 17.0;
const SCRIM: f32 = 0.10;
const CARD_RADIUS: f32 = 11.0;
const LINE_INSET: f32 = 14.0;
const LINE_PAD_V: f32 = 10.0;
/// A plate arrives at this scale and settles to full size (`App.swift`
/// `.transition(.scale(scale: 0.97))`).
const ENTER_SCALE: f32 = 0.97;

/// The red Remove of a row (`.red.opacity(0.75)`).
pub const RED: Swatch = Swatch::Fixed(Color {
    r: 1.0,
    g: 0.231,
    b: 0.188,
    a: 0.75,
});

/// Build a plate `width` wide titled `title` into `parent`, centred over
/// a dimmed window.
pub fn build(ui: &mut Ui, parent: NodeId, title: &str, width: f32) -> Plate {
    build_at(ui, parent, title, width, Spot::Center)
}

/// Build a plate `width` wide titled `title` into `parent` at `spot`.
pub fn build_at(ui: &mut Ui, parent: NodeId, title: &str, width: f32, spot: Spot) -> Plate {
    let scrim = match spot {
        Spot::Center => ui.tree.add(
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
        ),
        // No dimming: the point is to keep looking at the page.
        Spot::Corner { .. } => ui
            .tree
            .add(parent, Node::stack().absolute(Abs::FILL).layer().hit()),
    };
    let mut card = Node::column()
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
        .layer()
        .hit()
        .role(Role::Pane)
        .name(title);
    if let Spot::Corner { top, right } = spot {
        card = card.absolute(Abs {
            left: None,
            top: Some(top),
            right: Some(right),
            bottom: None,
        });
    }
    let outer = ui.tree.add(scrim, card);
    // The shadow is the outer node's; the inner one clips to the corners
    // (a clipped layer loses its shadow).
    let plate = ui
        .tree
        .add(outer, Node::column().radius(RADIUS).clip().shrink(1.0));
    let head = ui.tree.add(
        plate,
        Node::row()
            .padding(Insets::new(PAD_H, 18.0, PAD_H - 6.0, 14.0))
            .gap(10.0)
            .align(Align::Center),
    );
    let title = ui.tree.add(
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
            .hover_ink(Token::Ink)
            .role(Role::None),
    );
    let body = ui.tree.add(
        plate,
        Node::column()
            .padding(Insets::new(PAD_H, 0.0, PAD_H, 20.0))
            .gap(14.0)
            .shrink(1.0),
    );
    let (foot_box, foot) = foot(ui, plate);
    arrive(ui, scrim, outer, spot == Spot::Center);
    Plate {
        scrim,
        close,
        body,
        foot,
        foot_box,
        title: Some(title),
    }
}

/// The hairline and the foot's row, hidden until shown.
fn foot(ui: &mut Ui, plate: NodeId) -> (NodeId, NodeId) {
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
    (foot_box, foot)
}

/// The plate comes in: the dimming fades up, the plate grows from just
/// under its size (`App.swift`: scale .97 and opacity, `Motion.settle`).
fn arrive(ui: &mut Ui, scrim: NodeId, plate: NodeId, dims: bool) {
    if dims {
        ui.tree.set_prop(scrim, Prop::Opacity, 0.0);
        ui.tree.animate(scrim, Prop::Opacity, 1.0, motion::QUICK);
    } else {
        ui.tree.set_prop(plate, Prop::Opacity, 0.0);
        ui.tree.animate(plate, Prop::Opacity, 1.0, motion::QUICK);
    }
    ui.tree.set_prop(plate, Prop::Scale, ENTER_SCALE);
    ui.tree.animate(plate, Prop::Scale, 1.0, motion::SETTLE);
}

/// A popover hanging from a door (`.popover`): no title, no cross, no
/// dimming; a click anywhere else closes it. `tall` is about how tall it
/// will be (to choose between under and over the door). The body is a
/// bare column; the panel draws its own foot.
pub fn popover(
    ui: &mut Ui,
    parent: NodeId,
    width: f32,
    radius: f32,
    anchor: Rect,
    tall: f32,
) -> Plate {
    let window = ui.size();
    let scrim = ui
        .tree
        .add(parent, Node::stack().absolute(Abs::FILL).layer().hit());
    let card = ui.tree.add(
        scrim,
        Node::column()
            .absolute(super::spot::popover(anchor, window, width, tall))
            .width(width)
            .max_height((window.h - 2.0 * super::spot::MARGIN).max(120.0))
            .fill(Token::Ground)
            .radius(radius)
            .stroke(Token::Hairline, 1.0)
            .shadow(Shadow {
                alpha: 0.14,
                radius: 22.0,
                y: 8.0,
            })
            .layer()
            .hit()
            .role(Role::Pane),
    );
    let inner = ui
        .tree
        .add(card, Node::column().radius(radius).clip().shrink(1.0));
    let body = ui.tree.add(inner, Node::column().shrink(1.0));
    let (foot_box, foot) = foot(ui, inner);
    arrive(ui, scrim, card, false);
    Plate {
        scrim,
        close: scrim,
        body,
        foot,
        foot_box,
        title: None,
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
    rule_inset(ui, card, LINE_INSET)
}

/// A hairline between two lines, `inset` from the leading edge.
pub fn rule_inset(ui: &mut Ui, card: NodeId, inset: f32) -> NodeId {
    let r = ui.tree.add(
        card,
        Node::row()
            .height(1.0)
            .shrink(0.0)
            .padding(Insets::new(inset, 0.0, 0.0, 0.0)),
    );
    ui.tree
        .add(r, Node::new().height(1.0).grow(1.0).fill(Token::Hairline));
    r
}

/// A full-width hairline (a popover's, over its foot).
pub fn hairline(ui: &mut Ui, parent: NodeId) -> NodeId {
    ui.tree.add(
        parent,
        Node::new().height(1.0).shrink(0.0).fill(Token::Hairline),
    )
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

/// A panel's heading over a card (`Caption` outside Settings: 11.5 medium,
/// muted, leading 2).
pub fn heading(ui: &mut Ui, parent: NodeId, text: &str) -> NodeId {
    ui.tree.add(
        parent,
        Node::text(text, 11.5)
            .weight(Weight::Medium)
            .ink(Token::Muted)
            .padding(Insets::new(2.0, 0.0, 0.0, 0.0)),
    )
}

/// The capsule button (`Pill`): outlined on the ground, the hover tone
/// under the pointer. Returns the button.
pub fn pill(ui: &mut Ui, parent: NodeId, label: &str) -> NodeId {
    pill_with(ui, parent, label, false)
}

/// The pill filled in ink: the thing you came here to press.
pub fn pill_filled(ui: &mut Ui, parent: NodeId, label: &str) -> NodeId {
    pill_with(ui, parent, label, true)
}

fn pill_with(ui: &mut Ui, parent: NodeId, label: &str, filled: bool) -> NodeId {
    let mut n = Node::row()
        .height(24.0)
        .padding(Insets::hv(10.0, 0.0))
        .radius(12.0)
        .align(Align::Center)
        .shrink(0.0)
        .button(label);
    n = if filled {
        n.fill(Token::Ink)
            .press_fill(Swatch::Fade(Token::Ink, 0.85))
    } else {
        n.fill(Token::Ground)
            .hover_fill(Token::Hover)
            .press_fill(Token::Wash)
            .stroke(Token::Hairline, 1.0)
    };
    let b = ui.tree.add(parent, n);
    ui.tree.add(
        b,
        Node::text(label, 11.5)
            .ink(if filled { Token::Ground } else { Token::Ink })
            .role(Role::None),
    );
    b
}

/// A small text action inside a row (`Quick`): Restore, Remove. Pass
/// `reveal` to show it only while its row is under the pointer.
pub fn quick(ui: &mut Ui, parent: NodeId, label: &str, ink: Swatch, reveal: bool) -> NodeId {
    let mut n = Node::row()
        .height(22.0)
        .padding(Insets::hv(8.0, 0.0))
        .radius(11.0)
        .fill(Token::Wash)
        .press_fill(Token::Hairline)
        .align(Align::Center)
        .shrink(0.0)
        .button(label);
    if reveal {
        n = n.reveal();
    }
    let b = ui.tree.add(parent, n);
    ui.tree
        .add(b, Node::text(label, 11.5).ink(ink).role(Role::None));
    b
}

/// A line of muted words where there is nothing to list (`Nothing`).
pub fn nothing(ui: &mut Ui, parent: NodeId, text: &str) -> NodeId {
    let n = ui.tree.add(
        parent,
        Node::column()
            .fill_width()
            .padding(Insets::hv(LINE_INSET, 18.0)),
    );
    ui.tree
        .add(n, Node::text(text, 13.0).ink(Token::Muted).trim(Trim::Wrap));
    n
}

/// A search field's nodes.
#[derive(Clone, Copy, Debug)]
pub struct Hunt {
    pub field: NodeId,
    /// The clearing cross, shown while there is something typed.
    pub clear: NodeId,
}

impl Hunt {
    /// Show the cross while there is something to clear.
    pub fn typed(&self, ui: &mut Ui, text: &str) {
        ui.tree.set_hidden(self.clear, text.is_empty());
    }

    /// Empty the field (the cross).
    pub fn clear(&self, ui: &mut Ui) {
        ui.set_field_text(self.field, "", false);
        ui.tree.set_hidden(self.clear, true);
        ui.focus(Some(self.field));
    }
}

/// The search field alone, for a list that never clears it from a cross
/// (Settings' site manager). Returns the field.
pub fn hunt(ui: &mut Ui, parent: NodeId, prompt: &str) -> NodeId {
    search(ui, parent, prompt).field
}

/// The search field with its clearing cross.
pub fn search(ui: &mut Ui, parent: NodeId, prompt: &str) -> Hunt {
    let row = ui.tree.add(
        parent,
        Node::row()
            .height(34.0)
            .shrink(0.0)
            .padding(Insets::new(12.0, 0.0, 6.0, 0.0))
            .gap(8.0)
            .radius(10.0)
            .fill(Token::Wash)
            .align(Align::Center),
    );
    ui.tree.add(
        row,
        Node::glyph(crate::chrome::glyph::SEARCH, 11.0, 14.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let field = ui
        .tree
        .add(row, Node::field(prompt, 13.0).fill_height().grow(1.0));
    let clear = ui.tree.add(
        row,
        Node::stack()
            .size(20.0, 20.0)
            .align(Align::Center)
            .justify(Justify::Center)
            .hidden(true)
            .button("Clear search"),
    );
    ui.tree.add(
        clear,
        Node::glyph(crate::chrome::glyph::CLEAR_FIELD, 11.0, 20.0)
            .ink(Token::Faint)
            .hover_ink(Token::Muted)
            .role(Role::None),
    );
    Hunt { field, clear }
}

/// What stands for a page (`Mark`): its icon, or its letter on a faint
/// rounded square. Returns the mark.
pub fn mark(ui: &mut Ui, parent: NodeId, icon: Option<u32>, letter: &str, size: f32) -> NodeId {
    let radius = size * 0.22;
    match icon {
        Some(id) => ui.tree.add(
            parent,
            Node::image(id, size)
                .radius(radius)
                .clip()
                .shrink(0.0)
                .role(Role::None),
        ),
        None => {
            let m = ui.tree.add(
                parent,
                Node::stack()
                    .size(size, size)
                    .shrink(0.0)
                    .radius(radius)
                    .fill(Swatch::Fade(Token::Ink, 0.06))
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .role(Role::None),
            );
            ui.tree.add(
                m,
                Node::text(letter, size * 0.56)
                    .weight(Weight::Medium)
                    .ink(Token::Muted)
                    .role(Role::None),
            );
            m
        }
    }
}

/// The letter a mark shows for a site key or host: its first, upper-cased.
pub fn letter(key: &str) -> String {
    key.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "•".into())
}
