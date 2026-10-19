//! The card (`Float.swift` / `Fork/CommandBar.swift`): one field and the
//! rows under it, dropped over the window a third of the way down. It is
//! where a new tab is launched (`Ctrl+T`, and the empty space), where the
//! address is edited (`Ctrl+L`, a click on the address) and the palette
//! (`Ctrl+K`).
//!
//! This is the view only. What the rows are and what picking one does is
//! [`crate::palette`]'s.

use super::glyph;
use super::images::Images;
use crate::platform::Icon;
use copper_shell_core::palette::complete::{Kind, Purpose, Row};
use copper_win_ui::tokens::{card, motion};
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Insets, Node, NodeId, Prop, Role, Swatch, Token, Trim, Weight};

/// The card's nodes and state.
pub struct Card {
    scrim: NodeId,
    card: NodeId,
    pub field: NodeId,
    results: NodeId,
    rule: NodeId,
    purpose: Purpose,
    private: bool,
    open: bool,
    /// Opened because the space is empty, and not touched since: a tab
    /// arriving closes it.
    auto: bool,
    rows: Vec<(Row, NodeId)>,
    picked: Option<usize>,
    /// What was typed, without the inline completion.
    pub typed: String,
    /// Focus to give back when it closes.
    before: Option<NodeId>,
}

/// One row's picture: a tab's icon, a glyph, or a letter.
pub enum RowMark {
    Icon(Icon),
    Glyph(char),
    Letter(String),
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
            .name("Search or enter address"),
    );
    let field_row = ui.tree.add(
        card_n,
        Node::row()
            .height(card::FIELD_HEIGHT)
            .padding(Insets::hv(card::FIELD_PAD, 0.0))
            .gap(card::FIELD_SPACING)
            .align(Align::Center),
    );
    ui.tree.add(
        field_row,
        Node::glyph(glyph::SEARCH, card::GLASS, card::GLASS + 4.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let field = ui.tree.add(
        field_row,
        Node::field(PLACEHOLDER, card::FIELD_TEXT)
            .fill_height()
            .grow(1.0),
    );
    let rule = ui.tree.add(
        card_n,
        Node::new()
            .height(1.0)
            .fill(Swatch::Fade(Token::Hairline, card::RULE_ALPHA))
            .hidden(true),
    );
    let results = ui.tree.add(
        card_n,
        Node::column()
            .padding(Insets::all(card::LIST_PAD))
            .gap(card::ROW_SPACING)
            .role(Role::List)
            .name("Suggestions")
            .hidden(true),
    );
    Card {
        scrim,
        card: card_n,
        field,
        results,
        rule,
        purpose: Purpose::Launch,
        private: false,
        open: false,
        auto: false,
        rows: Vec::new(),
        picked: None,
        typed: String::new(),
        before: None,
    }
}

/// The field's words for an address.
pub const PLACEHOLDER: &str = "Search or enter an address";
/// …and for the palette.
pub const PALETTE_PLACEHOLDER: &str = "Search tabs, commands and places";

impl Card {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// Launching a private tab.
    pub fn is_private(&self) -> bool {
        self.private
    }

    /// Opened for an empty space and not touched since.
    pub fn is_auto(&self) -> bool {
        self.open && self.auto && self.typed.is_empty()
    }

    /// The rows on show.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().map(|(r, _)| r)
    }

    /// The highlighted row.
    pub fn picked(&self) -> Option<&Row> {
        self.picked.and_then(|i| self.rows.get(i)).map(|(r, _)| r)
    }

    pub fn picked_index(&self) -> Option<usize> {
        self.picked
    }

    /// Open for `purpose` with `text` in the field, all of it selected.
    pub fn open(&mut self, ui: &mut Ui, purpose: Purpose, private: bool, text: &str, auto: bool) {
        let was_open = self.open;
        self.purpose = purpose;
        self.private = private;
        self.auto = auto;
        self.open = true;
        self.typed = text.to_string();
        let placeholder = match purpose {
            Purpose::Palette => PALETTE_PLACEHOLDER,
            _ if private => copper_shell_core::private::PLACEHOLDER,
            _ => PLACEHOLDER,
        };
        ui.tree.edit(self.field, |n| {
            if let Some(f) = n.field_mut() {
                f.placeholder = placeholder.to_string();
            }
            n.a11y.name = placeholder.to_string();
        });
        // A third of the way down the window (`Float.swift` 29).
        let drop = (ui.size().h * card::DROP).round();
        ui.tree.edit(self.scrim, |n| n.style.padding.top = drop);
        // Over an empty stage the window behind barely dims.
        let scrim = if auto {
            card::SCRIM_EMPTY
        } else {
            card::SCRIM_OVER
        };
        ui.tree.repaint(self.scrim, |n| {
            n.look.fill = Swatch::Fade(Token::Black, scrim);
        });
        if !was_open {
            self.before = ui.tree.focused();
            ui.tree.set_hidden(self.scrim, false);
            ui.tree.set_prop(self.scrim, Prop::Opacity, 0.0);
            ui.tree
                .animate(self.scrim, Prop::Opacity, 1.0, motion::QUICK);
            ui.tree.set_prop(self.card, Prop::Scale, card::ENTER_SCALE);
            ui.tree.animate(self.card, Prop::Scale, 1.0, motion::SETTLE);
        }
        ui.set_field_text(self.field, text, true);
        ui.focus(Some(self.field));
    }

    /// Put it away; focus goes back where it was (`page`: to the page).
    pub fn close(&mut self, ui: &mut Ui) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.auto = false;
        ui.tree.set_hidden(self.scrim, true);
        self.clear_rows(ui);
        let back = self.before.take().filter(|&id| ui.tree.contains(id));
        if ui.tree.focused() == Some(self.field) {
            ui.focus(back);
        }
        true
    }

    /// The field's whole text (with any completion offered).
    pub fn text(&self, ui: &Ui) -> String {
        ui.field_text(self.field).unwrap_or_default().to_string()
    }

    /// Offer `rest` after what was typed, selected so the next key
    /// replaces it.
    pub fn offer(&mut self, ui: &mut Ui, rest: &str) {
        let typed = self.typed.clone();
        ui.edit_field(self.field, |e| {
            let at = typed.len();
            e.set_text(&format!("{typed}{rest}"), false);
            e.select(at..at + rest.len());
        });
    }

    fn clear_rows(&mut self, ui: &mut Ui) {
        ui.tree.clear_children(self.results);
        self.rows.clear();
        self.picked = None;
        ui.tree.set_hidden(self.results, true);
        ui.tree.set_hidden(self.rule, true);
    }

    /// Show `rows` with `default` highlighted; `mark` gives each its
    /// picture.
    pub fn set_rows(
        &mut self,
        ui: &mut Ui,
        images: &mut Images,
        rows: Vec<Row>,
        default: Option<usize>,
        mut mark: impl FnMut(&Row) -> RowMark,
    ) {
        self.clear_rows(ui);
        if rows.is_empty() {
            return;
        }
        ui.tree.set_hidden(self.results, false);
        ui.tree.set_hidden(self.rule, false);
        let mut section = None;
        for row in rows {
            if row.section.is_some() && row.section != section {
                section = row.section;
                if let Some(s) = section {
                    ui.tree.add(
                        self.results,
                        Node::text(s.title(row.kind), card::HEADING)
                            .weight(Weight::Semibold)
                            .ink(Token::Muted)
                            .height(card::HEADING_HEIGHT)
                            .padding(Insets::hv(card::ROW_PAD, 0.0))
                            .role(Role::None),
                    );
                }
            }
            let m = mark(&row);
            let node = self.add_row(ui, images, &row, m);
            self.rows.push((row, node));
        }
        self.picked = default.filter(|&i| i < self.rows.len());
        self.show_picked(ui);
    }

    fn add_row(&self, ui: &mut Ui, images: &mut Images, row: &Row, m: RowMark) -> NodeId {
        let node = ui.tree.add(
            self.results,
            Node::row()
                .height(card::ROW_HEIGHT)
                .padding(Insets::hv(card::ROW_PAD, 0.0))
                .gap(card::ROW_GAP)
                .align(Align::Center)
                .radius(card::ROW_RADIUS)
                .hover_fill(Token::Hover)
                .selected_fill(Swatch::Fade(Token::Accent, card::PICKED_ALPHA))
                .button(&row.title)
                .role(Role::ListItem),
        );
        match m {
            RowMark::Icon(icon) => {
                let id = images.id(ui, &icon);
                super::build::mark(ui, node, Some(id), "", card::ROW_MARK);
            }
            RowMark::Glyph(g) => {
                ui.tree.add(
                    node,
                    Node::glyph(g, card::ROW_MARK - 3.0, card::ROW_MARK)
                        .ink(Token::Muted)
                        .role(Role::None),
                );
            }
            RowMark::Letter(l) => {
                let m = super::build::mark(ui, node, None, &l, card::ROW_MARK);
                ui.tree
                    .repaint(m, |n| n.look.fill = Swatch::Token(Token::Hover));
            }
        }
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
                    .trim(Trim::Middle)
                    .shrink(2.0)
                    .role(Role::None),
            );
        }
        ui.tree.add(node, Node::new().grow(1.0));
        if !row.hint.is_empty() {
            ui.tree.add(
                node,
                Node::text(&row.hint, card::ROW_HINT)
                    .weight(Weight::Medium)
                    .ink(Token::Muted)
                    .shrink(0.0)
                    .role(Role::None),
            );
        }
        node
    }

    fn show_picked(&self, ui: &mut Ui) {
        for (i, (_, node)) in self.rows.iter().enumerate() {
            ui.tree.set_selected(*node, Some(i) == self.picked);
        }
    }

    /// Move the highlight by `delta` rows (wrapping).
    pub fn step(&mut self, ui: &mut Ui, delta: i32) {
        let n = self.rows.len() as i32;
        if n == 0 {
            return;
        }
        let at = match self.picked {
            Some(i) => (i as i32 + delta).rem_euclid(n),
            None if delta > 0 => 0,
            None => n - 1,
        };
        self.picked = Some(at as usize);
        self.show_picked(ui);
    }

    /// Highlight row `i`.
    pub fn pick(&mut self, ui: &mut Ui, i: Option<usize>) {
        self.picked = i.filter(|&i| i < self.rows.len());
        self.show_picked(ui);
    }

    /// The row a click landed on (the row or anything in it).
    pub fn row_at(&self, ui: &Ui, id: NodeId) -> Option<usize> {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if let Some(i) = self.rows.iter().position(|(_, n)| *n == c) {
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
        id == self.scrim
    }

    /// The window was resized: keep the card a third of the way down.
    pub fn resized(&self, ui: &mut Ui) {
        if self.open {
            let drop = (ui.size().h * card::DROP).round();
            ui.tree.edit(self.scrim, |n| n.style.padding.top = drop);
        }
    }
}

/// A row's picture when it has no icon: a glyph by kind.
pub fn kind_glyph(kind: Kind) -> char {
    match kind {
        Kind::Address => glyph::GLOBE,
        Kind::Search => glyph::SEARCH,
        Kind::Tab => glyph::GLOBE,
        Kind::Bookmark => glyph::STAR,
        Kind::Visited => glyph::HISTORY,
        Kind::Command => glyph::GEAR,
    }
}
