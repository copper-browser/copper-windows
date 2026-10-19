//! History (`Recall.swift` `HistoryPanel`), the worked example of a panel:
//! search everywhere you have been, grouped by day, newest first; a click
//! goes there, Remove (on hover) forgets one; the foot counts the pages
//! and offers to clear — history, sign-ins or the cache, each said in
//! words nobody has to guess at.

use super::Panel;
use super::plate::{self, Plate};
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::sys::calendar::WinCalendar;
use copper_core::history::Trace;
use copper_engine::{Engine, Profile, WebsiteDataKinds};
use copper_shell_core::format;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Event, Insets, Node, NodeId, Role, Token, Trim};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// At most this many rows are drawn (a search narrows the rest).
const SHOWN: usize = 300;

/// What a click in the panel means.
#[derive(Clone, Debug)]
enum Hit {
    Go(String),
    Forget(String),
    Clear,
    Sweep(Sweep),
    Back,
}

#[derive(Clone, Copy, Debug)]
enum Sweep {
    History,
    Sites,
    Cache,
}

#[derive(Default)]
struct Nodes {
    field: Option<NodeId>,
    list: Option<NodeId>,
    sweeps: Option<NodeId>,
    count: Option<NodeId>,
    plate: Option<Plate>,
}

/// The History panel.
#[derive(Default)]
pub struct History {
    nodes: RefCell<Nodes>,
    hits: RefCell<HashMap<NodeId, Hit>>,
    clearing: Cell<bool>,
}

impl Panel for History {
    fn id(&self) -> &'static str {
        "history"
    }

    fn title(&self) -> String {
        "History".into()
    }

    fn glyph(&self) -> char {
        glyph::HISTORY
    }

    fn width(&self) -> f32 {
        600.0
    }

    fn build(&self, ui: &mut Ui, p: &Plate) {
        let field = plate::hunt(ui, p.body, "Search everywhere you have been");
        let list = ui.tree.add(
            p.body,
            Node::scroll().max_height(420.0).gap(14.0).shrink(1.0),
        );
        let sweeps = ui.tree.add(p.body, Node::column().gap(10.0).hidden(true));
        let card = plate::card(ui, sweeps);
        let mut hits = self.hits.borrow_mut();
        hits.clear();
        for (i, (title, detail, button, sweep)) in [
            (
                "History",
                "Everywhere you have been",
                "Clear",
                Sweep::History,
            ),
            (
                "Cookies and sign-ins",
                "Signs you out of every site",
                "Sign out of everything",
                Sweep::Sites,
            ),
            (
                "Cache",
                "Only what was fetched to draw pages",
                "Clear",
                Sweep::Cache,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                plate::rule(ui, card);
            }
            let control = plate::line(ui, card, title, Some(detail));
            let b = plate::pill(ui, control, button);
            hits.insert(b, Hit::Sweep(sweep));
        }
        let back_row = ui
            .tree
            .add(sweeps, Node::row().justify(copper_win_ui::Justify::End));
        let back = plate::pill(ui, back_row, "Back");
        hits.insert(back, Hit::Back);
        let count = ui
            .tree
            .add(p.foot, Node::text("", 12.0).ink(Token::Muted).grow(1.0));
        let clear = plate::pill(ui, p.foot, "Clear…");
        hits.insert(clear, Hit::Clear);
        p.show_foot(ui, true);
        ui.focus(Some(field));
        self.clearing.set(false);
        *self.nodes.borrow_mut() = Nodes {
            field: Some(field),
            list: Some(list),
            sweeps: Some(sweeps),
            count: Some(count),
            plate: Some(*p),
        };
    }

    fn refresh(&self, shell: &Shell) {
        let Some(field) = self.nodes.borrow().field else {
            return;
        };
        let chrome = shell.view().chrome().clone();
        let typed = chrome
            .read(move |ui, _| ui.field_text(field).map(str::to_string))
            .flatten()
            .unwrap_or_default();
        let traces = shell.history().search(&typed);
        let days = group(&traces);
        let total = traces.len();
        let empty = if typed.is_empty() {
            "Nothing yet."
        } else {
            "Nothing matches."
        };
        let (list, count, sweeps, plate) = {
            let n = self.nodes.borrow();
            (n.list, n.count, n.sweeps, n.plate)
        };
        let (Some(list), Some(count), Some(sweeps), Some(plate)) = (list, count, sweeps, plate)
        else {
            return;
        };
        let clearing = self.clearing.get();
        let mut made: Vec<(NodeId, Hit)> = Vec::new();
        let rows = chrome.read(move |ui, _| {
            ui.tree.clear_children(list);
            ui.tree.set_hidden(list, clearing);
            ui.tree.set_hidden(sweeps, !clearing);
            plate.show_foot(ui, !clearing);
            ui.tree.set_text(
                count,
                &match total {
                    1 => "1 page".to_string(),
                    n => format!("{n} pages"),
                },
            );
            if days.is_empty() {
                let card = plate::card(ui, list);
                plate::nothing(ui, card, empty);
                return made;
            }
            for (day, rows) in days {
                let group = ui.tree.add(list, Node::column().gap(6.0));
                plate::caption(ui, group, &day);
                let card = plate::card(ui, group);
                for (i, t) in rows.iter().enumerate() {
                    if i > 0 {
                        plate::rule(ui, card);
                    }
                    row(ui, card, t, &mut made);
                }
            }
            made
        });
        if let Some(made) = rows {
            let mut h = self.hits.borrow_mut();
            h.retain(|_, v| !matches!(v, Hit::Go(_) | Hit::Forget(_)));
            h.extend(made);
        }
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        let Some(hit) = self.hits.borrow().get(&id).cloned() else {
            return false;
        };
        match hit {
            Hit::Go(url) => {
                shell.view().panels().hide(shell);
                match shell.active_id() {
                    Some(tab) => shell.navigate(&tab, &url),
                    None => {
                        shell.open(
                            &url,
                            copper_shell_core::OpenTab {
                                foreground: true,
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            Hit::Forget(key) => {
                let forgot = shell.history_mut().forget(&key);
                if forgot {
                    shell.history_changed();
                }
                self.refresh(shell);
            }
            Hit::Clear => {
                self.clearing.set(true);
                self.refresh(shell);
            }
            Hit::Back => {
                self.clearing.set(false);
                self.refresh(shell);
            }
            Hit::Sweep(s) => {
                sweep(shell, s);
                self.clearing.set(false);
                self.refresh(shell);
            }
        }
        true
    }

    fn field(&self, shell: &Shell, id: NodeId, e: &Event) -> bool {
        if self.nodes.borrow().field != Some(id) {
            return false;
        }
        match e {
            Event::FieldChanged(_) => self.refresh(shell),
            Event::FieldCancel(_) => {
                shell.view().panels().hide(shell);
            }
            _ => {}
        }
        true
    }

    fn closed(&self) {
        *self.nodes.borrow_mut() = Nodes::default();
        self.hits.borrow_mut().clear();
    }
}

/// The traces by day, newest day first (`When.day`), at most [`SHOWN`].
fn group(traces: &[Trace]) -> Vec<(String, Vec<Trace>)> {
    let mut sorted: Vec<&Trace> = traces.iter().collect();
    sorted.sort_by(|a, b| b.last.0.total_cmp(&a.last.0));
    let mut out: Vec<(i64, String, Vec<Trace>)> = Vec::new();
    for t in sorted.into_iter().take(SHOWN) {
        let unix = t.last.unix();
        let d = format::day_number::<WinCalendar>(unix);
        match out.last_mut() {
            Some((day, _, rows)) if *day == d => rows.push(t.clone()),
            _ => out.push((d, format::day::<WinCalendar>(unix), vec![t.clone()])),
        }
    }
    out.into_iter()
        .map(|(_, name, rows)| (name, rows))
        .collect()
}

/// One line: a letter mark, the title over the address, and the time —
/// or, under the pointer, Remove.
fn row(ui: &mut Ui, card: NodeId, t: &Trace, hits: &mut Vec<(NodeId, Hit)>) {
    let r = ui.tree.add(
        card,
        Node::row()
            .padding(Insets::hv(14.0, 9.0))
            .gap(12.0)
            .align(Align::Center)
            .hover_fill(Token::Hover)
            .button(if t.title.is_empty() { &t.key } else { &t.title }),
    );
    hits.push((r, Hit::Go(t.url.clone())));
    let letter = t
        .key
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "•".into());
    let mark = ui.tree.add(
        r,
        Node::stack()
            .size(16.0, 16.0)
            .radius(4.0)
            .fill(Token::Wash)
            .align(Align::Center)
            .justify(copper_win_ui::Justify::Center)
            .role(Role::None),
    );
    ui.tree.add(
        mark,
        Node::text(&letter, 9.0).ink(Token::Muted).role(Role::None),
    );
    let words = ui
        .tree
        .add(r, Node::column().gap(2.0).grow(1.0).shrink(1.0));
    ui.tree.add(
        words,
        Node::text(if t.title.is_empty() { &t.key } else { &t.title }, 13.0)
            .ink(Token::Ink)
            .trim(Trim::End)
            .role(Role::None),
    );
    ui.tree.add(
        words,
        Node::text(&t.key, 11.5)
            .ink(Token::Muted)
            .trim(Trim::Middle)
            .role(Role::None),
    );
    let end = ui
        .tree
        .add(r, Node::stack().shrink(0.0).align(Align::Center));
    ui.tree.add(
        end,
        Node::text(&format::clock::<WinCalendar>(t.last.unix()), 11.5)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let forget = ui.tree.add(
        end,
        Node::row()
            .height(22.0)
            .padding(Insets::hv(8.0, 0.0))
            .radius(11.0)
            .fill(Token::Ground)
            .align(Align::Center)
            .reveal()
            .button("Remove"),
    );
    ui.tree.add(
        forget,
        Node::text("Remove", 11.5)
            .ink(copper_win_ui::Swatch::Fixed(copper_win_ui::Color::rgba(
                0.85, 0.2, 0.2, 0.75,
            )))
            .role(Role::None),
    );
    hits.push((forget, Hit::Forget(t.key.clone())));
}

/// Clear history, every site's cookies and storage, or the cache.
fn sweep(shell: &Shell, s: Sweep) {
    if let Sweep::History = s {
        shell.history_mut().clear();
        shell.history_changed();
        shell.toast("History cleared");
        return;
    }
    let kinds = match s {
        Sweep::Sites => WebsiteDataKinds::ALL,
        _ => WebsiteDataKinds::CACHES,
    };
    let mut names: Vec<Option<String>> = vec![None];
    names.extend(shell.browser().profiles().into_iter().map(Some));
    let engine = shell.engine().clone();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        for name in names {
            let id = copper_shell_core::shell::pages::profile_id(name.as_deref());
            if let Ok(p) = engine.profile(&id, false)
                && let Err(e) = p.clear_website_data(None, kinds).await
            {
                tracing::warn!("clearing {}: {e}", id.as_str());
            }
        }
        if let Some(s) = weak.upgrade() {
            s.toast(match kinds {
                WebsiteDataKinds::ALL => "Signed out of every site",
                _ => "Cache cleared",
            });
        }
    });
}
