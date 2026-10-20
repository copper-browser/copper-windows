//! Search results in the page's place (`SettingsResults`): hits grouped by
//! page under the page's icon and name, each a row with its matched
//! letters in a heavier weight, where it sits, and ↵ on the chosen one —
//! or, with nothing found, a few words to try instead.

use super::controls;
use super::draw::{Drawn, Hit};
use super::look::{Inks, metric};
use super::search::{Results, SUGGESTIONS};
use crate::chrome::glyph;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Node, NodeId, Role, Swatch, Token, Trim, Weight};

/// A page as the results name it.
pub struct PageName {
    pub id: &'static str,
    pub title: &'static str,
    pub glyph: char,
}

/// The page column as the results want it (`SettingsResults.list`): 22
/// between pages, a little closer under the header.
pub fn list_style(ui: &mut Ui, list: NodeId) {
    ui.tree.edit(list, |n| {
        n.style.gap = 22.0;
        n.style.padding = Insets::new(metric::PAGE_H, 6.0, metric::PAGE_H, 32.0);
    });
}

/// The page column as a page wants it.
pub fn page_style(ui: &mut Ui, list: NodeId) {
    let want = Insets::new(
        metric::PAGE_H,
        metric::PAGE_TOP,
        metric::PAGE_H,
        metric::PAGE_BOTTOM,
    );
    if ui.tree.get(list).is_some_and(|n| n.style.padding != want) {
        ui.tree.edit(list, |n| {
            n.style.gap = metric::SECTIONS;
            n.style.padding = want;
        });
    }
}

/// Light result row `row` as the chosen one, or not: its wash and its ↵.
pub fn choose(ui: &mut Ui, row: NodeId, on: bool) {
    ui.tree.repaint(row, |n| {
        n.look.fill = if on {
            Swatch::Fade(Token::Ink, 0.055)
        } else {
            Swatch::Clear
        };
        n.flags.selected = on;
    });
    if let Some(&mark) = ui.tree.children(row).last() {
        ui.tree
            .repaint(mark, |n| n.transform.opacity = if on { 1.0 } else { 0.0 });
    }
}

/// Draw `results` for `query` into `list`, the `selection`-th row chosen;
/// `wide`: room for where each row sits on the right. Returns each row's
/// node in results order (for keeping the chosen one in view).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    list: NodeId,
    query: &str,
    results: &Results,
    selection: usize,
    names: &[PageName],
    ink: &Inks,
    wide: bool,
    drawn: &mut Drawn,
) -> Vec<NodeId> {
    if results.is_empty() {
        empty(ui, list, query, ink, drawn);
        return Vec::new();
    }
    let mut rows = Vec::with_capacity(results.len());
    let mut index = 0;
    for g in &results.groups {
        let group = ui.tree.add(list, Node::column().gap(metric::SECTION_GAP));
        let head = ui.tree.add(
            group,
            Node::row()
                .gap(7.0)
                .align(Align::Center)
                .padding(Insets::new(metric::LINE_INSET, 0.0, 0.0, 0.0)),
        );
        let name = names.iter().find(|n| n.id == g.page);
        if let Some(n) = name {
            ui.tree.add(
                head,
                Node::glyph(n.glyph, 11.5, 15.0)
                    .ink(Swatch::Fixed(ink.heading))
                    .role(Role::None),
            );
        }
        ui.tree.add(
            head,
            Node::text(name.map_or(g.page, |n| n.title), metric::CAPTION)
                .weight(Weight::Semibold)
                .ink(Swatch::Fixed(ink.heading)),
        );
        let card = controls::card(ui, group, ink);
        for (i, hit) in g.hits.iter().enumerate() {
            if i > 0 {
                controls::rule(ui, card, ink);
            }
            let chosen = index == selection;
            let row = ui.tree.add(
                card,
                Node::row()
                    .padding(Insets::hv(metric::LINE_INSET, 10.0))
                    .gap(14.0)
                    .align(Align::Center)
                    .radius(7.0)
                    .fill(if chosen {
                        Swatch::Fade(Token::Ink, 0.055)
                    } else {
                        Swatch::Clear
                    })
                    .hover_fill(Swatch::Fade(Token::Ink, 0.055))
                    .button(&hit.entry.title)
                    .selected(chosen),
            );
            let words = ui
                .tree
                .add(row, Node::column().gap(3.0).grow(1.0).shrink(1.0));
            controls::marked(
                ui,
                words,
                &hit.entry.title,
                &hit.title_marks,
                metric::TITLE,
                (Weight::Regular, Swatch::Token(Token::Ink)),
                (Weight::Semibold, Swatch::Token(Token::Ink)),
            );
            if !hit.entry.subtitle.is_empty() {
                controls::marked(
                    ui,
                    words,
                    &hit.entry.subtitle,
                    &hit.subtitle_marks,
                    metric::DETAIL,
                    (Weight::Regular, Swatch::Fixed(ink.detail)),
                    (Weight::Medium, Swatch::Fade(Token::Ink, 0.85)),
                );
            }
            let crumb = if hit.entry.is_page {
                "Page"
            } else {
                hit.entry.section.as_str()
            };
            if wide && !crumb.is_empty() {
                ui.tree.add(
                    row,
                    Node::text(crumb, 11.5)
                        .ink(Swatch::Fixed(ink.detail))
                        .shrink(0.0)
                        .role(Role::None),
                );
            }
            ui.tree.add(
                row,
                Node::glyph(glyph::RETURN, 10.5, 14.0)
                    .ink(Swatch::Fixed(ink.detail))
                    .opacity(if chosen { 1.0 } else { 0.0 })
                    .role(Role::None),
            );
            drawn.hit(row, Hit::Result(index));
            rows.push(row);
            index += 1;
        }
    }
    rows
}

/// Nothing found: say so, and offer words that are.
fn empty(ui: &mut Ui, list: NodeId, query: &str, ink: &Inks, drawn: &mut Drawn) {
    let col = ui.tree.add(
        list,
        Node::column()
            .gap(14.0)
            .padding(Insets::new(0.0, 6.0, 0.0, 0.0)),
    );
    let words = ui.tree.add(col, Node::column().gap(5.0));
    ui.tree.add(
        words,
        Node::text(&format!("No settings match “{}”", query.trim()), 15.0)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .trim(Trim::Wrap),
    );
    ui.tree.add(
        words,
        Node::text("Try another word for it — or one of these:", 13.0)
            .ink(Swatch::Fixed(ink.detail)),
    );
    // Two rows of four: the toolkit's rows don't wrap.
    for chunk in SUGGESTIONS.chunks(4) {
        let row = ui.tree.add(col, Node::row().gap(6.0));
        for word in chunk {
            let id = controls::pill(ui, row, word, false, true);
            drawn.hit(id, Hit::Suggest(word));
        }
    }
}
