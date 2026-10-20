//! Downloads (`Recall.swift` `DownloadsPanel`, `Fork/DownloadsUI.swift`):
//! what is arriving now, then this session's finished and failed files,
//! then the files kept from earlier sessions — in two places that share
//! one row: the full list (`Ctrl+Shift+J`, a sheet) and the short list
//! that hangs from the download door (eight rows at most).
//!
//! A file opens with a click (only if it is still there); under the
//! pointer a row offers Show in Explorer and Remove from list. A running
//! download can be cancelled, a failed one retried. Nothing here ever
//! deletes a file: clearing the list only forgets rows.
//!
//! Progress events redraw only the figures and bars of the rows already
//! drawn; the rows are built again only when what is listed changes.

use super::plate::{self, Plate};
use super::{Panel, Place, files, when};
use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::{Icon, Shell};
use copper_core::downloads::{DownloadId, DownloadState, Keep, LiveDownload};
use copper_shell_core::format;
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Align, Insets, Justify, Node, NodeId, Prop, Role, Swatch, Token, Trim, Weight,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

/// The short list's id (its `show-` command is not in the palette).
pub const LIST: &str = "downloads-list";

/// The short list shows at most this many rows.
const SHORT: usize = 8;
/// How long the unknown-size shimmer takes to cross.
const SHIMMER: f32 = 1.2;
/// The shimmer's width, of the bar's.
const SHIMMER_PART: f32 = 0.28;

/// Register both lists, the folder command and the door.
pub fn register(shell: &Shell) {
    let panels = shell.view().panels();
    panels.add(shell, Rc::new(Downloads::new(false)));
    panels.add(shell, Rc::new(Downloads::new(true)));
    shell.view().commands().add(Command::new(
        "open-downloads-folder",
        "Open Downloads Folder",
        glyph::FOLDER,
        open_folder,
    ));
    let weak = shell.downgrade();
    shell.downloads().watch(move || {
        if let Some(s) = weak.upgrade() {
            super::refresh_soon(&s, "downloads");
            super::refresh_soon(&s, LIST);
        }
    });
    super::door::mount(shell);
}

/// Open the folder downloads go to.
fn open_folder(shell: &Shell) {
    let dir = shell.downloads().folder(shell);
    files::open(&dir.to_string_lossy());
}

/// Which row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Live(DownloadId),
    Kept(String),
}

/// What a row is doing (what it draws besides its words).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Running(Option<f32>),
    Finished,
    Failed,
    Kept,
}

impl Kind {
    /// The same kind of row, figures aside.
    fn same(self, o: Kind) -> bool {
        match (self, o) {
            (Kind::Running(a), Kind::Running(b)) => a.is_some() == b.is_some(),
            _ => self == o,
        }
    }
}

/// One row's facts, read from the downloads.
#[derive(Clone, Debug)]
struct Row {
    key: Key,
    name: String,
    detail: String,
    kind: Kind,
    path: Option<String>,
    exists: bool,
}

/// What a click means.
#[derive(Clone, Debug)]
enum Hit {
    Open(String),
    Cancel(DownloadId),
    Retry(DownloadId),
    Reveal(String),
    Remove(DownloadId),
    Forget(String),
    Folder,
    ClearList,
    ClearSession,
    ShowAll,
}

/// The nodes of one drawn row that change while it runs.
#[derive(Clone, Copy, Debug)]
struct Drawn {
    detail: NodeId,
    /// The bar's track, filled part and rest (known size), or the track
    /// and its sliding shimmer (unknown).
    bar: Option<(NodeId, NodeId, NodeId)>,
}

#[derive(Default, Clone, Copy)]
struct Nodes {
    list: Option<NodeId>,
    /// The manager's foot words and Clear list pill; the short list's
    /// Clear.
    words: Option<NodeId>,
    clear: Option<NodeId>,
}

/// The full list (`short == false`) or the door's short list.
pub struct Downloads {
    short: bool,
    nodes: Cell<Nodes>,
    hits: RefCell<HashMap<NodeId, Hit>>,
    /// What is drawn, in order, with its changing nodes.
    drawn: RefCell<Vec<(Key, Kind, Drawn)>>,
    since: Cell<Option<Instant>>,
    /// Drawn while some icons were still being fetched: draw again.
    plain: Cell<bool>,
}

impl Downloads {
    fn new(short: bool) -> Downloads {
        Downloads {
            short,
            nodes: Cell::default(),
            hits: RefCell::default(),
            plain: Cell::new(false),
            drawn: RefCell::default(),
            since: Cell::new(None),
        }
    }
}

impl Panel for Downloads {
    fn id(&self) -> &'static str {
        if self.short { LIST } else { "downloads" }
    }

    fn title(&self) -> String {
        "Downloads".into()
    }

    fn name(&self) -> String {
        if self.short {
            String::new()
        } else {
            self.title()
        }
    }

    fn glyph(&self) -> char {
        glyph::DOWNLOAD
    }

    fn width(&self) -> f32 {
        if self.short { 340.0 } else { 560.0 }
    }

    fn place(&self) -> Place {
        if self.short {
            Place::Door {
                radius: 13.0,
                tall: 50.0 * SHORT as f32 + 42.0,
            }
        } else {
            Place::Sheet
        }
    }

    fn build(&self, ui: &mut Ui, p: &Plate) {
        let mut hits = self.hits.borrow_mut();
        hits.clear();
        self.drawn.borrow_mut().clear();
        self.since.set(Some(Instant::now()));
        if self.short {
            let list = ui.tree.add(p.body, Node::column().shrink(1.0));
            plate::hairline(ui, p.body);
            let foot = ui.tree.add(
                p.body,
                Node::row()
                    .padding(Insets::hv(14.0, 11.0))
                    .gap(10.0)
                    .align(Align::Center),
            );
            let folder = text_button(ui, foot, "Open Downloads Folder", Token::Ink);
            ui.tree.add(foot, Node::new().grow(1.0));
            let clear = text_button(ui, foot, "Clear", Token::Muted);
            let all = text_button(ui, foot, "Show All…", Token::Muted);
            hits.insert(folder, Hit::Folder);
            hits.insert(clear, Hit::ClearSession);
            hits.insert(all, Hit::ShowAll);
            self.nodes.set(Nodes {
                list: Some(list),
                words: None,
                clear: Some(clear),
            });
        } else {
            let list = ui.tree.add(p.body, Node::column().shrink(1.0));
            let words = ui
                .tree
                .add(p.foot, Node::text("", 12.0).ink(Token::Muted).grow(1.0));
            let folder = text_button(ui, p.foot, "Open Folder", Token::Muted);
            let clear = plate::pill(ui, p.foot, "Clear list");
            hits.insert(folder, Hit::Folder);
            hits.insert(clear, Hit::ClearList);
            p.show_foot(ui, true);
            self.nodes.set(Nodes {
                list: Some(list),
                words: Some(words),
                clear: Some(clear),
            });
        }
    }

    fn refresh(&self, shell: &Shell) {
        let Nodes { list, words, clear } = self.nodes.get();
        let (Some(list), Some(clear)) = (list, clear) else {
            return;
        };
        if self.short && self.since.get().is_some() {
            // The door's list was looked at: nothing is new any more.
            shell.downloads().seen();
            super::door::changed(shell);
        }
        let (session, older, any_kept, finished) = read(shell);
        let mut rows: Vec<Row> = session;
        rows.extend(older);
        if self.short {
            rows.truncate(SHORT);
        }
        let empty = rows.is_empty();
        // Only the figures moved: patch them in place.
        let same = {
            let d = self.drawn.borrow();
            !self.plain.get()
                && !d.is_empty()
                && d.len() == rows.len()
                && d.iter()
                    .zip(&rows)
                    .all(|((k, kind, _), r)| *k == r.key && kind.same(r.kind))
        };
        if same {
            let drawn: Vec<Drawn> = self.drawn.borrow().iter().map(|(_, _, d)| *d).collect();
            let rows2 = rows.clone();
            shell.view().chrome().edit(move |ui, _| {
                for (d, r) in drawn.iter().zip(&rows2) {
                    ui.tree.set_text(d.detail, &r.detail);
                    if let (Some((_, fill, rest)), Kind::Running(Some(f))) = (d.bar, r.kind) {
                        ui.tree.edit(fill, |n| n.style.grow = f);
                        ui.tree.edit(rest, |n| n.style.grow = 1.0 - f);
                    }
                }
            });
            let mut d = self.drawn.borrow_mut();
            for ((_, kind, _), r) in d.iter_mut().zip(&rows) {
                *kind = r.kind;
            }
            return;
        }
        // Icons the shell has not given yet come on a thread of their own
        // (the first look at a kind of file costs tens of ms); the rows
        // wear the plain document until then.
        let at_hand: Vec<Option<Option<Icon>>> = rows
            .iter()
            .map(|r| files::cached(r.path.as_deref(), &r.name, r.exists))
            .collect();
        let missing: Vec<(Option<String>, String, bool)> = rows
            .iter()
            .zip(&at_hand)
            .filter(|(_, i)| i.is_none())
            .map(|(r, _)| (r.path.clone(), r.name.clone(), r.exists))
            .collect();
        self.plain.set(!missing.is_empty());
        if !missing.is_empty() {
            let weak = shell.downgrade();
            files::warm(missing, move || {
                if let Some(s) = weak.upgrade() {
                    super::refresh_soon(&s, "downloads");
                    super::refresh_soon(&s, LIST);
                }
            });
        }
        let icons: Vec<Option<Icon>> = at_hand.into_iter().map(Option::flatten).collect();
        let short = self.short;
        let folder = folder_name(&shell.downloads().folder(shell));
        let made = shell.view().chrome().read(move |ui, st| {
            ui.tree.clear_children(list);
            let mut hits: Vec<(NodeId, Hit)> = Vec::new();
            let mut drawn: Vec<(Key, Kind, Drawn)> = Vec::new();
            if let Some(words) = words {
                ui.tree.set_text(
                    words,
                    &if empty && !any_kept {
                        format!("Files land in {folder}")
                    } else {
                        "Clearing the list leaves the files where they are".to_string()
                    },
                );
            }
            ui.tree
                .set_hidden(clear, if short { !finished } else { empty && !any_kept });
            if empty {
                if short {
                    ui.tree.add(
                        list,
                        Node::text("Nothing downloaded yet.", 13.0)
                            .ink(Token::Muted)
                            .padding(Insets::all(18.0)),
                    );
                } else {
                    let card = plate::card(ui, list);
                    plate::nothing(ui, card, "Nothing downloaded yet.");
                }
                return (hits, drawn);
            }
            let into = if short {
                list
            } else {
                let scroll = ui.tree.add(
                    list,
                    Node::scroll()
                        .max_height(420.0)
                        .padding(Insets::new(0.0, 0.0, 0.0, 2.0))
                        .shrink(1.0),
                );
                plate::card(ui, scroll)
            };
            for (i, (r, icon)) in rows.iter().zip(&icons).enumerate() {
                if i > 0 {
                    if short {
                        plate::rule_inset(ui, into, 48.0);
                    } else {
                        plate::rule(ui, into);
                    }
                }
                let picture = icon.as_ref().map(|i| st.images.id(ui, i));
                let d = row(ui, into, r, picture, &mut hits);
                drawn.push((r.key.clone(), r.kind, d));
            }
            (hits, drawn)
        });
        let Some((made, drawn)) = made else {
            super::refresh_soon(shell, self.id());
            return;
        };
        let mut h = self.hits.borrow_mut();
        h.retain(|_, v| {
            matches!(
                v,
                Hit::Folder | Hit::ClearList | Hit::ClearSession | Hit::ShowAll
            )
        });
        h.extend(made);
        *self.drawn.borrow_mut() = drawn;
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        let Some(hit) = self.hits.borrow().get(&id).cloned() else {
            return false;
        };
        let d = shell.downloads();
        match hit {
            Hit::Open(path) => {
                if Path::new(&path).exists() {
                    files::open(&path);
                }
            }
            Hit::Cancel(row) => {
                tracing::debug!("downloads: cancel {row:?}");
                d.cancel(row);
            }
            Hit::Retry(row) => {
                let again = d.retry(shell, row);
                tracing::debug!("downloads: retry {row:?} → {again}");
            }
            Hit::Reveal(path) => files::reveal(&path),
            Hit::Remove(row) => d.remove(row),
            Hit::Forget(path) => d.forget(shell, &path),
            Hit::Folder => open_folder(shell),
            Hit::ClearList => d.clear(shell),
            Hit::ClearSession => {
                // Popover Clear: this session's finished rows; the files
                // kept from before stay listed.
                let done: Vec<DownloadId> = d
                    .model(shell)
                    .live()
                    .iter()
                    .filter(|l| l.state != DownloadState::Active)
                    .map(|l| l.id)
                    .collect();
                for row in done {
                    d.remove(row);
                }
            }
            Hit::ShowAll => super::show(shell, "downloads"),
        }
        true
    }

    fn ticks(&self) -> bool {
        true
    }

    /// Slide the shimmer of every download of unknown size.
    fn tick(&self, shell: &Shell) {
        let bars: Vec<(NodeId, NodeId)> = self
            .drawn
            .borrow()
            .iter()
            .filter(|(_, k, _)| *k == Kind::Running(None))
            .filter_map(|(_, _, d)| d.bar.map(|(track, shimmer, _)| (track, shimmer)))
            .collect();
        if bars.is_empty() {
            return;
        }
        let t = self
            .since
            .get()
            .map_or(0.0, |s| s.elapsed().as_secs_f32() / SHIMMER)
            .fract();
        shell.view().chrome().edit(move |ui, _| {
            if ui.tree.reduce_motion {
                return;
            }
            for (track, shimmer) in &bars {
                let w = ui.tree.window_rect(*track).w;
                let part = w * SHIMMER_PART;
                ui.tree.set_prop(*shimmer, Prop::Width, part);
                ui.tree.set_prop(*shimmer, Prop::X, -part + t * (w + part));
            }
        });
    }

    fn closed(&self) {
        self.nodes.set(Nodes::default());
        self.hits.borrow_mut().clear();
        self.drawn.borrow_mut().clear();
        self.since.set(None);
    }

    fn closing(&self, shell: &Shell) {
        if self.short {
            // The door stops looking pressed.
            let weak = shell.downgrade();
            copper_win_rt::spawn_local(async move {
                if let Some(s) = weak.upgrade() {
                    super::door::changed(&s);
                }
            });
        }
    }
}

/// The rows from the downloads: this session's (running first, then the
/// rest newest first; cancelled ones not at all), the files kept from
/// before that are not among them; whether any file is kept; whether a
/// session row is over (the short list's Clear).
fn read(shell: &Shell) -> (Vec<Row>, Vec<Row>, bool, bool) {
    let now = copper_core::date::now_unix();
    let m = shell.downloads().model(shell);
    let live: Vec<&LiveDownload> = m
        .live()
        .iter()
        .filter(|d| d.state != DownloadState::Cancelled)
        .collect();
    let finished = live.iter().any(|d| d.state != DownloadState::Active);
    let running = live.iter().filter(|d| d.state == DownloadState::Active);
    let rest = live.iter().filter(|d| d.state != DownloadState::Active);
    let session: Vec<Row> = running.chain(rest).map(|d| live_row(d, &m)).collect();
    let paths: Vec<&str> = live.iter().filter_map(|d| d.path.as_deref()).collect();
    let older: Vec<Row> = m
        .kept()
        .iter()
        .filter(|k| !paths.contains(&k.path.as_str()))
        .map(|k| kept_row(k, now))
        .collect();
    (session, older, !m.kept().is_empty(), finished)
}

fn live_row(d: &LiveDownload, m: &copper_core::downloads::Downloads) -> Row {
    let from = m
        .kept()
        .iter()
        .find(|k| Some(&k.path) == d.path.as_ref())
        .map(|k| k.from.clone())
        .unwrap_or_else(|| copper_shell_core::shell::host_of(&d.source_url));
    // A file's own address has no host to name: just "Downloaded".
    let from = if from.contains('/') {
        String::new()
    } else {
        from
    };
    let kind = match &d.state {
        DownloadState::Active => Kind::Running(d.fraction().map(|f| f as f32)),
        DownloadState::Failed { .. } => Kind::Failed,
        _ => Kind::Finished,
    };
    let exists = kind == Kind::Finished && d.path.as_ref().is_some_and(|p| Path::new(p).exists());
    Row {
        key: Key::Live(d.id),
        name: d.name.clone(),
        detail: format::download_line(d, &from),
        kind,
        path: d.path.clone(),
        exists,
    }
}

fn kept_row(k: &Keep, now: f64) -> Row {
    let said = when::said(k.date.unix(), now);
    Row {
        key: Key::Kept(k.path.clone()),
        name: k.name.clone(),
        detail: if k.from.is_empty() || k.from.contains('/') {
            said
        } else {
            format!("{} · {said}", k.from)
        },
        kind: Kind::Kept,
        path: Some(k.path.clone()),
        exists: k.still_there(),
    }
}

/// The last part of a folder's path (`Downloads`).
fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_string_lossy().into_owned())
}

/// A plain word button (`Button(...).buttonStyle(.plain)`).
fn text_button(ui: &mut Ui, parent: NodeId, label: &str, ink: Token) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::row()
            .shrink(0.0)
            .padding(Insets::hv(0.0, 2.0))
            .align(Align::Center)
            .button(label),
    );
    ui.tree
        .add(b, Node::text(label, 11.5).ink(ink).role(Role::None));
    b
}

/// One row (`DownloadRow`).
fn row(
    ui: &mut Ui,
    parent: NodeId,
    r: &Row,
    icon: Option<u32>,
    hits: &mut Vec<(NodeId, Hit)>,
) -> Drawn {
    let live = matches!(r.key, Key::Live(_));
    let n = ui.tree.add(
        parent,
        Node::row()
            .padding(Insets::hv(14.0, 9.0))
            .gap(10.0)
            .align(Align::Start)
            .hover_fill(Token::Hover)
            .button(&r.name),
    );
    if let Some(p) = r.path.as_ref().filter(|_| r.exists) {
        hits.push((n, Hit::Open(p.clone())));
    }
    // The file's icon, its kind's, or a plain page.
    match icon {
        Some(id) => {
            ui.tree.add(
                n,
                Node::image(id, 24.0)
                    .radius(5.0)
                    .clip()
                    .shrink(0.0)
                    .role(Role::None),
            );
        }
        None => {
            ui.tree.add(
                n,
                Node::glyph(glyph::DOC, 14.0, 24.0)
                    .ink(Token::Muted)
                    .shrink(0.0)
                    .role(Role::None),
            );
        }
    }
    let words = ui
        .tree
        .add(n, Node::column().gap(3.0).grow(1.0).shrink(1.0));
    ui.tree.add(
        words,
        Node::text(&r.name, 12.5)
            .ink(if r.exists || live {
                Token::Ink
            } else {
                Token::Faint
            })
            .trim(Trim::Middle)
            .role(Role::None),
    );
    let detail = ui.tree.add(
        words,
        Node::text(&r.detail, 11.0)
            .ink(Token::Muted)
            .trim(Trim::End)
            .role(Role::None),
    );
    let bar = match r.kind {
        Kind::Running(f) => Some(progress(ui, words, f)),
        _ => None,
    };
    ui.tree.add(n, Node::new().width(4.0).shrink(0.0));
    let actions = ui.tree.add(
        n,
        Node::row()
            .gap(2.0)
            .shrink(0.0)
            .align(Align::Center)
            .justify(Justify::End),
    );
    match (&r.key, r.kind) {
        (Key::Live(id), Kind::Running(_)) => {
            let b = icon_button(
                ui,
                actions,
                glyph::CLOSE,
                9.0,
                Weight::Semibold,
                "Cancel download",
                false,
            );
            hits.push((b, Hit::Cancel(*id)));
        }
        (Key::Live(id), kind) => {
            if kind == Kind::Failed {
                let b = ui.tree.add(
                    actions,
                    Node::row()
                        .padding(Insets::hv(7.0, 4.0))
                        .radius(11.0)
                        .fill(Token::Wash)
                        .press_fill(Token::Hairline)
                        .align(Align::Center)
                        .button("Retry"),
                );
                ui.tree.add(
                    b,
                    Node::text("Retry", 11.5).ink(Token::Ink).role(Role::None),
                );
                hits.push((b, Hit::Retry(*id)));
            }
            if let Some(p) = r.path.as_ref().filter(|_| r.exists) {
                let b = icon_button(
                    ui,
                    actions,
                    glyph::SEARCH,
                    10.0,
                    Weight::Medium,
                    "Show in Explorer",
                    true,
                );
                hits.push((b, Hit::Reveal(p.clone())));
            }
            let b = icon_button(
                ui,
                actions,
                glyph::CLOSE,
                10.0,
                Weight::Medium,
                "Remove from list",
                true,
            );
            hits.push((b, Hit::Remove(*id)));
        }
        (Key::Kept(path), _) => {
            if r.exists {
                let b = icon_button(
                    ui,
                    actions,
                    glyph::SEARCH,
                    10.0,
                    Weight::Medium,
                    "Show in Explorer",
                    true,
                );
                hits.push((b, Hit::Reveal(path.clone())));
            }
            let b = icon_button(
                ui,
                actions,
                glyph::CLOSE,
                10.0,
                Weight::Medium,
                "Remove from list",
                true,
            );
            hits.push((b, Hit::Forget(path.clone())));
        }
    }
    Drawn { detail, bar }
}

/// A 20×20 icon button (`FinderDoor`, the cancel cross); `reveal`: only
/// while its row is under the pointer.
fn icon_button(
    ui: &mut Ui,
    parent: NodeId,
    g: char,
    size: f32,
    weight: Weight,
    tip: &str,
    reveal: bool,
) -> NodeId {
    let mut n = Node::stack()
        .size(20.0, 20.0)
        .radius(6.0)
        .press_fill(Token::Hairline)
        .align(Align::Center)
        .justify(Justify::Center)
        .button(tip)
        .tooltip(tip);
    if reveal {
        n = n.reveal();
    }
    let b = ui.tree.add(parent, n);
    ui.tree.add(
        b,
        Node::glyph(g, size, 20.0)
            .weight(weight)
            .ink(Token::Muted)
            .role(Role::None),
    );
    b
}

/// The 2-high bar under a running row: filled to the fraction, or a
/// shimmer sliding across when the size is unknown.
fn progress(ui: &mut Ui, parent: NodeId, fraction: Option<f32>) -> (NodeId, NodeId, NodeId) {
    let track = ui.tree.add(
        parent,
        Node::row()
            .height(2.0)
            .fill_width()
            .radius(1.0)
            .fill(Token::Hairline)
            .clip()
            .layer(),
    );
    match fraction {
        Some(f) => {
            let fill = ui.tree.add(
                track,
                Node::new().height(2.0).grow(f).radius(1.0).fill(Token::Ink),
            );
            let rest = ui.tree.add(track, Node::new().height(2.0).grow(1.0 - f));
            (track, fill, rest)
        }
        None => {
            let shimmer = ui.tree.add(
                track,
                Node::new()
                    .height(2.0)
                    .width(60.0)
                    .radius(1.0)
                    .fill(Swatch::Fade(Token::Ink, 0.65))
                    .layer(),
            );
            (track, shimmer, shimmer)
        }
    }
}
