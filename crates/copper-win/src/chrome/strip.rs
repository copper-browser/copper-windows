//! Tabs across the top (`TabsPosition::Top`, `TabBar.swift`): pinned
//! squares, then titled pills sharing the room, a plus, the empty strip
//! that drags the window, back / forward / reload, and the caption
//! buttons' corner.

use super::build::{door, mark, tip};
use super::glyph;
use super::images::Images;
use super::model::{Model, Nav};
use crate::platform::Icon;
use copper_core::browser::TabId;
use copper_shell_core::TabView;
use copper_win_ui::chrome::Caption;
use copper_win_ui::tokens::{metrics, side};
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Insets, Justify, Node, NodeId, Role, Swatch, Token, Weight};
use std::collections::HashMap;
use std::rc::Rc;

/// A strip tab's height and its title's size (`TabBar.swift` 410–441).
const TAB_HEIGHT: f32 = 28.0;
const TITLE: f32 = 12.5;
const RADIUS: f32 = 8.0;
const DOOR: f32 = 26.0;
const DOOR_GLYPH: f32 = 13.0;

/// What a click in the strip means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Back,
    Forward,
    Reload,
    NewTab,
    Tab(TabId),
    Close(TabId),
}

/// What the strip last drew for a tab.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    id: TabId,
    label: String,
    letter: String,
    icon: Option<usize>,
    live: bool,
    pinned: bool,
}

/// The strip's nodes.
pub struct Strip {
    pub strip: NodeId,
    run: NodeId,
    back: NodeId,
    forward: NodeId,
    reload: NodeId,
    reload_glyph: NodeId,
    drawn: Vec<Drawn>,
    width: f32,
    titled: usize,
    targets: HashMap<NodeId, Hit>,
    nav: Nav,
}

pub fn build(ui: &mut Ui, parent: NodeId) -> Strip {
    let mut targets = HashMap::new();
    let strip = ui.tree.add(
        parent,
        Node::row()
            .height(metrics::STRIP)
            .shrink(0.0)
            .padding(Insets::new(side::INSET, 0.0, 0.0, 0.0))
            .gap(metrics::TAB_GAP)
            .align(Align::Center)
            .drag()
            .layer()
            .hidden(true)
            .role(Role::Tab)
            .name("Tabs"),
    );
    let run = ui.tree.add(
        strip,
        Node::row()
            .gap(metrics::TAB_GAP)
            .align(Align::Center)
            .shrink(1.0)
            .clip(),
    );
    let (plus, _) = door(
        ui,
        strip,
        glyph::PLUS,
        metrics::PLUS_WIDTH,
        DOOR_GLYPH,
        "New tab",
        &tip("New tab", "new-tab"),
    );
    targets.insert(plus, Hit::NewTab);
    ui.tree.add(strip, Node::new().grow(1.0).min_width(24.0));
    let helm = ui.tree.add(
        strip,
        Node::row()
            .gap(side::HELM_GAP)
            .align(Align::Center)
            .padding(Insets::new(0.0, 0.0, 8.0, 0.0)),
    );
    let (back, _) = door(
        ui,
        helm,
        glyph::BACK,
        DOOR,
        DOOR_GLYPH,
        "Back",
        &tip("Back", "back"),
    );
    let (forward, _) = door(
        ui,
        helm,
        glyph::FORWARD,
        DOOR,
        DOOR_GLYPH,
        "Forward",
        &tip("Forward", "forward"),
    );
    let (reload, reload_glyph) = door(
        ui,
        helm,
        glyph::RELOAD,
        DOOR,
        DOOR_GLYPH,
        "Reload",
        &tip("Reload", "reload"),
    );
    targets.insert(back, Hit::Back);
    targets.insert(forward, Hit::Forward);
    targets.insert(reload, Hit::Reload);
    // The caption buttons' corner.
    ui.tree
        .add(strip, Node::new().width(Caption::width()).shrink(0.0));
    Strip {
        strip,
        run,
        back,
        forward,
        reload,
        reload_glyph,
        drawn: Vec::new(),
        width: 0.0,
        titled: 0,
        targets,
        nav: Nav::default(),
    }
}

impl Strip {
    pub fn hit(&self, id: NodeId) -> Option<Hit> {
        self.targets.get(&id).cloned()
    }

    /// The tab a strip node (or anything in it) belongs to.
    pub fn tab_at(&self, ui: &Ui, id: NodeId) -> Option<TabId> {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if let Some(Hit::Tab(t) | Hit::Close(t)) = self.targets.get(&c) {
                return Some(t.clone());
            }
            cur = ui.tree.parent(c);
        }
        None
    }

    /// Draw `m`, sized for a window `window_w` DIPs wide.
    pub fn sync(&mut self, ui: &mut Ui, images: &mut Images, m: &Model) {
        self.sync_nav(ui, m.nav);
        let tabs: Vec<&TabView<Icon>> = m.visible.all().collect();
        let drawn: Vec<Drawn> = tabs
            .iter()
            .map(|t| Drawn {
                id: t.id.clone(),
                label: t.label.clone(),
                letter: t.letter.clone(),
                icon: t.favicon.as_ref().map(|i| Rc::as_ptr(i) as usize),
                live: t.live,
                pinned: t.pin.is_some(),
            })
            .collect();
        let width = ui.size().w;
        let titled = drawn.iter().filter(|d| !d.pinned).count();
        if drawn == self.drawn && (width - self.width).abs() < 0.5 && titled == self.titled {
            return;
        }
        self.width = width;
        self.titled = titled;
        let tab_w = self.tab_width(width, &drawn);
        for c in ui.tree.children(self.run).to_vec() {
            self.forget(ui, c);
        }
        ui.tree.clear_children(self.run);
        for (t, d) in tabs.iter().zip(&drawn) {
            let w = if d.pinned { metrics::PIN_WIDTH } else { tab_w };
            let icon = super::build::icon_id(ui, images, t.favicon.as_ref());
            self.add_tab(ui, t, d, w, icon);
        }
        self.drawn = drawn;
    }

    /// Room for the titled tabs: what the strip leaves, shared, between
    /// the minimum and the fixed width.
    fn tab_width(&self, window_w: f32, drawn: &[Drawn]) -> f32 {
        let pinned = drawn.iter().filter(|d| d.pinned).count() as f32;
        let titled = drawn.len() as f32 - pinned;
        let fixed = side::INSET
            + pinned * (metrics::PIN_WIDTH + metrics::TAB_GAP)
            + metrics::PLUS_WIDTH
            + 24.0
            + metrics::HELM
            + Caption::width();
        if titled < 1.0 {
            return metrics::TAB_WIDTH;
        }
        let each = (window_w - fixed) / titled - metrics::TAB_GAP;
        each.clamp(metrics::TAB_MIN_WIDTH, metrics::TAB_WIDTH)
    }

    fn add_tab(&mut self, ui: &mut Ui, t: &TabView<Icon>, d: &Drawn, w: f32, icon: Option<u32>) {
        let mut n = Node::row()
            .size(w, TAB_HEIGHT)
            .shrink(0.0)
            .padding(Insets::hv(if d.pinned { 7.0 } else { 9.0 }, 0.0))
            .gap(7.0)
            .align(Align::Center)
            .justify(if d.pinned || w < metrics::TAB_TITLED {
                Justify::Center
            } else {
                Justify::Start
            })
            .radius(RADIUS)
            .hover_fill(Token::ColumnHover)
            .button(&t.label)
            .tooltip(&t.label)
            .role(Role::TabItem)
            .selected(d.live);
        if d.live {
            n = n
                .selected_fill(Token::Pill)
                .stroke(Token::Rim, 1.0)
                .shadow(copper_win_ui::tokens::Shadow::new(
                    1.0,
                    side::PILL_SHADOW_RADIUS,
                    side::PILL_SHADOW_Y,
                ))
                .shadow_ink(Token::Lift);
        }
        let node = ui.tree.add(self.run, n);
        mark(ui, node, icon, &t.letter, side::MARK);
        self.targets.insert(node, Hit::Tab(t.id.clone()));
        if d.pinned || w < metrics::TAB_TITLED {
            return;
        }
        ui.tree.add(
            node,
            Node::text(&t.label, TITLE)
                .weight(if d.live {
                    Weight::Semibold
                } else {
                    Weight::Regular
                })
                .ink(Token::ColumnInk)
                .grow(1.0)
                .role(Role::None),
        );
        let close = ui.tree.add(
            node,
            Node::stack()
                .size(side::CLOSE, side::CLOSE)
                .shrink(0.0)
                .radius(side::CLOSE / 2.0)
                .fill(Swatch::Fade(Token::ColumnInk, side::CLOSE_FILL))
                .align(Align::Center)
                .justify(Justify::Center)
                .reveal()
                .button("Close tab")
                .tooltip(&tip("Close tab", "close-tab")),
        );
        ui.tree.add(
            close,
            Node::glyph(glyph::CLOSE, side::CLOSE_GLYPH, side::CLOSE)
                .ink(Token::ColumnMuted)
                .role(Role::None),
        );
        self.targets.insert(close, Hit::Close(t.id.clone()));
    }

    fn forget(&mut self, ui: &Ui, root: NodeId) {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            self.targets.remove(&n);
            stack.extend_from_slice(ui.tree.children(n));
        }
    }

    pub fn sync_nav(&mut self, ui: &mut Ui, nav: Nav) {
        if nav == self.nav {
            return;
        }
        self.nav = nav;
        for (id, on) in [
            (self.back, nav.back),
            (self.forward, nav.forward),
            (self.reload, nav.page),
        ] {
            ui.tree.edit(id, |n| {
                n.flags.disabled = !on;
                n.transform.opacity = if on { 1.0 } else { 0.3 };
            });
        }
        let g = if nav.loading {
            glyph::STOP
        } else {
            glyph::RELOAD
        };
        ui.tree.set_text(self.reload_glyph, &g.to_string());
    }
}
