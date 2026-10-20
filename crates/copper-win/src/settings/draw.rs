//! A page's [`Form`] as nodes (`SettingsSection`, `Card`, `Line`): titled
//! cards of lines, each with its control, and what every control, field
//! and anchor turned into — so a click, an edit or a search landing can be
//! traced back to the page and action it stands for.

use super::controls::{self, LineNodes};
use super::form::{Control, Form, Line, Tone};
use super::look::{Inks, metric};
use copper_win_ui::win::Ui;
use copper_win_ui::{Node, NodeId, Swatch};

/// What a click in Settings means.
#[derive(Clone, Debug)]
pub enum Hit {
    /// A page in the rail.
    Page(&'static str),
    /// A control on a page: the control, and which option or pill.
    Act(&'static str, Control, Option<usize>),
    /// A popup chooser: open its menu.
    Pick(&'static str, Control),
    /// The header's cross.
    Close,
    /// The search field's clear button.
    Clear,
    /// A search result, by its place in the results.
    Result(usize),
    /// A suggestion in the no-hits state.
    Suggest(&'static str),
}

/// A text box on a page: whose it is and what its edits and Enter do.
#[derive(Clone, Debug)]
pub struct FieldHit {
    pub page: &'static str,
    pub action: String,
    pub submit: String,
}

/// Where search can land: an anchor's node, and whether it is a whole card
/// (landed by its top edge) or a row (a fifth of the way down).
#[derive(Clone, Debug)]
pub struct Anchor {
    pub name: String,
    pub node: NodeId,
    pub card: bool,
}

/// Everything one drawing made that later events refer to.
#[derive(Default)]
pub struct Drawn {
    pub hits: Vec<(NodeId, Hit)>,
    pub fields: Vec<(NodeId, FieldHit)>,
    pub anchors: Vec<Anchor>,
}

impl Drawn {
    pub fn hit(&mut self, id: NodeId, hit: Hit) {
        self.hits.push((id, hit));
    }
}

/// The detail's ink for a line's tone.
fn tone_ink(tone: Tone, ink: &Inks) -> Swatch {
    Swatch::Fixed(match tone {
        Tone::Plain => ink.detail,
        Tone::Error => ink.error,
        Tone::Good => ink.good,
    })
}

/// Draw `form` (page `page`) into the scroll column `list`.
pub fn form(
    ui: &mut Ui,
    list: NodeId,
    page: &'static str,
    form: &Form,
    ink: &Inks,
    drawn: &mut Drawn,
) {
    let mut group: Option<NodeId> = None;
    for s in &form.sections {
        let g = match group {
            Some(g) if s.continues => {
                // Cards of one section stack 12 apart: the section's 8 gap
                // and 4 more.
                ui.tree
                    .add(g, Node::new().height(metric::STACK - metric::SECTION_GAP));
                g
            }
            _ => {
                let g = ui.tree.add(list, Node::column().gap(metric::SECTION_GAP));
                if !s.title.is_empty() {
                    controls::caption(ui, g, &s.title, ink);
                }
                g
            }
        };
        group = Some(g);
        let holder = if s.carded {
            controls::card(ui, g, ink)
        } else {
            ui.tree.add(g, Node::column().gap(12.0))
        };
        if let Some(a) = &s.anchor {
            drawn.anchors.push(Anchor {
                name: a.clone(),
                node: holder,
                card: true,
            });
        }
        for (i, l) in s.lines.iter().enumerate() {
            if i > 0 && s.carded {
                controls::rule(ui, holder, ink);
            }
            line(ui, holder, page, l, ink, drawn);
        }
        if let Some(n) = &s.note {
            controls::note(ui, g, n, ink);
        }
    }
}

/// One line and its control.
fn line(ui: &mut Ui, holder: NodeId, page: &'static str, l: &Line, ink: &Inks, drawn: &mut Drawn) {
    let LineNodes {
        row,
        words,
        control,
    } = controls::line(
        ui,
        holder,
        &l.title,
        l.detail.as_deref(),
        tone_ink(l.tone, ink),
    );
    if l.disabled {
        ui.tree
            .edit(words, |n| n.transform.opacity = metric::DISABLED);
    }
    // A line of only a control (a button under a box, a search box) gives
    // the control the whole line, still on its right.
    if l.title.is_empty() && l.detail.as_deref().is_none_or(str::is_empty) {
        ui.tree.set_hidden(words, true);
        ui.tree.edit(control, |n| n.style.grow = 1.0);
    }
    if !l.title.is_empty() {
        drawn.anchors.push(Anchor {
            name: l.anchor_or_title().to_string(),
            node: row,
            card: false,
        });
    }
    let on = !l.disabled;
    let c = &l.control;
    match c {
        Control::None => {
            ui.tree.set_hidden(control, true);
        }
        Control::Value(v) => {
            controls::value(ui, control, v, ink);
        }
        Control::Check => {
            controls::check(ui, control);
        }
        Control::Switch { on: state, .. } => {
            let id = controls::switch(ui, control, &l.title, *state, on);
            drawn.hit(id, Hit::Act(page, c.clone(), None));
        }
        Control::Choice {
            options, chosen, ..
        } => {
            let ids = controls::segmented(ui, control, &l.title, options, chosen, on);
            for (i, id) in ids.into_iter().enumerate() {
                drawn.hit(id, Hit::Act(page, c.clone(), Some(i)));
            }
        }
        Control::Button { label, filled, .. } => {
            let id = controls::pill(ui, control, label, *filled, on);
            drawn.hit(id, Hit::Act(page, c.clone(), None));
        }
        Control::Buttons(pills) => {
            for (i, p) in pills.iter().enumerate() {
                let id = controls::pill(ui, control, &p.label, p.filled, on && p.enabled);
                drawn.hit(id, Hit::Act(page, c.clone(), Some(i)));
            }
        }
        Control::Pick {
            options, chosen, ..
        } => {
            let label = options
                .iter()
                .find(|(v, _)| v == chosen)
                .or(options.first())
                .map(|(_, l)| l.as_str())
                .unwrap_or("");
            let id = controls::popup(ui, control, &l.title, label, on);
            drawn.hit(id, Hit::Pick(page, c.clone()));
        }
        Control::Field {
            action,
            submit,
            text,
            placeholder,
            secret,
            width,
            ..
        } => {
            let name = if l.title.is_empty() {
                placeholder
            } else {
                &l.title
            };
            if width.is_none() {
                // A field with the line to itself grows into it.
                ui.tree.edit(control, |n| {
                    n.style.grow = 1.0;
                    n.style.shrink = 1.0;
                });
            }
            let id = controls::field(ui, control, name, text, placeholder, *secret, *width, on);
            drawn.fields.push((
                id,
                FieldHit {
                    page,
                    action: action.clone(),
                    submit: submit.clone(),
                },
            ));
        }
    }
}
