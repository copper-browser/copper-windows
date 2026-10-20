//! History (`Recall.swift` `HistoryPanel`), the worked example of a panel:
//! search everywhere you have been, grouped by day, newest first; a click
//! goes there in the selected tab, Remove (on hover, in the time's place)
//! forgets one; the foot counts the pages and offers to clear — history,
//! sign-ins or the cache, each said in words nobody has to guess at.
//!
//! Every match is listed (the Mac has no cap). The first screenful is
//! drawn at once, so the plate (or the answer to a key) is in the very next
//! frame; the rest follow a chunk per frame, each after the last one is on
//! screen, so a search with thousands of matches never holds a frame up.

use super::plate::{self, Hunt, Plate};
use super::{Panel, when};
use crate::chrome::glyph;
use crate::platform::{Icon, Shell};
use crate::sys::calendar::WinCalendar;
use copper_core::history::Trace;
use copper_engine::{Engine, Profile, WebsiteDataKinds};
use copper_shell_core::format::Calendar;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Event, Insets, Justify, Node, NodeId, Role, Token, Trim};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// Items (rows and day headings) drawn at once: a little more than the
/// 420-high list shows. The rest come [`CHUNK`] a frame.
const FIRST: usize = 16;
const CHUNK: usize = 60;
/// The longest a chunk waits for the frame before it (a window that is
/// not drawing — minimised — still gets its list).
const FRAME_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

/// What a click in the panel means.
#[derive(Clone, Debug)]
enum Hit {
    Go(String),
    Forget(String),
    Clear,
    ClearSearch,
    Sweep(Sweep),
    Back,
}

#[derive(Clone, Copy, Debug)]
enum Sweep {
    History,
    Sites,
    Cache,
}

/// One thing in the list, in order.
enum Item {
    Day(String),
    Row(Trace, Option<Icon>),
}

#[derive(Default, Clone, Copy)]
struct Nodes {
    hunt: Option<Hunt>,
    list: Option<NodeId>,
    sweeps: Option<NodeId>,
    tally: Option<NodeId>,
    count: Option<NodeId>,
}

/// The History panel.
#[derive(Default)]
pub struct History(Rc<Inner>);

#[derive(Default)]
struct Inner {
    nodes: Cell<Nodes>,
    hits: RefCell<HashMap<NodeId, Hit>>,
    clearing: Cell<bool>,
    /// Bumped by every refresh: a chunk of an older one is dropped.
    drawing: Cell<u64>,
    /// The day card rows are going into.
    card: Cell<Option<NodeId>>,
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
        let hunt = plate::search(ui, p.body, "Search everywhere you have been");
        let list = ui.tree.add(
            p.body,
            Node::scroll()
                .max_height(420.0)
                .gap(14.0)
                .padding(Insets::new(0.0, 0.0, 0.0, 2.0))
                .shrink(1.0),
        );
        let mut hits = self.0.hits.borrow_mut();
        hits.clear();
        hits.insert(hunt.clear, Hit::ClearSearch);
        // The foot: the count and Clear…, or — clearing — the three sweeps
        // in its place (`Recall.swift` `sweeps`).
        let tally = ui
            .tree
            .add(p.foot, Node::row().grow(1.0).align(Align::Center));
        let count = ui
            .tree
            .add(tally, Node::text("", 12.0).ink(Token::Muted).grow(1.0));
        let clear = plate::pill(ui, tally, "Clear…");
        hits.insert(clear, Hit::Clear);
        let sweeps = ui
            .tree
            .add(p.foot, Node::column().gap(10.0).grow(1.0).hidden(true));
        let card = plate::card(ui, sweeps);
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
        let back_row = ui.tree.add(sweeps, Node::row().justify(Justify::End));
        let back = plate::pill(ui, back_row, "Back");
        hits.insert(back, Hit::Back);
        p.show_foot(ui, true);
        ui.focus(Some(hunt.field));
        self.0.clearing.set(false);
        self.0.nodes.set(Nodes {
            hunt: Some(hunt),
            list: Some(list),
            sweeps: Some(sweeps),
            tally: Some(tally),
            count: Some(count),
        });
    }

    fn refresh(&self, shell: &Shell) {
        let n = self.0.nodes.get();
        let (Some(hunt), Some(list), Some(sweeps), Some(tally), Some(count)) =
            (n.hunt, n.list, n.sweeps, n.tally, n.count)
        else {
            return;
        };
        let chrome = shell.view().chrome().clone();
        let typed = chrome
            .read(move |ui, _| ui.field_text(hunt.field).map(str::to_string))
            .flatten()
            .unwrap_or_default();
        let started = std::time::Instant::now();
        // Newest first already (`History::search`).
        let traces = shell.history().search(&typed);
        let total = traces.len();
        let empty = if typed.is_empty() {
            "Nothing yet."
        } else {
            "Nothing matches."
        };
        let items = Rc::new(RefCell::new(items(traces)));
        let clearing = self.0.clearing.get();
        let drawn = chrome.read(move |ui, _| {
            hunt.typed(ui, &typed);
            ui.tree.clear_children(list);
            ui.tree.set_hidden(tally, clearing);
            ui.tree.set_hidden(sweeps, !clearing);
            ui.tree.set_text(
                count,
                &match total {
                    1 => "1 page".to_string(),
                    n => format!("{n} pages"),
                },
            );
            if total == 0 {
                let card = plate::card(ui, list);
                plate::nothing(ui, card, empty);
            }
        });
        if drawn.is_none() {
            // The window was busy: once more, next turn.
            super::refresh_soon(shell, "history");
            return;
        }
        self.0
            .hits
            .borrow_mut()
            .retain(|_, v| !matches!(v, Hit::Go(_) | Hit::Forget(_)));
        self.0.card.set(None);
        let run = self.0.drawing.get() + 1;
        self.0.drawing.set(run);
        self.0.draw(shell, &items, 0, FIRST);
        tracing::debug!(
            target: "copper::perf",
            "history refresh: {:.2} ms ({total} pages)",
            started.elapsed().as_secs_f64() * 1000.0
        );
        if items.borrow().len() > FIRST {
            let weak = shell.downgrade();
            let me = Rc::downgrade(&self.0);
            copper_win_rt::spawn_local(async move {
                let mut from = FIRST;
                loop {
                    after_frame(&weak).await;
                    let Some(s) = weak.upgrade() else { return };
                    let Some(me) = me.upgrade() else { return };
                    if me.drawing.get() != run || from >= items.borrow().len() {
                        return;
                    }
                    me.draw(&s, &items, from, from + CHUNK);
                    from += CHUNK;
                }
            });
        }
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        let Some(hit) = self.0.hits.borrow().get(&id).cloned() else {
            return false;
        };
        match hit {
            Hit::Go(url) => {
                shell.view().panels().hide(shell);
                super::visit(shell, &url, false);
            }
            Hit::Forget(key) => {
                if shell.history_mut().forget(&key) {
                    shell.history_changed();
                    copper_shell_core::cloud::history_forgot(&key);
                }
                self.refresh(shell);
            }
            Hit::ClearSearch => {
                if let Some(h) = self.0.nodes.get().hunt {
                    shell.view().chrome().read(move |ui, _| h.clear(ui));
                }
                self.refresh(shell);
            }
            Hit::Clear => {
                self.0.clearing.set(true);
                self.refresh(shell);
            }
            Hit::Back => {
                self.0.clearing.set(false);
                self.refresh(shell);
            }
            Hit::Sweep(s) => {
                sweep(shell, s);
                if let Sweep::History = s {
                    self.0.clearing.set(false);
                }
                self.refresh(shell);
            }
        }
        true
    }

    fn field(&self, shell: &Shell, id: NodeId, e: &Event) -> bool {
        if self.0.nodes.get().hunt.map(|h| h.field) != Some(id) {
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
        self.0.nodes.set(Nodes::default());
        self.0.hits.borrow_mut().clear();
        self.0.drawing.set(self.0.drawing.get() + 1);
        self.0.card.set(None);
    }
}

impl Inner {
    /// Draw `items[from..to]` (each row's icon looked up just before).
    fn draw(&self, shell: &Shell, items: &Rc<RefCell<Vec<Item>>>, from: usize, to: usize) {
        let Some(list) = self.nodes.get().list else {
            return;
        };
        {
            let mut all = items.borrow_mut();
            let end = to.min(all.len());
            for item in &mut all[from.min(end)..end] {
                if let Item::Row(t, icon) = item {
                    *icon = shell.cached_icon(&t.url);
                }
            }
        }
        let items = items.clone();
        let mut card = self.card.get();
        let made = shell.view().chrome().read(move |ui, st| {
            let mut made: Vec<(NodeId, Hit)> = Vec::new();
            let all = items.borrow();
            let end = to.min(all.len());
            for item in &all[from.min(end)..end] {
                match item {
                    Item::Day(name) => {
                        let group = ui.tree.add(list, Node::column().gap(6.0));
                        plate::heading(ui, group, name);
                        card = Some(plate::card(ui, group));
                    }
                    Item::Row(t, icon) => {
                        let Some(c) = card else { continue };
                        if !ui.tree.children(c).is_empty() {
                            plate::rule(ui, c);
                        }
                        let icon = icon.as_ref().map(|i| st.images.id(ui, i));
                        row(ui, c, t, icon, &mut made);
                    }
                }
            }
            (made, card)
        });
        if let Some((made, card)) = made {
            self.card.set(card);
            self.hits.borrow_mut().extend(made);
        }
    }
}

/// Once the window has drawn what is waiting (nothing left to lay out),
/// or after [`FRAME_WAIT`].
async fn after_frame(shell: &copper_shell_core::shell::WeakShell<crate::platform::Win>) {
    let since = std::time::Instant::now();
    loop {
        copper_win_rt::sleep(std::time::Duration::from_millis(4)).await;
        let Some(s) = shell.upgrade() else { return };
        let waiting = s
            .view()
            .chrome()
            .read(|ui, _| ui.tree.needs_layout())
            .unwrap_or(true);
        if !waiting || since.elapsed() >= FRAME_WAIT {
            return;
        }
    }
}

/// The list in order: each day's heading, then its rows, newest first
/// (`traces` is sorted already). The local day is asked of Windows only
/// near a day's start ([`when::DaySpan`]), not once per page.
fn items(traces: Vec<Trace>) -> Vec<Item> {
    let now = copper_core::date::now_unix();
    let mut out = Vec::with_capacity(traces.len() + 8);
    let mut span: Option<when::DaySpan> = None;
    for t in traces {
        let unix = t.last.unix();
        if span.as_ref().is_some_and(|s| s.surely_holds(unix)) {
            out.push(Item::Row(t, None));
            continue;
        }
        let civil = WinCalendar::civil(unix);
        let next = when::DaySpan::of(unix, civil.as_ref());
        if span.as_ref().map(|s| s.day) != Some(next.day) {
            out.push(Item::Day(when::day::<WinCalendar>(unix, now)));
        }
        span = Some(next);
        out.push(Item::Row(t, None));
    }
    out
}

/// One line: the site's mark, the title over the address, and the time —
/// or, under the pointer, Remove.
fn row(ui: &mut Ui, card: NodeId, t: &Trace, icon: Option<u32>, hits: &mut Vec<(NodeId, Hit)>) {
    let title = if t.title.is_empty() { &t.key } else { &t.title };
    let r = ui.tree.add(
        card,
        Node::row()
            .padding(Insets::hv(14.0, 9.0))
            .gap(12.0)
            .align(Align::Center)
            .hover_fill(Token::Hover)
            .button(title),
    );
    hits.push((r, Hit::Go(t.url.clone())));
    plate::mark(ui, r, icon, &plate::letter(&t.key), 16.0);
    let words = ui
        .tree
        .add(r, Node::column().gap(2.0).grow(1.0).shrink(1.0));
    ui.tree.add(
        words,
        Node::text(title, 13.0)
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
    // The time, and over it (opaque, shown with the row's hover) Remove.
    let end = ui.tree.add(
        r,
        Node::stack()
            .shrink(0.0)
            .min_width(8.0)
            .align(Align::Center)
            .justify(Justify::End),
    );
    ui.tree.add(
        end,
        Node::text(&clock(t.last.unix()), 11.5)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let forget = plate::quick(ui, end, "Remove", plate::RED, true);
    hits.push((forget, Hit::Forget(t.key.clone())));
}

/// The time of day in the user's short style (`When.clock`: `3:41 PM`,
/// `15:41`).
fn clock(unix: f64) -> String {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
    use windows::core::PCWSTR;
    let Some(t) = WinCalendar::civil(unix) else {
        return String::new();
    };
    let time = SYSTEMTIME {
        wYear: t.year.clamp(1601, 30827) as u16,
        wMonth: t.month as u16,
        wDay: t.day as u16,
        wHour: t.hour as u16,
        wMinute: t.minute as u16,
        ..Default::default()
    };
    let mut buf = [0u16; 32];
    // SAFETY: the user's locale, a valid time, a buffer of its length.
    let n = unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            TIME_NOSECONDS,
            Some(&time),
            PCWSTR::null(),
            Some(&mut buf),
        )
    };
    if n > 1 {
        String::from_utf16_lossy(&buf[..n as usize - 1])
    } else {
        format!("{:02}:{:02}", t.hour, t.minute)
    }
}

/// Clear history (here and on Copper Cloud), every site's cookies and
/// storage, or the cache — each in every profile's jar.
fn sweep(shell: &Shell, s: Sweep) {
    let (kinds, said) = match s {
        Sweep::History => {
            shell.history_mut().clear();
            shell.history_changed();
            copper_shell_core::cloud::history_cleared();
            shell.toast("History cleared");
            return;
        }
        Sweep::Sites => (WebsiteDataKinds::ALL, "Signed out of everything"),
        Sweep::Cache => (
            WebsiteDataKinds::CACHES | WebsiteDataKinds::OFFLINE_APP_CACHE,
            "Cache cleared",
        ),
    };
    let mut names: Vec<Option<String>> = vec![None];
    names.extend(shell.browser().profiles().into_iter().map(Some));
    let engine = shell.engine().clone();
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let mut failed = false;
        for name in names {
            let id = copper_shell_core::shell::pages::profile_id(name.as_deref());
            if let Ok(p) = engine.profile(&id, false)
                && let Err(e) = p.clear_website_data(None, kinds).await
            {
                tracing::warn!("clearing {}: {e}", id.as_str());
                failed = true;
            }
        }
        if let Some(s) = weak.upgrade() {
            s.toast(if failed {
                "Some of it couldn’t be cleared"
            } else {
                said
            });
        }
    });
}
