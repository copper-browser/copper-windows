//! Hidden (`Hidden.swift`): what has been taken off the site in front, and
//! the way back. It sits undimmed in the window's top-right corner — the
//! whole point is to keep looking at the page.
//!
//! A list of selectors is not something anyone can read, so resting the
//! pointer on a row puts that one thing back on the page, outlined and
//! scrolled to: you decide what to restore by looking at it. Leaving the
//! rows puts the page back. (The toolkit has no hover events: the panel
//! is ticked and follows the pointer.)
//!
//! While a tab is picking things to hide, Esc stops and Ctrl+Z puts the
//! last one back — [`takes`] claims those two keys only then.

use super::plate::{self, Plate};
use super::redress::redress;
use super::{Panel, Place};
use crate::chrome::glyph;
use crate::commands::Command;
use crate::keys::Chord;
use crate::platform::{Shell, Win};
use copper_core::crashes::Veil;
use copper_shell_core::curtain;
use copper_win_ui::input::{Key, Mods};
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Node, NodeId, Role, Token, Trim};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const ID: &str = "hidden";

/// Register the panel, the picker's two keys, and the curtain's listener.
pub fn register(shell: &Shell) {
    let panel = Rc::new(Hidden::default());
    shell.view().panels().add(shell, panel.clone());
    // `show-hidden` again, so the plate is titled with the site it is for.
    let p = panel.clone();
    shell.view().commands().add(Command::new(
        "show-hidden",
        &panel.name(),
        glyph::HIDDEN,
        move |s| {
            *p.site.borrow_mut() = curtain::here(s);
            super::show(s, ID);
        },
    ));
    shell.view().commands().add(Command::key("hide-stop", |s| {
        if let Some(id) = s.active_id() {
            curtain::stop(s, &id);
        }
    }));
    shell.view().commands().add(Command::key("hide-undo", |s| {
        let Some(site) = curtain::here(s) else { return };
        let before = selectors(&site);
        curtain::undo(s);
        redress(s, &site, &before, None);
    }));
    // One listener: the chrome reads the picker's state, the panel the list.
    curtain::on_change::<Win>(Rc::new(|s: &Shell| {
        s.view().sync(s);
        super::refresh_soon(s, ID);
    }));
}

/// The command a key runs ahead of the app's own: Esc and Ctrl+Z while the
/// tab in front is picking things to hide.
pub fn takes(shell: &Shell, chord: Chord) -> Option<&'static str> {
    let id = shell.active_id()?;
    if !curtain::picking::<Win>(&id) {
        return None;
    }
    let plain = chord.mods == Mods::NONE;
    let ctrl = chord.mods
        == Mods {
            ctrl: true,
            shift: false,
            alt: false,
        };
    match chord.key {
        Key::Escape if plain => Some("hide-stop"),
        Key::Letter('Z') if ctrl => Some("hide-undo"),
        _ => None,
    }
}

/// The selectors hidden on `site` now.
fn selectors(site: &str) -> Vec<String> {
    curtain::veils::<Win>(site)
        .into_iter()
        .map(|v| v.selector)
        .collect()
}

#[derive(Clone, Debug)]
enum Hit {
    Restore(String),
    Pick,
    RestoreAll,
}

#[derive(Default, Clone, Copy)]
struct Nodes {
    list: Option<NodeId>,
    all: Option<NodeId>,
    /// The plate's title: the site it is for.
    title: Option<NodeId>,
    /// The window-wide layer round the plate: the pointer on it has left.
    scrim: Option<NodeId>,
}

/// The Hidden panel.
#[derive(Default)]
pub struct Hidden {
    /// The site it was opened for.
    site: RefCell<Option<String>>,
    nodes: Cell<Nodes>,
    hits: RefCell<HashMap<NodeId, Hit>>,
    /// Each row and its selector.
    rows: RefCell<Vec<(NodeId, String)>>,
    /// The row being rested on.
    peeking: RefCell<Option<String>>,
}

impl Panel for Hidden {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> String {
        self.site
            .borrow()
            .clone()
            .unwrap_or_else(|| "This page".into())
    }

    fn name(&self) -> String {
        "Hidden on This Site".into()
    }

    fn glyph(&self) -> char {
        glyph::HIDDEN
    }

    fn width(&self) -> f32 {
        380.0
    }

    fn place(&self) -> Place {
        Place::Corner
    }

    fn build(&self, ui: &mut Ui, p: &Plate) {
        let list = ui.tree.add(p.body, Node::column().gap(6.0).shrink(1.0));
        let mut hits = self.hits.borrow_mut();
        hits.clear();
        let pick = plate::pill_filled(ui, p.foot, "Hide something…");
        let all = plate::pill(ui, p.foot, "Restore all");
        ui.tree.add(p.foot, Node::new().grow(1.0));
        hits.insert(pick, Hit::Pick);
        hits.insert(all, Hit::RestoreAll);
        p.show_foot(ui, true);
        self.nodes.set(Nodes {
            list: Some(list),
            all: Some(all),
            title: p.title,
            scrim: Some(p.scrim),
        });
    }

    fn refresh(&self, shell: &Shell) {
        let Nodes {
            list: Some(list),
            all: Some(all),
            title,
            ..
        } = self.nodes.get()
        else {
            return;
        };
        let site = curtain::here(shell);
        let was = self.site.replace(site.clone());
        // Another tab or site in front: whatever was shown goes back.
        if was != site
            && self.peeking.borrow_mut().take().is_some()
            && let Some(was) = was
        {
            curtain::unpeek(shell);
            redress(shell, &was, &[], None);
        }
        let heading = self.title();
        let veils: Vec<Veil> = site
            .as_deref()
            .map(curtain::veils::<Win>)
            .unwrap_or_default();
        let made = shell.view().chrome().read(move |ui, _| {
            if let Some(title) = title {
                ui.tree.set_text(title, &heading);
            }
            ui.tree.clear_children(list);
            ui.tree.set_hidden(all, veils.is_empty());
            let mut rows = Vec::new();
            if veils.is_empty() {
                let card = plate::card(ui, list);
                plate::nothing(ui, card, "Nothing is hidden here.");
                return rows;
            }
            plate::caption(ui, list, "Hidden on this site — rest on a line to see it");
            let scroll = ui.tree.add(
                list,
                Node::scroll()
                    .max_height(320.0)
                    .padding(Insets::new(0.0, 0.0, 0.0, 2.0))
                    .shrink(1.0),
            );
            let card = plate::card(ui, scroll);
            for (i, v) in veils.iter().enumerate() {
                if i > 0 {
                    plate::rule(ui, card);
                }
                let r = ui.tree.add(
                    card,
                    Node::row()
                        .gap(10.0)
                        .padding(Insets::hv(14.0, 9.0))
                        .align(Align::Center)
                        .hover_fill(Token::Hover)
                        .hit()
                        .role(Role::Group)
                        .name(&v.label),
                );
                let words = ui
                    .tree
                    .add(r, Node::column().gap(2.0).grow(1.0).shrink(1.0));
                ui.tree.add(
                    words,
                    Node::text(&v.label, 13.0)
                        .ink(Token::Ink)
                        .trim(Trim::End)
                        .role(Role::None),
                );
                if let Some(note) = v.note.as_deref().filter(|n| !n.is_empty()) {
                    ui.tree.add(
                        words,
                        Node::text(note, 11.5)
                            .ink(Token::Muted)
                            .trim(Trim::End)
                            .role(Role::None),
                    );
                }
                ui.tree.add(r, Node::new().min_width(8.0));
                let restore = plate::quick(ui, r, "Restore", Token::Ink.into(), true);
                rows.push((r, restore, v.selector.clone()));
            }
            rows
        });
        let Some(made) = made else {
            super::refresh_soon(shell, ID);
            return;
        };
        let mut hits = self.hits.borrow_mut();
        hits.retain(|_, h| !matches!(h, Hit::Restore(_)));
        let mut rows = self.rows.borrow_mut();
        rows.clear();
        for (row, restore, selector) in made {
            hits.insert(restore, Hit::Restore(selector.clone()));
            rows.push((row, selector));
        }
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        let Some(hit) = self.hits.borrow().get(&id).cloned() else {
            return false;
        };
        let site = self.site.borrow().clone();
        match hit {
            Hit::Restore(selector) => {
                if let Some(site) = site {
                    *self.peeking.borrow_mut() = None;
                    curtain::restore(shell, &site, &selector);
                    redress(shell, &site, &[selector], None);
                }
            }
            Hit::Pick => curtain::toggle(shell),
            Hit::RestoreAll => {
                if let Some(site) = site {
                    *self.peeking.borrow_mut() = None;
                    let before = selectors(&site);
                    curtain::restore_all(shell, &site);
                    redress(shell, &site, &before, None);
                    shell.view().panels().hide(shell);
                }
            }
        }
        true
    }

    fn ticks(&self) -> bool {
        true
    }

    /// Follow the pointer over the rows: peek at the one rested on; leaving
    /// the whole plate puts the page back (between rows it stays).
    fn tick(&self, shell: &Shell) {
        // The tab or site in front moved under the open panel: retitle it
        // and list that site's things instead.
        if curtain::here(shell) != *self.site.borrow() {
            self.refresh(shell);
            return;
        }
        let rows: Vec<(NodeId, String)> = self.rows.borrow().clone();
        let scrim = self.nodes.get().scrim;
        let Some(over) = shell.view().chrome().read(move |ui, _| {
            let Some(h) = ui.tree.hovered().filter(|h| Some(*h) != scrim) else {
                return Some(None);
            };
            let row = rows
                .into_iter()
                .find(|(r, _)| ui.tree.is_within(h, *r))
                .map(|(_, s)| s);
            // Inside the plate but on no row: as it was.
            row.map(Some)
        }) else {
            return;
        };
        let Some(over) = over else { return };
        if *self.peeking.borrow() == over {
            return;
        }
        *self.peeking.borrow_mut() = over.clone();
        let site = self.site.borrow().clone();
        match (over, site) {
            (Some(selector), Some(site)) => {
                curtain::peek(shell, &site, &selector);
                redress(shell, &site, &[], Some(&selector));
            }
            (None, Some(site)) => {
                curtain::unpeek(shell);
                redress(shell, &site, &[], None);
            }
            (_, None) => curtain::unpeek(shell),
        }
    }

    fn closing(&self, shell: &Shell) {
        if self.peeking.borrow_mut().take().is_some() {
            curtain::unpeek(shell);
            if let Some(site) = self.site.borrow().clone() {
                redress(shell, &site, &[], None);
            }
        }
    }

    fn closed(&self) {
        self.nodes.set(Nodes::default());
        self.hits.borrow_mut().clear();
        self.rows.borrow_mut().clear();
    }
}
