//! One node of the retained tree: its layout, its look, its content, and
//! what it is to the pointer, the keyboard and UI Automation.
//!
//! Nodes are built with chained setters (`Node::row().gap(4.0).fill(..)`),
//! added to a [`crate::tree::Tree`], and changed through the tree so it can
//! mark exactly the layers that need redrawing.

use crate::edit::TextEdit;
use crate::geom::{Insets, Point, Rect, Size};
use crate::layout::{Abs, Align, Dim, Dir, Justify, Style};
use crate::scroll::Scroller;
use crate::theme::{Swatch, Token};
use crate::tokens::Shadow;

/// Font weight, as DirectWrite numbers it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Weight {
    #[default]
    Regular,
    Medium,
    Semibold,
    Bold,
}

impl Weight {
    pub fn value(self) -> u32 {
        match self {
            Weight::Regular => 400,
            Weight::Medium => 500,
            Weight::Semibold => 600,
            Weight::Bold => 700,
        }
    }
}

/// Where a line of text that does not fit is cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Trim {
    /// `…` at the end (SwiftUI `.truncationMode(.tail)`).
    #[default]
    End,
    /// `…` in the middle, for hosts (`.truncationMode(.middle)`).
    Middle,
    /// Wrap onto more lines.
    Wrap,
}

/// Horizontal alignment of text inside its frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    #[default]
    Leading,
    Center,
    Trailing,
}

/// A run of text, or one glyph from the icon font.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub size: f32,
    pub weight: Weight,
    pub trim: Trim,
    pub align: TextAlign,
    /// Drawn from the icon font (Segoe Fluent Icons) rather than the UI face.
    pub icon: bool,
    /// Bumped on every change so the backend knows to rebuild its layout.
    pub revision: u32,
}

/// The text field's state: the editing model plus what only drawing needs.
#[derive(Clone, Debug)]
pub struct Field {
    pub edit: TextEdit,
    pub placeholder: String,
    pub size: f32,
    pub weight: Weight,
    /// Placeholder ink.
    pub placeholder_ink: Swatch,
    /// How far the text is scrolled left to keep the caret in view.
    pub scroll_x: f32,
    /// The caret's blink phase (drawn when true and focused).
    pub caret_on: bool,
    /// Bumped on every text or composition change.
    pub revision: u32,
}

/// What a node shows besides its background.
#[derive(Clone, Debug, Default)]
pub enum Content {
    #[default]
    None,
    Text(Text),
    Field(Box<Field>),
    /// A bitmap registered with the backend (a favicon).
    Image(u32),
    /// A page's visual (see the backend's `WebSlot`).
    Slot,
}

/// What the pointer shows over a node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Arrow,
    Hand,
    IBeam,
    ResizeEw,
    ResizeNs,
}

/// A node's role for UI Automation (mapped to UIA control types).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    /// Not in the accessibility tree; its accessible children are lifted to
    /// its parent.
    #[default]
    None,
    Group,
    Button,
    Text,
    Edit,
    List,
    ListItem,
    Tab,
    TabItem,
    Menu,
    MenuItem,
    ToolTip,
    Pane,
    ScrollBar,
    TitleBar,
    Document,
    Image,
}

/// What assistive technology hears.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct A11y {
    pub role: Role,
    pub name: String,
    pub help: String,
}

/// How the background looks, at rest and under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: Swatch,
    pub hover: Option<Swatch>,
    pub press: Option<Swatch>,
    /// When the node is `selected` (the live row, a picked card row).
    pub selected: Option<Swatch>,
    pub radius: f32,
    /// An inner stroke (`strokeBorder`) and its width.
    pub stroke: Swatch,
    pub stroke_width: f32,
    /// A shadow cast by the background shape.
    pub shadow: Option<Shadow>,
    /// A second shadow (the card's two-shadow lift).
    pub shadow2: Option<Shadow>,
    /// The shadows' colour, black by default (`tint.lift` for the pill).
    pub shadow_ink: Swatch,
    /// Text and glyph ink.
    pub ink: Swatch,
    pub hover_ink: Option<Swatch>,
    pub selected_ink: Option<Swatch>,
    /// A one-DIP line on one edge (the column's hairline edge).
    pub edge: Option<(Edge, Swatch)>,
}

/// Which side an edge line is drawn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            fill: Swatch::Clear,
            hover: None,
            press: None,
            selected: None,
            radius: 0.0,
            stroke: Swatch::Clear,
            stroke_width: 0.0,
            shadow: None,
            shadow2: None,
            shadow_ink: Swatch::Token(Token::Black),
            ink: Swatch::Token(Token::Ink),
            hover_ink: None,
            selected_ink: None,
            edge: None,
        }
    }
}

impl Look {
    /// Whether anything is drawn for the background at all.
    pub fn paints(&self) -> bool {
        !self.fill.is_clear()
            || self.hover.is_some()
            || self.press.is_some()
            || self.selected.is_some()
            || self.stroke_width > 0.0
            || self.shadow.is_some()
            || self.edge.is_some()
    }
}

/// Per-node switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// Gets its own composition visual and surface; changes inside it only
    /// redraw it, and it can move, fade and scale without a redraw.
    pub layer: bool,
    /// A pointer target (hover, press, click).
    pub hit: bool,
    pub focusable: bool,
    /// Children are clipped to the frame.
    pub clip: bool,
    pub disabled: bool,
    pub selected: bool,
    /// Empty space here drags the window (the title bar's role).
    pub drag: bool,
    /// Holds focus inside it while it is the modal (the open card).
    pub modal: bool,
}

/// Live interaction state (set by the tree, read by paint).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    /// The focus came from the keyboard: draw the ring.
    pub focus_visible: bool,
    /// Hover and press, eased (0–1): what paint mixes by.
    pub hover_t: f32,
    pub press_t: f32,
}

/// Render-only transform of a layer, animated without relayout or redraw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translate: Point,
    pub opacity: f32,
    pub scale: f32,
    /// The point scaling is about, as a fraction of the frame.
    pub anchor: Point,
}

impl Default for Transform {
    fn default() -> Transform {
        Transform {
            translate: Point::ZERO,
            opacity: 1.0,
            scale: 1.0,
            anchor: Point::new(0.5, 0.0),
        }
    }
}

/// A node.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub style: Style,
    pub look: Look,
    pub content: Content,
    pub flags: Flags,
    pub state: State,
    pub transform: Transform,
    pub a11y: A11y,
    pub tooltip: Option<String>,
    pub cursor: Cursor,
    /// Present on a scroll container; its single child is the content.
    pub scroll: Option<Box<Scroller>>,
    /// Local to the parent; written by layout.
    pub frame: Rect,
    /// Free for the app: a tab id, a command index.
    pub tag: u64,
}

impl Node {
    pub fn new() -> Node {
        Node::default()
    }

    pub fn row() -> Node {
        Node::new().dir(Dir::Row)
    }

    pub fn column() -> Node {
        Node::new().dir(Dir::Column)
    }

    pub fn stack() -> Node {
        Node::new().dir(Dir::Stack)
    }

    /// A line of text at `size`, cut with an ellipsis at the end.
    pub fn text(text: &str, size: f32) -> Node {
        let mut n = Node::new();
        n.content = Content::Text(Text {
            text: text.to_owned(),
            size,
            weight: Weight::Regular,
            trim: Trim::End,
            align: TextAlign::Leading,
            icon: false,
            revision: 0,
        });
        n.a11y.role = Role::Text;
        n.a11y.name = text.to_owned();
        n.style.shrink = 1.0;
        n
    }

    /// One icon-font glyph centred in a `box_size` square.
    pub fn glyph(ch: char, size: f32, box_size: f32) -> Node {
        let mut n = Node::new().size(box_size, box_size);
        n.content = Content::Text(Text {
            text: ch.to_string(),
            size,
            weight: Weight::Regular,
            trim: Trim::End,
            align: TextAlign::Center,
            icon: true,
            revision: 0,
        });
        n
    }

    /// The text field (one line, IME, undo).
    pub fn field(placeholder: &str, size: f32) -> Node {
        let mut n = Node::new();
        n.content = Content::Field(Box::new(Field {
            edit: TextEdit::new(),
            placeholder: placeholder.to_owned(),
            size,
            weight: Weight::Regular,
            placeholder_ink: Swatch::Token(Token::Faint),
            scroll_x: 0.0,
            caret_on: true,
            revision: 0,
        }));
        n.flags.hit = true;
        n.flags.focusable = true;
        n.flags.layer = true;
        n.flags.clip = true;
        n.cursor = Cursor::IBeam;
        n.a11y.role = Role::Edit;
        n.a11y.name = placeholder.to_owned();
        n.style.grow = 1.0;
        n.style.shrink = 1.0;
        n
    }

    /// A scroll container on the vertical axis, rubber-banding at its ends
    /// like the Mac's. Add one content child.
    pub fn scroll() -> Node {
        let mut n = Node::column();
        n.scroll = Some(Box::new(Scroller::new(true)));
        n.flags.clip = true;
        n.flags.layer = true;
        n.flags.hit = true;
        n
    }

    /// The page's slot (see the backend's `WebSlot`).
    pub fn slot() -> Node {
        let mut n = Node::new();
        n.content = Content::Slot;
        n.flags.hit = true;
        n.flags.layer = true;
        n.a11y.role = Role::Document;
        n
    }

    pub fn image(id: u32, size: f32) -> Node {
        let mut n = Node::new().size(size, size);
        n.content = Content::Image(id);
        n
    }

    // — layout —

    pub fn dir(mut self, d: Dir) -> Node {
        self.style.dir = d;
        self
    }
    pub fn width(mut self, w: f32) -> Node {
        self.style.width = Dim::Px(w);
        self
    }
    pub fn height(mut self, h: f32) -> Node {
        self.style.height = Dim::Px(h);
        self
    }
    pub fn size(self, w: f32, h: f32) -> Node {
        self.width(w).height(h)
    }
    pub fn fill_width(mut self) -> Node {
        self.style.width = Dim::Fill;
        self
    }
    pub fn fill_height(mut self) -> Node {
        self.style.height = Dim::Fill;
        self
    }
    pub fn min_width(mut self, v: f32) -> Node {
        self.style.min_w = v;
        self
    }
    pub fn max_width(mut self, v: f32) -> Node {
        self.style.max_w = v;
        self
    }
    pub fn min_height(mut self, v: f32) -> Node {
        self.style.min_h = v;
        self
    }
    pub fn padding(mut self, p: Insets) -> Node {
        self.style.padding = p;
        self
    }
    pub fn gap(mut self, g: f32) -> Node {
        self.style.gap = g;
        self
    }
    pub fn align(mut self, a: Align) -> Node {
        self.style.align = a;
        self
    }
    pub fn align_self(mut self, a: Align) -> Node {
        self.style.align_self = Some(a);
        self
    }
    pub fn justify(mut self, j: Justify) -> Node {
        self.style.justify = j;
        self
    }
    pub fn grow(mut self, g: f32) -> Node {
        self.style.grow = g;
        self
    }
    pub fn shrink(mut self, s: f32) -> Node {
        self.style.shrink = s;
        self
    }
    pub fn absolute(mut self, a: Abs) -> Node {
        self.style.absolute = Some(a);
        self
    }
    pub fn hidden(mut self, h: bool) -> Node {
        self.style.hidden = h;
        self
    }

    // — look —

    pub fn fill(mut self, s: impl Into<Swatch>) -> Node {
        self.look.fill = s.into();
        self
    }
    pub fn hover_fill(mut self, s: impl Into<Swatch>) -> Node {
        self.look.hover = Some(s.into());
        self.flags.hit = true;
        self
    }
    pub fn press_fill(mut self, s: impl Into<Swatch>) -> Node {
        self.look.press = Some(s.into());
        self.flags.hit = true;
        self
    }
    pub fn selected_fill(mut self, s: impl Into<Swatch>) -> Node {
        self.look.selected = Some(s.into());
        self
    }
    pub fn radius(mut self, r: f32) -> Node {
        self.look.radius = r;
        self
    }
    pub fn stroke(mut self, s: impl Into<Swatch>, width: f32) -> Node {
        self.look.stroke = s.into();
        self.look.stroke_width = width;
        self
    }
    pub fn shadow(mut self, s: Shadow) -> Node {
        if self.look.shadow.is_none() {
            self.look.shadow = Some(s);
        } else {
            self.look.shadow2 = Some(s);
        }
        self
    }
    pub fn shadow_ink(mut self, s: impl Into<Swatch>) -> Node {
        self.look.shadow_ink = s.into();
        self
    }
    pub fn ink(mut self, s: impl Into<Swatch>) -> Node {
        self.look.ink = s.into();
        self
    }
    pub fn hover_ink(mut self, s: impl Into<Swatch>) -> Node {
        self.look.hover_ink = Some(s.into());
        self
    }
    pub fn selected_ink(mut self, s: impl Into<Swatch>) -> Node {
        self.look.selected_ink = Some(s.into());
        self
    }
    pub fn edge(mut self, e: Edge, s: impl Into<Swatch>) -> Node {
        self.look.edge = Some((e, s.into()));
        self
    }
    pub fn weight(mut self, w: Weight) -> Node {
        match &mut self.content {
            Content::Text(t) => t.weight = w,
            Content::Field(f) => f.weight = w,
            _ => {}
        }
        self
    }
    pub fn trim(mut self, tr: Trim) -> Node {
        if let Content::Text(t) = &mut self.content {
            t.trim = tr;
        }
        self
    }
    pub fn text_align(mut self, a: TextAlign) -> Node {
        if let Content::Text(t) = &mut self.content {
            t.align = a;
        }
        self
    }

    // — behaviour —

    pub fn layer(mut self) -> Node {
        self.flags.layer = true;
        self
    }
    pub fn hit(mut self) -> Node {
        self.flags.hit = true;
        self
    }
    pub fn focusable(mut self) -> Node {
        self.flags.focusable = true;
        self.flags.hit = true;
        self
    }
    pub fn clip(mut self) -> Node {
        self.flags.clip = true;
        self
    }
    pub fn drag(mut self) -> Node {
        self.flags.drag = true;
        self
    }
    pub fn modal(mut self) -> Node {
        self.flags.modal = true;
        self
    }
    pub fn selected(mut self, on: bool) -> Node {
        self.flags.selected = on;
        self
    }
    pub fn cursor(mut self, c: Cursor) -> Node {
        self.cursor = c;
        self
    }
    pub fn tooltip(mut self, t: &str) -> Node {
        self.tooltip = Some(t.to_owned());
        if self.a11y.help.is_empty() {
            self.a11y.help = t.to_owned();
        }
        self
    }
    pub fn role(mut self, r: Role) -> Node {
        self.a11y.role = r;
        self
    }
    pub fn name(mut self, n: &str) -> Node {
        self.a11y.name = n.to_owned();
        self
    }
    pub fn tag(mut self, t: u64) -> Node {
        self.tag = t;
        self
    }
    pub fn opacity(mut self, o: f32) -> Node {
        self.transform.opacity = o;
        self
    }

    /// A button: a pointer target that takes focus and is announced as one.
    pub fn button(mut self, name: &str) -> Node {
        self.flags.hit = true;
        self.flags.focusable = true;
        self.a11y.role = Role::Button;
        self.a11y.name = name.to_owned();
        self
    }

    // — reading —

    /// The text of a text node.
    pub fn text_str(&self) -> Option<&str> {
        match &self.content {
            Content::Text(t) => Some(&t.text),
            _ => None,
        }
    }

    /// The text of a text node or a field.
    pub fn content_text(&self) -> Option<&str> {
        match &self.content {
            Content::Text(t) => Some(&t.text),
            Content::Field(f) => Some(f.edit.text()),
            _ => None,
        }
    }

    pub fn field_ref(&self) -> Option<&Field> {
        match &self.content {
            Content::Field(f) => Some(f),
            _ => None,
        }
    }

    pub fn field_mut(&mut self) -> Option<&mut Field> {
        match &mut self.content {
            Content::Field(f) => Some(f),
            _ => None,
        }
    }

    pub fn is_slot(&self) -> bool {
        matches!(self.content, Content::Slot)
    }

    /// The size of the frame.
    pub fn size_now(&self) -> Size {
        self.frame.size()
    }

    /// The colour of the background for the current state, as swatches and
    /// mixing weights: `(rest, hover, hover_t, press, press_t)`.
    pub fn fill_now(&self) -> (Swatch, Option<Swatch>, f32, Option<Swatch>, f32) {
        let rest = if self.flags.selected {
            self.look.selected.unwrap_or(self.look.fill)
        } else {
            self.look.fill
        };
        // A selected node does not also light up under the pointer: the live
        // row is found by its pill alone (`Side.swift` 700–720).
        let hover = if self.flags.selected && self.look.selected.is_some() {
            None
        } else {
            self.look.hover
        };
        (
            rest,
            hover,
            self.state.hover_t,
            self.look.press,
            self.state.press_t,
        )
    }

    /// The ink for the current state.
    pub fn ink_now(&self) -> (Swatch, Option<Swatch>, f32) {
        let ink = if self.flags.selected {
            self.look.selected_ink.unwrap_or(self.look.ink)
        } else {
            self.look.ink
        };
        (ink, self.look.hover_ink, self.state.hover_t)
    }
}
