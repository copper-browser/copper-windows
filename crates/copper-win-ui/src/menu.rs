//! Context menus and tooltips: their model, their node trees and their
//! keyboard.
//!
//! The backend shows each in a popup window of its own (a layered
//! composition popup that may reach past the main window's edges); what is
//! in it is built here, from the tokens in [`crate::tokens::popup`], so a
//! menu looks like the ⌘K card it is cut from.

use crate::geom::Insets;
use crate::layout::{Align, Justify};
use crate::node::{Node, Role, Trim, Weight};
use crate::theme::{Swatch, Token};
use crate::tokens::{card, popup};
use crate::tree::{NodeId, Tree};

/// One line of a menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Action {
        id: u32,
        /// The label; `&` marks the access key (`&&` is a literal `&`).
        label: String,
        /// The shortcut shown on the right ("Ctrl+T").
        shortcut: Option<String>,
        enabled: bool,
        checked: bool,
    },
    Separator,
}

/// A menu's items.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Menu {
    pub items: Vec<Item>,
}

impl Menu {
    pub fn new() -> Menu {
        Menu::default()
    }

    /// Add an action.
    pub fn item(mut self, id: u32, label: &str) -> Menu {
        self.items.push(Item::Action {
            id,
            label: label.to_owned(),
            shortcut: None,
            enabled: true,
            checked: false,
        });
        self
    }

    /// Give the last action a shortcut hint.
    pub fn shortcut(mut self, s: &str) -> Menu {
        if let Some(Item::Action { shortcut, .. }) = self.items.last_mut() {
            *shortcut = Some(s.to_owned());
        }
        self
    }

    /// Grey out the last action.
    pub fn disabled(mut self) -> Menu {
        if let Some(Item::Action { enabled, .. }) = self.items.last_mut() {
            *enabled = false;
        }
        self
    }

    /// Tick the last action.
    pub fn checked(mut self, on: bool) -> Menu {
        if let Some(Item::Action { checked, .. }) = self.items.last_mut() {
            *checked = on;
        }
        self
    }

    pub fn separator(mut self) -> Menu {
        self.items.push(Item::Separator);
        self
    }

    fn actionable(&self, i: usize) -> bool {
        matches!(self.items.get(i), Some(Item::Action { enabled: true, .. }))
    }

    /// The next enabled action after `from` (or the first), wrapping.
    pub fn step(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        let n = self.items.len();
        if n == 0 {
            return None;
        }
        let mut i = from.unwrap_or(if forward { n - 1 } else { 0 });
        for _ in 0..n {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if self.actionable(i) {
                return Some(i);
            }
        }
        None
    }

    /// The enabled action whose access key (or, failing that, first letter)
    /// is `c`, searching after `from`.
    pub fn access(&self, c: char, from: Option<usize>) -> Option<usize> {
        let c = c.to_lowercase().next()?;
        let n = self.items.len();
        let start = from.map_or(0, |f| f + 1);
        let found = |by_mark: bool| {
            (0..n).map(|k| (start + k) % n).find(|&i| {
                self.actionable(i)
                    && match &self.items[i] {
                        Item::Action { label, .. } => {
                            let key = if by_mark {
                                access_key(label)
                            } else {
                                strip_access(label).chars().next()
                            };
                            key.and_then(|k| k.to_lowercase().next()) == Some(c)
                        }
                        Item::Separator => false,
                    }
            })
        };
        found(true).or_else(|| found(false))
    }

    /// The id of the action at `i`.
    pub fn id_at(&self, i: usize) -> Option<u32> {
        match self.items.get(i) {
            Some(Item::Action {
                id, enabled: true, ..
            }) => Some(*id),
            _ => None,
        }
    }
}

/// The label without its `&` marks.
pub fn strip_access(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                out.push('&');
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The character after a single `&`.
pub fn access_key(label: &str) -> Option<char> {
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.next() {
                Some('&') => continue,
                other => return other,
            }
        }
    }
    None
}

/// A built menu: its root and the row standing for each item (`None` for
/// separators).
#[derive(Clone, Debug)]
pub struct Built {
    pub root: NodeId,
    pub rows: Vec<Option<NodeId>>,
}

/// The card's look shared by menus and tooltips: Copper's ground, a
/// hairline ring, a lift.
fn surface(n: Node, radius: f32, shadow: crate::tokens::Shadow) -> Node {
    n.fill(Token::Ground)
        .radius(radius)
        .stroke(Swatch::Fade(Token::Hairline, 1.0), 1.0)
        .shadow(shadow)
}

/// Build `menu` into `tree` as its root. `margin` leaves room for the
/// shadow inside the popup window.
pub fn build(tree: &mut Tree, menu: &Menu, margin: f32) -> Built {
    let root = tree.add_root(
        Node::stack()
            .padding(Insets::all(margin))
            .align(Align::Start),
    );
    let cardn = tree.add(
        root,
        surface(
            Node::column()
                .padding(Insets::all(popup::MENU_PAD))
                .min_width(popup::MENU_MIN_WIDTH)
                .role(Role::Menu)
                .name("Menu"),
            popup::MENU_RADIUS,
            popup::MENU_SHADOW,
        ),
    );
    let mut rows = Vec::with_capacity(menu.items.len());
    for (i, item) in menu.items.iter().enumerate() {
        match item {
            Item::Separator => {
                let rule = tree.add(
                    cardn,
                    Node::new()
                        .height(1.0 + 2.0 * popup::MENU_RULE_PAD)
                        .padding(Insets::hv(popup::MENU_ROW_PAD, popup::MENU_RULE_PAD)),
                );
                tree.add(
                    rule,
                    Node::new()
                        .height(1.0)
                        .fill(Swatch::Fade(Token::Hairline, card::RULE_ALPHA * 1.6)),
                );
                rows.push(None);
            }
            Item::Action {
                label,
                shortcut,
                enabled,
                checked,
                ..
            } => {
                let shown = strip_access(label);
                let ink = if *enabled { Token::Ink } else { Token::Faint };
                let mut row = Node::row()
                    .height(popup::MENU_ROW)
                    .padding(Insets::hv(popup::MENU_ROW_PAD, 0.0))
                    .gap(card::ROW_GAP)
                    .align(Align::Center)
                    .radius(popup::MENU_ROW_RADIUS)
                    .role(Role::MenuItem)
                    .name(&shown)
                    .tag(i as u64)
                    .ink(ink);
                row.flags.hit = true;
                row.flags.disabled = !enabled;
                if *enabled {
                    row = row
                        .hover_fill(Swatch::Fade(Token::Accent, card::PICKED_ALPHA))
                        .selected_fill(Swatch::Fade(Token::Accent, card::PICKED_ALPHA));
                }
                let row = tree.add(cardn, row);
                let tick = if *checked { "\u{E73E}" } else { "" };
                tree.add(
                    row,
                    Node::glyph(tick.chars().next().unwrap_or(' '), 12.0, 14.0).ink(ink),
                );
                tree.add(
                    row,
                    Node::text(&shown, popup::MENU_TEXT)
                        .weight(Weight::Regular)
                        .grow(1.0)
                        .ink(ink),
                );
                if let Some(s) = shortcut {
                    tree.add(
                        row,
                        Node::text(s, card::ROW_HINT)
                            .weight(Weight::Medium)
                            .trim(Trim::End)
                            .shrink(0.0)
                            .ink(Token::Muted),
                    );
                }
                rows.push(Some(row));
            }
        }
    }
    Built { root, rows }
}

/// Build a tooltip saying `text` into `tree`.
pub fn build_tooltip(tree: &mut Tree, text: &str, margin: f32) -> NodeId {
    let root = tree.add_root(
        Node::stack()
            .padding(Insets::all(margin))
            .align(Align::Start),
    );
    let tip = tree.add(
        root,
        surface(
            Node::row()
                .padding(Insets::hv(popup::TIP_PAD_H, popup::TIP_PAD_V))
                .justify(Justify::Center)
                .role(Role::ToolTip)
                .name(text),
            popup::TIP_RADIUS,
            popup::TIP_SHADOW,
        ),
    );
    tree.add(
        tip,
        Node::text(text, popup::TIP_TEXT)
            .ink(Token::Ink)
            .max_width(360.0)
            .trim(Trim::Wrap),
    );
    root
}

/// Highlight row `i` of a built menu (keyboard navigation).
pub fn highlight(tree: &mut Tree, built: &Built, i: Option<usize>) {
    for (k, row) in built.rows.iter().enumerate() {
        if let Some(r) = row {
            tree.set_selected(*r, Some(k) == i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Menu {
        Menu::new()
            .item(1, "&Back")
            .shortcut("Alt+Left")
            .item(2, "&Reload")
            .disabled()
            .separator()
            .item(3, "Save page &as…")
            .item(4, "Bookmarks && history")
    }

    #[test]
    fn steps_skip_separators_and_disabled() {
        let m = sample();
        assert_eq!(m.step(None, true), Some(0));
        assert_eq!(m.step(Some(0), true), Some(3));
        assert_eq!(m.step(Some(4), true), Some(0));
        assert_eq!(m.step(Some(0), false), Some(4));
        assert_eq!(Menu::new().step(None, true), None);
    }

    #[test]
    fn access_keys() {
        let m = sample();
        assert_eq!(m.access('a', None), Some(3));
        assert_eq!(m.access('B', None), Some(0));
        assert_eq!(m.access('r', None), None); // Reload is disabled
        assert_eq!(strip_access("Bookmarks && history"), "Bookmarks & history");
        assert_eq!(access_key("Save page &as…"), Some('a'));
        assert_eq!(m.id_at(3), Some(3));
        assert_eq!(m.id_at(1), None);
    }

    #[test]
    fn builds_a_row_per_item() {
        let mut t = Tree::new();
        let b = build(&mut t, &sample(), 10.0);
        assert_eq!(b.rows.len(), 5);
        assert!(b.rows[2].is_none());
        highlight(&mut t, &b, Some(3));
        assert!(t.get(b.rows[3].unwrap()).unwrap().flags.selected);
        assert!(t.get(b.rows[1].unwrap()).unwrap().flags.disabled);
        let mut tt = Tree::new();
        let tip = build_tooltip(&mut tt, "Reload", 6.0);
        let label = tt.children(tip)[0];
        assert_eq!(tt.get(label).unwrap().a11y.role, Role::ToolTip);
    }
}
