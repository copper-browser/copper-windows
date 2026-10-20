//! Drawing Settings: what is on show now (a page, a page that draws
//! itself, or search results), compared with what was drawn last so an
//! unchanged page is left alone; the rail kept when nothing about it
//! changed; and the keyboard kept in the box it was in.

use super::{Owner, Settings, Shown};
use crate::platform::Shell;
use crate::settings::draw::{self, Drawn, Hit};
use crate::settings::frame;
use crate::settings::index;
use crate::settings::look::{self, Inks};
use crate::settings::results::{self, PageName};
use crate::settings::search;
use copper_win_ui::Prop;

impl Settings {
    /// Draw what is on show now, if it changed (`Panel::refresh`).
    pub(super) fn redraw(&self, shell: &Shell) {
        let Some(nodes) = self.nodes.get() else {
            return;
        };
        if self.shown.borrow().is_none() {
            // Freshly built: start the hairline's watch.
            self.watch(shell);
        }
        let dark = shell.is_dark();
        let ink = Inks::new(dark);
        let width = shell
            .view()
            .chrome()
            .read(|ui, _| look::width(ui.size().w))
            .unwrap_or(920.0);
        let showing_results = self.finder.borrow().showing;
        let page = self.current();
        let owner = if showing_results {
            Owner::Results
        } else {
            match &page {
                Some(p) => Owner::Page(p.id()),
                None => return,
            }
        };
        let body = match (&owner, &page) {
            (Owner::Page(_), Some(p)) => p.body(shell),
            _ => None,
        };
        let form = match (&owner, &page, &body) {
            (Owner::Page(_), Some(p), None) => Some(p.draw(shell)),
            _ => None,
        };
        let rail_rows = self.rail_rows();
        let shown = Shown {
            owner,
            form: form.clone(),
            dark,
            width,
            rail: rail_rows
                .iter()
                .flat_map(|(_, r)| r.iter().map(|r| (r.id, r.count)))
                .collect(),
        };
        let before = self.shown.borrow().clone();
        let same_owner = before.as_ref().map(|b| b.owner) == Some(owner);
        if body.is_none() && before.as_ref() == Some(&shown) && !showing_results {
            return;
        }
        // The page going off screen hears of it.
        if let Some(Shown {
            owner: Owner::Page(old),
            ..
        }) = &before
            && !same_owner
            && let Some(p) = self.find(old)
        {
            p.left();
        }
        let (title, blurb) = match (owner, &page) {
            (Owner::Results, _) => {
                let f = self.finder.borrow();
                (
                    "Search".to_string(),
                    search::results_line(&f.query, &f.results),
                )
            }
            (_, Some(p)) => (p.title().to_string(), p.blurb().to_string()),
            _ => return,
        };
        let names: Vec<PageName> = self
            .pages
            .borrow()
            .iter()
            .map(|p| PageName {
                id: p.id(),
                title: p.title(),
                glyph: p.glyph(),
            })
            .collect();
        let (query, results, selection, empty) = {
            let f = self.finder.borrow();
            (
                f.query.clone(),
                f.results.clone(),
                f.selection,
                f.query.is_empty(),
            )
        };
        let chosen = (!showing_results).then(|| self.page.get());
        let keep = self.shown.borrow().as_ref().is_some_and(|b| {
            b.rail == shown.rail && b.dark == dark && b.width == width && b.owner == owner
        });
        let made = shell.view().chrome().read(|ui, _| {
            let focused = self.focused_field(ui);
            let mut drawn = Drawn::default();
            frame::restyle(ui, &nodes, &ink, width);
            frame::header(ui, &nodes, &title, &blurb);
            frame::search_mark(ui, &nodes, empty);
            // The rail: kept when nothing about it changed (its rows' hover
            // is the pointer's), else drawn again.
            if keep {
                drawn.hits.extend(
                    self.drawn
                        .borrow()
                        .hits
                        .iter()
                        .filter(|(_, h)| matches!(h, Hit::Page(_) | Hit::Close | Hit::Clear))
                        .cloned(),
                );
            } else {
                frame::rail(ui, &nodes, &rail_rows, chosen, &ink, &mut drawn);
            }
            drawn.hit(nodes.close, Hit::Close);
            drawn.hit(nodes.clear, Hit::Clear);
            if !same_owner {
                ui.tree.clear_children(nodes.list);
                ui.tree.scroll(nodes.list, |s| s.scroll_to(0.0, false));
                ui.tree.set_prop(nodes.rule, Prop::Opacity, 0.0);
            }
            let mut rows = Vec::new();
            match (owner, body, &form) {
                (Owner::Results, _, _) => {
                    ui.tree.clear_children(nodes.list);
                    results::list_style(ui, nodes.list);
                    rows = results::draw(
                        ui,
                        nodes.list,
                        &query,
                        &results,
                        selection,
                        &names,
                        &ink,
                        width >= 760.0,
                        &mut drawn,
                    );
                }
                (Owner::Page(_), Some(draw_body), _) => {
                    results::page_style(ui, nodes.list);
                    draw_body(ui, nodes.list);
                }
                (Owner::Page(id), None, Some(form)) => {
                    results::page_style(ui, nodes.list);
                    ui.tree.clear_children(nodes.list);
                    // The top of every page, where a page's own result lands.
                    let top = ui.tree.add(
                        nodes.list,
                        copper_win_ui::Node::new()
                            .height(1.0)
                            .role(copper_win_ui::Role::None),
                    );
                    drawn.anchors.push(draw::Anchor {
                        name: index::top_anchor(id),
                        node: top,
                        card: false,
                    });
                    draw::form(ui, nodes.list, id, form, &ink, &mut drawn);
                    // The keyboard stays in the box it was in.
                    if let Some(action) = focused
                        && let Some((f, _)) = drawn.fields.iter().find(|(_, h)| h.action == action)
                    {
                        ui.focus(Some(*f));
                    }
                }
                _ => {}
            }
            (drawn, rows)
        });
        if let Some((drawn, rows)) = made {
            *self.drawn.borrow_mut() = drawn;
            self.finder.borrow_mut().rows = rows;
            *self.shown.borrow_mut() = Some(shown);
        } else {
            tracing::debug!("settings: the window was busy; not redrawn");
        }
    }
}
