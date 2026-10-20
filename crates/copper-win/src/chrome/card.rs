//! The card and the field (`Fork/CommandPalette.swift`, `Omnibox.swift`):
//! one field and the rows under it, over the whole window.
//!
//! Two looks share the one field. `Ctrl+K` and `Ctrl+T` are the floating
//! card — 640 wide, a third of the way down, the rows in their sections
//! under a hairline. `Ctrl+L` (and a click on the address) is the plain
//! field: 560 wide, its own pill with its few places in a separate box
//! under it, lifted a little above the middle; standing on an empty stage
//! it breathes.
//!
//! The card also carries the space-delete question, the Mac's
//! window-attached warning sheet with its three answers (`SpaceDelete`).
//!
//! This is the view only. What the rows are and what picking one does is
//! [`crate::palette`]'s.

use super::glyph;
use super::images::Images;
use crate::palette::rank::{Kind, Row, Section};
use crate::platform::Icon;
use copper_shell_core::palette::complete::Purpose;
use copper_win_ui::tokens::{Shadow, card, motion};
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Abs, Align, Dim, Insets, Justify, Motion, Node, NodeId, Prop, Role, Swatch, Token, Trim, Weight,
};

/// The plain field's measures (`Omnibox.swift` 47–112): 560 wide, the
/// field 22 high inside 22 × 14, the list 6 in, corners 14.
mod plain {
    use copper_win_ui::tokens::Shadow;
    pub const WIDTH: f32 = 560.0;
    pub const GAP: f32 = 8.0;
    pub const LIFT: f32 = 60.0;
    pub const TEXT: f32 = 15.5;
    pub const FIELD: f32 = 22.0;
    pub const PAD_H: f32 = 22.0;
    pub const PAD_V: f32 = 14.0;
    pub const RADIUS: f32 = 14.0;
    pub const VEIL: f32 = 0.74;
    pub const FIELD_SHADOW: Shadow = Shadow::new(0.06, 24.0, 8.0);
    pub const LIST_PAD: f32 = 6.0;
    pub const LIST_SHADOW: Shadow = Shadow::new(0.07, 20.0, 6.0);
    pub const ROW_GAP: f32 = 10.0;
    pub const ROW_PAD_V: f32 = 9.0;
    pub const ROW_TITLE: f32 = 13.0;
    pub const ROW_DETAIL: f32 = 12.0;
    pub const ROW_GLYPH: f32 = 10.0;
    /// The breath: ink × .05, blurred 26, corners 26, .97 ↔ 1.03 and
    /// .65 ↔ 1 over 2.6 s.
    pub const BREATH: f32 = 0.05;
    pub const BREATH_RADIUS: f32 = 26.0;
    pub const BREATH_LOW: f32 = 0.97;
    pub const BREATH_HIGH: f32 = 1.03;
    pub const BREATH_DIM: f32 = 0.65;
}

/// The card's placeholders (spec §4).
pub const PALETTE_PLACEHOLDER: &str = "Search or enter an address";
pub const LAUNCH_PLACEHOLDER: &str = "Search or Enter URL…";
pub const PLAIN_PLACEHOLDER: &str = "Enter a web address";

/// A field's placeholder: ink at .30 (`AddressField`).
const PLACEHOLDER_ALPHA: f32 = 0.30;
/// The hint at rest on a hovered row: muted × .8.
const HINT_REST: f32 = 0.8;
/// `Ctrl+T`'s other ways to take a row: 11, muted × .85.
const ASIDE: f32 = 11.0;
const ASIDE_ALPHA: f32 = 0.85;
/// A command's or the search's glyph: 13 medium in the 18 square.
const ROW_GLYPH: f32 = 13.0;
/// At least this much between the words and the hint.
const ROW_SPACER: f32 = 14.0;
/// A heading's two DIPs under it, and the one between rows.
const HEADING_BOTTOM: f32 = 2.0;
/// The list's height before the window's is known.
const LIST_FALLBACK: f32 = 480.0;
/// The breath's own fill: next to nothing, so only its blur shows.
const BREATH_FILL: f32 = 0.01;

/// The breath's half cycle.
pub const BREATH_SECONDS: f32 = 2.6;

/// The card's nodes and state.
pub struct Card {
    scrim: NodeId,
    card: NodeId,
    field_row: NodeId,
    glass: NodeId,
    breath: NodeId,
    pub field: NodeId,
    rule: NodeId,
    list: NodeId,
    results: NodeId,
    purpose: Purpose,
    private: bool,
    open: bool,
    /// Opened because the stage is empty, and not touched since: a tab
    /// arriving closes it.
    auto: bool,
    rows: Vec<Drawn>,
    /// What each row on show drew its picture and its aside from: with
    /// the rows themselves, how a keystroke that changes nothing in the
    /// list is told from one that does.
    shown: Vec<(MarkKey, String)>,
    picked: Option<usize>,
    /// What was typed, without any completion offered after it.
    pub typed: String,
    /// The grey ending on offer after what was typed.
    pub ending: Option<String>,
    /// The arrows have moved since the last keystroke (Tab takes the row
    /// they are on).
    pub walked: bool,
    /// `Ctrl+K` pressed again while still held: let go to take the row.
    pub cycling: bool,
    refused: bool,
    breathing: bool,
    /// Focus to give back when it closes.
    before: Option<NodeId>,
    ask: Option<Ask>,
    /// The column of tabs: while it shows, the scrim leaves it alone and
    /// the card is shifted back over it by half, so it belongs to the
    /// window rather than to the page area.
    side: Option<NodeId>,
    /// The panels' plates: the standing field stays under them.
    plates: Option<NodeId>,
    /// A page is under the card (the scrim dims it more, the plain field
    /// veils it); `false` over a blank tab or none.
    over: bool,
}

/// One row on show.
struct Drawn {
    row: Row,
    node: NodeId,
    hint: Option<NodeId>,
    aside: Option<(NodeId, String)>,
    glyph: Option<NodeId>,
    /// Where it sits in the list (for following the highlight).
    top: f32,
}

/// One row's picture: a site's icon, a glyph, a letter, or the plain
/// field's open-page dot.
pub enum RowMark {
    Icon(Icon),
    Glyph(char),
    Letter(String),
    Dot,
    None,
}

/// What a [`RowMark`] was drawn from (an icon by identity).
#[derive(Clone, Debug, PartialEq)]
enum MarkKey {
    Icon(usize),
    Glyph(char),
    Letter(String),
    Dot,
    None,
}

impl MarkKey {
    fn of(m: &RowMark) -> MarkKey {
        match m {
            RowMark::Icon(i) => MarkKey::Icon(std::rc::Rc::as_ptr(i) as usize),
            RowMark::Glyph(c) => MarkKey::Glyph(*c),
            RowMark::Letter(l) => MarkKey::Letter(l.clone()),
            RowMark::Dot => MarkKey::Dot,
            RowMark::None => MarkKey::None,
        }
    }
}

/// The space-delete question on show.
struct Ask {
    root: NodeId,
    buttons: Vec<NodeId>,
    before: Option<NodeId>,
}

pub fn build(ui: &mut Ui, parent: NodeId) -> Card {
    let scrim = ui.tree.add(
        parent,
        Node::column()
            .absolute(Abs::FILL)
            .align(Align::Center)
            .padding(Insets::new(24.0, 240.0, 24.0, 24.0))
            .fill(Swatch::Fade(Token::Black, card::SCRIM_OVER))
            .layer()
            .hit()
            .modal()
            .hidden(true),
    );
    let card_n = ui.tree.add(
        scrim,
        Node::column()
            .width(card::WIDTH)
            .shrink(1.0)
            .fill(Token::Ground)
            .radius(card::RADIUS)
            .shadow(card::EDGE_SHADOW)
            .shadow(card::LIFT_SHADOW)
            .layer()
            .hit()
            .role(Role::Group)
            .name(PALETTE_PLACEHOLDER),
    );
    // The plain field's breath sits behind its pill.
    let field_box = ui.tree.add(card_n, Node::stack());
    let breath = ui.tree.add(
        field_box,
        Node::new()
            .absolute(Abs::FILL)
            .radius(plain::BREATH_RADIUS)
            // The toolkit's shadows are cast by a fill (and fade with it):
            // a fill too faint to see casts the whole glow.
            .fill(Swatch::Fade(Token::Ink, BREATH_FILL))
            .shadow(Shadow::new(
                plain::BREATH / BREATH_FILL,
                plain::BREATH_RADIUS,
                0.0,
            ))
            .shadow_ink(Token::Ink)
            .layer()
            .role(Role::None)
            .hidden(true),
    );
    let field_row = ui.tree.add(
        field_box,
        Node::row()
            .height(card::FIELD_HEIGHT)
            .padding(Insets::hv(card::FIELD_PAD, 0.0))
            .gap(card::FIELD_SPACING)
            .align(Align::Center)
            // Its own layer, so the breath's layer stays under it.
            .layer(),
    );
    let glass = ui.tree.add(
        field_row,
        Node::glyph(glyph::SEARCH, card::GLASS, card::GLASS + 4.0)
            .weight(Weight::Medium)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let field = ui.tree.add(
        field_row,
        Node::field(PALETTE_PLACEHOLDER, card::FIELD_TEXT)
            .height(24.0)
            .grow(1.0)
            .app_keys(),
    );
    ui.tree.edit(field, |n| {
        if let Some(f) = n.field_mut() {
            f.placeholder_ink = Swatch::Fade(Token::Ink, PLACEHOLDER_ALPHA);
        }
    });
    let rule = ui.tree.add(
        card_n,
        Node::new()
            .height(1.0)
            .fill(Swatch::Fade(Token::Hairline, card::RULE_ALPHA))
            .hidden(true),
    );
    let list = ui.tree.add(
        card_n,
        Node::scroll()
            .height(0.0)
            .role(Role::List)
            .name("Suggestions")
            .hidden(true),
    );
    let results = ui.tree.add(
        list,
        Node::column()
            .padding(Insets::all(card::LIST_PAD))
            .gap(card::ROW_SPACING),
    );
    Card {
        scrim,
        card: card_n,
        field_row,
        glass,
        breath,
        field,
        rule,
        list,
        results,
        purpose: Purpose::Launch,
        private: false,
        open: false,
        auto: false,
        rows: Vec::new(),
        shown: Vec::new(),
        picked: None,
        typed: String::new(),
        ending: None,
        walked: false,
        cycling: false,
        refused: false,
        breathing: false,
        before: None,
        ask: None,
        side: None,
        plates: None,
        over: true,
    }
}

impl Card {
    /// The card, or the delete question, is up.
    pub fn is_open(&self) -> bool {
        self.open || self.ask.is_some()
    }

    /// The card itself (not the question) is up.
    pub fn card_open(&self) -> bool {
        self.open
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// Launching a private tab.
    pub fn is_private(&self) -> bool {
        self.private
    }

    /// Opened for an empty stage and not touched since.
    pub fn is_auto(&self) -> bool {
        self.open && self.auto && self.typed.is_empty()
    }

    /// The plain field standing on an empty stage, with nothing behind it
    /// to go back to.
    pub fn is_standing(&self) -> bool {
        self.open && self.auto
    }

    /// The rows on show.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().map(|d| &d.row)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The highlighted row.
    pub fn picked(&self) -> Option<&Row> {
        self.picked.and_then(|i| self.rows.get(i)).map(|d| &d.row)
    }

    pub fn picked_index(&self) -> Option<usize> {
        self.picked
    }

    /// The row with this index.
    pub fn row(&self, i: usize) -> Option<&Row> {
        self.rows.get(i).map(|d| &d.row)
    }

    /// Whether a page is under the card (set before [`Card::open`]).
    pub fn set_over(&mut self, over: bool) {
        self.over = over;
    }

    /// Keep clear of the column of tabs `side` while it shows, and stand
    /// under the panels' `plates` (above them when asked for).
    pub fn keep_clear(&mut self, side: NodeId, plates: NodeId) {
        self.side = Some(side);
        self.plates = Some(plates);
    }

    /// The empty stage's standing field is part of the stage: a panel
    /// (Settings) opens over it. A card asked for, or a question, is over
    /// everything.
    fn stack(&self, ui: &mut Ui, top: bool) {
        let Some(parent) = ui.tree.parent(self.scrim) else {
            return;
        };
        let kids: Vec<NodeId> = ui
            .tree
            .children(parent)
            .iter()
            .copied()
            .filter(|&k| k != self.scrim)
            .collect();
        let at = match self.plates {
            Some(p) if !top => kids.iter().position(|&k| k == p).unwrap_or(kids.len()),
            _ => kids.len(),
        };
        if ui
            .tree
            .children(parent)
            .iter()
            .position(|&k| k == self.scrim)
            != Some(at)
        {
            ui.tree.reorder(self.scrim, at);
        }
    }

    /// The column's width now (0 with no column showing): the scrim leaves
    /// it alone and the card is shifted back over it by half.
    fn place(&mut self, ui: &mut Ui) {
        let width = self
            .side
            .filter(|&s| ui.tree.is_shown(s))
            .map_or(0.0, |s| ui.tree.window_rect(s).w);
        ui.tree.edit(self.scrim, |n| {
            n.style.absolute = Some(Abs {
                left: Some(width),
                ..Abs::FILL
            });
        });
        let dx = if self.purpose == Purpose::Edit {
            0.0
        } else {
            -(width / 2.0).round()
        };
        ui.tree.set_prop(self.card, Prop::X, dx);
    }

    /// Open for `purpose` with `text` in the field. `Edit` selects all of
    /// it (`Ctrl+L`); the cards leave the caret at the end. `auto`: the
    /// field standing on an empty stage.
    pub fn open(&mut self, ui: &mut Ui, purpose: Purpose, private: bool, text: &str, auto: bool) {
        // The empty stage's field is the plain one, standing.
        let purpose = if auto { Purpose::Edit } else { purpose };
        self.close_ask(ui);
        let was_open = self.open;
        let was_plain = self.open && self.purpose == Purpose::Edit;
        self.purpose = purpose;
        self.private = private;
        self.auto = auto;
        self.open = true;
        self.typed = text.to_string();
        self.ending = None;
        self.walked = false;
        self.cycling = false;
        self.refused = false;
        let placeholder = match purpose {
            Purpose::Palette => PALETTE_PLACEHOLDER,
            Purpose::Launch if private => copper_shell_core::private::PLACEHOLDER,
            Purpose::Launch => LAUNCH_PLACEHOLDER,
            Purpose::Edit => PLAIN_PLACEHOLDER,
        };
        ui.tree.edit(self.field, |n| {
            if let Some(f) = n.field_mut() {
                f.placeholder = placeholder.to_string();
            }
            n.a11y.name = placeholder.to_string();
        });
        ui.tree
            .edit(self.card, |n| n.a11y.name = placeholder.to_string());
        self.clear_rows(ui);
        if purpose == Purpose::Edit {
            self.look_plain(ui);
        } else {
            self.look_card(ui);
        }
        self.place(ui);
        self.stack(ui, !auto);
        if !was_open || was_plain != (purpose == Purpose::Edit) {
            self.before = if was_open {
                self.before
            } else {
                ui.tree.focused()
            };
            ui.tree.set_hidden(self.scrim, false);
            ui.tree.set_prop(self.scrim, Prop::Opacity, 0.0);
            ui.tree
                .animate(self.scrim, Prop::Opacity, 1.0, motion::QUICK);
            ui.tree.set_prop(self.card, Prop::Scale, card::ENTER_SCALE);
            ui.tree.animate(self.card, Prop::Scale, 1.0, motion::SETTLE);
        }
        ui.set_field_text(self.field, text, purpose == Purpose::Edit);
        set_ghost(ui, self.field, "");
        ui.focus(Some(self.field));
    }

    /// The floating card's look (`CommandPalette`).
    fn look_card(&mut self, ui: &mut Ui) {
        let drop = (ui.size().h * card::DROP).round();
        let scrim = if self.over {
            card::SCRIM_OVER
        } else {
            card::SCRIM_EMPTY
        };
        ui.tree.edit(self.scrim, |n| {
            n.style.padding = Insets::new(24.0, drop, 24.0, 24.0);
            n.style.justify = Justify::Start;
            n.look.fill = Swatch::Fade(Token::Black, scrim);
        });
        ui.tree.edit(self.card, |n| {
            n.style.width = Dim::Px(card::WIDTH);
            n.style.gap = 0.0;
            n.look.fill = Swatch::Token(Token::Ground);
            n.look.radius = card::RADIUS;
            n.look.shadow = Some(card::EDGE_SHADOW);
            n.look.shadow2 = Some(card::LIFT_SHADOW);
        });
        ui.tree.edit(self.field_row, |n| {
            n.style.height = Dim::Px(card::FIELD_HEIGHT);
            n.style.padding = Insets::hv(card::FIELD_PAD, 0.0);
            n.look.fill = Swatch::Clear;
            n.look.radius = 0.0;
            n.look.stroke_width = 0.0;
            n.look.shadow = None;
        });
        ui.tree.set_hidden(self.glass, false);
        ui.tree.edit(self.field, |n| {
            n.style.height = Dim::Px(24.0);
            if let Some(f) = n.field_mut() {
                f.size = card::FIELD_TEXT;
            }
        });
        ui.tree.edit(self.results, |n| {
            n.style.padding = Insets::all(card::LIST_PAD);
            n.style.gap = card::ROW_SPACING;
        });
        ui.tree.edit(self.list, |n| {
            n.look.fill = Swatch::Clear;
            n.look.radius = 0.0;
            n.look.stroke_width = 0.0;
            n.look.shadow = None;
        });
        self.breathe(ui, false);
    }

    /// The plain field's look (`Omnibox.plain`).
    fn look_plain(&mut self, ui: &mut Ui) {
        let veil = if self.auto || !self.over {
            Swatch::Clear
        } else {
            Swatch::Fade(Token::Ground, plain::VEIL)
        };
        ui.tree.edit(self.scrim, |n| {
            n.style.padding = Insets::new(24.0, 24.0, 24.0, 24.0 + plain::LIFT);
            n.style.justify = Justify::Center;
            n.look.fill = veil;
        });
        ui.tree.edit(self.card, |n| {
            n.style.width = Dim::Px(plain::WIDTH);
            n.style.gap = plain::GAP;
            n.look.fill = Swatch::Clear;
            n.look.radius = 0.0;
            n.look.shadow = None;
            n.look.shadow2 = None;
        });
        ui.tree.edit(self.field_row, |n| {
            n.style.height = Dim::Px(plain::FIELD + 2.0 * plain::PAD_V);
            n.style.padding = Insets::hv(plain::PAD_H, plain::PAD_V);
            n.look.fill = Swatch::Token(Token::Ground);
            n.look.radius = plain::RADIUS;
            n.look.stroke = Swatch::Token(Token::Hairline);
            n.look.stroke_width = 1.0;
            n.look.shadow = Some(plain::FIELD_SHADOW);
        });
        ui.tree.set_hidden(self.glass, true);
        ui.tree.edit(self.field, |n| {
            n.style.height = Dim::Px(plain::FIELD);
            if let Some(f) = n.field_mut() {
                f.size = plain::TEXT;
            }
        });
        ui.tree.edit(self.results, |n| {
            n.style.padding = Insets::all(plain::LIST_PAD);
            n.style.gap = 0.0;
        });
        ui.tree.edit(self.list, |n| {
            n.look.fill = Swatch::Token(Token::Ground);
            n.look.radius = plain::RADIUS;
            n.look.stroke = Swatch::Token(Token::Hairline);
            n.look.stroke_width = 1.0;
            n.look.shadow = Some(plain::LIST_SHADOW);
        });
        let standing = self.auto;
        self.breathe(ui, standing);
    }

    /// Start or stop the plain field's breath.
    fn breathe(&mut self, ui: &mut Ui, on: bool) {
        self.breathing = on;
        ui.tree.set_hidden(self.breath, !on);
        if on {
            ui.tree
                .set_prop(self.breath, Prop::Scale, plain::BREATH_LOW);
            ui.tree
                .set_prop(self.breath, Prop::Opacity, plain::BREATH_DIM);
        }
    }

    /// Whether the breath should go on (the caller's timer asks every
    /// half cycle).
    pub fn is_breathing(&self) -> bool {
        self.open && self.breathing
    }

    /// One half of the breath: in (`out` false) or out.
    pub fn breath_step(&self, ui: &mut Ui, out: bool) {
        if !self.is_breathing() {
            return;
        }
        let how = Motion::Curve {
            curve: copper_win_ui::spring::Bezier::new(0.42, 0.0, 0.58, 1.0),
            seconds: BREATH_SECONDS,
        };
        let (scale, opacity) = if out {
            (plain::BREATH_HIGH, 1.0)
        } else {
            (plain::BREATH_LOW, plain::BREATH_DIM)
        };
        ui.tree.animate(self.breath, Prop::Scale, scale, how);
        ui.tree.animate(self.breath, Prop::Opacity, opacity, how);
    }

    /// Put it away; focus goes back where it was.
    pub fn close(&mut self, ui: &mut Ui) -> bool {
        let asked = self.close_ask(ui);
        if !self.open {
            return asked;
        }
        self.open = false;
        self.auto = false;
        self.cycling = false;
        self.breathe(ui, false);
        ui.tree.set_hidden(self.scrim, true);
        self.clear_rows(ui);
        self.ending = None;
        set_ghost(ui, self.field, "");
        let back = self.before.take().filter(|&id| ui.tree.contains(id));
        if ui.tree.focused() == Some(self.field) {
            ui.focus(back);
        }
        true
    }

    /// The field's whole text.
    pub fn text(&self, ui: &Ui) -> String {
        ui.field_text(self.field).unwrap_or_default().to_string()
    }

    /// Put `text` in the field, the caret after it (an accepted
    /// completion, a walked row's words).
    pub fn set_text(&mut self, ui: &mut Ui, text: &str) {
        self.typed = text.to_string();
        self.ending = None;
        set_ghost(ui, self.field, "");
        ui.set_field_text(self.field, text, false);
    }

    /// The plain field's inline completion: `rest` after what was typed,
    /// selected so the next key replaces it (`AddressField`).
    pub fn offer(&mut self, ui: &mut Ui, rest: &str) {
        let typed = self.typed.clone();
        ui.edit_field(self.field, |e| {
            let at = typed.len();
            e.set_text(&format!("{typed}{rest}"), false);
            e.select(at..at + rest.len());
        });
    }

    /// The cards' grey ending, kept out of the literal field.
    pub fn show_ending(&mut self, ui: &mut Ui, ending: Option<String>) {
        set_ghost(ui, self.field, ending.as_deref().unwrap_or(""));
        self.ending = ending;
    }

    fn clear_rows(&mut self, ui: &mut Ui) {
        ui.tree.clear_children(self.results);
        self.rows.clear();
        self.shown.clear();
        self.picked = None;
        ui.tree.set_hidden(self.list, true);
        ui.tree.set_hidden(self.rule, true);
    }

    /// Show `rows` with `default` highlighted; `mark` gives each its
    /// picture, `aside` `Ctrl+T`'s other ways to take it.
    pub fn set_rows(
        &mut self,
        ui: &mut Ui,
        images: &mut Images,
        rows: Vec<Row>,
        default: Option<usize>,
        mut mark: impl FnMut(&Row) -> RowMark,
        mut aside: impl FnMut(&Row) -> String,
    ) {
        let plain = self.purpose == Purpose::Edit;
        let marks: Vec<RowMark> = rows.iter().map(&mut mark).collect();
        let asides: Vec<String> = rows
            .iter()
            .map(|r| if plain { String::new() } else { aside(r) })
            .collect();
        let shown: Vec<(MarkKey, String)> = marks
            .iter()
            .map(MarkKey::of)
            .zip(asides.iter().cloned())
            .collect();
        // The same list as on show (typing on through a word often is):
        // keep its nodes, only the highlight goes back to the default.
        // Rebuilding would lay out every row's text again for nothing.
        if !rows.is_empty()
            && rows.len() == self.rows.len()
            && shown == self.shown
            && rows.iter().zip(&self.rows).all(|(r, d)| *r == d.row)
        {
            self.picked = default.filter(|&i| i < self.rows.len());
            self.show_picked(ui);
            ui.tree.scroll(self.list, |s| s.scroll_to(0.0, false));
            return;
        }
        self.clear_rows(ui);
        if rows.is_empty() {
            return;
        }
        ui.tree.set_hidden(self.list, false);
        ui.tree.set_hidden(self.rule, plain);
        let pad = if plain {
            plain::LIST_PAD
        } else {
            card::LIST_PAD
        };
        let mut top = pad;
        let mut headings = 0usize;
        let mut section: Option<Section> = None;
        let mut marks = marks.into_iter();
        let mut asides = asides.into_iter();
        for row in rows {
            if !plain && Some(row.section) != section {
                section = Some(row.section);
                if let Some(title) = row.section.title() {
                    headings += 1;
                    let h = ui.tree.add(
                        self.results,
                        Node::column()
                            .height(card::HEADING_HEIGHT + HEADING_BOTTOM)
                            .padding(Insets::new(
                                card::ROW_PAD,
                                0.0,
                                card::ROW_PAD,
                                HEADING_BOTTOM,
                            ))
                            .justify(Justify::End)
                            .role(Role::None),
                    );
                    ui.tree.add(
                        h,
                        Node::text(title, card::HEADING)
                            .weight(Weight::Semibold)
                            .ink(Token::Muted)
                            .role(Role::Text),
                    );
                    top += card::HEADING_HEIGHT + HEADING_BOTTOM + card::ROW_SPACING;
                }
            }
            let m = marks.next().unwrap_or(RowMark::None);
            let a = asides.next().unwrap_or_default();
            let drawn = if plain {
                self.add_plain_row(ui, row, m, top)
            } else {
                self.add_row(ui, images, row, m, a, top)
            };
            top += if plain {
                plain_row_height()
            } else {
                card::ROW_HEIGHT + card::ROW_SPACING
            };
            self.rows.push(drawn);
        }
        let height = if plain {
            top + pad
        } else {
            let natural = self.rows.len() as f32 * (card::ROW_HEIGHT + card::ROW_SPACING)
                + headings as f32 * (card::HEADING_HEIGHT + 3.0)
                + 2.0 * card::LIST_PAD;
            let h = ui.size().h;
            let room = if h > 0.0 {
                h * (1.0 - card::DROP) - card::FIELD_HEIGHT - 40.0
            } else {
                LIST_FALLBACK
            };
            natural.min(room.max(200.0))
        };
        ui.tree
            .edit(self.list, |n| n.style.height = Dim::Px(height.round()));
        ui.tree.scroll(self.list, |s| s.scroll_to(0.0, false));
        self.shown = shown;
        self.picked = default.filter(|&i| i < self.rows.len());
        self.show_picked(ui);
    }

    fn add_row(
        &self,
        ui: &mut Ui,
        images: &mut Images,
        row: Row,
        m: RowMark,
        aside: String,
        top: f32,
    ) -> Drawn {
        let node = ui.tree.add(
            self.results,
            Node::row()
                .height(card::ROW_HEIGHT)
                .shrink(0.0)
                .padding(Insets::hv(card::ROW_PAD, 0.0))
                .gap(card::ROW_GAP)
                .align(Align::Center)
                .radius(card::ROW_RADIUS)
                .hover_fill(Token::Hover)
                .selected_fill(Swatch::Fade(Token::Accent, card::PICKED_ALPHA))
                .hit()
                .role(Role::ListItem)
                .name(&row.title),
        );
        let mut glyph_node = None;
        match m {
            RowMark::Icon(icon) => {
                let id = images.id(ui, &icon);
                super::build::mark(ui, node, Some(id), "", card::ROW_MARK);
            }
            RowMark::Glyph(g) => {
                glyph_node = Some(
                    ui.tree.add(
                        node,
                        Node::glyph(g, ROW_GLYPH, card::ROW_MARK)
                            .weight(Weight::Medium)
                            .ink(Token::Muted)
                            .selected_ink(Token::Accent)
                            .shrink(0.0)
                            .role(Role::None),
                    ),
                );
            }
            RowMark::Letter(l) => {
                let m = super::build::mark(ui, node, None, &l, card::ROW_MARK);
                ui.tree
                    .repaint(m, |n| n.look.fill = Swatch::Token(Token::Hover));
            }
            RowMark::Dot | RowMark::None => {
                ui.tree
                    .add(node, Node::new().size(card::ROW_MARK, card::ROW_MARK));
            }
        }
        // One weight, picked or not; the host is kept whole and the title
        // gives way (`CommandPalette.Row`).
        ui.tree.add(
            node,
            Node::text(&row.title, card::ROW_TITLE)
                .weight(Weight::Medium)
                .ink(Token::Ink)
                .shrink(1.0)
                .role(Role::None),
        );
        if !row.detail.is_empty() {
            ui.tree.add(
                node,
                Node::text(&row.detail, card::ROW_DETAIL)
                    .ink(Token::Muted)
                    .shrink(0.0)
                    .role(Role::None),
            );
        }
        if !row.badge.is_empty() {
            ui.tree.add(
                node,
                Node::text(&row.badge, card::ROW_DETAIL)
                    .ink(Token::Muted)
                    .shrink(0.0)
                    .role(Role::None),
            );
        }
        ui.tree.add(
            node,
            Node::new().grow(1.0).min_width(ROW_SPACER - card::ROW_GAP),
        );
        let aside = (!aside.is_empty()).then(|| {
            let id = ui.tree.add(
                node,
                Node::text(&aside, ASIDE)
                    .ink(Swatch::Fade(Token::Muted, ASIDE_ALPHA))
                    .shrink(0.0)
                    .hidden(true)
                    .role(Role::None),
            );
            (id, aside)
        });
        let hint = (!row.hint.is_empty()).then(|| {
            ui.tree.add(
                node,
                Node::text(&row.hint, card::ROW_HINT)
                    .weight(Weight::Medium)
                    .ink(Swatch::Fade(Token::Muted, HINT_REST))
                    .selected_ink(Token::Accent)
                    .shrink(0.0)
                    .reveal()
                    .role(Role::None),
            )
        });
        if !row.hint.is_empty() {
            ui.tree.edit(node, |n| n.a11y.help = row.hint.clone());
        }
        Drawn {
            row,
            node,
            hint,
            aside,
            glyph: glyph_node,
            top,
        }
    }

    fn add_plain_row(&self, ui: &mut Ui, row: Row, m: RowMark, top: f32) -> Drawn {
        let node = ui.tree.add(
            self.results,
            // As tall as the list's height counts it: a row left to its
            // text's line height came out a DIP or two taller, the list
            // overflowed by that much, and a lone row wore a scroll thumb.
            Node::row()
                .height(plain_row_height())
                .shrink(0.0)
                .padding(Insets::hv(card::ROW_PAD, plain::ROW_PAD_V))
                .gap(plain::ROW_GAP)
                .align(Align::Center)
                .radius(card::ROW_RADIUS)
                .hover_fill(Token::Hover)
                .selected_fill(Token::Wash)
                .hit()
                .role(Role::ListItem)
                .name(&row.title),
        );
        match m {
            RowMark::Glyph(g) => {
                ui.tree.add(
                    node,
                    Node::glyph(g, plain::ROW_GLYPH, plain::ROW_GLYPH + 2.0)
                        .weight(Weight::Medium)
                        .ink(Token::Muted)
                        .role(Role::None),
                );
            }
            RowMark::Dot => {
                let d = ui
                    .tree
                    .add(node, Node::stack().padding(Insets::hv(2.0, 0.0)));
                ui.tree.add(
                    d,
                    Node::new()
                        .size(5.0, 5.0)
                        .radius(2.5)
                        .fill(Swatch::Fade(Token::Ink, 0.55)),
                );
            }
            _ => {}
        }
        ui.tree.add(
            node,
            Node::text(&row.title, plain::ROW_TITLE)
                .ink(Token::Ink)
                .shrink(0.0)
                .role(Role::None),
        );
        if !row.detail.is_empty() {
            ui.tree.add(
                node,
                Node::text(&row.detail, plain::ROW_DETAIL)
                    .ink(Token::Muted)
                    .shrink(1.0)
                    .role(Role::None),
            );
        }
        Drawn {
            row,
            node,
            hint: None,
            aside: None,
            glyph: None,
            top,
        }
    }

    fn show_picked(&self, ui: &mut Ui) {
        for (i, d) in self.rows.iter().enumerate() {
            let on = Some(i) == self.picked;
            ui.tree.set_selected(d.node, on);
            if let Some(g) = d.glyph {
                ui.tree.set_selected(g, on);
            }
            if let Some(h) = d.hint {
                ui.tree.set_selected(h, on);
                // Laid out on every row, shown on the picked one and the
                // one under the pointer.
                ui.tree.edit(h, |n| n.flags.reveal = !on);
            }
            if let Some((a, _)) = &d.aside {
                ui.tree.set_hidden(*a, !on);
            }
        }
    }

    /// Highlight row `i` (or none), keeping it in view.
    pub fn pick(&mut self, ui: &mut Ui, i: Option<usize>) {
        self.picked = i.filter(|&i| i < self.rows.len());
        self.show_picked(ui);
        self.follow(ui);
    }

    /// Scroll the list so the picked row (and its heading) shows.
    fn follow(&self, ui: &mut Ui) {
        let Some(d) = self.picked.and_then(|i| self.rows.get(i)) else {
            return;
        };
        let view = match ui.tree.get(self.list).map(|n| n.style.height) {
            Some(Dim::Px(h)) => h,
            _ => return,
        };
        let heading = if self.picked == Some(0)
            || self
                .picked
                .and_then(|i| self.rows.get(i.wrapping_sub(1)))
                .is_some_and(|p| p.row.section != d.row.section)
        {
            card::HEADING_HEIGHT + HEADING_BOTTOM + card::ROW_SPACING
        } else {
            0.0
        };
        let top = (d.top - heading - card::LIST_PAD).max(0.0);
        let bottom = d.top + card::ROW_HEIGHT + card::LIST_PAD;
        ui.tree.scroll(self.list, |s| {
            let at = s.offset();
            if top < at {
                s.scroll_to(top, true);
            } else if bottom > at + view {
                s.scroll_to(bottom - view, true);
            }
        });
    }

    /// The row a click landed on (the row or anything in it).
    pub fn row_at(&self, ui: &Ui, id: NodeId) -> Option<usize> {
        if !self.open {
            return None;
        }
        let mut cur = Some(id);
        while let Some(c) = cur {
            if let Some(i) = self.rows.iter().position(|d| d.node == c) {
                return Some(i);
            }
            if c == self.results {
                return None;
            }
            cur = ui.tree.parent(c);
        }
        None
    }

    /// A click on the dimmed window around the card.
    pub fn is_scrim(&self, id: NodeId) -> bool {
        self.open && id == self.scrim
    }

    /// The window was resized: keep the card a third of the way down.
    pub fn resized(&mut self, ui: &mut Ui) {
        if !self.open {
            return;
        }
        if self.purpose != Purpose::Edit {
            let drop = (ui.size().h * card::DROP).round();
            ui.tree.edit(self.scrim, |n| n.style.padding.top = drop);
        }
        self.place(ui);
    }

    /// Return found no place to go: the field shivers — the toolkit's
    /// shared shake, three tapering there-and-backs 7 DIPs out over half a
    /// second (`Design.swift` `Shake`, `Omnibox.swift` 90–100) — and its
    /// edge goes red until the next keystroke. The field line shakes
    /// rather than the whole card: on `Ctrl+L` it *is* the card, and the
    /// launch/palette card's own offset holds it centred on the window.
    pub fn refuse(&mut self, ui: &mut Ui) {
        self.refused = true;
        if self.purpose == Purpose::Edit {
            ui.tree.repaint(self.field_row, |n| {
                n.look.stroke = Swatch::Fixed(copper_win_ui::Color::rgba(1.0, 0.23, 0.19, 0.35));
            });
        }
        ui.tree.shake(self.field_row);
    }

    /// The field changed: a refusal is forgotten.
    pub fn typed_again(&mut self, ui: &mut Ui) {
        if self.refused {
            self.refused = false;
            if self.purpose == Purpose::Edit {
                ui.tree.repaint(self.field_row, |n| {
                    n.look.stroke = Swatch::Token(Token::Hairline)
                });
            }
        }
    }

    // MARK: - the delete question

    /// Ask `title` / `detail` with `buttons` (`(label, destructive)`, the
    /// last one the cancel) over the dimmed window, the way the Mac's
    /// warning sheet does. Answered through [`Card::answer`].
    pub fn ask(&mut self, ui: &mut Ui, title: &str, detail: &str, buttons: &[(String, bool)]) {
        self.close_ask(ui);
        let root = ui.tree.insert(
            ui.tree.parent(self.scrim).unwrap_or(self.scrim),
            usize::MAX,
            Node::column()
                .absolute(Abs::FILL)
                .align(Align::Center)
                .justify(Justify::Center)
                .fill(Swatch::Fade(Token::Black, card::SCRIM_OVER))
                .layer()
                .hit()
                .modal(),
        );
        let panel = ui.tree.add(
            root,
            Node::column()
                .width(ASK_WIDTH)
                .padding(Insets::all(20.0))
                .gap(12.0)
                .fill(Token::Ground)
                .radius(card::RADIUS)
                .shadow(card::EDGE_SHADOW)
                .shadow(card::LIFT_SHADOW)
                .role(Role::Group)
                .name(title),
        );
        ui.tree.add(
            panel,
            Node::text(title, 13.5)
                .weight(Weight::Semibold)
                .ink(Token::Ink)
                .trim(Trim::Wrap),
        );
        if !detail.is_empty() {
            ui.tree.add(
                panel,
                Node::text(detail, 12.0).ink(Token::Muted).trim(Trim::Wrap),
            );
        }
        let stack = ui.tree.add(
            panel,
            Node::column()
                .gap(6.0)
                .padding(Insets::new(0.0, 6.0, 0.0, 0.0)),
        );
        let mut ids = Vec::new();
        for (label, destructive) in buttons {
            let b = ui.tree.add(
                stack,
                Node::row()
                    .height(30.0)
                    .justify(Justify::Center)
                    .align(Align::Center)
                    .radius(8.0)
                    .fill(if *destructive {
                        Swatch::Fixed(copper_win_ui::Color::rgba(0.86, 0.21, 0.18, 1.0))
                    } else {
                        Swatch::Token(Token::Wash)
                    })
                    .hover_fill(if *destructive {
                        Swatch::Fixed(copper_win_ui::Color::rgba(0.78, 0.17, 0.15, 1.0))
                    } else {
                        Swatch::Token(Token::Hairline)
                    })
                    .button(label),
            );
            ui.tree.add(
                b,
                Node::text(label, 12.5)
                    .weight(Weight::Medium)
                    .ink(if *destructive {
                        Swatch::Token(Token::White)
                    } else {
                        Swatch::Token(Token::Ink)
                    })
                    .role(Role::None),
            );
            ids.push(b);
        }
        let before = ui.tree.focused();
        // The safe answer has the keyboard: Cancel, the last.
        ui.focus(ids.last().copied());
        ui.tree.set_prop(panel, Prop::Opacity, 0.0);
        ui.tree.animate(panel, Prop::Opacity, 1.0, motion::QUICK);
        self.ask = Some(Ask {
            root,
            buttons: ids,
            before,
        });
    }

    /// A click on `id`: which of the question's buttons it was (the
    /// question is put away). `None`: not one of them.
    pub fn answer(&mut self, ui: &mut Ui, id: NodeId) -> Option<usize> {
        let ask = self.ask.as_ref()?;
        let mut cur = Some(id);
        while let Some(c) = cur {
            if let Some(i) = ask.buttons.iter().position(|b| *b == c) {
                self.close_ask(ui);
                return Some(i);
            }
            if c == ask.root {
                return None;
            }
            cur = ui.tree.parent(c);
        }
        None
    }

    /// The question is up.
    pub fn asking(&self) -> bool {
        self.ask.is_some()
    }

    fn close_ask(&mut self, ui: &mut Ui) -> bool {
        let Some(ask) = self.ask.take() else {
            return false;
        };
        ui.tree.remove(ask.root);
        if let Some(b) = ask.before.filter(|&b| ui.tree.contains(b)) {
            ui.focus(Some(b));
        }
        true
    }
}

/// The delete question's width (the Mac's alert, about).
const ASK_WIDTH: f32 = 300.0;

/// A plain-field row: 9 above and below 13-point words.
fn plain_row_height() -> f32 {
    2.0 * plain::ROW_PAD_V + 16.0
}

/// The grey ending after the caret (`Ui::set_field_ghost`); Right/End/Tab
/// take it (the field is `app_keys`, so they reach the palette).
fn set_ghost(ui: &mut Ui, field: NodeId, rest: &str) {
    ui.set_field_ghost(field, rest);
}

/// A row's picture when it has no icon: a glyph by kind.
pub fn kind_glyph(kind: Kind) -> char {
    match kind {
        Kind::Known | Kind::Open => glyph::GLOBE,
        Kind::Search => glyph::SEARCH,
        Kind::Bookmark => glyph::STAR,
        Kind::Visited => glyph::HISTORY,
        Kind::Command => glyph::GEAR,
        Kind::Canvas => glyph::CANVAS,
    }
}
