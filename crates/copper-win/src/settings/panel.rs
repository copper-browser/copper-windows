//! The Settings panel: a rail of pages, the chosen page's title and line,
//! and its sections — drawn from each [`Page`]'s [`Form`], redrawn
//! whenever a page says something changed.

use super::form::{Control, Form, act_of};
use crate::chrome::glyph;
use crate::commands::Command;
use crate::panels::plate::{self, Plate};
use crate::panels::{self, Panel};
use crate::platform::Shell;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim, Weight};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// One page of Settings.
pub trait Page: 'static {
    /// Its id: `settings::open(shell, id)` shows it.
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    /// What the page is for, in a line under its title.
    fn blurb(&self) -> &'static str;
    fn glyph(&self) -> char;
    /// Describe the page from what the shell knows now.
    fn draw(&self, shell: &Shell) -> Form;
    /// A control was used: `action` is the control's, `value` the switch's
    /// new state (`"on"`/`"off"`) or the option chosen.
    fn act(&self, shell: &Shell, action: &str, value: &str);
}

const RAIL: f32 = 210.0;
const WIDTH: f32 = 900.0;
const PAGE_HEIGHT: f32 = 520.0;

/// What a click in Settings means.
#[derive(Clone)]
enum Hit {
    Page(&'static str),
    Act(&'static str, Control, Option<usize>),
}

#[derive(Clone, Copy)]
struct Nodes {
    rail: NodeId,
    title: NodeId,
    blurb: NodeId,
    list: NodeId,
}

/// The panel: its pages, the one shown, and what its controls do.
pub struct Settings {
    pages: RefCell<Vec<Rc<dyn Page>>>,
    page: RefCell<&'static str>,
    nodes: RefCell<Option<Nodes>>,
    hits: RefCell<HashMap<NodeId, Hit>>,
}

thread_local! {
    static SETTINGS: RefCell<Option<Rc<Settings>>> = const { RefCell::new(None) };
}

fn get() -> Option<Rc<Settings>> {
    SETTINGS.with(|s| s.borrow().clone())
}

/// Register Settings, its pages and its command.
pub fn register(shell: &Shell) {
    let s = Rc::new(Settings {
        pages: RefCell::new(Vec::new()),
        page: RefCell::new("general"),
        nodes: RefCell::new(None),
        hits: RefCell::default(),
    });
    SETTINGS.with(|g| *g.borrow_mut() = Some(s.clone()));
    for p in super::pages::all() {
        add(p);
    }
    shell.view().panels().add(shell, s);
    shell
        .view()
        .commands()
        .add(Command::new("settings", "Settings", glyph::SETTINGS, |s| {
            open(s, "general")
        }));
}

/// Add (or replace) a page.
pub fn add(page: Rc<dyn Page>) {
    let Some(s) = get() else { return };
    let mut pages = s.pages.borrow_mut();
    match pages.iter().position(|p| p.id() == page.id()) {
        Some(i) => pages[i] = page,
        None => pages.push(page),
    }
}

/// Show Settings at `page` (General when there is no such page).
pub fn open(shell: &Shell, page: &str) {
    let Some(s) = get() else { return };
    let id = s
        .pages
        .borrow()
        .iter()
        .find(|p| p.id() == page)
        .map(|p| p.id())
        .unwrap_or("general");
    *s.page.borrow_mut() = id;
    panels::show(shell, "settings");
}

/// What page `id` shows changed: redraw it if it is on screen.
pub fn changed(shell: &Shell, id: &str) {
    if let Some(s) = get()
        && *s.page.borrow() == id
        && shell.view().panels().showing() == Some("settings")
    {
        s.refresh(shell);
    }
}

impl Panel for Settings {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn title(&self) -> String {
        "Settings".into()
    }

    fn glyph(&self) -> char {
        glyph::SETTINGS
    }

    fn width(&self) -> f32 {
        WIDTH
    }

    fn build(&self, ui: &mut Ui, p: &Plate) {
        ui.tree.edit(p.body, |n| {
            n.style.padding = Insets::new(0.0, 0.0, 0.0, 0.0)
        });
        let row = ui
            .tree
            .add(p.body, Node::row().height(PAGE_HEIGHT).shrink(1.0));
        let rail = ui.tree.add(
            row,
            Node::scroll()
                .width(RAIL)
                .padding(Insets::new(12.0, 4.0, 12.0, 14.0))
                .gap(1.0)
                .fill(Token::Wash)
                .role(Role::List)
                .name("Pages"),
        );
        ui.tree
            .add(row, Node::new().width(1.0).fill(Token::Hairline));
        let page = ui.tree.add(row, Node::column().grow(1.0).shrink(1.0));
        let head = ui.tree.add(
            page,
            Node::column()
                .padding(Insets::new(32.0, 18.0, 32.0, 14.0))
                .gap(4.0),
        );
        let title = ui.tree.add(
            head,
            Node::text("", 20.0)
                .weight(Weight::Semibold)
                .ink(Token::Ink),
        );
        let blurb = ui.tree.add(
            head,
            Node::text("", 12.5).ink(Token::Muted).trim(Trim::Wrap),
        );
        ui.tree
            .add(page, Node::new().height(1.0).fill(Token::Hairline));
        let list = ui.tree.add(
            page,
            Node::scroll()
                .grow(1.0)
                .padding(Insets::new(32.0, 20.0, 32.0, 24.0))
                .gap(26.0),
        );
        *self.nodes.borrow_mut() = Some(Nodes {
            rail,
            title,
            blurb,
            list,
        });
    }

    fn refresh(&self, shell: &Shell) {
        let Some(nodes) = *self.nodes.borrow() else {
            return;
        };
        let pages = self.pages.borrow().clone();
        let current = *self.page.borrow();
        let Some(page) = pages.iter().find(|p| p.id() == current).cloned() else {
            return;
        };
        let form = page.draw(shell);
        let rail: Vec<(&'static str, &'static str, char)> = pages
            .iter()
            .map(|p| (p.id(), p.title(), p.glyph()))
            .collect();
        let (title, blurb, id) = (page.title(), page.blurb(), page.id());
        let made = shell.view().chrome().read(move |ui, _| {
            let mut hits: Vec<(NodeId, Hit)> = Vec::new();
            ui.tree.set_text(nodes.title, title);
            ui.tree.set_text(nodes.blurb, blurb);
            ui.tree.clear_children(nodes.rail);
            for (pid, name, g) in rail {
                let b = rail_row(ui, nodes.rail, name, g, pid == current);
                hits.push((b, Hit::Page(pid)));
            }
            ui.tree.clear_children(nodes.list);
            draw_form(ui, nodes.list, id, &form, &mut hits);
            hits
        });
        if let Some(made) = made {
            *self.hits.borrow_mut() = made.into_iter().collect();
        }
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        let Some(hit) = self.hits.borrow().get(&id).cloned() else {
            return false;
        };
        match hit {
            Hit::Page(p) => {
                *self.page.borrow_mut() = p;
                self.refresh(shell);
            }
            Hit::Act(page, control, option) => {
                let target = self.pages.borrow().iter().find(|p| p.id() == page).cloned();
                if let (Some(p), Some((action, value))) = (target, act_of(&control, option)) {
                    p.act(shell, &action, &value);
                    self.refresh(shell);
                }
            }
        }
        true
    }

    fn closed(&self) {
        *self.nodes.borrow_mut() = None;
        self.hits.borrow_mut().clear();
    }
}

/// One page in the rail.
fn rail_row(ui: &mut Ui, rail: NodeId, name: &str, g: char, on: bool) -> NodeId {
    let b = ui.tree.add(
        rail,
        Node::row()
            .height(30.0)
            .padding(Insets::hv(10.0, 0.0))
            .gap(9.0)
            .radius(7.0)
            .align(Align::Center)
            .fill(if on {
                Swatch::Token(Token::Hover)
            } else {
                Swatch::Clear
            })
            .hover_fill(Token::Hover)
            .press_fill(Token::Hover)
            .button(name)
            .selected(on),
    );
    ui.tree.add(
        b,
        Node::glyph(g, 12.0, 16.0)
            .ink(if on { Token::Ink } else { Token::Muted })
            .role(Role::None),
    );
    ui.tree.add(
        b,
        Node::text(name, 13.0)
            .ink(Token::Ink)
            .weight(if on { Weight::Medium } else { Weight::Regular })
            .role(Role::None),
    );
    b
}

/// The page's sections into `list`.
fn draw_form(
    ui: &mut Ui,
    list: NodeId,
    page: &'static str,
    form: &Form,
    hits: &mut Vec<(NodeId, Hit)>,
) {
    for s in &form.sections {
        let group = ui.tree.add(list, Node::column().gap(8.0));
        if !s.title.is_empty() {
            plate::caption(ui, group, &s.title);
        }
        let card = plate::card(ui, group);
        for (i, line) in s.lines.iter().enumerate() {
            if i > 0 {
                plate::rule(ui, card);
            }
            let at = plate::line(ui, card, &line.title, line.detail.as_deref());
            control(ui, at, page, &line.control, &line.title, hits);
        }
    }
}

/// A line's control into `at`.
fn control(
    ui: &mut Ui,
    at: NodeId,
    page: &'static str,
    c: &Control,
    name: &str,
    hits: &mut Vec<(NodeId, Hit)>,
) {
    match c {
        Control::None => {}
        Control::Value(v) => {
            ui.tree.add(at, Node::text(v, 12.5).ink(Token::Muted));
        }
        Control::Switch { on, .. } => {
            let track = ui.tree.add(
                at,
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
                    .button(name)
                    .selected(*on),
            );
            ui.tree.add(
                track,
                Node::new()
                    .size(16.0, 16.0)
                    .radius(8.0)
                    .fill(Token::White)
                    .role(Role::None),
            );
            hits.push((track, Hit::Act(page, c.clone(), None)));
        }
        Control::Choice {
            options, chosen, ..
        } => {
            let row = ui.tree.add(
                at,
                Node::row()
                    .padding(Insets::all(2.0))
                    .gap(2.0)
                    .radius(8.0)
                    .fill(Token::Wash)
                    .role(Role::Group)
                    .name(name),
            );
            for (i, (value, label)) in options.iter().enumerate() {
                let on = value == chosen;
                let b = ui.tree.add(
                    row,
                    Node::row()
                        .height(24.0)
                        .padding(Insets::hv(12.0, 0.0))
                        .radius(6.0)
                        .align(Align::Center)
                        .fill(if on {
                            Swatch::Token(Token::Ground)
                        } else {
                            Swatch::Clear
                        })
                        .stroke(
                            if on {
                                Swatch::Token(Token::Hairline)
                            } else {
                                Swatch::Clear
                            },
                            1.0,
                        )
                        .hover_fill(if on { Token::Ground } else { Token::Hover })
                        .button(label)
                        .selected(on),
                );
                ui.tree.add(
                    b,
                    Node::text(label, 12.0)
                        .ink(if on { Token::Ink } else { Token::Muted })
                        .role(Role::None),
                );
                hits.push((b, Hit::Act(page, c.clone(), Some(i))));
            }
        }
        Control::Button { label, filled, .. } => {
            let b = plate::pill(ui, at, label);
            if *filled {
                ui.tree.edit(b, |n| {
                    n.look.fill = Swatch::Token(Token::Accent);
                    n.look.hover = Some(Swatch::Fade(Token::Accent, 0.88));
                    n.look.press = Some(Swatch::Fade(Token::Accent, 0.8));
                });
                if let Some(t) = ui.tree.children(b).first().copied() {
                    ui.tree
                        .edit(t, |n| n.look.ink = Swatch::Token(Token::White));
                }
            }
            hits.push((b, Hit::Act(page, c.clone(), None)));
        }
    }
}
