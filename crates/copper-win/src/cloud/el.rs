//! What Settings › Cloud shows, as data: a tree of keyed elements in the
//! Mac page's own vocabulary — captions, cards, lines, pills, switches,
//! segmented choices, fields — built fresh from the cloud's state on every
//! refresh ([`super::view`]) and handed to the window ([`super::mount`]),
//! which keeps every node whose element didn't change. A field keeps its
//! node (and so its focus, caret and IME state) while the words around it
//! change as you type.
//!
//! Pure: built and compared on any machine.

use super::model::{Dot, Mark, Mode, Span};
use copper_cloud::Domain;
use copper_win_ui::Weight;

/// One element: a key unique among its siblings, what it is, and what is
/// inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct El {
    pub key: String,
    pub kind: Kind,
    pub children: Vec<El>,
}

impl El {
    pub fn new(key: impl Into<String>, kind: Kind) -> El {
        El {
            key: key.into(),
            kind,
            children: Vec::new(),
        }
    }

    /// With these inside.
    pub fn with(mut self, children: Vec<El>) -> El {
        self.children = children;
        self
    }

    /// Words in this weight (a text element; others are left as they are).
    pub fn weight(mut self, w: Weight) -> El {
        if let Kind::Text { weight, .. } = &mut self.kind {
            *weight = w;
        }
        self
    }

    /// Words on one line, cut with `…` (a text element).
    pub fn one_line(mut self) -> El {
        if let Kind::Text { wrap, .. } = &mut self.kind {
            *wrap = false;
        }
        self
    }

    /// Every element at or under this one, depth first.
    #[cfg(test)]
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a El)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }

    /// All the words this element and those under it show.
    #[cfg(test)]
    pub fn words(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&mut |e| match &e.kind {
            Kind::Caption(t) | Kind::Problem(t) => out.push(t.clone()),
            Kind::Text { text, .. } => out.push(text.clone()),
            Kind::Line { title, detail, .. } => {
                out.push(title.clone());
                if let Some(d) = detail {
                    out.push(d.clone());
                }
            }
            Kind::Pill { label, .. } | Kind::Quiet { label, .. } => out.push(label.clone()),
            Kind::Segmented { options, .. } => out.extend(options.iter().map(|o| o.label.clone())),
            _ => {}
        });
        out
    }
}

/// How words are coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Ink,
    Muted,
    /// The destructive and the wrong: semantic red at .85.
    Red,
    /// A pairing code's last minute: semantic orange at .95.
    Orange,
}

/// A pill's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PillLook {
    /// Outlined with the hairline.
    #[default]
    Plain,
    /// Ink fill, ground words: the step's primary action.
    Filled,
    /// Outlined, words in red: Disconnect, Delete history on cloud….
    Red,
}

/// What a click on an element does (the page carries it out).
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    // 1. connect
    Submit,
    ToggleManual,
    Paste,
    DismissOffer,
    SsoCancel,
    // the instance
    ToggleDetails,
    Disconnect,
    // 2. account
    Mode(Mode),
    AccountSubmit,
    SsoContinue,
    UsePassword,
    // 3. sync
    Domain(Domain),
    TurnOn,
    SyncNow,
    TurnOff,
    Device(String),
    OpenTab(String),
    SignOut,
    // pairing
    MakeCode,
    CopyCode,
    RevokeCode,
    // delete history on cloud
    Span(Span),
    DeleteHistory,
    /// Stop the delete under way (shown while looking, as on the Mac).
    StopDelete,
    // the log
    ToggleLog,
}

/// The page's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldId {
    Code,
    Host,
    Key,
    Fingerprint,
    Email,
    Password,
    Name,
    DeviceName,
    Site,
}

impl FieldId {
    /// What assistive technology calls it.
    pub fn name(self) -> &'static str {
        match self {
            FieldId::Code => "Link code or pairing code",
            FieldId::Host => "Address",
            FieldId::Key => "Key",
            FieldId::Fingerprint => "Certificate fingerprint",
            FieldId::Email => "Email",
            FieldId::Password => "Password",
            FieldId::Name => "Your name",
            FieldId::DeviceName => "Device name",
            FieldId::Site => "Only this site",
        }
    }

    /// Hides what is typed (a key, a password).
    pub fn secure(self) -> bool {
        matches!(self, FieldId::Key | FieldId::Password)
    }
}

/// One choice of a segmented control.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub label: String,
    pub act: Act,
    pub on: bool,
}

/// What an element is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A plain column (`VStack`), `gap` between its children.
    Stack { gap: f32 },
    /// A plain row (`HStack`), centred across; `pad` horizontal, vertical.
    Row { gap: f32, pad: (f32, f32) },
    /// A column with padding: a card's free-form block.
    Block { gap: f32, pad: (f32, f32) },
    /// The small heading over a card.
    Caption(String),
    /// A hairline card; its children are its rows.
    Card,
    /// The inset hairline between two rows of a card.
    Rule,
    /// One thing to set or do: words on the left, its children (the
    /// controls) on the right.
    Line {
        title: String,
        detail: Option<String>,
        detail_tone: Tone,
    },
    /// Words.
    Text {
        text: String,
        size: f32,
        tone: Tone,
        weight: Weight,
        wrap: bool,
        mono: bool,
    },
    /// One of the three steps' marks (18×18): its number, or a check once
    /// done.
    StepMark { n: u8, mark: Mark },
    /// The link between two marks (22×1).
    StepBar { done: bool },
    /// A capsule button.
    Pill {
        label: String,
        act: Act,
        look: PillLook,
        enabled: bool,
    },
    /// A quiet text button ("Enter it by hand", "Change…").
    Quiet { label: String, act: Act, tone: Tone },
    /// An icon-only button.
    Icon { glyph: char, act: Act, name: String },
    /// An on/off switch.
    Switch { on: bool, act: Act, name: String },
    /// A segmented choice; `wide` fills the row.
    Segmented {
        options: Vec<Choice>,
        name: String,
        wide: bool,
    },
    /// A text field; its text is the page's draft, never part of the
    /// element (typing doesn't change the tree).
    Field {
        id: FieldId,
        placeholder: String,
        width: Option<f32>,
        mono: bool,
    },
    /// A status dot (7×7), with what it means; `None` is the faint "not
    /// reached yet".
    Dot { dot: Option<Dot>, help: String },
    /// An icon-font glyph.
    Glyph { glyph: char, size: f32, tone: Tone },
    /// The pairing countdown's thin bar.
    Bar { fraction: f32, warn: bool },
    /// Pushes what follows to the far end of a row.
    Spacer,
    /// A row that is one button: a device's disclosure, a remote tab, the
    /// log's header. Its children are what it shows.
    Press {
        act: Act,
        name: String,
        help: Option<String>,
        pad: (f32, f32),
        /// Extra room before its children (a remote tab's indent).
        lead: f32,
        /// Its accessible value ("Shown", "Hidden"), if it has one.
        value: Option<String>,
    },
    /// The small trust capsule beside a host.
    Capsule { help: String },
    /// A wash box for a code (radius 9, hairline border).
    CodeBox,
    /// A red line under a card: what went wrong.
    Problem(String),
}

// MARK: builders, in the Mac's names

/// A caption over a card.
pub fn caption(key: &str, text: &str) -> El {
    El::new(key, Kind::Caption(text.to_string()))
}

/// A card of `rows`, with a rule between each two.
pub fn card(key: &str, rows: Vec<El>) -> El {
    let mut out = Vec::with_capacity(rows.len() * 2);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            out.push(El::new(format!("rule-{}", r.key), Kind::Rule));
        }
        out.push(r);
    }
    El::new(key, Kind::Card).with(out)
}

/// A captioned card: the caption, the card, then anything after it (a
/// problem line), 8 apart.
pub fn section(key: &str, title: Option<&str>, rows: Vec<El>, after: Vec<El>) -> El {
    let mut kids = Vec::new();
    if let Some(t) = title {
        kids.push(caption("caption", t));
    }
    kids.push(card("card", rows));
    kids.extend(after);
    El::new(key, Kind::Stack { gap: 8.0 }).with(kids)
}

/// A line with its controls.
pub fn line(key: &str, title: &str, detail: Option<&str>, controls: Vec<El>) -> El {
    El::new(
        key,
        Kind::Line {
            title: title.to_string(),
            detail: detail.map(str::to_string),
            detail_tone: Tone::Muted,
        },
    )
    .with(controls)
}

/// Words, one size and tone, wrapping.
pub fn text(key: &str, words: &str, size: f32, tone: Tone) -> El {
    El::new(
        key,
        Kind::Text {
            text: words.to_string(),
            size,
            tone,
            weight: Weight::Regular,
            wrap: true,
            mono: false,
        },
    )
}

/// Monospaced words (hosts, fingerprints, codes).
pub fn mono(key: &str, words: &str, size: f32, tone: Tone) -> El {
    El::new(
        key,
        Kind::Text {
            text: words.to_string(),
            size,
            tone,
            weight: Weight::Regular,
            wrap: true,
            mono: true,
        },
    )
}

/// A pill.
pub fn pill(key: &str, label: &str, act: Act, look: PillLook, enabled: bool) -> El {
    El::new(
        key,
        Kind::Pill {
            label: label.to_string(),
            act,
            look,
            enabled,
        },
    )
}

/// A quiet text button.
pub fn quiet(key: &str, label: &str, act: Act) -> El {
    El::new(
        key,
        Kind::Quiet {
            label: label.to_string(),
            act,
            tone: Tone::Muted,
        },
    )
}

/// A row of `children` padded like a card's line (14×10).
pub fn row(key: &str, children: Vec<El>) -> El {
    El::new(
        key,
        Kind::Row {
            gap: 8.0,
            pad: (14.0, 10.0),
        },
    )
    .with(children)
}

/// A free-form block in a card (14 × `v`).
pub fn block(key: &str, v: f32, gap: f32, children: Vec<El>) -> El {
    El::new(
        key,
        Kind::Block {
            gap,
            pad: (14.0, v),
        },
    )
    .with(children)
}

/// The spacer.
pub fn spacer() -> El {
    El::new("spacer", Kind::Spacer)
}

/// A field.
pub fn field(id: FieldId, placeholder: &str, width: Option<f32>, mono: bool) -> El {
    El::new(
        format!("{id:?}"),
        Kind::Field {
            id,
            placeholder: placeholder.to_string(),
            width,
            mono,
        },
    )
}

/// A problem line.
pub fn problem(key: &str, words: &str) -> El {
    El::new(key, Kind::Problem(words.to_string()))
}
