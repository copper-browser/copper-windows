//! The card's logic (Linux `palette/mod.rs`): what is offered for what was
//! typed — ranked by `copper_shell_core::palette::complete`, shared with
//! Linux and the Mac — and what picking a row does. The card itself is
//! [`crate::chrome::card`].
//!
//! Every function here runs with nothing of the window's borrowed: it
//! reads the card through [`Chrome::read`], asks the shell, then hands the
//! card what to draw through [`Chrome::edit`].
//!
//! [`Chrome::read`]: crate::chrome::Chrome::read
//! [`Chrome::edit`]: crate::chrome::Chrome::edit

use crate::chrome::card::{RowMark, kind_glyph};
use crate::chrome::glyph;
use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_shell_core::OpenTab;
use copper_shell_core::palette::complete::{
    self, Action, CommandRow, Inputs, Kind, OpenRow, Purpose, Row,
};
use std::collections::HashMap;

/// Open the card for `purpose` (`private`: a private tab's launch).
pub fn open(shell: &Shell, purpose: Purpose, private: bool) {
    let text = match purpose {
        Purpose::Edit => shell
            .browser()
            .active()
            .map(|t| t.url.clone())
            .filter(|u| !u.starts_with("about:blank"))
            .unwrap_or_default(),
        _ => String::new(),
    };
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, st| st.card.open(ui, purpose, private, &text, false));
    shell.view().overlay_changed();
    if purpose == Purpose::Palette {
        changed(shell);
    }
}

/// `Ctrl+L` / a click on the address: edit it, or launch when there is
/// nothing to edit.
pub fn edit_address(shell: &Shell) {
    if shell.active_id().is_some() {
        open(shell, Purpose::Edit, false);
    } else {
        open(shell, Purpose::Launch, false);
    }
}

/// Put the card away (focus back to the page).
pub fn close(shell: &Shell) {
    let chrome = shell.view().chrome().clone();
    chrome.edit(|ui, st| {
        st.card.close(ui);
    });
    shell.view().overlay_changed();
    shell.view().focus_page();
}

/// The field changed: new rows, and maybe an inline completion.
pub fn changed(shell: &Shell) {
    let chrome = shell.view().chrome().clone();
    let Some((text, before, purpose, caret_at_end)) = chrome.read(|ui, st| {
        let before = std::mem::take(&mut st.card.typed);
        let text = st.card.text(ui);
        st.card.typed = text.clone();
        let at_end = ui
            .tree
            .get(st.card.field)
            .and_then(|n| n.field_ref())
            .is_some_and(|f| f.edit.caret() == f.edit.text().len() && !f.edit.has_selection());
        (text, before, st.card.purpose(), at_end)
    }) else {
        return;
    };
    let commands = palette_commands(shell, purpose);
    let mut inputs = inputs(shell, &text, &text, purpose, commands);
    let rest = if complete::grew(&before, &text) && caret_at_end {
        complete::completion(purpose, &inputs)
    } else {
        None
    };
    let full = rest.as_ref().map(|r| format!("{text}{r}"));
    if let Some(full) = &full
        && inputs.is_address
    {
        inputs.full = full;
        inputs.destination = shell.destination(full);
    }
    let offers = complete::offers(purpose, &inputs);
    let marks = marks(shell, &offers.rows);
    chrome.edit(move |ui, st| {
        // Typing reached the field after this was read: its own change
        // will bring its rows.
        if st.card.text(ui) != text {
            return;
        }
        let mut marks = marks;
        st.card
            .set_rows(ui, &mut st.images, offers.rows, offers.default, |row| {
                marks
                    .remove(&row_key(row))
                    .unwrap_or(RowMark::Glyph(kind_glyph(row.kind)))
            });
        if let Some(rest) = rest {
            st.card.offer(ui, &rest);
        }
    });
}

/// Return in the field (`new_tab`: Alt+Return, into a new tab).
pub fn submit(shell: &Shell, new_tab: bool) {
    let chrome = shell.view().chrome().clone();
    let Some((full, row, purpose, private)) = chrome.read(|ui, st| {
        (
            st.card.text(ui),
            st.card.picked().cloned(),
            st.card.purpose(),
            st.card.is_private(),
        )
    }) else {
        return;
    };
    let row = match row {
        Some(r) => r,
        None => {
            let (tabs, visited) = shell.suggestions(&full, 6);
            let known = complete::known_place(&full, &tabs, &visited).map(|k| k.url.clone());
            let Some(dest) = known.or_else(|| shell.destination(&full)) else {
                return;
            };
            Row {
                kind: Kind::Address,
                title: full.clone(),
                detail: String::new(),
                hint: String::new(),
                action: Action::Go(dest),
                url: None,
                icon: None,
                section: None,
            }
        }
    };
    close(shell);
    perform(shell, &row, purpose, private, new_tab);
}

/// A row was clicked.
pub fn fire(shell: &Shell, index: usize) {
    let chrome = shell.view().chrome().clone();
    let Some(Some((row, purpose, private))) = chrome.read(|_, st| {
        let row = st.card.rows().nth(index).cloned()?;
        Some((row, st.card.purpose(), st.card.is_private()))
    }) else {
        return;
    };
    close(shell);
    perform(shell, &row, purpose, private, false);
}

/// Up / Down / Tab in the card.
pub fn step(shell: &Shell, delta: i32) {
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, st| st.card.step(ui, delta));
}

/// Tab / Shift+Tab: the next (previous) group's first row.
pub fn jump(shell: &Shell, forward: bool) {
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, st| {
        let rows: Vec<Row> = st.card.rows().cloned().collect();
        let at = complete::jump(&rows, st.card.picked_index(), forward);
        st.card.pick(ui, at);
    });
}

/// Do what `row` says.
pub fn perform(shell: &Shell, row: &Row, purpose: Purpose, private: bool, new_tab: bool) {
    match &row.action {
        Action::Switch(id) => switch_to(shell, id),
        Action::Command(id) => {
            if let Some(space) = id.strip_prefix("space-") {
                shell.switch_space(space);
            } else {
                crate::commands::run(shell, id);
            }
        }
        Action::Go(url) if crate::cloud::open_link(shell, url) => {}
        Action::Go(url) => match purpose {
            Purpose::Edit if !new_tab && shell.active_id().is_some() => {
                if let Some(id) = shell.active_id() {
                    shell.navigate(&id, url);
                }
            }
            _ => {
                let private = private || (purpose == Purpose::Edit && shell.active_is_private());
                shell.open(
                    url,
                    OpenTab {
                        foreground: true,
                        private,
                        ..Default::default()
                    },
                );
            }
        },
    }
    shell.view().focus_page();
}

/// Select tab `id`, going to its space first when it lives in another.
fn switch_to(shell: &Shell, id: &TabId) {
    let (space, pinned) = {
        let b = shell.browser();
        let t = b.tab(id);
        (t.map(|t| t.space.clone()), t.is_some_and(|t| t.is_pinned()))
    };
    if let Some(space) = space
        && !pinned
        && space != shell.browser().current_space_id()
    {
        shell.switch_space(&space);
    }
    shell.select(id);
}

/// The commands the palette offers (none for the address card).
fn palette_commands(shell: &Shell, purpose: Purpose) -> Vec<CommandRow> {
    if purpose != Purpose::Palette {
        return Vec::new();
    }
    let mut rows = shell.view().commands().palette_rows(shell);
    rows.extend(
        crate::actions::space_commands(shell)
            .into_iter()
            .map(|(id, name)| CommandRow {
                id,
                name,
                icon: glyph::FORWARD.to_string(),
                shortcut: None,
            }),
    );
    rows
}

/// What the ranking reads, gathered from the shell.
fn inputs<'a>(
    shell: &Shell,
    typed: &'a str,
    full: &'a str,
    purpose: Purpose,
    commands: Vec<CommandRow>,
) -> Inputs<'a> {
    let engine = shell.settings().search_engine();
    // Whether it is an address is a question about what was typed, not the
    // completion offered after it ("git" + "hub.com" is still words).
    let is_address = copper_core::address::url_from_typed(typed).is_some();
    let destination = shell.destination(if is_address { full } else { typed });
    let (tabs, visited) = shell.suggestions(typed, 6);
    let bookmarks = if typed.trim().is_empty() {
        Vec::new()
    } else {
        shell.bookmarks().search(typed)
    };
    let open = if purpose == Purpose::Palette && typed.trim().is_empty() {
        open_rows(shell)
    } else {
        Vec::new()
    };
    Inputs {
        typed,
        full,
        engine: engine.name,
        destination,
        is_address,
        tabs,
        visited,
        bookmarks,
        commands,
        open,
    }
}

/// Every open tab, most recently used first: this space, then the others.
fn open_rows(shell: &Shell) -> Vec<OpenRow> {
    let b = shell.browser();
    let current = b.current_space_id().to_string();
    let active = b.active_id();
    let mut here: Vec<&copper_core::Tab> = b
        .pins()
        .into_iter()
        .chain(b.row(&current))
        .filter(|t| Some(&t.id) != active.as_ref())
        .collect();
    here.sort_by(|a, b| b.seen.0.total_cmp(&a.seen.0));
    let mut out: Vec<OpenRow> = here
        .into_iter()
        .map(|t| OpenRow {
            id: t.id.clone(),
            title: t.label(),
            url: t.url.clone(),
            elsewhere: None,
        })
        .collect();
    for sp in b.spaces().filter(|s| s.id != current) {
        let mut row = b.row(&sp.id);
        row.sort_by(|a, b| b.seen.0.total_cmp(&a.seen.0));
        out.extend(row.into_iter().map(|t| OpenRow {
            id: t.id.clone(),
            title: t.label(),
            url: t.url.clone(),
            elsewhere: Some(sp.name.clone()),
        }));
    }
    out
}

/// A row's identity, to match it to the picture gathered for it.
fn row_key(row: &Row) -> String {
    match &row.action {
        Action::Switch(id) => format!("t:{}", id.0),
        Action::Command(id) => format!("c:{id}"),
        Action::Go(url) => format!("g:{url}"),
    }
}

/// The pictures for the rows: an open tab's icon, a command's glyph.
fn marks(shell: &Shell, rows: &[Row]) -> HashMap<String, RowMark> {
    let icons = shell.favicons().borrow();
    let commands = shell.view().commands();
    rows.iter()
        .filter_map(|row| {
            let mark = match &row.action {
                Action::Switch(id) => icons.get(id).cloned().map(RowMark::Icon),
                Action::Command(id) if id.starts_with("space-") => {
                    Some(RowMark::Glyph(glyph::FORWARD))
                }
                Action::Command(id) => commands.glyph(id).map(RowMark::Glyph),
                // A place you have been or kept: its letter (`Mark`); the
                // typed text itself keeps its kind's glyph.
                Action::Go(_) if matches!(row.kind, Kind::Visited | Kind::Bookmark) => {
                    letter(row).map(RowMark::Letter)
                }
                Action::Go(_) => None,
            }?;
            Some((row_key(row), mark))
        })
        .collect()
}

/// A place's letter (`Mark`): its key's first, uppercased.
fn letter(row: &Row) -> Option<String> {
    let key = row
        .url
        .as_deref()
        .and_then(|u| url::Url::parse(u).ok())
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_string())
        })
        .unwrap_or_else(|| row.title.clone());
    key.chars().next().map(|c| c.to_uppercase().to_string())
}
