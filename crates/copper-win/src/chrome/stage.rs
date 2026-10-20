//! The stage: where pages show (`Stage.swift`).
//!
//! Each live page has a pane — its slot, and over it the page's status
//! when the page could not be shown (offline, crashed, refused) with the
//! button that retries. One pane is shown at a time; switching tabs only
//! flips which, so a tab switch costs no navigation and no relayout of
//! the page. Over the panes: the progress line along the top and the
//! hovered link's address at the bottom.

use crate::slot::PageHost;
use copper_core::browser::TabId;
use copper_engine::Backstage;
use copper_engine_webview2::{WebView2Backstage, WebView2Page};
use copper_shell_core::PageStatus;
use copper_win_ui::tokens::motion;
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Abs, Align, Insets, Justify, Node, NodeId, Prop, Role, TextAlign, Token, Trim,
};
use std::collections::HashMap;

/// One page's place on the stage.
struct Pane {
    page: WebView2Page,
    /// Lent to the engine's backstage (an agent works in it): the slot
    /// hosts the page only while the pane is shown.
    lent: bool,
    pane: NodeId,
    slot: NodeId,
    status: NodeId,
    title: NodeId,
    detail: NodeId,
    button: NodeId,
    button_text: NodeId,
}

/// The stage's nodes.
pub struct Stage {
    pub stage: NodeId,
    progress: NodeId,
    /// Bumped by every change of load: a pending fade checks it.
    progress_ticket: u64,
    hover: NodeId,
    hover_text: NodeId,
    panes: HashMap<TabId, Pane>,
    /// Status to show for a tab, kept until its pane exists.
    statuses: HashMap<TabId, PageStatus>,
    shown: Option<TabId>,
    loading: bool,
}

/// What a click on the stage means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The status's button (Reload, Try again).
    StatusButton(TabId),
}

const PROGRESS_HEIGHT: f32 = 2.0;
/// The hovered link's address: in from the foot and the side, and at most
/// this wide.
const HOVER_INSET: f32 = 8.0;
const HOVER_WIDEST: f32 = 560.0;

pub fn build(ui: &mut Ui, parent: NodeId) -> Stage {
    let stage = ui.tree.add(
        parent,
        Node::stack()
            .grow(1.0)
            .fill(Token::Ground)
            .clip()
            .role(Role::Pane)
            .name("Page"),
    );
    let progress = ui.tree.add(
        stage,
        Node::new()
            .absolute(Abs {
                left: Some(0.0),
                top: Some(0.0),
                right: Some(0.0),
                bottom: None,
            })
            .height(PROGRESS_HEIGHT)
            .fill(Token::Accent)
            .layer()
            .opacity(0.0)
            .role(Role::None),
    );
    // The hovered link's address (a Linux / Windows addition: the Mac has
    // none): 8 in from the foot and the side, 12 muted on the ground,
    // 9 × 3 in, corners 7, a hairline and a small shadow; cut in the
    // middle so the host and the end of the path both show; never in the
    // pointer's way.
    let hover = ui.tree.add(
        stage,
        Node::row()
            .absolute(Abs {
                left: Some(HOVER_INSET),
                top: None,
                right: None,
                bottom: Some(HOVER_INSET),
            })
            .max_width(HOVER_WIDEST)
            .padding(Insets::hv(9.0, 3.0))
            .align(Align::Center)
            .radius(7.0)
            .fill(Token::Ground)
            .stroke(Token::Hairline, 1.0)
            .shadow(copper_win_ui::tokens::Shadow::new(0.10, 3.0, 2.0))
            .layer()
            .inert()
            .hidden(true)
            .role(Role::ToolTip),
    );
    let hover_text = ui.tree.add(
        hover,
        Node::text("", 12.0)
            .ink(Token::Muted)
            .trim(Trim::Middle)
            .shrink(1.0)
            .role(Role::None),
    );
    Stage {
        stage,
        progress,
        progress_ticket: 0,
        hover,
        hover_text,
        panes: HashMap::new(),
        statuses: HashMap::new(),
        shown: None,
        loading: false,
    }
}

impl Stage {
    /// What clicking `id` means, if it is the stage's.
    pub fn hit(&self, id: NodeId) -> Option<Hit> {
        self.panes
            .iter()
            .find(|(_, p)| p.button == id)
            .map(|(t, _)| Hit::StatusButton(t.clone()))
    }

    /// The slot node showing `tab`'s page.
    pub fn slot_of(&self, tab: &TabId) -> Option<NodeId> {
        self.panes.get(tab).map(|p| p.slot)
    }

    /// The tab on stage.
    pub fn shown(&self) -> Option<&TabId> {
        self.shown.as_ref()
    }

    /// Give `tab`'s page a pane (hidden until shown).
    pub fn add_page(&mut self, ui: &mut Ui, tab: &TabId, page: WebView2Page) {
        if self.panes.contains_key(tab) {
            return;
        }
        // The shell may show a tab before its page exists: then the pane
        // comes up shown.
        let shown = self.shown.as_ref() == Some(tab);
        let pane = ui.tree.insert(
            self.stage,
            0,
            Node::stack().absolute(Abs::FILL).hidden(!shown),
        );
        let slot = ui
            .tree
            .add(pane, Node::slot().absolute(Abs::FILL).name("Page"));
        // `Stage.swift`'s `Trouble`: the sentence, centred on the page's
        // ground, and a quiet button under it. (Linux adds a title line;
        // the shell's title is the sentence here, its detail a second line.)
        let status = ui.tree.add(
            pane,
            Node::column()
                .absolute(Abs::FILL)
                .fill(Token::Ground)
                .align(Align::Center)
                .justify(Justify::Center)
                .padding(Insets::all(32.0))
                .gap(10.0)
                .layer()
                .hidden(true)
                .role(Role::Group),
        );
        let title = ui.tree.add(
            status,
            Node::text("", 14.0)
                .ink(Token::Ink)
                .text_align(TextAlign::Center)
                .max_width(520.0),
        );
        let detail = ui.tree.add(
            status,
            Node::text("", 12.5)
                .trim(Trim::Wrap)
                .ink(Token::Muted)
                .text_align(TextAlign::Center)
                .max_width(520.0),
        );
        // `.buttonStyle(.plain)`: the words alone, no plate.
        let button = ui.tree.add(
            status,
            Node::row()
                .height(22.0)
                .padding(Insets::hv(6.0, 0.0))
                .radius(6.0)
                .align(Align::Center)
                .button("Try again"),
        );
        let button_text = ui.tree.add(
            button,
            Node::text("Try again", 12.0)
                .ink(Token::Muted)
                .role(Role::None),
        );
        ui.set_slot(slot, Box::new(PageHost::new(page.clone())));
        let p = Pane {
            page,
            lent: false,
            pane,
            slot,
            status,
            title,
            detail,
            button,
            button_text,
        };
        if let Some(s) = self.statuses.get(tab) {
            fill_status(ui, &p, Some(s));
        }
        if self.shown.as_ref() == Some(tab) {
            ui.tree.set_hidden(pane, false);
        }
        self.panes.insert(tab.clone(), p);
    }

    /// The page went away: its pane with it.
    pub fn remove_page(&mut self, ui: &mut Ui, tab: &TabId) {
        self.statuses.remove(tab);
        if let Some(p) = self.panes.remove(tab) {
            ui.remove_slot(p.slot);
            ui.tree.remove(p.pane);
        }
    }

    /// Show `tab`'s pane (`None`: an empty stage). A page lent to the
    /// backstage comes onto the pane while it is shown and goes back when
    /// it is not (`copper_engine::Backstage::hand_off`).
    pub fn show(&mut self, ui: &mut Ui, tab: Option<&TabId>, backstage: &WebView2Backstage) {
        if self.shown.as_ref() == tab {
            return;
        }
        if let Some(old) = self.shown.take()
            && let Some(p) = self.panes.get(&old)
        {
            ui.tree.set_hidden(p.pane, true);
            if p.lent {
                // Detached, the page goes back to the backstage.
                ui.remove_slot(p.slot);
            }
        }
        if let Some(t) = tab
            && let Some(p) = self.panes.get(t)
        {
            if p.lent {
                backstage.hand_off(&p.page);
                ui.set_slot(p.slot, Box::new(PageHost::new(p.page.clone())));
            }
            ui.tree.set_hidden(p.pane, false);
        }
        self.shown = tab.cloned();
        self.set_hover(ui, None);
    }

    /// Take `tab`'s page off its (hidden) pane so the backstage can hold
    /// it. `false` if there is no such pane; a shown page stays where it
    /// is.
    pub fn lend(&mut self, ui: &mut Ui, tab: &TabId) -> bool {
        let shown = self.shown.as_ref() == Some(tab);
        let Some(p) = self.panes.get_mut(tab) else {
            return false;
        };
        if !p.lent && !shown {
            ui.remove_slot(p.slot);
        }
        p.lent = true;
        true
    }

    /// The backstage let `tab`'s page go: back onto its pane.
    pub fn take_back(&mut self, ui: &mut Ui, tab: &TabId) {
        let shown = self.shown.as_ref() == Some(tab);
        let Some(p) = self.panes.get_mut(tab) else {
            return;
        };
        if !p.lent {
            return;
        }
        p.lent = false;
        if !shown {
            ui.set_slot(p.slot, Box::new(PageHost::new(p.page.clone())));
        }
    }

    /// Show (or clear) a tab's status over its page.
    pub fn set_status(&mut self, ui: &mut Ui, tab: &TabId, status: Option<PageStatus>) {
        if let Some(p) = self.panes.get(tab) {
            fill_status(ui, p, status.as_ref());
        }
        match status {
            Some(s) => {
                self.statuses.insert(tab.clone(), s);
            }
            None => {
                self.statuses.remove(tab);
            }
        }
    }

    /// The line along the top: how far the shown page has come (a Linux /
    /// Windows addition). It starts at 0.08, runs to the end when the load
    /// is done and goes 220 ms later — unless another load began.
    /// Returns a ticket to [`Stage::progress_done`] after that wait.
    pub fn set_progress(&mut self, ui: &mut Ui, loading: bool, fraction: f64) -> Option<u64> {
        let w = ui.tree.get(self.stage).map_or(0.0, |n| n.frame.w);
        let f = fraction.clamp(0.0, 1.0) as f32;
        let mut ticket = None;
        if loading {
            // A new load cancels the fade still waiting.
            self.progress_ticket += 1;
            if !self.loading {
                ui.tree.set_prop(self.progress, Prop::X, -w);
                ui.tree.set_prop(self.progress, Prop::Opacity, 1.0);
            }
            // A little shows at once, so a slow start still says "coming".
            let shown = f.clamp(0.08, 1.0);
            ui.tree
                .animate(self.progress, Prop::X, -(1.0 - shown) * w, motion::QUICK);
        } else if self.loading {
            self.progress_ticket += 1;
            ui.tree.animate(self.progress, Prop::X, 0.0, motion::QUICK);
            ticket = Some(self.progress_ticket);
        }
        self.loading = loading;
        ticket
    }

    /// The wait after a finished load is over: the line goes, if no load
    /// began since.
    pub fn progress_done(&mut self, ui: &mut Ui, ticket: u64) {
        if ticket == self.progress_ticket && !self.loading {
            ui.tree
                .animate(self.progress, Prop::Opacity, 0.0, motion::QUICK);
        }
    }

    /// The hovered link's address at the foot of the page.
    pub fn set_hover(&mut self, ui: &mut Ui, url: Option<&str>) {
        match url.filter(|u| !u.is_empty()) {
            Some(u) => {
                ui.tree.set_text(self.hover_text, u);
                ui.tree.set_hidden(self.hover, false);
            }
            None => ui.tree.set_hidden(self.hover, true),
        }
    }
}

fn fill_status(ui: &mut Ui, p: &Pane, s: Option<&PageStatus>) {
    match s {
        Some(s) => {
            ui.tree.set_text(p.title, &s.title);
            ui.tree.set_text(p.detail, &s.description);
            ui.tree.set_hidden(p.detail, s.description.is_empty());
            let label = if s.button.is_empty() {
                "Try again"
            } else {
                &s.button
            };
            ui.tree.set_text(p.button_text, label);
            let label = label.to_string();
            ui.tree.edit(p.button, |n| n.a11y.name = label);
            ui.tree.set_hidden(p.button, false);
            if ui.tree.get(p.status).is_some_and(|n| n.style.hidden) {
                // In with `Motion.quick`.
                ui.tree.set_prop(p.status, Prop::Opacity, 0.0);
                ui.tree.animate(p.status, Prop::Opacity, 1.0, motion::QUICK);
            }
            ui.tree.set_hidden(p.status, false);
            ui.tree.set_hidden(p.slot, true);
        }
        None => {
            ui.tree.set_hidden(p.status, true);
            ui.tree.set_hidden(p.slot, false);
        }
    }
}
