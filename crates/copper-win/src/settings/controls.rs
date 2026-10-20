//! Settings' controls, drawn the Mac's way (`Settings.swift` 605–777,
//! `Plate.swift`): the ink switch, the segmented chooser with its lifted
//! choice, the capsule pill, the popup chooser, the field on its wash, the
//! card, its rules and its lines. Each adds nodes under a parent and
//! returns what a click lands on; what a click *means* is the caller's.
//!
//! Disabled controls are drawn at 45% and are neither pointer targets nor
//! Tab stops (`Settings.swift` 625–736: disabled means not focusable).

use super::look::{Inks, metric};
use crate::chrome::glyph;
use copper_win_ui::tokens::Shadow;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim, Weight};

/// A fixed colour.
fn fixed(c: copper_win_ui::Color) -> Swatch {
    Swatch::Fixed(c)
}

/// Out of reach: faded, no pointer target, no Tab stop, and announced as
/// not enabled.
fn disable(mut n: Node) -> Node {
    n.flags.disabled = true;
    n.opacity(metric::DISABLED)
}

/// A hairline card holding lines (`Card`): Settings' card fill and edge.
pub fn card(ui: &mut Ui, parent: NodeId, ink: &Inks) -> NodeId {
    ui.tree.add(
        parent,
        Node::column()
            .radius(metric::CARD_RADIUS)
            .fill(fixed(ink.card))
            .stroke(fixed(ink.card_edge), 1.0)
            .clip(),
    )
}

/// The rule between two lines of a card, inset like their text.
pub fn rule(ui: &mut Ui, card: NodeId, ink: &Inks) -> NodeId {
    let r = ui.tree.add(
        card,
        Node::row()
            .height(1.0)
            .padding(Insets::new(metric::LINE_INSET, 0.0, 0.0, 0.0))
            .role(Role::None),
    );
    ui.tree.add(
        r,
        Node::new().height(1.0).grow(1.0).fill(fixed(ink.card_edge)),
    );
    r
}

/// A section's title over its card (`SettingsSectionTitle`).
pub fn caption(ui: &mut Ui, parent: NodeId, text: &str, ink: &Inks) -> NodeId {
    ui.tree.add(
        parent,
        Node::text(text, metric::CAPTION)
            .weight(Weight::Semibold)
            .ink(fixed(ink.heading))
            .padding(Insets::new(metric::LINE_INSET, 0.0, 0.0, 0.0)),
    )
}

/// A sentence under a card (`SettingsNote`).
pub fn note(ui: &mut Ui, parent: NodeId, text: &str, ink: &Inks) -> NodeId {
    ui.tree.add(
        parent,
        Node::text(text, 11.5)
            .ink(fixed(ink.detail))
            .trim(Trim::Wrap)
            .padding(Insets::hv(metric::LINE_INSET, 0.0)),
    )
}

/// What [`line`] made: the row, and the box its control goes in.
pub struct LineNodes {
    pub row: NodeId,
    /// The title and detail.
    pub words: NodeId,
    pub control: NodeId,
}

/// One thing to set or do (`Line`): its words on the left, a box for its
/// control on the right. `detail_ink` colours the sentence (red for an
/// error).
pub fn line(
    ui: &mut Ui,
    card: NodeId,
    title: &str,
    detail: Option<&str>,
    detail_ink: Swatch,
) -> LineNodes {
    let row = ui.tree.add(
        card,
        Node::row()
            .padding(Insets::hv(metric::LINE_INSET, metric::LINE_PAD_V))
            .gap(16.0)
            .align(Align::Center),
    );
    let words = ui.tree.add(
        row,
        Node::column()
            .gap(3.0)
            .grow(1.0)
            .shrink(1.0)
            .min_width(160.0),
    );
    if !title.is_empty() {
        ui.tree.add(
            words,
            Node::text(title, metric::TITLE)
                .ink(Token::Ink)
                .trim(Trim::Wrap),
        );
    }
    if let Some(d) = detail.filter(|d| !d.is_empty()) {
        ui.tree.add(
            words,
            Node::text(d, metric::DETAIL)
                .ink(detail_ink)
                .trim(Trim::Wrap),
        );
    }
    let control = ui.tree.add(
        row,
        Node::row()
            .gap(8.0)
            .align(Align::Center)
            .justify(Justify::End)
            .shrink(0.0),
    );
    LineNodes {
        row,
        words,
        control,
    }
}

/// The ink switch (`Switch`): 30×18, ink when on, faint when off, the knob
/// on the side it is set to. Returns the switch.
pub fn switch(ui: &mut Ui, parent: NodeId, name: &str, on: bool, enabled: bool) -> NodeId {
    let mut track = Node::row()
        .size(metric::SWITCH_W, metric::SWITCH_H)
        .padding(Insets::all(2.0))
        .radius(metric::SWITCH_H / 2.0)
        .align(Align::Center)
        .justify(if on { Justify::End } else { Justify::Start })
        .fill(if on { Token::Ink } else { Token::Faint })
        .shrink(0.0);
    track = track.button(name).selected(on);
    if !enabled {
        track = disable(track);
    }
    let id = ui.tree.add(parent, track);
    ui.tree.add(
        id,
        Node::new()
            .size(metric::SWITCH_H - 4.0, metric::SWITCH_H - 4.0)
            .radius((metric::SWITCH_H - 4.0) / 2.0)
            .fill(Token::Ground)
            .shadow(Shadow {
                alpha: 0.18,
                radius: 1.5,
                y: 1.0,
            })
            .role(Role::None),
    );
    id
}

/// The segmented chooser (`Segmented`): a wash track, the chosen option
/// lifted out on the ground. Returns each option's button, in order.
pub fn segmented(
    ui: &mut Ui,
    parent: NodeId,
    name: &str,
    options: &[(String, String)],
    chosen: &str,
    enabled: bool,
) -> Vec<NodeId> {
    let mut track = Node::row()
        .padding(Insets::all(2.0))
        .gap(2.0)
        .radius(metric::SEGMENT_RADIUS)
        .fill(Token::Wash)
        .shrink(0.0)
        .role(Role::Group)
        .name(name);
    if !enabled {
        track = track.opacity(metric::DISABLED);
    }
    let track = ui.tree.add(parent, track);
    let mut out = Vec::with_capacity(options.len());
    for (value, label) in options {
        let on = value == chosen;
        let mut b = Node::row()
            .padding(Insets::hv(metric::PILL_PAD_H, metric::PILL_PAD_V))
            .radius(metric::SEGMENT_ITEM_RADIUS)
            .align(Align::Center)
            .justify(Justify::Center)
            .fill(if on {
                Swatch::Token(Token::Ground)
            } else {
                Swatch::Clear
            });
        if on {
            b = b.shadow(Shadow {
                alpha: 0.08,
                radius: 3.0,
                y: 1.0,
            });
        }
        b = b
            .hover_fill(if on { Token::Ground } else { Token::Hover })
            .button(label)
            .selected(on);
        if !enabled {
            b.flags.disabled = true;
        }
        let id = ui.tree.add(track, b);
        ui.tree.add(
            id,
            Node::text(label, metric::PILL_TEXT)
                .weight(if on { Weight::Medium } else { Weight::Regular })
                .ink(if on { Token::Ink } else { Token::Muted })
                .role(Role::None),
        );
        out.push(id);
    }
    out
}

/// The capsule pill (`Pill`): outlined, or filled in ink when it is the
/// thing the line is for. Returns the pill.
pub fn pill(ui: &mut Ui, parent: NodeId, label: &str, filled: bool, enabled: bool) -> NodeId {
    let mut b = Node::row()
        .height(24.0)
        .padding(Insets::hv(metric::PILL_PAD_H, 0.0))
        .radius(12.0)
        .align(Align::Center)
        .justify(Justify::Center)
        .shrink(0.0);
    b = if filled {
        b.fill(Token::Ink)
            .hover_fill(Swatch::Fade(Token::Ink, 0.86))
            .press_fill(Swatch::Fade(Token::Ink, 0.78))
    } else {
        b.fill(Token::Ground)
            .stroke(Token::Hairline, 1.0)
            .hover_fill(Token::Hover)
            .press_fill(Token::Hover)
    };
    b = b.button(label);
    if !enabled {
        b = disable(b);
    }
    let id = ui.tree.add(parent, b);
    ui.tree.add(
        id,
        Node::text(label, metric::PILL_TEXT)
            .ink(if filled { Token::Ground } else { Token::Ink })
            .role(Role::None),
    );
    id
}

/// The popup chooser (`Picker(.menu)`): the chosen label and a chevron.
/// Returns the button; the caller opens the menu under it.
pub fn popup(ui: &mut Ui, parent: NodeId, name: &str, label: &str, enabled: bool) -> NodeId {
    let mut b = Node::row()
        .height(24.0)
        .padding(Insets::new(10.0, 0.0, 6.0, 0.0))
        .gap(6.0)
        .radius(7.0)
        .align(Align::Center)
        .fill(Token::Ground)
        .stroke(Token::Hairline, 1.0)
        .hover_fill(Token::Hover)
        .press_fill(Token::Hover)
        .shrink(0.0);
    b = b.button(name).tooltip(label);
    if !enabled {
        b = disable(b);
    }
    let id = ui.tree.add(parent, b);
    ui.tree
        .add(id, Node::text(label, 12.0).ink(Token::Ink).role(Role::None));
    ui.tree.add(
        id,
        Node::glyph(glyph::CHEVRON_DOWN, 8.0, 12.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    id
}

/// A text box on the wash (`settingsField`). Returns the field.
#[allow(clippy::too_many_arguments)]
pub fn field(
    ui: &mut Ui,
    parent: NodeId,
    name: &str,
    text: &str,
    placeholder: &str,
    secret: bool,
    width: Option<f32>,
    enabled: bool,
) -> NodeId {
    let mut well = Node::row()
        .height(metric::FIELD_H)
        .padding(Insets::hv(8.0, 0.0))
        .radius(7.0)
        .fill(Token::Wash)
        .align(Align::Center);
    well = match width {
        Some(w) => well.width(w).shrink(0.0),
        None => well.grow(1.0).shrink(1.0).min_width(160.0),
    };
    if !enabled {
        well = well.opacity(metric::DISABLED);
    }
    let well = ui.tree.add(parent, well);
    let mut f = Node::field(placeholder, 12.5)
        .fill_height()
        .grow(1.0)
        .name(name);
    if secret {
        f = secure_field(f);
    }
    if !enabled {
        f.flags.disabled = true;
    }
    let id = ui.tree.add(well, f);
    if !text.is_empty() {
        ui.set_field_text(id, text, false);
    }
    id
}

/// A field that hides what is typed in it.
///
/// Shim: the toolkit's secure field (`Node::secure(true)`: one `●` per
/// character, no copy or cut, UIA `IsPassword`, no IME) lands with the
/// chrome worker's copper-win-ui change. Until it is merged this marks the
/// field for assistive technology only; the one-line swap is
/// `n.secure(true)`.
pub fn secure_field(n: Node) -> Node {
    n.name("Password")
}

/// The checkmark a line shows when the thing is already so (12 pt, 24
/// wide).
pub fn check(ui: &mut Ui, parent: NodeId) -> NodeId {
    ui.tree.add(
        parent,
        Node::glyph(glyph::CHECK, 12.0, 24.0)
            .ink(Token::Ink)
            .role(Role::Image)
            .name("Done"),
    )
}

/// A short muted value (a version, a count).
pub fn value(ui: &mut Ui, parent: NodeId, text: &str, ink: &Inks) -> NodeId {
    ui.tree
        .add(parent, Node::text(text, 12.5).ink(fixed(ink.detail)))
}

/// The round door in a header corner (`Door`): a glyph that does one thing.
pub fn door(ui: &mut Ui, parent: NodeId, g: char, name: &str, help: &str) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::stack()
            .size(28.0, 28.0)
            .radius(8.0)
            .hover_fill(Token::Hover)
            .press_fill(Token::Hover)
            .align(Align::Center)
            .justify(Justify::Center)
            .shrink(0.0)
            .button(name)
            .tooltip(help),
    );
    ui.tree.add(
        b,
        Node::glyph(g, 11.0, 28.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    b
}

/// Text with the letters at `marks` set apart by weight, not colour
/// (`SettingsMarks`): runs of plain and lit text side by side on one line,
/// only the last of which is cut short ([`look::runs`]).
pub fn marked(
    ui: &mut Ui,
    parent: NodeId,
    text: &str,
    marks: &std::collections::BTreeSet<usize>,
    size: f32,
    plain: (Weight, Swatch),
    lit: (Weight, Swatch),
) -> NodeId {
    let row = ui
        .tree
        .add(parent, Node::row().shrink(1.0).role(Role::Text).name(text));
    let runs = super::look::runs(text, marks, MARKED_BUDGET);
    let last = runs.len().saturating_sub(1);
    for (i, (run, on)) in runs.into_iter().enumerate() {
        let (w, ink) = if on { lit } else { plain };
        ui.tree.add(
            row,
            Node::text(&run, size)
                .weight(w)
                .ink(ink)
                .shrink(if i == last { 1.0 } else { 0.0 })
                .trim(Trim::End)
                .role(Role::None),
        );
    }
    row
}

/// The letters a marked line keeps whole before its last run, which alone
/// is cut short — about what the results' narrowest column holds.
const MARKED_BUDGET: usize = 56;
