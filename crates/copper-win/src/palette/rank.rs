//! What the cards offer, the Mac's way, without a window: `Ctrl+K`'s rows
//! (`Fork/CommandBar.swift` `offers`), `Ctrl+T`'s (`Fork/Launcher.swift`
//! `rows`) and the plain address field's (`Browser.swift` `guess`), in the
//! card's sections (`Fork/CardSections.swift`), and how the keys move
//! through them.
//!
//! The shared ranking (`copper_shell_core::palette::complete`) is the
//! Linux card's: it has no Canvases section, caps the groups differently,
//! leaves commands out of an empty `Ctrl+K`, ranks commands by word prefix
//! and never completes in the palette (`specs/palette-spaces.md` §6.1–2).
//! This is the Mac's, written against plain inputs so it is tested here;
//! it belongs in `copper-shell-core` and should move there.
//!
//! Nothing here asks the network: the search row is an address until
//! Return is pressed.

use copper_core::address::{pretty_str, strip_typed};
use copper_core::browser::TabId;
use copper_core::history::{self, Suggestion, SuggestionKind};

/// Which card is asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `Ctrl+K`: everything, literal field.
    Palette,
    /// `Ctrl+T`: where a new tab should go, literal field, no tab yet.
    Launch,
    /// `Ctrl+L`: the address field, completing in place.
    Address,
}

/// The card's groups, in the order they are drawn
/// (`Fork/CardSections.swift` 18–31). `Lead` is the row under no heading
/// (`Ctrl+T`'s "Open github.com"); `Recent` and `Closed` are the empty
/// `Ctrl+T` card's two groups (`Fork/Launcher.swift` 280).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    Lead,
    Recent,
    Closed,
    Tabs,
    History,
    Bookmarks,
    Canvases,
    Commands,
    Search,
    /// The plain field's list: no headings.
    Plain,
}

impl Section {
    /// The quiet label over the group; `None` draws none.
    pub fn title(self) -> Option<&'static str> {
        Some(match self {
            Section::Lead | Section::Plain => return None,
            Section::Recent => "Recent Tabs",
            Section::Closed => "Recently Closed",
            Section::Tabs => "Open Tabs",
            Section::History => "History",
            Section::Bookmarks => "Bookmarks",
            Section::Canvases => "Canvases",
            Section::Commands => "Commands",
            Section::Search => "Search",
        })
    }
}

/// What a row is (`Suggestion.Kind`), for its mark and its hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A page open in a tab.
    Open,
    /// Somewhere you have been.
    Visited,
    /// The typed address itself (`Ctrl+T`'s "Open …").
    Known,
    Bookmark,
    Canvas,
    Command,
    /// Words, and an engine to ask.
    Search,
}

/// What taking a row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Select this tab (in its own space).
    Switch(TabId),
    /// Load this address.
    Go(String),
    /// Run this command.
    Command(String),
    /// Bring back a closed tab: the last one closed (`last`), or this
    /// address.
    Reopen { url: String, last: bool },
}

/// One row (`Suggestion` with the fork's `detail`, `badge`, `hint`).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: Kind,
    /// The row's name (`key`): a page's title, a command's name, the words.
    pub title: String,
    /// The quiet half: a host, kept whole.
    pub detail: String,
    /// What gives way last: "· Work", "· ✦", "· saved".
    pub badge: String,
    /// What Return does, on the right: "Switch to Tab", "Run".
    pub hint: String,
    pub action: Action,
    /// The page's address, for its mark and for the completion.
    pub url: Option<String>,
    /// A command's glyph (its registry glyph, as text).
    pub glyph: Option<char>,
    pub section: Section,
}

impl Row {
    /// Its identity across a re-rank (`Browser.relaunch` keeps the walked
    /// row by what it is, not where it was).
    pub fn identity(&self) -> String {
        match &self.action {
            Action::Switch(id) => format!("t:{}", id.0),
            Action::Go(u) => format!("g:{u}"),
            Action::Command(c) => format!("c:{c}"),
            Action::Reopen { url, last } => format!("r:{last}:{url}"),
        }
    }
}

/// The rows, the one highlighted to begin with, and the grey ending the
/// field may offer after what was typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Offers {
    pub rows: Vec<Row>,
    pub default: Option<usize>,
    pub ending: Option<String>,
}

/// An open tab as the cards see it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenTab {
    pub id: TabId,
    /// Its label (title, else its address).
    pub title: String,
    pub url: String,
    /// In this window's row (this space or a pin), not parked elsewhere.
    pub current: bool,
    pub pinned: bool,
    pub saved: bool,
    /// The space it is parked in (its title), for the badge.
    pub space: String,
    /// That space's emoji, if it wears one (`Ctrl+T`'s badge).
    pub space_emoji: Option<String>,
    /// When it was last looked at: higher is more recent.
    pub seen: f64,
}

/// A place from history, as `History::suggestions` ranks it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Visit {
    /// What you would have typed: no scheme, no `www.`.
    pub key: String,
    pub title: String,
    pub url: String,
}

/// A bookmark, flattened.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mark {
    pub title: String,
    pub url: String,
}

/// A canvas you can open, newest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Canvas {
    pub name: String,
    pub url: String,
    pub shared: bool,
    pub personal: bool,
}

/// The canvases a question names (`CanvasTabs.offers`): "canvas" (or
/// "canvases", or four letters of it) alone lists them all; otherwise the
/// rest of the words — two letters at the start of the name or a word of
/// it, three anywhere — and three at most.
pub fn canvas_offers<'a>(typed: &str, all: &'a [Canvas]) -> Vec<&'a Canvas> {
    let needle = typed.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let word = if needle.starts_with("canvases") {
        Some("canvases")
    } else if needle.starts_with("canvas") {
        Some("canvas")
    } else {
        None
    };
    let rest = word.map_or(needle.as_str(), |w| needle[w.len()..].trim());
    let everything = (word.is_some() && rest.is_empty())
        || (needle.chars().count() >= 4 && "canvases".starts_with(&needle));
    let matches = |name: &str| {
        let name = name.to_lowercase();
        if rest.chars().count() >= 3 {
            return name.contains(rest);
        }
        name.starts_with(rest)
            || name
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| !w.is_empty() && w.starts_with(rest))
    };
    let mut out = Vec::new();
    for c in all {
        if !(everything || matches(&c.name)) {
            continue;
        }
        out.push(c);
        if !everything && out.len() == 3 {
            break;
        }
    }
    out
}

/// A command the cards can run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cmd {
    pub id: String,
    pub name: String,
    pub glyph: Option<char>,
}

/// A tab closed this session, newest last.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Closed {
    pub title: String,
    pub url: String,
}

/// Everything the ranking reads, gathered from the shell.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    /// What was typed (never the offered ending).
    pub typed: String,
    /// The engine's name ("Google").
    pub engine: String,
    /// The engine's page for what was typed.
    pub search: Option<String>,
    /// What was typed, as a place, if it can be one (`Address.url(from:)`).
    pub address: Option<String>,
    /// This window's open tabs (this space and the pins), the selected
    /// one and blank ones left out, most recently looked at first.
    pub open: Vec<OpenTab>,
    /// Tabs parked in the other spaces.
    pub parked: Vec<OpenTab>,
    /// History's best places for what was typed, best first.
    pub visits: Vec<Visit>,
    /// Every bookmark, flattened, in shelf order.
    pub marks: Vec<Mark>,
    /// Every canvas, newest first.
    pub canvases: Vec<Canvas>,
    /// Every command, in the registry's order.
    pub commands: Vec<Cmd>,
    /// The empty `Ctrl+K`'s doors, in order (`CommandBar.resting`).
    pub resting: Vec<Cmd>,
    /// Closed tabs, newest last.
    pub closed: Vec<Closed>,
    /// The tab under the card is blank (`Ctrl+T` "Open" rather than "Open
    /// in New Tab").
    pub blank: bool,
}

/// How many rows `Ctrl+K` draws (`CommandBar.limit`).
pub const LIMIT: usize = 8;

/// The rows for `mode`.
pub fn offers(mode: Mode, inputs: &Inputs) -> Offers {
    match mode {
        Mode::Palette => palette(inputs),
        Mode::Launch => launch(inputs),
        Mode::Address => address(inputs),
    }
}

// MARK: - Ctrl+K

/// `CommandBar.offers`: open pages first (this space's, then the ones parked
/// in other spaces), then history, bookmarks, canvases, commands, and a
/// search last. An empty field is the open pages and a few doors.
pub fn palette(inputs: &Inputs) -> Offers {
    let needle = inputs.typed.trim().to_lowercase();
    // This window's pages, kept ones first, then by when they were last
    // looked at (`Browser.openPages`).
    let mut open: Vec<&OpenTab> = inputs
        .open
        .iter()
        .filter(|t| {
            needle.is_empty()
                || t.title.to_lowercase().contains(&needle)
                || pretty_str(&t.url).contains(needle.as_str())
        })
        .collect();
    open.sort_by(|a, b| kept(b).cmp(&kept(a)).then(b.seen.total_cmp(&a.seen)));
    if needle.is_empty() {
        let mut list: Vec<Row> = open.iter().take(6).map(|t| open_row(t, false)).collect();
        list.extend(inputs.resting.iter().map(command_row));
        let rows = sectioned(dressed(list, &inputs.engine));
        let default = (!rows.is_empty()).then_some(0);
        return Offers {
            rows,
            default,
            ending: None,
        };
    }
    let command_needle = needle.strip_prefix('/').unwrap_or(&needle).to_string();
    let mut ranked: Vec<(Row, f64)> = Vec::new();
    let add = |ranked: &mut Vec<(Row, f64)>, row: Row, base: f64, against: &str| {
        if let Some(m) = match_score(&row, against) {
            ranked.push((row, base + m));
        }
    };

    // This window's pages that hold the words.
    for t in open.into_iter().take(3) {
        let row = open_row(t, true);
        add(&mut ranked, row, 1_000.0 + boost(t) + 20.0, &needle);
    }
    // Pages parked in other spaces: five at most, kept ones first.
    let mut parked: Vec<&OpenTab> = inputs.parked.iter().collect();
    parked.sort_by(|a, b| kept(b).cmp(&kept(a)).then(b.seen.total_cmp(&a.seen)));
    let mut count = 0;
    for t in parked {
        if count == 5 {
            break;
        }
        let row = open_row(t, true);
        if match_score(&row, &needle).is_none() {
            continue;
        }
        add(&mut ranked, row, 1_000.0 + boost(t), &needle);
        count += 1;
    }
    // Bookmarks: the first three that answer.
    let mut marks = 0;
    for m in &inputs.marks {
        if marks == 3 {
            break;
        }
        let row = mark_row(m, "");
        if match_score(&row, &needle).is_none() {
            continue;
        }
        add(&mut ranked, row, 700.0, &needle);
        marks += 1;
    }
    // Canvases, matched their own way, below bookmarks.
    for (i, c) in canvas_offers(&inputs.typed, &inputs.canvases)
        .into_iter()
        .enumerate()
    {
        ranked.push((canvas_row(c), 650.0 - i as f64));
    }
    // History's four best, in its order.
    let been: Vec<&Visit> = inputs.visits.iter().take(4).collect();
    let n = been.len();
    for (i, v) in been.into_iter().enumerate() {
        add(
            &mut ranked,
            visit_row(v, ""),
            500.0 + (n - i) as f64,
            &needle,
        );
    }
    // Two commands whose names hold the words.
    let mut doing = 0;
    if !command_needle.is_empty() {
        for c in &inputs.commands {
            if doing == 2 {
                break;
            }
            if !c.name.to_lowercase().contains(&strip(&command_needle)) {
                continue;
            }
            // A command is matched by its own name.
            add(&mut ranked, command_row(c), 200.0, &command_needle);
            doing += 1;
        }
    }

    // Section first, in the card's fixed order; the score orders the rows
    // within one. A page open and visited both is the open tab.
    ranked.sort_by(|(a, sa), (b, sb)| {
        a.section
            .cmp(&b.section)
            .then(sb.total_cmp(sa))
            .then(a.title.cmp(&b.title))
    });
    let mut list: Vec<Row> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (row, _) in ranked {
        let key = canonical(&row);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        list.push(row);
        if list.len() == LIMIT - 1 {
            break;
        }
    }
    // The search is always last: the way out when nothing known answers.
    if let Some(url) = &inputs.search {
        list.push(Row {
            kind: Kind::Search,
            title: inputs.typed.clone(),
            detail: String::new(),
            badge: String::new(),
            hint: String::new(),
            action: Action::Go(url.clone()),
            url: Some(url.clone()),
            glyph: None,
            section: Section::Search,
        });
    }
    let rows = sectioned(dressed(list, &inputs.engine));
    let ending = completion(&inputs.typed, &rows);
    let default = (!rows.is_empty()).then_some(0);
    Offers {
        rows,
        default,
        ending,
    }
}

/// A pinned or Saved tab.
fn kept(t: &OpenTab) -> bool {
    t.pinned || t.saved
}

/// `CommandBar.keptBoost`.
fn boost(t: &OpenTab) -> f64 {
    if kept(t) { 2.0 } else { 0.0 }
}

/// `CommandBar.matchScore`: the whole address from its start first, then
/// the name, a host label, the title, and anywhere at all.
pub fn match_score(row: &Row, typed: &str) -> Option<f64> {
    let needle = strip(typed);
    if needle.is_empty() {
        return None;
    }
    let address = row
        .url
        .as_deref()
        .map(|u| strip(&pretty_str(u)))
        .unwrap_or_default();
    let key = strip(&row.title);
    // The Mac's `title` here is the second half of the suggestion: the
    // address for a page, the history key for a visit.
    let title = row
        .url
        .as_deref()
        .map(|u| pretty_str(u).to_lowercase())
        .unwrap_or_default();
    if !address.is_empty() && address.starts_with(&needle) {
        return Some(100.0);
    }
    if key.starts_with(&needle) {
        return Some(96.0);
    }
    let host = address.split('/').next().unwrap_or(&address);
    if host
        .split('.')
        .any(|l| !l.is_empty() && l.starts_with(&needle))
    {
        return Some(82.0);
    }
    if title.starts_with(&needle) {
        return Some(72.0);
    }
    if host.contains(&needle) {
        return Some(62.0);
    }
    if key.contains(&needle) || title.contains(&needle) {
        return Some(45.0);
    }
    if address.contains(&needle) {
        return Some(38.0);
    }
    None
}

/// `CommandBar.strip`: trimmed, lowercased, no scheme, no `www.`.
fn strip(text: &str) -> String {
    strip_typed(text)
}

/// A row's address as `Ctrl+K` folds duplicates: shown form, lowercased,
/// no final slash (`CommandBar.canonical`).
fn canonical(row: &Row) -> String {
    match &row.action {
        Action::Command(id) => format!("copper://command/{id}"),
        Action::Reopen { url, last } => format!("copper://reopen/{last}/{url}"),
        _ => {
            let u = row.url.as_deref().unwrap_or(&row.title);
            let mut t = pretty_str(u).to_lowercase();
            if t.ends_with('/') {
                t.pop();
            }
            t
        }
    }
}

/// `CommandBar.dressed`: one row per address, each with its quiet half and
/// what Return does; eight at most.
fn dressed(list: Vec<Row>, engine: &str) -> Vec<Row> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for mut row in list {
        let key = canonical(&row);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if row.detail.is_empty() {
            row.detail = short_detail(&row);
        }
        row.hint = hint(&row, engine).to_string();
        out.push(row);
        if out.len() == LIMIT {
            break;
        }
    }
    out
}

/// `CommandBar.hint`.
fn hint(row: &Row, engine: &str) -> String {
    match row.kind {
        Kind::Open => "Switch to Tab".into(),
        Kind::Command => "Run".into(),
        Kind::Search => format!("Search {engine}"),
        Kind::Bookmark => "Open Bookmark".into(),
        Kind::Canvas | Kind::Visited | Kind::Known => "Open".into(),
    }
}

/// `CommandBar.shortDetail`: the host, unless the row is already called
/// by it.
fn short_detail(row: &Row) -> String {
    match row.kind {
        Kind::Command | Kind::Search => String::new(),
        Kind::Canvas => "Canvas".into(),
        _ => {
            let host = row.url.as_deref().map(host_of).unwrap_or_default();
            if host.is_empty() || host.eq_ignore_ascii_case(&row.title) {
                String::new()
            } else {
                short(&host, 24)
            }
        }
    }
}

/// `CommandBar.short`: a name cut to `max` characters with an ellipsis.
pub fn short(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_string()
    } else {
        let mut s: String = name.chars().take(max - 1).collect();
        s.push('…');
        s
    }
}

/// A host without `www.`.
pub fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .map(|h| h.strip_prefix("www.").map(str::to_string).unwrap_or(h))
        .unwrap_or_default()
}

/// `CommandBar.sectioned`: a ranked list cut into the fixed order, each
/// group keeping its rows' order.
fn sectioned(rows: Vec<Row>) -> Vec<Row> {
    let mut rows = rows;
    rows.sort_by_key(|r| r.section);
    rows
}

/// An open tab's row. `decorated`: with its badge (the typed `Ctrl+K`
/// card's `decorate`).
fn open_row(t: &OpenTab, decorated: bool) -> Row {
    let badge = if !decorated {
        String::new()
    } else if t.pinned {
        // A pin is in every space, so naming one would be wrong.
        "· ✦".into()
    } else if t.saved {
        if t.current {
            "· saved".into()
        } else {
            format!("· {} · saved", short(&t.space, 24))
        }
    } else if !t.current {
        format!("· {}", short(&t.space, 24))
    } else {
        String::new()
    };
    Row {
        kind: Kind::Open,
        title: if t.title.trim().is_empty() {
            pretty_str(&t.url)
        } else {
            t.title.clone()
        },
        detail: String::new(),
        badge,
        hint: String::new(),
        action: Action::Switch(t.id.clone()),
        url: Some(t.url.clone()),
        glyph: None,
        section: Section::Tabs,
    }
}

fn mark_row(m: &Mark, hint: &str) -> Row {
    Row {
        kind: Kind::Bookmark,
        title: if m.title.trim().is_empty() {
            pretty_str(&m.url)
        } else {
            m.title.clone()
        },
        detail: String::new(),
        badge: String::new(),
        hint: hint.into(),
        action: Action::Go(m.url.clone()),
        url: Some(m.url.clone()),
        glyph: None,
        section: Section::Bookmarks,
    }
}

fn canvas_row(c: &Canvas) -> Row {
    Row {
        kind: Kind::Canvas,
        title: format!("Open Canvas · {}", c.name),
        detail: if c.shared {
            "Shared".into()
        } else if c.personal {
            "Private".into()
        } else {
            String::new()
        },
        badge: String::new(),
        hint: "Open".into(),
        action: Action::Go(c.url.clone()),
        url: Some(c.url.clone()),
        glyph: None,
        section: Section::Canvases,
    }
}

fn visit_row(v: &Visit, hint: &str) -> Row {
    Row {
        kind: Kind::Visited,
        title: if v.title.trim().is_empty() {
            v.key.clone()
        } else {
            v.title.clone()
        },
        detail: String::new(),
        badge: String::new(),
        hint: hint.into(),
        action: Action::Go(v.url.clone()),
        url: Some(v.url.clone()),
        glyph: None,
        section: Section::History,
    }
}

fn command_row(c: &Cmd) -> Row {
    Row {
        kind: Kind::Command,
        title: c.name.clone(),
        detail: String::new(),
        badge: String::new(),
        hint: "Run".into(),
        action: Action::Command(c.id.clone()),
        url: None,
        glyph: c.glyph,
        section: Section::Commands,
    }
}

// MARK: - Ctrl+T

/// How many rows each `Ctrl+T` group may bring (`Launcher.Cap`).
mod cap {
    pub const TABS: usize = 4;
    pub const HISTORY: usize = 4;
    pub const BOOKMARKS: usize = 2;
    pub const CANVASES: usize = 3;
    pub const COMMANDS: usize = 2;
    pub const RECENT: usize = 6;
    pub const ELSEWHERE: usize = 2;
    pub const CLOSED: usize = 2;
}

/// "Open" over a blank tab, which takes the page itself; "Open in New Tab"
/// anywhere else (`Launcher.pageHint`).
fn page_hint(inputs: &Inputs) -> &'static str {
    if inputs.blank {
        "Open"
    } else {
        "Open in New Tab"
    }
}

/// `Ctrl+T`'s other ways to take the row you are on, said quietly beside
/// what Return does (`Launcher.aside`). Windows has no split view, so a
/// page offers only the background and a tab nothing more.
pub fn aside(row: &Row) -> String {
    match row.action {
        Action::Go(_) if row.kind != Kind::Canvas => "Shift+Enter Background".into(),
        _ => String::new(),
    }
}

/// `Launcher.rows`: the card for `Ctrl+T`.
pub fn launch(inputs: &Inputs) -> Offers {
    let query = inputs.typed.trim();
    if query.is_empty() {
        return resting(inputs);
    }
    let needle = query.to_lowercase();
    let page = page_hint(inputs);
    let mut seen: Vec<String> = Vec::new();
    let keep = |seen: &mut Vec<String>, row: &Row| -> bool {
        let k = launch_canonical(row);
        if seen.contains(&k) {
            return false;
        }
        seen.push(k);
        true
    };
    let top = top_row(inputs, page);
    let is_address = top.kind != Kind::Search;
    // "/" asks for the browser's own commands, and only for them.
    let commanding = needle.starts_with('/');
    let probe = if commanding {
        None
    } else {
        Needle::new(&needle)
    };

    // Open tabs: this space's above every other's, every match claimed.
    let mut tabs: Vec<Row> = Vec::new();
    if let Some(probe) = &probe {
        let mut scored: Vec<(&OpenTab, f64)> = inputs
            .open
            .iter()
            .chain(&inputs.parked)
            .filter_map(|t| {
                let address = strip(&pretty_str(&t.url));
                let m = probe.score(&address, &t.title.to_lowercase())?;
                Some((t, m + if t.current { 1_000.0 } else { 0.0 }))
            })
            .collect();
        scored.sort_by(|(a, sa), (b, sb)| sb.total_cmp(sa).then(b.seen.total_cmp(&a.seen)));
        for (t, _) in scored {
            let row = launch_tab_row(t);
            if keep(&mut seen, &row) && tabs.len() < cap::TABS {
                tabs.push(row);
            }
        }
    }
    // What Return does with the words is claimed before history.
    keep(&mut seen, &top);

    let mut been: Vec<Row> = Vec::new();
    let mut marks: Vec<Row> = Vec::new();
    let mut boards: Vec<Row> = Vec::new();
    if let Some(probe) = &probe {
        // A site's untitled front door beside one of its titled pages is
        // the same answer said twice.
        let hits: Vec<&Visit> = inputs
            .visits
            .iter()
            .filter(|v| probe.score(&v.key, &v.title.to_lowercase()).is_some())
            .collect();
        let titled: Vec<String> = hits
            .iter()
            .filter(|v| !v.title.is_empty())
            .map(|v| host_raw(&v.url))
            .collect();
        for v in hits {
            if v.title.is_empty() && !v.key.contains('/') && titled.contains(&host_raw(&v.url)) {
                continue;
            }
            let mut row = visit_row(v, page);
            row.detail = besides(&v.url, &row.title);
            if !keep(&mut seen, &row) {
                continue;
            }
            been.push(row);
            if been.len() == cap::HISTORY {
                break;
            }
        }
        for m in &inputs.marks {
            let address = strip(&pretty_str(&m.url));
            if probe.score(&address, &m.title.to_lowercase()).is_none() {
                continue;
            }
            let mut row = mark_row(m, page);
            row.detail = besides(&m.url, &row.title);
            if !keep(&mut seen, &row) {
                continue;
            }
            marks.push(row);
            if marks.len() == cap::BOOKMARKS {
                break;
            }
        }
        for c in canvas_offers(&inputs.typed, &inputs.canvases) {
            let row = canvas_row(c);
            if !keep(&mut seen, &row) {
                continue;
            }
            boards.push(row);
            if boards.len() == cap::CANVASES {
                break;
            }
        }
    }
    // The browser's own commands, few: `Ctrl+K` is where commands live.
    let command_needle = needle.strip_prefix('/').unwrap_or(&needle);
    let mut doing: Vec<Row> = Vec::new();
    if !command_needle.is_empty() {
        for c in &inputs.commands {
            if !c.name.to_lowercase().contains(command_needle) {
                continue;
            }
            doing.push(command_row(c));
            if doing.len() == cap::COMMANDS {
                break;
            }
        }
    }
    // The search, last: an address can still be words you meant to look up.
    let mut searches: Vec<Row> = Vec::new();
    if is_address
        && !needle.contains("://")
        && let Some(url) = &inputs.search
    {
        let row = search_row(query, url, &inputs.engine, page);
        if keep(&mut seen, &row) {
            searches.push(row);
        }
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut top = top;
    if is_address {
        top.section = Section::Lead;
        rows.push(top);
    } else {
        top.section = Section::Search;
        searches.insert(0, top);
    }
    for (section, group) in [
        (Section::Tabs, tabs),
        (Section::History, been),
        (Section::Bookmarks, marks),
        (Section::Canvases, boards),
        (Section::Commands, doing),
        (Section::Search, searches),
    ] {
        for mut r in group {
            r.section = section;
            rows.push(r);
        }
    }
    let ending = completion(&inputs.typed, &rows);
    let default = (!rows.is_empty()).then_some(0);
    Offers {
        rows,
        default,
        ending,
    }
}

/// The row for the words themselves (`Launcher.top`).
fn top_row(inputs: &Inputs, page: &str) -> Row {
    let query = inputs.typed.trim();
    if let Some(url) = &inputs.address {
        return Row {
            kind: Kind::Known,
            title: format!("Open {}", spoken(url)),
            detail: String::new(),
            badge: String::new(),
            hint: page.into(),
            action: Action::Go(url.clone()),
            url: Some(url.clone()),
            glyph: None,
            section: Section::Lead,
        };
    }
    let url = inputs
        .search
        .clone()
        .unwrap_or_else(|| "https://www.google.com".into());
    search_row(query, &url, &inputs.engine, page)
}

fn search_row(query: &str, url: &str, engine: &str, page: &str) -> Row {
    Row {
        kind: Kind::Search,
        title: format!("Search {engine} for \u{201C}{query}\u{201D}"),
        detail: String::new(),
        badge: String::new(),
        hint: page.into(),
        action: Action::Go(url.to_string()),
        url: Some(url.to_string()),
        glyph: None,
        section: Section::Search,
    }
}

/// An address as the row says it: the shown form with its port kept
/// (`Launcher.spoken`).
pub fn spoken(url: &str) -> String {
    let pretty = pretty_str(url);
    let Ok(u) = url::Url::parse(url) else {
        return pretty;
    };
    let (Some(port), Some(full)) = (u.port(), u.host_str()) else {
        return pretty;
    };
    let host = full.strip_prefix("www.").unwrap_or(full);
    match pretty.strip_prefix(host) {
        Some(rest) => format!("{host}:{port}{rest}"),
        None => pretty,
    }
}

/// The host, unless the row is already called by it (`Launcher.host`).
fn besides(url: &str, name: &str) -> String {
    let host = host_of(url);
    if host.eq_ignore_ascii_case(name) {
        String::new()
    } else {
        host
    }
}

fn host_raw(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

/// One address however it was written, its query kept
/// (`Launcher.canonical`).
fn launch_canonical(row: &Row) -> String {
    match &row.action {
        Action::Command(id) => format!("copper://command/{id}"),
        Action::Reopen { url, last } => format!("copper://reopen/{last}/{url}"),
        _ => {
            let mut text = strip(row.url.as_deref().unwrap_or(&row.title));
            if let Some(hash) = text.find('#') {
                text.truncate(hash);
            }
            if text.ends_with('/') {
                text.pop();
            }
            text
        }
    }
}

/// `Launcher.row(for:)`: an open tab, with its space when it is parked.
fn launch_tab_row(t: &OpenTab) -> Row {
    let mut row = open_row(t, false);
    row.detail = besides(&t.url, &row.title);
    row.hint = "Switch to Tab".into();
    if !t.current {
        let name = short(&t.space, 22);
        let emoji = t
            .space_emoji
            .as_ref()
            .map(|e| format!("{e} "))
            .unwrap_or_default();
        row.badge = format!("· {emoji}{name}");
    }
    row
}

/// The empty `Ctrl+T`: the tabs you were just on, then a couple from other
/// spaces, then the tabs you closed (`Launcher.resting`).
fn resting(inputs: &Inputs) -> Offers {
    let mut recent: Vec<Row> = inputs
        .open
        .iter()
        .take(cap::RECENT)
        .map(launch_tab_row)
        .collect();
    let mut away: Vec<&OpenTab> = inputs.parked.iter().collect();
    away.sort_by(|a, b| b.seen.total_cmp(&a.seen));
    recent.extend(away.into_iter().take(cap::ELSEWHERE).map(launch_tab_row));
    let closed: Vec<Row> = inputs
        .closed
        .iter()
        .rev()
        .take(cap::CLOSED)
        .enumerate()
        .map(|(i, g)| Row {
            kind: Kind::Command,
            title: format!("Reopen \u{201C}{}\u{201D}", g.title),
            detail: besides(&g.url, &g.title),
            badge: String::new(),
            hint: "Reopen".into(),
            action: Action::Reopen {
                url: g.url.clone(),
                last: i == 0,
            },
            url: None,
            // Segoe Fluent's Undo, the Mac's `arrow.uturn.backward`.
            glyph: Some('\u{E7A7}'),
            section: Section::Closed,
        })
        .collect();
    let mut rows = Vec::new();
    for mut r in recent {
        r.section = Section::Recent;
        rows.push(r);
    }
    rows.extend(closed);
    let default = (!rows.is_empty()).then_some(0);
    Offers {
        rows,
        default,
        ending: None,
    }
}

/// What was typed, made ready for matching against places
/// (`Launcher.Needle`): the address from its start, a host label, a word
/// of the title, and — past two letters — anywhere in the host or title.
pub struct Needle {
    text: String,
    words: Vec<String>,
    length: usize,
    spaced: bool,
    dotted: bool,
}

impl Needle {
    pub fn new(typed: &str) -> Option<Needle> {
        let text = strip(typed);
        if text.is_empty() {
            return None;
        }
        Some(Needle {
            words: text
                .split(' ')
                .filter(|w| !w.is_empty())
                .map(str::to_string)
                .collect(),
            length: text.chars().count(),
            spaced: text.contains(' '),
            dotted: text.contains('.'),
            text,
        })
    }

    /// How well this fits a place: `key` its stripped address, `title`
    /// lowercased.
    pub fn score(&self, key: &str, title: &str) -> Option<f64> {
        if self.spaced {
            for w in &self.words {
                if !title.contains(w.as_str()) && !key.contains(w.as_str()) {
                    return None;
                }
            }
            return Some(if title.starts_with(self.words[0].as_str()) {
                70.0
            } else {
                55.0
            });
        }
        let needle = self.text.as_str();
        if key.starts_with(needle) {
            return Some(100.0);
        }
        let host_end = key.find('/').unwrap_or(key.len());
        let host = &key[..host_end];
        if !self.dotted {
            let mut start = 0;
            for label in host.split('.') {
                if !label.is_empty() && host[start..].starts_with(needle) {
                    return Some(85.0);
                }
                start += label.len() + 1;
            }
        }
        if title.starts_with(needle) {
            return Some(75.0);
        }
        if title
            .match_indices(needle)
            .any(|(at, _)| at > 0 && title.as_bytes()[at - 1] == b' ')
        {
            return Some(75.0);
        }
        if self.length < 2 {
            return None;
        }
        if host.contains(needle) {
            return Some(60.0);
        }
        if title.contains(needle) {
            return Some(50.0);
        }
        if self.length >= 3 && key.contains(needle) {
            return Some(35.0);
        }
        None
    }
}

// MARK: - Ctrl+L

/// `Browser.guess`, the plain field: three places you have been and, when
/// the words cannot be a place, the search — no open pages, no commands.
pub fn address(inputs: &Inputs) -> Offers {
    let typed = inputs.typed.as_str();
    if typed.trim().is_empty() {
        return Offers::default();
    }
    let mut rows: Vec<Row> = inputs
        .visits
        .iter()
        .take(3)
        .map(|v| Row {
            kind: Kind::Visited,
            title: v.key.clone(),
            detail: v.title.clone(),
            badge: String::new(),
            hint: String::new(),
            action: Action::Go(v.url.clone()),
            url: Some(v.url.clone()),
            glyph: None,
            section: Section::Plain,
        })
        .collect();
    let places: Vec<Suggestion> = rows.iter().filter_map(suggestion).collect();
    if inputs.address.is_none()
        && let Some(url) = &inputs.search
    {
        rows.push(Row {
            kind: Kind::Search,
            title: typed.to_string(),
            detail: inputs.engine.clone(),
            badge: String::new(),
            hint: String::new(),
            action: Action::Go(url.clone()),
            url: Some(url.clone()),
            glyph: None,
            section: Section::Plain,
        });
    }
    let ending = history::completion(typed, &places);
    Offers {
        rows,
        // A picked row stops being right the moment the question changes.
        default: None,
        ending,
    }
}

// MARK: - the ending, the keys

/// `Ctrl+K`'s and `Ctrl+T`'s grey ending (`Browser.completion(in:)`): from
/// the places — open, visited, kept — and only from a search when there is
/// no place; never a command, never a canvas's id.
pub fn completion(typed: &str, rows: &[Row]) -> Option<String> {
    let addresses: Vec<&Row> = rows
        .iter()
        .filter(|r| r.kind != Kind::Canvas)
        .filter(|r| {
            r.url.as_deref().is_some_and(|u| {
                let l = u.to_ascii_lowercase();
                l.starts_with("http://") || l.starts_with("https://") || l.starts_with("file:")
            })
        })
        .collect();
    let places: Vec<&Row> = addresses
        .iter()
        .copied()
        .filter(|r| r.kind != Kind::Search && r.kind != Kind::Command)
        .collect();
    let among = if places.is_empty() { addresses } else { places };
    let options: Vec<Suggestion> = among.into_iter().filter_map(suggestion).collect();
    history::completion(typed, &options)
}

/// A row as history's completion reads it: its address and its name.
fn suggestion(row: &Row) -> Option<Suggestion> {
    Some(Suggestion {
        key: row.title.clone(),
        title: row.title.clone(),
        url: row.url.clone()?,
        kind: SuggestionKind::Visited,
        tab: None,
        score: 0.0,
    })
}

/// Up and Down (`Browser.walk`): one row; off either end lets go, and the
/// next press comes back in at the first (or last).
pub fn walk(count: usize, picked: Option<usize>, step: i32) -> Option<usize> {
    if count == 0 {
        return picked;
    }
    match picked {
        None if step > 0 => Some(0),
        None => Some(count - 1),
        Some(here) => {
            let next = here as i64 + i64::from(step);
            (next >= 0 && next < count as i64).then_some(next as usize)
        }
    }
}

/// Where each group's rows begin: every heading, and the lead row(s) at the
/// top when the list starts under none (`CardSections.starts`).
pub fn starts(rows: &[Row]) -> Vec<usize> {
    (0..rows.len())
        .filter(|&i| i == 0 || rows[i].section != rows[i - 1].section)
        .collect()
}

/// Tab (`forward`) and Shift+Tab: the first row of the next or previous
/// group, round the end; from inside a group Shift+Tab goes to the one
/// before it. Nothing picked: Tab starts at the first, Shift+Tab at the
/// last (`CardSections.jump`).
pub fn jump(rows: &[Row], picked: Option<usize>, forward: bool) -> Option<usize> {
    let s = starts(rows);
    if s.is_empty() {
        return None;
    }
    let Some(p) = picked.filter(|&p| p < rows.len()) else {
        return if forward {
            s.first().copied()
        } else {
            s.last().copied()
        };
    };
    let here = s.iter().rposition(|&x| x <= p).unwrap_or(0);
    let n = s.len();
    let next = if forward {
        (here + 1) % n
    } else {
        (here + n - 1) % n
    };
    Some(s[next])
}

/// The headings over the rows: the row each sits above and what it says.
#[cfg(test)]
pub fn headings(rows: &[Row]) -> Vec<(usize, &'static str)> {
    starts(rows)
        .into_iter()
        .filter_map(|i| rows[i].section.title().map(|t| (i, t)))
        .collect()
}

/// What a row puts in the field when Tab (or Right/End) takes it
/// (`Browser.words(for:)`). In the cards: a place's address, the words of
/// a search, nothing for a command; in the plain field, its key.
pub fn words(mode: Mode, row: &Row) -> Option<String> {
    if mode == Mode::Address {
        return Some(row.title.clone());
    }
    let url = row.url.as_deref()?;
    let parsed = url::Url::parse(url).ok()?;
    if row.kind == Kind::Search {
        return parsed
            .query_pairs()
            .find(|(k, _)| k == "q")
            .map(|(_, v)| v.into_owned());
    }
    if row.kind == Kind::Command || !matches!(parsed.scheme(), "http" | "https" | "file") {
        return None;
    }
    let q = parsed.query().map(|q| format!("?{q}")).unwrap_or_default();
    Some(format!("{}{q}", pretty_str(url)))
}

/// What accepting the field's offer puts in it (`Browser.acceptCompletion`):
/// the walked row, else the grey ending, else the row picked for you.
/// `None`: nothing to take.
pub fn accept(
    mode: Mode,
    typed: &str,
    ending: Option<&str>,
    picked: Option<&Row>,
    walked: bool,
) -> Option<String> {
    if walked && let Some(w) = picked.and_then(|r| words(mode, r)) {
        return Some(w);
    }
    if let Some(e) = ending.filter(|e| !e.is_empty()) {
        return Some(format!("{typed}{e}"));
    }
    picked.and_then(|r| words(mode, r))
}

/// Escape, one step at a time (`App.swift` 793–815): a picked row is let go
/// of first; then the card goes — except the plain field standing on an
/// empty stage, which has nothing behind it to go back to. `Ctrl+T` goes
/// in one press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape {
    Unpick,
    Close,
    Stay,
}

pub fn escape(mode: Mode, picked: bool, standing: bool) -> Escape {
    match mode {
        Mode::Launch => Escape::Close,
        _ if picked => Escape::Unpick,
        Mode::Address if standing => Escape::Stay,
        _ => Escape::Close,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str, title: &str, url: &str, current: bool) -> OpenTab {
        OpenTab {
            id: TabId(id.into()),
            title: title.into(),
            url: url.into(),
            current,
            space: if current {
                String::new()
            } else {
                "Work".into()
            },
            seen: 1.0,
            ..Default::default()
        }
    }

    fn cmd(id: &str, name: &str) -> Cmd {
        Cmd {
            id: id.into(),
            name: name.into(),
            glyph: None,
        }
    }

    fn inputs(typed: &str) -> Inputs {
        let address = copper_core::address::url_from_typed(typed).map(|u| u.to_string());
        let engine = copper_core::address::SearchEngine::by_id("google");
        Inputs {
            typed: typed.into(),
            engine: "Google".into(),
            search: engine.url_for(typed).map(|u| u.to_string()),
            address,
            ..Default::default()
        }
    }

    fn titles(o: &Offers) -> Vec<String> {
        o.rows.iter().map(|r| r.title.clone()).collect()
    }

    #[test]
    fn empty_palette_is_recent_tabs_then_the_doors() {
        let mut i = inputs("");
        i.open = (0..8)
            .map(|n| {
                tab(
                    &n.to_string(),
                    &format!("T{n}"),
                    &format!("https://t{n}.com/"),
                    true,
                )
            })
            .collect();
        i.resting = vec![
            cmd("new-tab", "New Tab"),
            cmd("new-space", "New Space"),
            cmd("show-history", "History"),
        ];
        let o = palette(&i);
        assert_eq!(o.rows.len(), LIMIT, "six tabs and two doors, eight at most");
        assert_eq!(&titles(&o)[..6], &["T0", "T1", "T2", "T3", "T4", "T5"]);
        assert_eq!(o.rows[6].title, "New Tab");
        assert_eq!(o.default, Some(0), "Return goes back to the last tab");
        assert_eq!(o.rows[0].hint, "Switch to Tab");
        assert_eq!(o.rows[0].detail, "t0.com");
        assert_eq!(o.rows[6].hint, "Run");
        assert_eq!(headings(&o.rows), vec![(0, "Open Tabs"), (6, "Commands")]);
    }

    #[test]
    fn empty_palette_with_no_tabs_still_has_doors() {
        let mut i = inputs("");
        i.resting = vec![cmd("new-tab", "New Tab")];
        let o = palette(&i);
        assert_eq!(titles(&o), vec!["New Tab"]);
    }

    #[test]
    fn typed_palette_sections_order_and_cap() {
        let mut i = inputs("git");
        i.open = vec![tab("a", "GitHub", "https://github.com/", true)];
        i.parked = vec![tab("b", "GitLab", "https://gitlab.com/", false)];
        i.visits = vec![
            Visit {
                key: "github.com".into(),
                title: "GitHub".into(),
                url: "https://github.com/".into(),
            },
            Visit {
                key: "git-scm.com".into(),
                title: "Git".into(),
                url: "https://git-scm.com/".into(),
            },
            Visit {
                key: "gitea.io".into(),
                title: "Gitea".into(),
                url: "https://gitea.io/".into(),
            },
            Visit {
                key: "gitbook.com".into(),
                title: "GitBook".into(),
                url: "https://gitbook.com/".into(),
            },
        ];
        i.marks = vec![
            Mark {
                title: "Git docs".into(),
                url: "https://git-scm.com/doc".into(),
            },
            Mark {
                title: "GitHub".into(),
                url: "https://github.com".into(),
            },
        ];
        i.commands = vec![
            cmd("git-x", "Git Thing"),
            cmd("git-y", "Digital"),
            cmd("git-z", "Gitter"),
        ];
        let o = palette(&i);
        assert!(o.rows.len() <= LIMIT);
        assert_eq!(o.rows.last().unwrap().kind, Kind::Search, "search last");
        let order: Vec<Section> = o.rows.iter().map(|r| r.section).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted, "fixed section order");
        // The open tab beats its visit and its bookmark.
        let github: Vec<&Row> = o
            .rows
            .iter()
            .filter(|r| r.url.as_deref().is_some_and(|u| u.contains("github.com")))
            .collect();
        assert_eq!(github.len(), 1);
        assert_eq!(github[0].kind, Kind::Open);
        // Parked tab wears its space.
        let lab = o.rows.iter().find(|r| r.title == "GitLab").unwrap();
        assert_eq!(lab.badge, "· Work");
        assert_eq!(o.rows.len(), LIMIT, "seven known and the search");
        assert_eq!(o.default, Some(0));
    }

    #[test]
    fn nothing_known_selects_the_search() {
        let o = palette(&inputs("quantum flux"));
        assert_eq!(o.rows.len(), 1);
        assert_eq!(o.rows[0].kind, Kind::Search);
        assert_eq!(o.rows[0].title, "quantum flux");
        assert_eq!(o.rows[0].hint, "Search Google");
        assert_eq!(o.default, Some(0));
        assert_eq!(headings(&o.rows), vec![(0, "Search")]);
    }

    #[test]
    fn commands_match_anywhere_in_the_name_and_slash_is_stripped() {
        let mut i = inputs("/space");
        i.commands = vec![
            cmd("new-space", "New Space"),
            cmd("next-space", "Next Space"),
            cmd("prev", "Previous Space"),
        ];
        let o = palette(&i);
        let cmds: Vec<&str> = o
            .rows
            .iter()
            .filter(|r| r.kind == Kind::Command)
            .map(|r| r.title.as_str())
            .collect();
        assert_eq!(cmds.len(), 2, "two commands at most");
        assert!(cmds.contains(&"New Space"));
    }

    #[test]
    fn pinned_and_saved_badges() {
        let mut i = inputs("exa");
        let mut p = tab("p", "Example", "https://example.com/", true);
        p.pinned = true;
        let mut s = tab("s", "Example Two", "https://example.org/", false);
        s.saved = true;
        i.open = vec![p];
        i.parked = vec![s];
        let o = palette(&i);
        assert_eq!(o.rows[0].badge, "· ✦");
        assert_eq!(o.rows[1].badge, "· Work · saved");
    }

    #[test]
    fn match_scores_like_the_mac() {
        let row = |title: &str, url: &str| Row {
            kind: Kind::Visited,
            title: title.into(),
            detail: String::new(),
            badge: String::new(),
            hint: String::new(),
            action: Action::Go(url.into()),
            url: Some(url.into()),
            glyph: None,
            section: Section::History,
        };
        assert_eq!(
            match_score(&row("x", "https://www.linear.app/"), "lin"),
            Some(100.0)
        );
        assert_eq!(
            match_score(&row("Linear", "https://a.com/"), "lin"),
            Some(96.0)
        );
        assert_eq!(
            match_score(&row("x", "https://cloud-ems.dev.exowatt.com/"), "exo"),
            Some(82.0)
        );
        assert_eq!(
            match_score(&row("x", "https://cloud.exowatt.com/"), "owatt"),
            Some(62.0)
        );
        assert_eq!(
            match_score(&row("My Docs", "https://a.com/"), "docs"),
            Some(45.0)
        );
        // A page's second half is its address, so an address match is
        // caught at 45 before the 38 (as on the Mac).
        assert_eq!(
            match_score(&row("x", "https://a.com/zebra"), "zeb"),
            Some(45.0)
        );
        assert_eq!(match_score(&row("x", "https://a.com/"), "nope"), None);
    }

    #[test]
    fn launch_empty_is_recent_then_closed() {
        let mut i = inputs("");
        i.open = vec![
            tab("a", "A", "https://a.com/", true),
            tab("b", "B", "https://b.com/", true),
        ];
        i.parked = vec![tab("c", "C", "https://c.com/", false)];
        i.closed = vec![
            Closed {
                title: "Old".into(),
                url: "https://old.com/".into(),
            },
            Closed {
                title: "Newer".into(),
                url: "https://newer.com/".into(),
            },
            Closed {
                title: "Newest".into(),
                url: "https://newest.com/".into(),
            },
        ];
        let o = launch(&i);
        assert_eq!(
            titles(&o),
            vec![
                "A",
                "B",
                "C",
                "Reopen \u{201C}Newest\u{201D}",
                "Reopen \u{201C}Newer\u{201D}"
            ]
        );
        assert_eq!(
            headings(&o.rows),
            vec![(0, "Recent Tabs"), (3, "Recently Closed")]
        );
        assert_eq!(o.rows[2].badge, "· Work");
        assert_eq!(o.default, Some(0));
        assert!(matches!(
            o.rows[3].action,
            Action::Reopen { last: true, .. }
        ));
    }

    #[test]
    fn launch_address_leads_under_no_heading() {
        let mut i = inputs("github.com");
        i.visits = vec![Visit {
            key: "github.com/x".into(),
            title: "X".into(),
            url: "https://github.com/x".into(),
        }];
        let o = launch(&i);
        assert_eq!(o.rows[0].title, "Open github.com");
        assert_eq!(o.rows[0].section, Section::Lead);
        assert_eq!(o.rows[0].hint, "Open in New Tab");
        assert_eq!(headings(&o.rows)[0], (1, "History"));
        // An address can still be words to look up, last.
        let last = o.rows.last().unwrap();
        assert_eq!(last.title, "Search Google for \u{201C}github.com\u{201D}");
        assert_eq!(o.default, Some(0));
    }

    #[test]
    fn launch_words_search_heads_the_search_group() {
        let mut i = inputs("rust book");
        i.blank = true;
        i.open = vec![tab(
            "a",
            "The Rust Book",
            "https://doc.rust-lang.org/book/",
            true,
        )];
        let o = launch(&i);
        assert_eq!(o.rows[0].title, "The Rust Book", "what you know first");
        assert_eq!(
            o.rows[1].title,
            "Search Google for \u{201C}rust book\u{201D}"
        );
        assert_eq!(o.rows[1].hint, "Open");
        assert_eq!(headings(&o.rows), vec![(0, "Open Tabs"), (1, "Search")]);
    }

    #[test]
    fn launch_port_is_spoken() {
        assert_eq!(spoken("http://localhost:3000/x"), "localhost:3000/x");
        assert_eq!(spoken("https://www.a.com/"), "a.com");
    }

    #[test]
    fn launch_slash_is_commands_only() {
        let mut i = inputs("/new");
        i.open = vec![tab("a", "new things", "https://new.com/", true)];
        i.commands = vec![
            cmd("new-tab", "New Tab"),
            cmd("new-space", "New Space"),
            cmd("new-canvas", "New Canvas"),
        ];
        let o = launch(&i);
        let kinds: Vec<Kind> = o.rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, vec![Kind::Command, Kind::Command, Kind::Search]);
    }

    #[test]
    fn needle_scores() {
        let n = Needle::new("exo").unwrap();
        assert_eq!(n.score("cloud-ems.dev.exowatt.com/x", "cloud"), Some(85.0));
        assert_eq!(n.score("exowatt.com", ""), Some(100.0));
        assert_eq!(n.score("a.com/exo", "my exo page"), Some(75.0));
        let two = Needle::new("rust book").unwrap();
        assert_eq!(
            two.score("doc.rust-lang.org/book", "the rust book"),
            Some(55.0)
        );
        assert_eq!(Needle::new("x").unwrap().score("a.com/x", "z"), None);
    }

    #[test]
    fn plain_field_is_history_and_a_search_only_for_words() {
        let mut i = inputs("git");
        i.visits = (0..5)
            .map(|n| Visit {
                key: format!("github.com/{n}"),
                title: format!("G{n}"),
                url: format!("https://github.com/{n}"),
            })
            .collect();
        let o = address(&i);
        assert_eq!(o.rows.len(), 4, "three places and the search");
        assert_eq!(o.rows[0].title, "github.com/0");
        assert_eq!(o.rows[0].detail, "G0");
        assert_eq!(o.default, None);
        assert_eq!(o.ending.as_deref(), Some("hub.com/0"));
        let o = address(&inputs("github.com"));
        assert!(
            o.rows.iter().all(|r| r.kind != Kind::Search),
            "an address is not searched"
        );
        assert!(address(&inputs("  ")).rows.is_empty());
    }

    #[test]
    fn palette_ending_comes_from_places_not_commands() {
        let mut i = inputs("ex");
        i.commands = vec![cmd("x", "Exact time")];
        i.visits = vec![Visit {
            key: "example.com".into(),
            title: "Example".into(),
            url: "https://example.com/".into(),
        }];
        let o = palette(&i);
        assert_eq!(o.ending.as_deref(), Some("ample.com"));
    }

    #[test]
    fn walking_off_either_end_lets_go() {
        assert_eq!(walk(3, None, 1), Some(0));
        assert_eq!(walk(3, None, -1), Some(2));
        assert_eq!(walk(3, Some(2), 1), None);
        assert_eq!(walk(3, Some(0), -1), None);
        assert_eq!(walk(3, Some(1), 1), Some(2));
        assert_eq!(walk(0, None, 1), None);
    }

    #[test]
    fn tab_jumps_sections_round_the_end() {
        let mut i = inputs("git");
        i.open = vec![tab("a", "GitHub", "https://github.com/", true)];
        i.visits = vec![
            Visit {
                key: "git-scm.com".into(),
                title: "Git".into(),
                url: "https://git-scm.com/".into(),
            },
            Visit {
                key: "gitea.io".into(),
                title: "Gitea".into(),
                url: "https://gitea.io/".into(),
            },
        ];
        let o = palette(&i);
        // Open Tabs 0, History 1–2, Search 3.
        assert_eq!(starts(&o.rows), vec![0, 1, 3]);
        assert_eq!(jump(&o.rows, Some(0), true), Some(1));
        assert_eq!(jump(&o.rows, Some(2), true), Some(3));
        assert_eq!(jump(&o.rows, Some(3), true), Some(0), "round the end");
        assert_eq!(
            jump(&o.rows, Some(2), false),
            Some(0),
            "from inside a group, the one before"
        );
        assert_eq!(jump(&o.rows, None, true), Some(0));
        assert_eq!(jump(&o.rows, None, false), Some(3));
    }

    #[test]
    fn accepting_takes_walked_row_then_ending_then_default() {
        let mut i = inputs("exa");
        i.visits = vec![Visit {
            key: "example.com/a?b=1".into(),
            title: "Ex".into(),
            url: "https://example.com/a?b=1".into(),
        }];
        let o = palette(&i);
        let row = &o.rows[0];
        assert_eq!(
            accept(Mode::Palette, "exa", o.ending.as_deref(), Some(row), true).as_deref(),
            Some("example.com/a?b=1")
        );
        assert_eq!(
            accept(Mode::Palette, "exa", Some("mple.com"), Some(row), false).as_deref(),
            Some("example.com")
        );
        let search = palette(&inputs("hello world"));
        assert_eq!(
            accept(
                Mode::Palette,
                "hello world",
                None,
                search.rows.first(),
                false
            )
            .as_deref(),
            Some("hello world"),
            "a search gives back its words"
        );
        let mut c = inputs("new");
        c.commands = vec![cmd("new-tab", "New Tab")];
        let o = palette(&c);
        let command = o.rows.iter().find(|r| r.kind == Kind::Command).unwrap();
        assert_eq!(
            accept(Mode::Palette, "new", None, Some(command), true),
            None
        );
    }

    #[test]
    fn escape_steps() {
        assert_eq!(escape(Mode::Palette, true, false), Escape::Unpick);
        assert_eq!(escape(Mode::Palette, false, false), Escape::Close);
        assert_eq!(
            escape(Mode::Launch, true, false),
            Escape::Close,
            "Ctrl+T in one press"
        );
        assert_eq!(escape(Mode::Address, true, true), Escape::Unpick);
        assert_eq!(escape(Mode::Address, false, true), Escape::Stay);
        assert_eq!(escape(Mode::Address, false, false), Escape::Close);
    }

    #[test]
    fn identity_survives_rerank() {
        let a = palette(&inputs("x y"));
        let b = palette(&inputs("x y"));
        assert_eq!(a.rows[0].identity(), b.rows[0].identity());
    }
}

#[cfg(test)]
mod canvas_tests {
    use super::*;

    fn c(name: &str) -> Canvas {
        Canvas {
            name: name.into(),
            url: format!("copper://canvas/{name}"),
            ..Default::default()
        }
    }

    #[test]
    fn asides_name_the_other_ways() {
        let mut r = canvas_row(&c("Plan"));
        assert_eq!(aside(&r), "");
        r.kind = Kind::Visited;
        r.action = Action::Go("https://example.com/".into());
        assert_eq!(aside(&r), "Shift+Enter Background");
        r.action = Action::Switch(TabId("t".into()));
        assert_eq!(aside(&r), "");
    }

    #[test]
    fn canvas_word_lists_them_all() {
        let all: Vec<Canvas> = ["Plan", "Roadmap", "Ideas", "Plants"]
            .iter()
            .map(|n| c(n))
            .collect();
        assert_eq!(canvas_offers("canvas", &all).len(), 4);
        assert_eq!(canvas_offers("canv", &all).len(), 4);
        let plan: Vec<&str> = canvas_offers("canvas pla", &all)
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(plan, vec!["Plan", "Plants"]);
        assert_eq!(canvas_offers("ro", &all)[0].name, "Roadmap");
        assert!(canvas_offers("zz", &all).is_empty());
        assert!(canvas_offers("", &all).is_empty());
    }

    #[test]
    fn canvas_rows_sit_under_bookmarks() {
        let mut i = Inputs {
            typed: "canvas".into(),
            engine: "Google".into(),
            search: Some("https://www.google.com/search?q=canvas".into()),
            ..Default::default()
        };
        i.canvases = vec![c("Plan")];
        let o = palette(&i);
        assert_eq!(o.rows[0].title, "Open Canvas · Plan");
        assert_eq!(o.rows[0].hint, "Open");
        assert_eq!(headings(&o.rows), vec![(0, "Canvases"), (1, "Search")]);
        assert_eq!(o.ending, None, "never a canvas id");
    }
}
