//! The Calendar pin's card (Mac: `Fork/CalendarPeek.swift`): rest the
//! pointer on a Google Calendar pin and the next meetings show beside it,
//! with Join for a video call. What it lists and when it opens and goes
//! are copper-shell-core's `calendar`, shared with Linux; this is the card
//! as nodes, platform-free so it is tested on any host, and ([`hover`],
//! Windows) the pointer and the clicks.
//!
//! The card is a layer over the column and the page. Nothing in it can be
//! focused: showing it never takes the keyboard from the page.

#[cfg(windows)]
mod hover;
#[cfg(windows)]
pub use hover::{click, hover};

use copper_shell_core::calendar::{self, Card as Model, Meeting};
use copper_shell_core::format::Calendar;
use copper_win_ui::tokens::Shadow;
use copper_win_ui::tree::Tree;
use copper_win_ui::{
    Abs, Align, Color, Insets, Node, NodeId, Rect, Role, Swatch, Token, Trim, Weight,
};

/// The card's width (the Mac's 290), its gap from the pin, its corners.
pub const WIDTH: f32 = 290.0;
const GAP: f32 = 8.0;
const RADIUS: f32 = 12.0;
/// A pill's corners: half its height, whatever that is.
const CAPSULE: f32 = 999.0;

/// What a click in the card does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Open the meeting's call in a new tab.
    Join(String),
    /// Settings › Tabs, where calendars are added.
    Settings,
}

/// The card's node and its buttons.
pub struct Card {
    pub node: NodeId,
    acts: Vec<(NodeId, Act)>,
}

impl Card {
    /// The card, hidden, over everything in `parent` drawn before it.
    pub fn build(tree: &mut Tree, parent: NodeId) -> Card {
        let node = tree.add(
            parent,
            Node::column()
                .absolute(at(0.0, 0.0))
                .width(WIDTH)
                .padding(Insets::new(0.0, 0.0, 0.0, 4.0))
                .fill(Token::Ground)
                .stroke(Token::Hairline, 1.0)
                .radius(RADIUS)
                .shadow(Shadow::new(0.12, 20.0, 6.0))
                .layer()
                .hit()
                .role(Role::Group)
                .name(calendar::HEADING)
                .hidden(true),
        );
        Card {
            node,
            acts: Vec::new(),
        }
    }

    /// Draw `model` as it is at `now` (Unix seconds), times on `C`'s clock.
    pub fn draw<C: Calendar>(&mut self, tree: &mut Tree, model: &Model, now: i64) {
        tree.clear_children(self.node);
        self.acts.clear();
        let card = self.node;
        let words = |tree: &mut Tree, text: &str, ink: Token, pad: Insets| {
            let line = tree.add(card, Node::row().padding(pad));
            tree.add(
                line,
                Node::text(text, 13.0)
                    .ink(ink)
                    .trim(Trim::Wrap)
                    .grow(1.0)
                    .shrink(1.0),
            );
        };
        let head = tree.add(
            card,
            Node::row().padding(Insets::new(14.0, 12.0, 14.0, 6.0)),
        );
        tree.add(
            head,
            Node::text(calendar::HEADING, 11.0)
                .weight(Weight::Medium)
                .ink(Token::Muted)
                .role(Role::None),
        );
        match model {
            Model::NoFeeds => {
                words(
                    tree,
                    calendar::NO_FEEDS,
                    Token::Ink,
                    Insets::new(14.0, 0.0, 14.0, 10.0),
                );
                let line = tree.add(
                    card,
                    Node::row().padding(Insets::new(14.0, 0.0, 14.0, 10.0)),
                );
                let b = pill(
                    tree,
                    line,
                    calendar::OPEN_SETTINGS,
                    false,
                    "Settings › Tabs",
                );
                self.acts.push((b, Act::Settings));
            }
            Model::Loading => words(tree, calendar::LOADING, Token::Muted, quiet()),
            Model::Meetings { list, error } => {
                if list.is_empty() && error.is_none() {
                    words(tree, calendar::NOTHING, Token::Muted, quiet());
                }
                for (i, m) in list.iter().enumerate() {
                    if i > 0 {
                        let rule =
                            tree.add(card, Node::row().padding(Insets::new(31.0, 0.0, 0.0, 0.0)));
                        tree.add(
                            rule,
                            Node::row().grow(1.0).height(1.0).fill(Token::Hairline),
                        );
                    }
                    self.row::<C>(tree, m, now);
                }
                if let Some(e) = error {
                    let line =
                        tree.add(card, Node::row().padding(Insets::new(14.0, 4.0, 14.0, 8.0)));
                    tree.add(
                        line,
                        Node::text(e, 11.5)
                            .ink(Token::Muted)
                            .grow(1.0)
                            .shrink(1.0)
                            .tooltip(e),
                    );
                }
            }
        }
    }

    /// One meeting: its calendar's dot, the title on one line, when, Join.
    fn row<C: Calendar>(&mut self, tree: &mut Tree, m: &Meeting, now: i64) {
        let row = tree.add(
            self.node,
            Node::row()
                .padding(Insets::new(14.0, 8.0, 12.0, 8.0))
                .gap(10.0)
                .align(Align::Center),
        );
        tree.add(
            row,
            Node::stack()
                .size(7.0, 7.0)
                .shrink(0.0)
                .radius(3.5)
                .fill(Swatch::Fixed(color(&m.color)))
                .role(Role::None),
        );
        let words = tree.add(row, Node::column().gap(2.0).grow(1.0).shrink(1.0));
        tree.add(
            words,
            Node::text(&m.title, 13.0)
                .weight(Weight::Medium)
                .ink(Token::Ink)
                .shrink(1.0),
        );
        tree.add(
            words,
            Node::text(&calendar::when::<C>(m, now), 11.5)
                .ink(if m.start <= now {
                    Token::Accent
                } else {
                    Token::Muted
                })
                .shrink(1.0),
        );
        if let Some(link) = &m.join {
            let host = copper_shell_core::shell::host_of(link);
            let b = pill(tree, row, calendar::JOIN, true, &host);
            self.acts.push((b, Act::Join(link.clone())));
        }
    }

    /// Beside the pin at `pin` (window DIPs), its heading level with it.
    pub fn place(&self, tree: &mut Tree, pin: Rect) {
        let to = at(pin.right() + GAP, (pin.y - 4.0).max(GAP));
        tree.edit(self.node, |n| n.style.absolute = Some(to));
        tree.set_hidden(self.node, false);
    }

    pub fn hide(&self, tree: &mut Tree) {
        tree.set_hidden(self.node, true);
    }

    /// Whether `id` is the card or something in it.
    pub fn contains(&self, tree: &Tree, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == self.node {
                return true;
            }
            cur = tree.parent(c);
        }
        false
    }

    /// What clicking `id` does, if it is one of the card's buttons.
    pub fn act(&self, id: NodeId) -> Option<Act> {
        self.acts
            .iter()
            .find(|(n, _)| *n == id)
            .map(|(_, a)| a.clone())
    }
}

fn at(left: f32, top: f32) -> Abs {
    Abs {
        left: Some(left),
        top: Some(top),
        right: None,
        bottom: None,
    }
}

fn quiet() -> Insets {
    Insets::new(14.0, 0.0, 14.0, 10.0)
}

/// A pill in `parent`: `filled` in ink (Join), else quiet. A pointer
/// target, never a focus stop.
fn pill(tree: &mut Tree, parent: NodeId, label: &str, filled: bool, tip: &str) -> NodeId {
    let b = Node::row()
        .padding(Insets::hv(11.0, 4.0))
        .radius(CAPSULE)
        .align(Align::Center)
        .shrink(0.0)
        .hit()
        .role(Role::Button)
        .name(label)
        .tooltip(tip);
    let (b, ink) = if filled {
        (b.fill(Token::Ink), Token::Ground)
    } else {
        (b.fill(Token::Wash).hover_fill(Token::Hover), Token::Ink)
    };
    let b = tree.add(parent, b);
    tree.add(
        b,
        Node::text(label, 12.0)
            .weight(Weight::Medium)
            .ink(ink)
            .role(Role::None),
    );
    b
}

/// A calendar's `#rrggbb`; Google Calendar's peacock for anything else.
fn color(hex: &str) -> Color {
    let v = hex
        .strip_prefix('#')
        .filter(|h| h.len() == 6)
        .and_then(|h| u32::from_str_radix(h, 16).ok());
    Color::hex(v.unwrap_or(0x039be5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use copper_shell_core::format::Civil;
    use copper_win_ui::node::Content;

    /// A clock on UTC: hours and minutes are all the card reads.
    enum Utc {}

    impl Calendar for Utc {
        fn civil(unix: f64) -> Option<Civil> {
            let s = unix as i64;
            Some(Civil {
                year: 2026,
                month: 10,
                day: 28,
                day_of_year: 301,
                hour: (s.rem_euclid(86400) / 3600) as u32,
                minute: (s.rem_euclid(3600) / 60) as u32,
            })
        }
        fn month_name(_: u32) -> String {
            String::new()
        }
    }

    /// 2026-10-28 10:00 UTC.
    const NOW: i64 = 1_793_181_600;

    fn meeting(title: &str, start: i64, end: i64, join: Option<&str>) -> Meeting {
        Meeting {
            id: title.into(),
            title: title.into(),
            start: NOW + start * 60,
            end: NOW + end * 60,
            color: "#0b8043".into(),
            join: join.map(String::from),
        }
    }

    fn card() -> (Tree, Card) {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let c = Card::build(&mut t, root);
        (t, c)
    }

    /// Every text in the card, top to bottom.
    fn texts(t: &Tree, id: NodeId) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(Content::Text(x)) = t.get(id).map(|n| &n.content) {
            out.push(x.text.clone());
        }
        for c in t.children(id).to_vec() {
            out.extend(texts(t, c));
        }
        out
    }

    #[test]
    fn the_card_lists_the_meetings_with_join_for_calls() {
        let (mut t, mut c) = card();
        let model = Model::Meetings {
            list: vec![
                meeting(
                    "Daily standup",
                    -5,
                    30,
                    Some("https://meet.google.com/abc-defg-hij"),
                ),
                meeting("Design review", 60, 90, Some("https://zoom.us/j/1")),
                meeting("Lunch", 240, 300, None),
            ],
            error: None,
        };
        c.draw::<Utc>(&mut t, &model, NOW);
        assert_eq!(
            texts(&t, c.node),
            [
                "Coming up",
                "Daily standup",
                "Now · until 10:30 AM",
                "Join",
                "Design review",
                "In 60 min · 11:00 – 11:30 AM",
                "Join",
                "Lunch",
                "2:00 – 3:00 PM",
            ]
        );
        let joins: Vec<Act> = c.acts.iter().map(|(_, a)| a.clone()).collect();
        assert_eq!(
            joins,
            [
                Act::Join("https://meet.google.com/abc-defg-hij".into()),
                Act::Join("https://zoom.us/j/1".into()),
            ]
        );
        let (join, _) = c.acts[0];
        assert!(c.contains(&t, join));
        assert_eq!(
            t.get(join).unwrap().tooltip.as_deref(),
            Some("meet.google.com")
        );
        // Nothing in it is a focus stop: the page keeps the keyboard.
        let mut stack = vec![c.node];
        while let Some(n) = stack.pop() {
            assert!(!t.get(n).unwrap().flags.focusable);
            stack.extend_from_slice(t.children(n));
        }
    }

    #[test]
    fn the_cards_other_states() {
        let (mut t, mut c) = card();
        c.draw::<Utc>(&mut t, &Model::NoFeeds, NOW);
        assert_eq!(
            texts(&t, c.node),
            ["Coming up", calendar::NO_FEEDS, calendar::OPEN_SETTINGS]
        );
        assert_eq!(c.acts.len(), 1);
        assert_eq!(c.act(c.acts[0].0), Some(Act::Settings));
        c.draw::<Utc>(&mut t, &Model::Loading, NOW);
        assert_eq!(texts(&t, c.node), ["Coming up", calendar::LOADING]);
        assert!(c.acts.is_empty(), "a redraw forgets the old buttons");
        let empty = Model::Meetings {
            list: Vec::new(),
            error: None,
        };
        c.draw::<Utc>(&mut t, &empty, NOW);
        assert_eq!(texts(&t, c.node), ["Coming up", calendar::NOTHING]);
        let failed = Model::Meetings {
            list: Vec::new(),
            error: Some("Couldn't read Exowatt: no connection".into()),
        };
        c.draw::<Utc>(&mut t, &failed, NOW);
        assert_eq!(
            texts(&t, c.node),
            ["Coming up", "Couldn't read Exowatt: no connection"]
        );
    }

    #[test]
    fn the_card_sits_beside_the_pin() {
        let (mut t, c) = card();
        assert!(t.get(c.node).unwrap().style.hidden);
        c.place(&mut t, Rect::new(10.0, 80.0, 52.0, 40.0));
        let n = t.get(c.node).unwrap();
        assert!(!n.style.hidden);
        assert_eq!(n.style.absolute, Some(at(70.0, 76.0)));
        c.hide(&mut t);
        assert!(t.get(c.node).unwrap().style.hidden);
    }

    #[test]
    fn colours() {
        assert_eq!(color("#0b8043"), Color::hex(0x0b8043));
        assert_eq!(color("green"), Color::hex(0x039be5));
    }
}
