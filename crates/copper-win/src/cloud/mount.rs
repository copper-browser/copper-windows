//! Settings › Cloud's [`El`]s on screen: each element becomes nodes in the
//! page's list ([`expand`]), and every refresh lays the new elements over
//! the nodes already there ([`Mount::sync`]): a node whose key and kind
//! stay is patched in place — new words, a new look — and keeps its focus,
//! hover and caret; only what appeared is built and only what went away is
//! removed. So typing in a field, which changes the words around it,
//! never rebuilds the field.
//!
//! Pure: works on a [`Tree`], so it is tested without a window.

use super::el::{Act, El, FieldId, Kind, PillLook, Tone};
use super::model::{Dot, Mark};
use copper_win_ui::node::Content;
use copper_win_ui::tree::Tree;
use copper_win_ui::{
    Align, Color, Cursor, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim, Weight,
};
use std::collections::HashMap;

/// The glyphs drawn for checks and problems.
const CHECK: char = '\u{E73E}';
const ERROR: char = '\u{E783}';

/// One node to be, with what pressing it does and the field it is.
#[derive(Clone, Debug)]
pub struct Prim {
    pub key: String,
    pub node: Node,
    pub act: Option<Act>,
    pub field: Option<FieldId>,
    pub kids: Vec<Prim>,
}

impl Prim {
    fn new(key: impl Into<String>, node: Node) -> Prim {
        Prim {
            key: key.into(),
            node,
            act: None,
            field: None,
            kids: Vec::new(),
        }
    }

    fn with(mut self, kids: Vec<Prim>) -> Prim {
        self.kids = kids;
        self
    }

    fn act(mut self, act: Act) -> Prim {
        self.act = Some(act);
        self
    }
}

/// A node as built: its key, which kind of content it has, and the nodes
/// under it.
#[derive(Debug)]
struct Mounted {
    key: String,
    node: NodeId,
    shape: u8,
    kids: Vec<Mounted>,
}

/// The page as it is on screen: the list it was drawn into, the nodes,
/// and what each pressable node does.
#[derive(Debug, Default)]
pub struct Mount {
    list: Option<NodeId>,
    kids: Vec<Mounted>,
    acts: HashMap<NodeId, Act>,
    fields: HashMap<FieldId, NodeId>,
}

impl Mount {
    /// Lay `els` into `list`, keeping every node that can be kept. A new
    /// list (Settings opened again) or one emptied under it starts afresh.
    pub fn sync(&mut self, tree: &mut Tree, list: NodeId, els: &[El]) {
        let alive = self
            .kids
            .first()
            .is_none_or(|m| tree.contains(m.node) && tree.parent(m.node) == Some(list));
        if self.list != Some(list) || !alive {
            self.kids.clear();
            tree.clear_children(list);
            self.list = Some(list);
        }
        let prims: Vec<Prim> = els.iter().map(expand).collect();
        self.acts.clear();
        self.fields.clear();
        let old = std::mem::take(&mut self.kids);
        self.kids = place(tree, list, old, prims, &mut self.acts, &mut self.fields);
    }

    /// The list the page was last laid into.
    pub fn list(&self) -> Option<NodeId> {
        self.list
    }

    /// Forget the nodes (the list went with Settings).
    pub fn forget(&mut self) {
        *self = Mount::default();
    }

    /// What pressing `id` does.
    pub fn act(&self, id: NodeId) -> Option<&Act> {
        self.acts.get(&id)
    }

    /// The field node `id` is, if it is one of the page's.
    pub fn field_of(&self, id: NodeId) -> Option<FieldId> {
        self.fields.iter().find(|(_, n)| **n == id).map(|(f, _)| *f)
    }

    /// The node of field `f`, while it is shown.
    pub fn node_of(&self, f: FieldId) -> Option<NodeId> {
        self.fields.get(&f).copied()
    }

    /// The fields shown, in the page's order (top to bottom).
    pub fn fields(&self) -> Vec<(FieldId, NodeId)> {
        let mut out: Vec<_> = self.fields.iter().map(|(f, n)| (*f, *n)).collect();
        out.sort_by_key(|(f, _)| *f as u8);
        out
    }
}

/// What kind of content a node has: a node keeps its id only while this
/// stays the same.
fn shape(n: &Node) -> u8 {
    match n.content {
        Content::None => 0,
        Content::Text(_) => 1,
        Content::Field(_) => 2,
        Content::Image(_) => 3,
        Content::Slot => 4,
    }
}

/// Lay `new` under `parent` over `old`.
fn place(
    tree: &mut Tree,
    parent: NodeId,
    old: Vec<Mounted>,
    new: Vec<Prim>,
    acts: &mut HashMap<NodeId, Act>,
    fields: &mut HashMap<FieldId, NodeId>,
) -> Vec<Mounted> {
    let mut old: Vec<Option<Mounted>> = old.into_iter().map(Some).collect();
    // Which old node each new one keeps.
    let kept: Vec<Option<Mounted>> = new
        .iter()
        .map(|p| {
            let s = shape(&p.node);
            old.iter_mut()
                .find(|o| o.as_ref().is_some_and(|o| o.key == p.key && o.shape == s))
                .and_then(Option::take)
        })
        .collect();
    for gone in old.into_iter().flatten() {
        tree.remove(gone.node);
    }
    let mut out = Vec::with_capacity(new.len());
    for (i, (p, k)) in new.into_iter().zip(kept).enumerate() {
        let (id, kids) = match k {
            Some(m) => {
                patch(tree, m.node, &p.node);
                if tree.children(parent).get(i) != Some(&m.node) {
                    tree.reorder(m.node, i);
                }
                (m.node, m.kids)
            }
            None => (tree.insert(parent, i, p.node.clone()), Vec::new()),
        };
        if let Some(a) = p.act {
            acts.insert(id, a);
        }
        if let Some(f) = p.field {
            fields.insert(f, id);
        }
        let kids = place(tree, id, kids, p.kids, acts, fields);
        out.push(Mounted {
            key: p.key,
            node: id,
            shape: shape(&p.node),
            kids,
        });
    }
    out
}

/// Make node `id` look and read like `new`, keeping its live state (hover,
/// focus), its place and, for a field, what is typed in it.
fn patch(tree: &mut Tree, id: NodeId, new: &Node) {
    let Some(old) = tree.get(id) else {
        return;
    };
    let same_text = match (&old.content, &new.content) {
        (Content::Text(a), Content::Text(b)) => {
            a.text == b.text
                && a.size == b.size
                && a.weight == b.weight
                && a.trim == b.trim
                && a.align == b.align
                && a.icon == b.icon
        }
        (Content::Field(a), Content::Field(b)) => {
            a.placeholder == b.placeholder
                && a.size == b.size
                && a.weight == b.weight
                && a.placeholder_ink == b.placeholder_ink
        }
        _ => true,
    };
    if same_text
        && old.style == new.style
        && old.look == new.look
        && old.a11y == new.a11y
        && old.flags == new.flags
        && old.tooltip == new.tooltip
        && old.cursor == new.cursor
        && old.tag == new.tag
    {
        return;
    }
    tree.edit(id, |n| {
        n.style = new.style;
        n.look = new.look;
        n.a11y = new.a11y.clone();
        n.flags = new.flags;
        n.tooltip = new.tooltip.clone();
        n.cursor = new.cursor;
        n.tag = new.tag;
        if same_text {
            return;
        }
        match (&mut n.content, &new.content) {
            (Content::Text(a), Content::Text(b)) => {
                let revision = a.revision.wrapping_add(1);
                *a = b.clone();
                a.revision = revision;
            }
            (Content::Field(a), Content::Field(b)) => {
                a.placeholder = b.placeholder.clone();
                a.size = b.size;
                a.weight = b.weight;
                a.placeholder_ink = b.placeholder_ink;
                a.revision = a.revision.wrapping_add(1);
            }
            _ => {}
        }
    });
}

// MARK: elements as nodes

/// A tone's ink.
fn ink(t: Tone) -> Swatch {
    match t {
        Tone::Ink => Swatch::Token(Token::Ink),
        Tone::Muted => Swatch::Token(Token::Muted),
        // The system's red and orange, at the Mac's opacities.
        Tone::Red => Swatch::Fixed(Color::hex(0xE5_3935).alpha(0.85)),
        Tone::Orange => Swatch::Fixed(Color::hex(0xF5_8A00).alpha(0.95)),
    }
}

/// A status dot's colour.
fn dot(d: Option<Dot>) -> Swatch {
    match d {
        None => Swatch::Token(Token::Faint),
        Some(Dot::Green) => Swatch::Fixed(Color::hex(0x2E_B85C).alpha(0.9)),
        Some(Dot::Yellow) => Swatch::Fixed(Color::hex(0xE6_B800)),
        Some(Dot::Orange) => Swatch::Fixed(Color::hex(0xF5_8A00)),
        Some(Dot::Red) => Swatch::Fixed(Color::hex(0xE5_3935)),
    }
}

/// Words that are part of a button (the button says them).
fn label(words: &str, size: f32, ink_: Swatch) -> Node {
    Node::text(words, size).ink(ink_).role(Role::None)
}

/// `el`'s nodes.
pub fn expand(el: &El) -> Prim {
    let kids = || el.children.iter().map(expand).collect::<Vec<_>>();
    let key = el.key.clone();
    match &el.kind {
        Kind::Stack { gap } => Prim::new(key, Node::column().gap(*gap)).with(kids()),
        Kind::Row { gap, pad } => Prim::new(
            key,
            Node::row()
                .gap(*gap)
                .padding(Insets::hv(pad.0, pad.1))
                .align(Align::Center),
        )
        .with(kids()),
        Kind::Block { gap, pad } => Prim::new(
            key,
            Node::column().gap(*gap).padding(Insets::hv(pad.0, pad.1)),
        )
        .with(kids()),
        Kind::Caption(t) => Prim::new(
            key,
            Node::text(t, 11.5)
                .weight(Weight::Semibold)
                .ink(Token::Muted)
                .padding(Insets::new(4.0, 0.0, 0.0, 0.0)),
        ),
        Kind::Card => Prim::new(
            key,
            Node::column()
                .radius(11.0)
                .stroke(Token::Hairline, 1.0)
                .clip(),
        )
        .with(kids()),
        Kind::Rule => Prim::new(
            key,
            Node::row()
                .height(1.0)
                .padding(Insets::new(14.0, 0.0, 0.0, 0.0)),
        )
        .with(vec![Prim::new(
            "hairline",
            Node::new().height(1.0).grow(1.0).fill(Token::Hairline),
        )]),
        Kind::Line {
            title,
            detail,
            detail_tone,
        } => {
            let mut words = vec![Prim::new("title", Node::text(title, 13.0).ink(Token::Ink))];
            if let Some(d) = detail {
                words.push(Prim::new(
                    "detail",
                    Node::text(d, 11.5).ink(ink(*detail_tone)).trim(Trim::Wrap),
                ));
            }
            Prim::new(
                key,
                Node::row()
                    .padding(Insets::hv(14.0, 10.0))
                    .gap(16.0)
                    .align(Align::Center),
            )
            .with(vec![
                Prim::new("words", Node::column().gap(3.0).grow(1.0).shrink(1.0)).with(words),
                Prim::new(
                    "controls",
                    Node::row().gap(8.0).align(Align::Center).shrink(0.0),
                )
                .with(kids()),
            ])
        }
        Kind::Text {
            text,
            size,
            tone,
            weight,
            wrap,
            // Monospaced type needs a font family the toolkit doesn't
            // offer yet: the words show in the UI face.
            mono: _,
        } => Prim::new(
            key,
            Node::text(text, *size)
                .ink(ink(*tone))
                .weight(*weight)
                .trim(if *wrap { Trim::Wrap } else { Trim::End }),
        ),
        Kind::StepMark { n, mark } => {
            let (fill, stroke) = match mark {
                Mark::Done => (Swatch::Token(Token::Ink), Swatch::Token(Token::Hairline)),
                Mark::Here => (Swatch::Token(Token::Ground), Swatch::Token(Token::Ink)),
                Mark::Ahead => (Swatch::Token(Token::Wash), Swatch::Token(Token::Hairline)),
            };
            let inside = match mark {
                Mark::Done => Node::glyph(CHECK, 8.0, 12.0).ink(Token::Ground),
                Mark::Here => Node::text(&n.to_string(), 10.0)
                    .weight(Weight::Semibold)
                    .ink(Token::Ink),
                Mark::Ahead => Node::text(&n.to_string(), 10.0)
                    .weight(Weight::Semibold)
                    .ink(Token::Muted),
            };
            Prim::new(
                key,
                Node::row()
                    .size(18.0, 18.0)
                    .radius(9.0)
                    .fill(fill)
                    .stroke(stroke, 1.0)
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .shrink(0.0),
            )
            .with(vec![Prim::new("inside", inside.role(Role::None))])
        }
        Kind::StepBar { done } => Prim::new(
            key,
            Node::new().size(22.0, 1.0).shrink(0.0).fill(if *done {
                Token::Ink
            } else {
                Token::Hairline
            }),
        ),
        Kind::Pill {
            label: words,
            act,
            look,
            enabled,
        } => pill(key, words, act, *look, *enabled),
        Kind::Quiet {
            label: words,
            act,
            tone,
        } => Prim::new(
            key,
            Node::text(words, 11.5)
                .ink(ink(*tone))
                .hover_ink(if *tone == Tone::Red {
                    ink(Tone::Red)
                } else {
                    Swatch::Token(Token::Ink)
                })
                .shrink(0.0)
                .cursor(Cursor::Hand)
                .button(words),
        )
        .act(act.clone()),
        Kind::Icon { glyph, act, name } => Prim::new(
            key,
            Node::glyph(*glyph, 12.0, 30.0)
                .radius(7.0)
                .ink(Token::Muted)
                .hover_ink(Token::Ink)
                .fill(Token::Wash)
                .hover_fill(Token::Hover)
                .press_fill(Token::Hover)
                .shrink(0.0)
                .tooltip(name)
                .button(name),
        )
        .act(act.clone()),
        Kind::Switch { on, act, name } => Prim::new(
            key,
            Node::row()
                .size(34.0, 20.0)
                .padding(Insets::all(2.0))
                .radius(10.0)
                .align(Align::Center)
                .justify(if *on { Justify::End } else { Justify::Start })
                .fill(if *on {
                    Swatch::Token(Token::Accent)
                } else {
                    Swatch::Fade(Token::Ink, 0.16)
                })
                .shrink(0.0)
                .button(name)
                .selected(*on),
        )
        .act(act.clone())
        .with(vec![Prim::new(
            "knob",
            Node::new()
                .size(16.0, 16.0)
                .radius(8.0)
                .fill(Token::White)
                .role(Role::None),
        )]),
        Kind::Segmented {
            options,
            name,
            wide,
        } => {
            let mut group = Node::row()
                .padding(Insets::all(2.0))
                .gap(2.0)
                .radius(8.0)
                .fill(Token::Wash)
                .role(Role::Group)
                .name(name);
            if *wide {
                group = group.grow(1.0);
            }
            let buttons = options
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let mut b = Node::row()
                        .height(24.0)
                        .padding(Insets::hv(if *wide { 4.0 } else { 10.0 }, 0.0))
                        .radius(6.0)
                        .align(Align::Center)
                        .justify(Justify::Center)
                        .fill(if c.on {
                            Swatch::Token(Token::Ground)
                        } else {
                            Swatch::Clear
                        })
                        .stroke(
                            if c.on {
                                Swatch::Token(Token::Hairline)
                            } else {
                                Swatch::Clear
                            },
                            1.0,
                        )
                        .hover_fill(if c.on { Token::Ground } else { Token::Hover })
                        .button(&c.label)
                        .selected(c.on);
                    if *wide {
                        b = b.grow(1.0);
                    }
                    Prim::new(format!("option-{i}"), b)
                        .act(c.act.clone())
                        .with(vec![Prim::new(
                            "label",
                            label(
                                &c.label,
                                11.5,
                                Swatch::Token(if c.on { Token::Ink } else { Token::Muted }),
                            )
                            .weight(if c.on {
                                Weight::Medium
                            } else {
                                Weight::Regular
                            }),
                        )])
                })
                .collect();
            Prim::new(key, group).with(buttons)
        }
        Kind::Field {
            id,
            placeholder,
            width,
            mono: _,
        } => {
            let mut boxed = Node::row()
                .height(if *id == FieldId::Code { 30.0 } else { 26.0 })
                .padding(Insets::hv(8.0, 0.0))
                .radius(6.0)
                .fill(Token::Ground)
                .stroke(Token::Hairline, 1.0)
                .align(Align::Center);
            boxed = match width {
                Some(w) => boxed.width(*w).shrink(0.0),
                None => boxed.grow(1.0),
            };
            let mut field = secure(
                Node::field(placeholder, 12.0)
                    .fill_height()
                    .grow(1.0)
                    .name(id.name()),
                id.secure(),
            );
            field.a11y.help = placeholder.clone();
            let mut inner = Prim::new("field", field);
            inner.field = Some(*id);
            Prim::new(key, boxed).with(vec![inner])
        }
        Kind::Dot { dot: d, help } => Prim::new(
            key,
            Node::new()
                .size(7.0, 7.0)
                .radius(3.5)
                .fill(dot(*d))
                .shrink(0.0)
                .hit()
                .tooltip(help)
                .role(Role::Image)
                .name(help),
        ),
        Kind::Glyph { glyph, size, tone } => Prim::new(
            key,
            Node::glyph(*glyph, *size, size + 4.0)
                .ink(ink(*tone))
                .shrink(0.0)
                .role(Role::None),
        ),
        Kind::Bar { fraction, warn } => {
            let f = fraction.clamp(0.0, 1.0);
            Prim::new(
                key,
                Node::row()
                    .height(3.0)
                    .radius(1.5)
                    .fill(Swatch::Fade(Token::Ink, 0.08))
                    .clip()
                    .role(Role::None),
            )
            .with(vec![
                Prim::new(
                    "done",
                    Node::new().height(3.0).grow(f).radius(1.5).fill(if *warn {
                        ink(Tone::Orange)
                    } else {
                        Swatch::Fade(Token::Ink, 0.5)
                    }),
                ),
                Prim::new("rest", Node::new().height(3.0).grow(1.0 - f)),
            ])
        }
        Kind::Spacer => Prim::new(key, Node::new().grow(1.0).min_width(8.0)),
        Kind::Press {
            act,
            name,
            help,
            pad,
            lead,
            value,
        } => {
            let mut n = Node::row()
                .padding(Insets::new(pad.0 + lead, pad.1, pad.0, pad.1))
                .gap(8.0)
                .align(Align::Center)
                .hover_fill(Token::Hover)
                .press_fill(Token::Hover)
                .button(&match value {
                    Some(v) => format!("{name}, {v}"),
                    None => name.clone(),
                });
            if let Some(h) = help {
                n = n.tooltip(h);
            }
            Prim::new(key, n).act(act.clone()).with(
                el.children
                    .iter()
                    .map(|c| quiet_inside(expand(c)))
                    .collect(),
            )
        }
        Kind::Capsule { help } => Prim::new(
            key,
            Node::row()
                .padding(Insets::hv(7.0, 2.5))
                .gap(3.0)
                .radius(9.0)
                .fill(Token::Wash)
                .align(Align::Center)
                .shrink(0.0)
                .hit()
                .tooltip(help)
                .role(Role::Text)
                .name(help),
        )
        .with(
            el.children
                .iter()
                .map(|c| quiet_inside(expand(c)))
                .collect(),
        ),
        Kind::CodeBox => Prim::new(
            key,
            Node::column()
                .gap(2.0)
                .padding(Insets::hv(12.0, 10.0))
                .radius(9.0)
                .fill(Token::Wash)
                .stroke(Token::Hairline, 1.0),
        )
        .with(kids()),
        Kind::Problem(t) => Prim::new(
            key,
            Node::row()
                .gap(6.0)
                .padding(Insets::new(2.0, 0.0, 0.0, 0.0))
                .align(Align::Start),
        )
        .with(vec![
            Prim::new(
                "icon",
                Node::glyph(ERROR, 11.0, 15.0)
                    .ink(ink(Tone::Red))
                    .role(Role::None),
            ),
            Prim::new(
                "text",
                Node::text(t, 11.5).ink(ink(Tone::Red)).trim(Trim::Wrap),
            ),
        ]),
    }
}

/// A capsule button.
fn pill(key: String, words: &str, act: &Act, look: PillLook, enabled: bool) -> Prim {
    let (fill, hover, press, text) = match look {
        PillLook::Plain => (
            Swatch::Token(Token::Wash),
            Swatch::Token(Token::Hover),
            Swatch::Token(Token::Hover),
            Swatch::Token(Token::Ink),
        ),
        PillLook::Filled => (
            Swatch::Token(Token::Accent),
            Swatch::Fade(Token::Accent, 0.88),
            Swatch::Fade(Token::Accent, 0.8),
            Swatch::Token(Token::White),
        ),
        PillLook::Red => (
            Swatch::Token(Token::Wash),
            Swatch::Token(Token::Hover),
            Swatch::Token(Token::Hover),
            ink(Tone::Red),
        ),
    };
    let mut n = Node::row()
        .height(26.0)
        .padding(Insets::hv(12.0, 0.0))
        .radius(13.0)
        .align(Align::Center)
        .shrink(0.0)
        .button(words);
    let text = if enabled {
        n = n.fill(fill).hover_fill(hover).press_fill(press);
        text
    } else {
        // Held back: faded, and no target for the pointer or the keyboard.
        n = n.fill(Swatch::Fade(Token::Wash, 0.6));
        n.flags.disabled = true;
        n.flags.focusable = false;
        Swatch::Fade(Token::Ink, 0.4)
    };
    Prim::new(key, n)
        .act(act.clone())
        .with(vec![Prim::new("label", label(words, 12.0, text))])
}

/// Words and glyphs inside a button: the button says them, and the pointer
/// presses the button.
fn quiet_inside(mut p: Prim) -> Prim {
    if p.act.is_none() {
        p.node.a11y.role = Role::None;
        if p.node.tooltip.is_none() {
            p.node.flags.hit = false;
        }
    }
    p.kids = p.kids.into_iter().map(quiet_inside).collect();
    p
}

/// A field that hides what is typed, when `on`. The toolkit's secure field
/// (`Node::secure`) arrives with the chrome toolkit's next merge; until it
/// does a key or password field shows its text.
fn secure(n: Node, on: bool) -> Node {
    let _ = on;
    n
}

#[cfg(test)]
mod tests;
