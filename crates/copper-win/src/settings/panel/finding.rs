//! Settings' search and landing (`SettingsFinder`, `SettingsAnchor`): the
//! index made lazily from every page, the results in the page's place, ↑/↓
//! through them, and a pick landing on its page with its row scrolled into
//! view and lit for a moment. Also the watcher that keeps the header's
//! hairline and the rail in step while Settings is open.

use super::{Landing, Settings, get};
use crate::panels::Panel;
use crate::platform::Shell;
use crate::settings::index;
use crate::settings::look::{self, metric};
use crate::settings::results;
use crate::settings::search::{Index, Results};
use copper_win_ui::tokens::motion;
use copper_win_ui::win::Ui;
use copper_win_ui::{NodeId, Prop, Swatch, Token};
use std::rc::Rc;
use std::time::Duration;

/// How many times a landing looks for its row before giving up (50 ms
/// apart).
const LAND_TRIES: u32 = 20;

impl Settings {
    /// The search index, made now if a page changed since the last one.
    pub(super) fn index(&self, shell: &Shell) -> Rc<Index> {
        if let Some(i) = self.finder.borrow().index.clone() {
            return i;
        }
        let pages = self.pages.borrow().clone();
        let mut entries = Vec::new();
        for id in index::order(&self.ids()) {
            let Some(p) = pages.iter().find(|p| p.id() == id) else {
                continue;
            };
            let mut page = index::page_entry(p.id(), p.title(), p.blurb());
            page.keywords = p.keywords();
            entries.push(page);
            entries.extend(crate::settings::fold::indexing(|| p.entries(shell)));
        }
        let words: Vec<(&'static str, String)> = pages
            .iter()
            .map(|p| (p.id(), format!("{} {}", p.title(), p.keywords().join(" "))))
            .collect();
        let index = Rc::new(Index::new(entries, move |page| {
            words
                .iter()
                .find(|(id, _)| *id == page)
                .map(|(_, w)| w.clone())
                .unwrap_or_default()
        }));
        self.finder.borrow_mut().index = Some(index.clone());
        index
    }

    /// Search for what the field says now.
    pub(super) fn search(&self, shell: &Shell, query: String) {
        let searching = !query.trim().is_empty();
        let results = if searching {
            self.index(shell).search(&query)
        } else {
            Results::default()
        };
        {
            let mut f = self.finder.borrow_mut();
            f.query = query;
            f.results = results;
            f.selection = 0;
            f.showing = searching;
        }
        self.refresh(shell);
    }

    /// Empty the search (`clear`); `false` if there was nothing to clear.
    pub(super) fn clear(&self, shell: &Shell) -> bool {
        let (had, search) = {
            let f = self.finder.borrow();
            (
                !f.query.is_empty() || f.showing,
                self.nodes.get().map(|n| n.search),
            )
        };
        if !had {
            return false;
        }
        if let Some(field) = search {
            shell.view().chrome().read(move |ui, _| {
                ui.set_field_text(field, "", false);
                ui.focus(Some(field));
            });
        }
        self.search(shell, String::new());
        true
    }

    /// Go to a result: its page, scrolled to its row, the row lit.
    pub(super) fn pick(&self, shell: &Shell, i: usize) {
        let hit = {
            let f = self.finder.borrow();
            f.results.flat().get(i).map(|h| (*h).clone())
        };
        let Some(hit) = hit else { return };
        self.finder.borrow_mut().showing = false;
        self.switch(shell, hit.entry.page);
        crate::settings::fold::reveal(&hit.entry.anchor);
        *self.landing.borrow_mut() = Some(Landing {
            anchor: hit.entry.anchor.clone(),
            flash: !hit.entry.is_page,
        });
        self.refresh(shell);
        self.land_soon(shell);
    }

    /// Move the chosen result by `by`, wrapping, and keep it in view.
    pub(super) fn step(&self, shell: &Shell, by: isize) {
        let (old, new, rows) = {
            let mut f = self.finder.borrow_mut();
            let n = f.results.len();
            if n == 0 || !f.showing {
                return;
            }
            let old = f.selection;
            f.selection = (old as isize + by).rem_euclid(n as isize) as usize;
            (old, f.selection, f.rows.clone())
        };
        let (Some(nodes), Some(&from), Some(&to)) =
            (self.nodes.get(), rows.get(old), rows.get(new))
        else {
            return;
        };
        shell.view().chrome().read(move |ui, _| {
            results::choose(ui, from, false);
            results::choose(ui, to, true);
            let (row, list) = (ui.tree.window_rect(to), ui.tree.window_rect(nodes.list));
            let off = ui.tree.scroll_offset(nodes.list);
            let top = row.y - list.y + off;
            ui.tree
                .scroll(nodes.list, |s| s.reveal(top - 8.0, top + row.h + 8.0, true));
        });
    }

    /// Land a pick once its page has been laid out.
    pub(super) fn land_soon(&self, shell: &Shell) {
        let weak = shell.downgrade();
        let generation = self.generation.get();
        copper_win_rt::spawn_local(async move {
            // The page draws first, and says which anchors it has; its rows
            // have a place once the window has laid it out.
            for _ in 0..LAND_TRIES {
                copper_win_rt::sleep(Duration::from_millis(50)).await;
                let Some(shell) = weak.upgrade() else { return };
                let Some(s) = get().filter(|s| s.generation.get() == generation) else {
                    return;
                };
                if s.land(&shell) {
                    return;
                }
            }
            if let Some(s) = get() {
                s.landing.borrow_mut().take();
            }
        });
    }

    /// Scroll the page to the landing's anchor and light it for a moment
    /// (`SettingsAnchor.land`): a row a fifth of the way down, a card by
    /// its top edge with room for its title above. `false` while the
    /// anchor has no place yet (the landing waits for the next try).
    pub(super) fn land(&self, shell: &Shell) -> bool {
        let Some(target) = self.landing.borrow().clone() else {
            return true;
        };
        let Some(nodes) = self.nodes.get() else {
            return true;
        };
        let found = self
            .drawn
            .borrow()
            .anchors
            .iter()
            .find(|a| a.name == target.anchor)
            .map(|a| (a.node, a.card))
            .or_else(|| self.current().and_then(|p| p.anchor(&target.anchor)));
        let flash = target.flash;
        let lit = shell.view().chrome().read(move |ui, _| {
            let Some((node, card)) = found else {
                tracing::debug!(anchor = %target.anchor, "settings: no such anchor; to the top");
                ui.tree.scroll(nodes.list, |s| s.scroll_to(0.0, false));
                return Some(None);
            };
            let (at, list) = (ui.tree.window_rect(node), ui.tree.window_rect(nodes.list));
            tracing::debug!(anchor = %target.anchor, ?at, ?list, "settings: landing");
            if at.h <= 0.0 || list.h <= 0.0 {
                return None;
            }
            let off = ui.tree.scroll_offset(nodes.list);
            let top = at.y - list.y + off;
            let y = if card {
                top - 36.0
            } else {
                top - list.h * 0.22
            };
            ui.tree
                .scroll(nodes.list, |s| s.scroll_to(y.max(0.0), true));
            Some(flash.then(|| light(ui, node, card)))
        });
        let Some(Some(lit)) = lit else {
            return false;
        };
        self.landing.borrow_mut().take();
        if let Some(was) = lit {
            let weak = shell.downgrade();
            let generation = self.generation.get();
            copper_win_rt::spawn_local(async move {
                copper_win_rt::sleep(Duration::from_secs_f32(metric::FLASH_SECONDS)).await;
                let Some(shell) = weak.upgrade() else { return };
                if get().is_some_and(|s| s.generation.get() == generation) {
                    shell.view().chrome().read(move |ui, _| unlight(ui, was));
                }
            });
        }
        true
    }

    /// Keep the header's hairline in step with the page's scroll, and the
    /// panel's rail with the window's width, while Settings is open (the
    /// toolkit tells no one about a scroll).
    pub(super) fn watch(&self, shell: &Shell) {
        let weak = shell.downgrade();
        let generation = self.generation.get();
        copper_win_rt::spawn_local(async move {
            let mut ruled = false;
            let mut width = 0.0;
            loop {
                copper_win_rt::sleep(Duration::from_millis(120)).await;
                let Some(shell) = weak.upgrade() else { return };
                let Some(s) = get() else { return };
                if s.generation.get() != generation {
                    return;
                }
                let Some(nodes) = s.nodes.get() else { return };
                let results = s.finder.borrow().showing;
                let Some((scrolled, w)) = shell
                    .view()
                    .chrome()
                    .read(move |ui, _| (ui.tree.scroll_offset(nodes.list) > 2.0, ui.size().w))
                else {
                    continue;
                };
                let rule = scrolled && !results;
                if rule != ruled {
                    ruled = rule;
                    shell.view().chrome().read(move |ui, _| {
                        ui.tree.animate(
                            nodes.rule,
                            Prop::Opacity,
                            if rule { 1.0 } else { 0.0 },
                            motion::QUICK,
                        );
                    });
                }
                if w != width {
                    if width != 0.0 && look::rail(look::width(w)) != look::rail(look::width(width))
                    {
                        s.refresh(&shell);
                    }
                    width = w;
                }
            }
        });
    }
}

/// Light a landed anchor (`SettingsAnchor`'s wash and ring); returns what
/// to put back.
pub(super) fn light(ui: &mut Ui, node: NodeId, card: bool) -> Lit {
    let before = ui.tree.get(node).map(|n| n.look).unwrap_or_default();
    ui.tree.repaint(node, |n| {
        if !card {
            n.look.fill = Swatch::Fade(Token::Ink, 0.045);
            n.look.radius = 8.0;
        }
        n.look.stroke = Swatch::Fade(Token::Ink, 0.22);
        n.look.stroke_width = 1.25;
    });
    Lit { node, before }
}

/// A lit anchor and how it looked before.
pub(super) struct Lit {
    node: NodeId,
    before: copper_win_ui::node::Look,
}

pub(super) fn unlight(ui: &mut Ui, lit: Lit) {
    if ui.tree.get(lit.node).is_some() {
        ui.tree.repaint(lit.node, |n| n.look = lit.before);
    }
}
