//! The cards' Windows half: gather what the ranking reads from the shell,
//! hand the card its rows, and do what the row taken says
//! (`Browser.submit`, `Launcher.take`, `CommandBar.run`).
//!
//! Every function here runs with nothing of the window borrowed: it reads
//! the card through [`Chrome::read`], asks the shell, then hands the card
//! what to draw through [`Chrome::edit`].
//!
//! [`Chrome::read`]: crate::chrome::Chrome::read
//! [`Chrome::edit`]: crate::chrome::Chrome::edit

use super::rank::{self, Action, Inputs, Kind, Mode, Row};
use super::symbols;
use crate::chrome::card::{RowMark, kind_glyph};
use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::Shell;
use copper_core::address::url_from_typed;
use copper_core::browser::TabId;
use copper_core::date::SwiftDate;
use copper_shell_core::OpenTab as Opening;
use copper_shell_core::palette::complete::Purpose;
use copper_win_ui::input::{Key, Mods};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

/// How a row is taken (`Launcher.How`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// Return: here (a blank tab, the tab being edited) or a new tab.
    Open,
    /// Shift+Return on `Ctrl+T`: a new tab behind this one.
    Background,
    /// Alt+Return. The Mac splits the window; Windows has no split, so it
    /// is a new tab in front.
    Split,
}

/// What the ranking calls each card.
fn mode(p: Purpose) -> Mode {
    match p {
        Purpose::Palette => Mode::Palette,
        Purpose::Launch => Mode::Launch,
        Purpose::Edit => Mode::Address,
    }
}

thread_local! {
    /// `Ctrl+K` pressed again with Ctrl still down: letting go of Ctrl
    /// takes the row the walk is on (`Browser.landSummon`).
    static CYCLING: Cell<bool> = const { Cell::new(false) };
    /// Every bookmark, flat, and the generation it was read at.
    static SHELF: RefCell<(u64, Vec<rank::Mark>)> = const { RefCell::new((u64::MAX, Vec::new())) };
    /// Closed tabs brought back from the middle of the list: shell-core
    /// only gives back the newest (`reopen_closed` pops it), so these stay
    /// in its list and are left out of the cards' Closed rows here.
    static TAKEN: RefCell<Vec<copper_core::browser::ClosedTab>> = const { RefCell::new(Vec::new()) };
}

/// The cards' own commands, Ctrl's release, the spaces' commands.
pub fn register(shell: &Shell) {
    let reg = shell.view().commands();
    reg.add(Command::new("new-space", "New Space", glyph::PLUS, |s| {
        super::spaces::new_space(s)
    }));
    reg.add(Command::new(
        "edit-space",
        "Edit Space…",
        glyph::SETTINGS,
        |s| {
            let id = s.browser().current_space_id().to_string();
            super::spaces::edit(s, &id);
        },
    ));
    reg.add(
        Command::new("delete-space", "Delete Space…", glyph::CLOSE, |s| {
            let id = s.browser().current_space_id().to_string();
            super::spaces::ask_delete(s, &id);
        })
        .when(|s| s.browser().spaces().count() > 1),
    );
    for n in 1..=9usize {
        reg.add(Command::key(&format!("space-{n}"), move |s| {
            s.switch_space_nth(n - 1)
        }));
    }
    // Escape with a panel (Settings) over the empty stage's standing
    // field: the panel goes, the field stays. Everything else is the
    // view's own Escape.
    reg.add(Command::key("escape", |s| {
        if !panel_over_standing(s) || !s.view().panels().hide(s) {
            s.view().escape(s);
        }
    }));
    // `Ctrl+L` is the plain field even with the tabs on top, where a
    // click on the live tab edits it in place.
    reg.add(Command::key("focus-field", plain_field));
    super::spaces::watch(shell);
    super::spaces::pointer(shell);
    crate::chrome::drag::pointer(shell);
    super::strip_edit::pointer(shell);
    // Ctrl let go of while `Ctrl+K` walks: take the row. The hook only
    // looks; the work runs on the loop's next turn.
    let weak = shell.downgrade();
    copper_win_rt::add_message_hook(move |m| {
        const WM_KEYUP: u32 = 0x0101;
        const WM_SYSKEYUP: u32 = 0x0105;
        const VK_CONTROL: usize = 0x11;
        if (m.message == WM_KEYUP || m.message == WM_SYSKEYUP)
            && m.wparam == VK_CONTROL
            && CYCLING.with(Cell::get)
        {
            CYCLING.with(|c| c.set(false));
            let weak = weak.clone();
            copper_win_rt::spawn_local(async move {
                if let Some(s) = weak.upgrade() {
                    land(&s);
                }
            });
        }
        false
    });
    let chrome = shell.view().chrome().clone();
    chrome.edit(|_, st| st.card.keep_clear(st.column.side, st.ids.plates));
    breathe(shell);
}

/// Open the card for `purpose` (`private`: a private tab's launch).
/// `Ctrl+K` with the card already up walks one row on instead.
pub fn open(shell: &Shell, purpose: Purpose, private: bool) {
    let chrome = shell.view().chrome().clone();
    let up = chrome
        .read(|_, st| (st.card.card_open() && !st.card.asking()).then(|| st.card.purpose()))
        .flatten();
    if purpose == Purpose::Palette && up == Some(Purpose::Palette) {
        CYCLING.with(|c| c.set(true));
        step(shell, 1);
        return;
    }
    let text = match purpose {
        Purpose::Edit => shell
            .browser()
            .active()
            .map(|t| t.url.clone())
            .filter(|u| !u.starts_with("about:blank"))
            .unwrap_or_default(),
        _ => String::new(),
    };
    CYCLING.with(|c| c.set(false));
    let over = !active_blank(shell) && shell.active_id().is_some();
    chrome.edit(move |ui, st| {
        st.card.set_over(over);
        st.card.open(ui, purpose, private, &text, false);
    });
    shell.view().overlay_changed();
    changed(shell);
}

/// A click on the address (or, tabs on top, the live tab): edit it, or
/// launch when there is nothing to edit.
pub fn edit_address(shell: &Shell) {
    // Tabs on top: the live tab itself becomes the address.
    if shell.view().layout() == copper_core::settings::TabsPosition::Top
        && super::strip_edit::begin(shell)
    {
        return;
    }
    plain_field(shell);
}

/// `Ctrl+L` (`Browser.edit`): the plain field, in either layout.
fn plain_field(shell: &Shell) {
    if shell.active_id().is_some() {
        open(shell, Purpose::Edit, false);
    } else {
        open(shell, Purpose::Launch, false);
    }
}

/// Put the card away (focus back to the page). With no tab left, the
/// plain field stands on the empty stage again.
pub fn close(shell: &Shell) {
    CYCLING.with(|c| c.set(false));
    let chrome = shell.view().chrome().clone();
    let empty = shell.active_id().is_none();
    chrome.edit(move |ui, st| {
        st.card.close(ui);
        if empty {
            st.card.open(ui, Purpose::Launch, false, "", true);
        }
    });
    shell.view().overlay_changed();
    shell.view().focus_page();
}

/// A panel (Settings) is up over the plain field standing on the empty
/// stage: Escape is the panel's.
fn panel_over_standing(shell: &Shell) -> bool {
    shell.view().panels().showing().is_some()
        && shell
            .view()
            .chrome()
            .read(|_, st| st.card.is_standing())
            .unwrap_or(false)
}

/// Escape in the field, a step at a time (`App.swift` 793–815): a picked
/// row is let go of first, then the card goes; `Ctrl+T` goes at once; the
/// plain field standing on an empty stage stays. With Settings over the
/// standing field, Settings goes first.
pub fn cancel(shell: &Shell) {
    if panel_over_standing(shell) && shell.view().panels().hide(shell) {
        return;
    }
    let chrome = shell.view().chrome().clone();
    let Some((purpose, picked, standing, asking)) = chrome.read(|_, st| {
        (
            st.card.purpose(),
            st.card.picked_index().is_some(),
            st.card.is_standing(),
            st.card.asking(),
        )
    }) else {
        return;
    };
    if asking {
        chrome.edit(|ui, st| {
            st.card.close(ui);
        });
        shell.view().overlay_changed();
        return;
    }
    match rank::escape(mode(purpose), picked, standing) {
        rank::Escape::Unpick => chrome.edit(|ui, st| {
            st.card.walked = true;
            st.card.pick(ui, None);
            let typed = st.card.typed.clone();
            if st.card.purpose() == Purpose::Edit {
                st.card.set_text(ui, &typed);
            }
        }),
        rank::Escape::Stay => {
            // Nothing behind it: the words go, the field stays.
            chrome.edit(|ui, st| st.card.set_text(ui, ""));
            changed(shell);
        }
        rank::Escape::Close => close(shell),
    }
}

/// The breath under the standing field: half a cycle each beat while it
/// stands. One loop for the window's life: between breaths it only looks.
fn breathe(shell: &Shell) {
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let mut out = true;
        loop {
            copper_win_rt::sleep(Duration::from_secs_f32(crate::chrome::card::BREATH_SECONDS))
                .await;
            let Some(s) = weak.upgrade() else { break };
            let chrome = s.view().chrome().clone();
            drop(s);
            if chrome.read(|_, st| st.card.is_breathing()) != Some(true) {
                out = true;
                continue;
            }
            chrome.edit(move |ui, st| st.card.breath_step(ui, out));
            out = !out;
        }
    });
}

/// The field changed: new rows, and the ending on offer.
pub fn changed(shell: &Shell) {
    let chrome = shell.view().chrome().clone();
    let Some((text, before, purpose, private, caret_at_end)) = chrome.read(|ui, st| {
        let before = std::mem::take(&mut st.card.typed);
        let text = st.card.text(ui);
        st.card.typed = text.clone();
        st.card.walked = false;
        st.card.typed_again(ui);
        let at_end = ui
            .tree
            .get(st.card.field)
            .and_then(|n| n.field_ref())
            .is_some_and(|f| f.edit.caret() == f.edit.text().len() && !f.edit.has_selection());
        (
            text,
            before,
            st.card.purpose(),
            st.card.is_private(),
            at_end,
        )
    }) else {
        return;
    };
    let began = std::time::Instant::now();
    let m = mode(purpose);
    // `copper://cloud/join?code=…`: Settings › Cloud, never a search.
    let offers = match copper_shell_core::cloud::join::palette_row(&text) {
        Some(join) => rank::Offers {
            rows: vec![Row {
                kind: Kind::Known,
                title: join.title,
                detail: join.detail,
                badge: String::new(),
                hint: join.hint,
                action: Action::Go(text.trim().to_string()),
                url: None,
                glyph: Some(glyph::CLOUD),
                section: rank::Section::Plain,
            }],
            default: Some(0),
            ending: None,
        },
        None => rank::offers(m, &gather(shell, &text, m, private)),
    };
    // The plain field puts its ending in the field, selected, only while
    // the words grow at the end; a backspace means it was not wanted.
    let grew = text.len() > before.len() && text.starts_with(&before);
    let inline = (m == Mode::Address && grew && caret_at_end)
        .then(|| offers.ending.clone())
        .flatten();
    let ending = (m != Mode::Address)
        .then(|| offers.ending.clone())
        .flatten();
    let marks = marks(shell, &offers.rows);
    let ranked = began.elapsed();
    chrome.edit(move |ui, st| {
        // Typing reached the field after this was read: its own change
        // will bring its rows.
        if st.card.text(ui) != text {
            return;
        }
        let drawing = std::time::Instant::now();
        let count = offers.rows.len();
        let mut marks = marks;
        let launch = m == Mode::Launch;
        st.card.set_rows(
            ui,
            &mut st.images,
            offers.rows,
            offers.default,
            |row| {
                marks
                    .remove(&row.identity())
                    .unwrap_or_else(|| fallback_mark(m, row))
            },
            |row| {
                if launch {
                    rank::aside(row)
                } else {
                    String::new()
                }
            },
        );
        st.card.show_ending(ui, ending);
        if let Some(rest) = inline {
            st.card.offer(ui, &rest);
        }
        // Our share of a keystroke's frame: the ranking, then the rows.
        tracing::debug!(
            target: "copper::perf",
            "palette rows: {:.2} ms ranked, {:.2} ms drawn ({count} rows)",
            ranked.as_secs_f64() * 1000.0,
            drawing.elapsed().as_secs_f64() * 1000.0
        );
    });
}

/// A row's picture when nothing better was found for it.
fn fallback_mark(m: Mode, row: &Row) -> RowMark {
    if m == Mode::Address {
        return match row.kind {
            Kind::Search => RowMark::Glyph(glyph::SEARCH),
            Kind::Bookmark => RowMark::Glyph(glyph::STAR),
            Kind::Command => RowMark::Glyph(glyph::GEAR),
            Kind::Open => RowMark::Dot,
            _ => RowMark::None,
        };
    }
    RowMark::Glyph(row.glyph.unwrap_or_else(|| kind_glyph(row.kind)))
}

/// Return in the field (`alt`: Alt was down). Shift is read here: the
/// field sends Shift+Return as a plain submit.
pub fn submit(shell: &Shell, alt: bool) {
    let how = if alt {
        How::Split
    } else if shift_down() {
        How::Background
    } else {
        How::Open
    };
    take(shell, how);
}

/// Take the picked row (or what was typed) `how`.
fn take(shell: &Shell, how: How) {
    let chrome = shell.view().chrome().clone();
    let Some((full, typed, ending, row, purpose, private)) = chrome.read(|ui, st| {
        (
            st.card.text(ui),
            st.card.typed.clone(),
            st.card.ending.clone(),
            st.card.picked().cloned(),
            st.card.purpose(),
            st.card.is_private(),
        )
    }) else {
        return;
    };
    // `Ctrl+K`'s and `Ctrl+L`'s Shift is nothing of theirs.
    let how = if purpose == Purpose::Launch || how == How::Split {
        how
    } else {
        How::Open
    };
    let row = match row {
        Some(r) => r,
        None => {
            if purpose == Purpose::Palette && typed.trim().is_empty() {
                close(shell);
                return;
            }
            // What the field was finishing for you, else what was typed.
            let target = match (&ending, purpose) {
                (Some(e), Purpose::Palette | Purpose::Launch) if !e.is_empty() => {
                    url_from_typed(&format!("{typed}{e}")).map(|u| u.to_string())
                }
                _ => None,
            }
            .or_else(|| {
                if purpose == Purpose::Edit && full != typed {
                    url_from_typed(&full).map(|u| u.to_string())
                } else {
                    None
                }
            })
            .or_else(|| shell.destination(&typed));
            let Some(url) = target else {
                if typed.trim().is_empty() && purpose != Purpose::Edit {
                    close(shell);
                } else {
                    chrome.edit(|ui, st| st.card.refuse(ui));
                }
                return;
            };
            Row {
                kind: Kind::Known,
                title: typed.clone(),
                detail: String::new(),
                badge: String::new(),
                hint: String::new(),
                action: Action::Go(url),
                url: None,
                glyph: None,
                section: rank::Section::Plain,
            }
        }
    };
    close_for(shell);
    perform(shell, &row, purpose, private, how);
}

/// The card goes before what it asked for happens (no empty-stage field
/// in between: a tab is on its way).
fn close_for(shell: &Shell) {
    CYCLING.with(|c| c.set(false));
    let chrome = shell.view().chrome().clone();
    chrome.edit(|ui, st| {
        st.card.close(ui);
    });
    shell.view().overlay_changed();
}

/// A row was clicked.
pub fn fire(shell: &Shell, index: usize) {
    let chrome = shell.view().chrome().clone();
    let Some(Some((row, purpose, private))) = chrome.read(|_, st| {
        let row = st.card.row(index).cloned()?;
        Some((row, st.card.purpose(), st.card.is_private()))
    }) else {
        return;
    };
    close_for(shell);
    perform(shell, &row, purpose, private, How::Open);
}

/// `Ctrl+K` let go of after walking: take the row it landed on.
fn land(shell: &Shell) {
    let picked = shell
        .view()
        .chrome()
        .read(|_, st| st.card.card_open() && st.card.picked_index().is_some())
        .unwrap_or(false);
    if picked {
        take(shell, How::Open);
    }
}

/// Up / Down: one row; off either end lets go (`Browser.walk`).
pub fn step(shell: &Shell, delta: i32) {
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, st| {
        let at = rank::walk(st.card.row_count(), st.card.picked_index(), delta);
        st.card.walked = true;
        st.card.pick(ui, at);
        // The plain field shows the row the arrows are on.
        if st.card.purpose() == Purpose::Edit {
            let words = match at.and_then(|i| st.card.row(i)) {
                Some(r) => r.title.clone(),
                None => st.card.typed.clone(),
            };
            let typed = st.card.typed.clone();
            ui.set_field_text(st.card.field, &words, false);
            st.card.typed = typed;
        }
    });
}

/// Tab / Shift+Tab on the cards: the next (previous) group's first row.
pub fn jump(shell: &Shell, forward: bool) {
    let chrome = shell.view().chrome().clone();
    let plain = chrome
        .read(|_, st| st.card.purpose() == Purpose::Edit)
        .unwrap_or(false);
    if plain {
        // The plain field's Tab takes what it offers; Shift+Tab does
        // nothing rather than throw the keyboard somewhere unexpected.
        if forward {
            accept(shell);
        }
        return;
    }
    chrome.edit(move |ui, st| {
        let rows: Vec<Row> = st.card.rows().cloned().collect();
        let at = rank::jump(&rows, st.card.picked_index(), forward);
        st.card.walked = true;
        st.card.pick(ui, at);
    });
}

/// Take what the field offers into it, the caret after it
/// (`Browser.acceptCompletion`). Nothing opens.
pub fn accept(shell: &Shell) -> bool {
    let chrome = shell.view().chrome().clone();
    let Some((purpose, full, typed, ending, picked, walked)) = chrome.read(|ui, st| {
        (
            st.card.purpose(),
            st.card.text(ui),
            st.card.typed.clone(),
            st.card.ending.clone(),
            st.card.picked().cloned(),
            st.card.walked,
        )
    }) else {
        return false;
    };
    let m = mode(purpose);
    // The plain field's ending is in the field already, selected.
    let ending = if m == Mode::Address {
        full.strip_prefix(typed.as_str())
            .filter(|r| !r.is_empty())
            .map(str::to_string)
    } else {
        ending
    };
    let Some(text) = rank::accept(m, &typed, ending.as_deref(), picked.as_ref(), walked) else {
        return false;
    };
    chrome.edit(move |ui, st| st.card.set_text(ui, &text));
    changed(shell);
    true
}

/// A key the focused card field did not use. `true`: it was the card's.
pub fn key(shell: &Shell, key: Key, mods: Mods) -> bool {
    let chrome = shell.view().chrome().clone();
    let Some((purpose, rows)) = chrome.read(|_, st| (st.card.purpose(), st.card.row_count()))
    else {
        return false;
    };
    match key {
        Key::Down if !mods.ctrl && !mods.alt => step(shell, 1),
        Key::Up if !mods.ctrl && !mods.alt => step(shell, -1),
        Key::Tab if !mods.ctrl && !mods.alt => jump(shell, !mods.shift),
        // The field leaves Alt to the window: Alt+Return comes here.
        Key::Enter if mods.alt && !mods.ctrl => take(shell, How::Split),
        // `Ctrl+T`'s Ctrl+1…9: that row, without walking to it.
        Key::Digit(d) if purpose == Purpose::Launch && mods.ctrl && !mods.alt && !mods.shift => {
            let i = usize::from(d).wrapping_sub(1);
            if (1..=9).contains(&d) && i < rows {
                fire(shell, i);
            }
        }
        _ => return false,
    }
    true
}

/// A click the rest of the chrome did not claim: the delete question's
/// buttons.
pub fn click(shell: &Shell, id: copper_win_ui::NodeId) -> bool {
    super::spaces::answered(shell, id)
}

/// Shift held right now.
fn shift_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
    // SAFETY: reads the calling thread's key state.
    unsafe { GetKeyState(i32::from(VK_SHIFT.0)) < 0 }
}

/// Do what `row` says, `how`.
pub fn perform(shell: &Shell, row: &Row, purpose: Purpose, private: bool, how: How) {
    let new_tab = how != How::Open;
    match &row.action {
        Action::Switch(id) => switch_to(shell, id),
        Action::Command(id) => {
            let space = id
                .strip_prefix("space-")
                .filter(|s| shell.browser().space(s).is_some());
            match space {
                Some(space) => {
                    shell.switch_space(space);
                }
                None => {
                    crate::commands::run(shell, id);
                }
            }
        }
        Action::Reopen { url, .. } => reopen_ghost(shell, url),
        // `copper://cloud/…` links open Settings › Cloud; `copper://canvas/…`
        // opens in the canvas's own way.
        Action::Go(url) if crate::cloud::open_link(shell, url) => {}
        Action::Go(url) if crate::canvas::go(shell, url, purpose == Purpose::Edit && !new_tab) => {}
        Action::Go(url) => match purpose {
            // `Ctrl+L` and `Ctrl+K` go in the tab you are on (`Browser.submit`).
            Purpose::Edit | Purpose::Palette if !new_tab && shell.active_id().is_some() => {
                if let Some(id) = shell.active_id() {
                    shell.navigate(&id, url);
                }
            }
            // `Ctrl+T` over a blank tab uses it (`Launcher.take`).
            Purpose::Launch if how == How::Open && !private && active_blank(shell) => {
                if let Some(id) = shell.active_id() {
                    shell.navigate(&id, url);
                }
            }
            _ => {
                let private = private || (purpose != Purpose::Launch && shell.active_is_private());
                shell.open(
                    url,
                    Opening {
                        foreground: how != How::Background,
                        private,
                        ..Default::default()
                    },
                );
                if how == How::Background {
                    shell.toast("Opened in the background");
                }
            }
        },
    }
    shell.view().focus_page();
}

/// A closed tab, by address (`Browser.reopen(_ ghost:)`): the newest is
/// shell-core's own `reopen_closed`; any other comes back the same way —
/// in its space at the place it had, in front, in Saved again if it was.
/// Like the Mac's, it starts a fresh history at its address: a closed tab
/// keeps no back/forward list (`ClosedTab` and the Mac's `Ghost` are
/// address, title and place only).
fn reopen_ghost(shell: &Shell, url: &str) {
    let ghosts = untaken(shell);
    let newest = ghosts.last().cloned();
    let ghost = ghosts.into_iter().rev().find(|g| g.url == url);
    // The newest of all: shell-core's own reopen, which also lets it go.
    if ghost.is_some() && ghost == newest && shell.browser().closed_tabs().last() == ghost.as_ref()
    {
        shell.reopen();
        return;
    }
    let Some(g) = ghost else {
        shell.open(
            url,
            Opening {
                foreground: true,
                ..Default::default()
            },
        );
        return;
    };
    shell.with_browser(|b| {
        let id = b.open_tab(
            &g.url,
            copper_core::browser::OpenOptions {
                title: g.title.clone(),
                // A space that has gone since: the one you are in.
                space: Some(g.space.clone()),
                index: Some(g.index),
                foreground: true,
                ..Default::default()
            },
        );
        b.update_tab(&id, |t| {
            t.group = g.group.clone();
            t.saved = g.saved;
        });
    });
    TAKEN.with(|t| t.borrow_mut().push(g));
}

/// Shell-core's closed tabs, oldest first, less the ones already brought
/// back from the middle (each taken once).
fn untaken(shell: &Shell) -> Vec<copper_core::browser::ClosedTab> {
    let closed = shell.browser().closed_tabs().to_vec();
    TAKEN.with(|t| {
        let mut t = t.borrow_mut();
        // Forget the ones shell-core has let go of since.
        let mut left = closed.clone();
        t.retain(|g| match left.iter().position(|c| c == g) {
            Some(i) => {
                left.remove(i);
                true
            }
            None => false,
        });
        let mut skip = t.clone();
        closed
            .into_iter()
            .filter(|c| match skip.iter().position(|g| g == c) {
                Some(i) => {
                    skip.remove(i);
                    false
                }
                None => true,
            })
            .collect()
    })
}

/// The selected tab shows nothing yet.
fn active_blank(shell: &Shell) -> bool {
    shell
        .browser()
        .active()
        .is_some_and(|t| t.url.is_empty() || t.url.starts_with("about:blank"))
}

/// Select tab `id`, in its own space (`Spaces.reveal`).
fn switch_to(shell: &Shell, id: &TabId) {
    let space = {
        let b = shell.browser();
        let current = b.current_space_id().to_string();
        b.tab(id)
            .filter(|t| !t.is_pinned() && t.space != current)
            .map(|t| t.space.clone())
    };
    if let Some(space) = space {
        shell.switch_space(&space);
    }
    shell.select(id);
}

// MARK: - what the ranking reads

/// Everything the ranking reads for `typed`, from the shell.
fn gather(shell: &Shell, typed: &str, m: Mode, private: bool) -> Inputs {
    let engine = shell.settings().search_engine();
    let trimmed = typed.trim();
    let address = url_from_typed(trimmed).map(|u| u.to_string());
    let search = (!trimmed.is_empty())
        .then(|| engine.url_for(trimmed).map(|u| u.to_string()))
        .flatten();
    let mut inputs = Inputs {
        typed: typed.to_string(),
        engine: engine.name.to_string(),
        search,
        address,
        blank: active_blank(shell),
        ..Default::default()
    };
    if m == Mode::Address {
        // The plain field asks history alone (`Browser.guess`).
        inputs.visits = shell
            .history()
            .suggestions(typed, 3)
            .into_iter()
            .map(|s| rank::Visit {
                key: s.key,
                title: s.title,
                url: s.url,
            })
            .collect();
        return inputs;
    }
    let (open, parked) = tabs(shell, private);
    inputs.open = open;
    inputs.parked = parked;
    if !trimmed.is_empty() {
        inputs.visits = visits(shell, trimmed, 24);
        inputs.marks = shelf(shell);
        inputs.canvases = canvases();
    }
    inputs.closed = untaken(shell)
        .iter()
        .map(|c| rank::Closed {
            title: if c.title.is_empty() {
                c.url.clone()
            } else {
                c.title.clone()
            },
            url: c.url.clone(),
        })
        .collect();
    if m == Mode::Palette {
        inputs.commands = commands(shell);
        inputs.resting = ["new-tab", "new-space", "show-history"]
            .iter()
            .filter_map(|id| inputs.commands.iter().find(|c| c.id == *id).cloned())
            .collect();
    }
    inputs
}

/// This window's tabs (this space and the pins, the selected and blank
/// ones left out) and the ones parked in other spaces, most recently
/// looked at first.
fn tabs(shell: &Shell, private: bool) -> (Vec<rank::OpenTab>, Vec<rank::OpenTab>) {
    let b = shell.browser();
    let current = b.current_space_id().to_string();
    let active = b.active_id();
    let view = |t: &copper_core::Tab, here: bool| -> rank::OpenTab {
        let sp = b.space(&t.space);
        rank::OpenTab {
            id: t.id.clone(),
            title: t.label(),
            url: t.url.clone(),
            current: here,
            pinned: t.is_pinned(),
            saved: t.saved,
            space: sp
                .map(|s| symbols::title(s.icon.as_deref(), &s.name))
                .unwrap_or_default(),
            space_emoji: sp.and_then(|s| symbols::emoji(s.icon.as_deref(), &s.name)),
            seen: t.seen.0,
        }
    };
    let usable = |t: &&copper_core::Tab| {
        Some(&t.id) != active.as_ref()
            && !t.url.is_empty()
            && !t.url.starts_with("about:blank")
            && (private || !t.private)
    };
    let mut here: Vec<rank::OpenTab> = b
        .pins()
        .into_iter()
        .chain(b.row(&current))
        .filter(usable)
        .map(|t| view(t, true))
        .collect();
    here.sort_by(|a, b| b.seen.total_cmp(&a.seen));
    let mut away: Vec<rank::OpenTab> = Vec::new();
    for sp in b.spaces().filter(|s| s.id != current) {
        away.extend(
            b.row(&sp.id)
                .into_iter()
                .filter(usable)
                .map(|t| view(t, false)),
        );
    }
    away.sort_by(|a, b| b.seen.total_cmp(&a.seen));
    (here, away)
}

/// History's best places for `typed` (`Launcher.Ranker.rank`): the match,
/// a bare front door's credit, and how often and lately it was visited.
fn visits(shell: &Shell, typed: &str, limit: usize) -> Vec<rank::Visit> {
    let Some(needle) = rank::Needle::new(typed) else {
        return Vec::new();
    };
    let h = shell.history();
    let now = SwiftDate::now();
    let mut scored: Vec<(f64, &copper_core::history::Visit)> = h
        .visits()
        .filter_map(|v| {
            let m = needle.score(&v.key, &v.title.to_lowercase())?;
            let door = if v.key.contains('/') { 0.0 } else { 4.0 };
            Some((m + door + 12.0 * v.frecency(now).ln_1p(), v))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.key.len().cmp(&b.1.key.len()))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, v)| rank::Visit {
            key: v.key.clone(),
            title: v.title.clone(),
            url: v.url.clone(),
        })
        .collect()
}

/// Every bookmark, flat, worked out once per change of the shelf.
fn shelf(shell: &Shell) -> Vec<rank::Mark> {
    let store = shell.bookmarks();
    let generation = store.generation();
    SHELF.with(|s| {
        let mut s = s.borrow_mut();
        if s.0 != generation {
            let mut out = Vec::new();
            flatten(store.roots(), &mut out);
            *s = (generation, out);
        }
        s.1.clone()
    })
}

fn flatten(nodes: &[copper_core::bookmarks::Bookmark], out: &mut Vec<rank::Mark>) {
    for n in nodes {
        if let Some(url) = &n.url {
            out.push(rank::Mark {
                title: n.title.clone(),
                url: url.clone(),
            });
        }
        if let Some(c) = &n.children {
            flatten(c, out);
        }
    }
}

/// Every canvas you can open, newest first.
fn canvases() -> Vec<rank::Canvas> {
    use copper_shell_core::canvas::{registry, url_for};
    let mut all = registry::visible();
    all.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
    all.into_iter()
        .map(|e| rank::Canvas {
            shared: registry::is_shared(&e),
            personal: registry::is_personal(&e),
            url: url_for(&e.id),
            name: e.name,
        })
        .collect()
}

/// Every command the palette may offer, and a door to each other space.
fn commands(shell: &Shell) -> Vec<rank::Cmd> {
    let reg = shell.view().commands();
    let mut out: Vec<rank::Cmd> = reg
        .palette_rows(shell)
        .into_iter()
        .map(|c| rank::Cmd {
            glyph: reg.glyph(&c.id),
            id: c.id,
            name: c.name,
        })
        .collect();
    out.extend(
        crate::actions::space_commands(shell)
            .into_iter()
            .map(|(id, name)| rank::Cmd {
                id,
                name,
                glyph: Some(glyph::FORWARD),
            }),
    );
    out
}

/// The pictures for the rows: an open tab's icon, else the site's kept one
/// (asked for once when there is none, `Shell::want_mark`), else a letter.
fn marks(shell: &Shell, rows: &[Row]) -> HashMap<String, RowMark> {
    let icons = shell.favicons().borrow();
    let kept = |row: &Row| -> Option<RowMark> {
        let url = row.url.as_deref()?;
        let hit = shell.cached_icon(url);
        if hit.is_none() {
            shell.want_mark(url);
        }
        hit.map(RowMark::Icon)
    };
    rows.iter()
        .filter_map(|row| {
            let mark = match &row.action {
                Action::Switch(id) => match icons.get(id) {
                    Some(i) => RowMark::Icon(i.clone()),
                    None => kept(row).or_else(|| letter(row).map(RowMark::Letter))?,
                },
                Action::Go(_) if matches!(row.kind, Kind::Visited | Kind::Bookmark) => {
                    if row.section == rank::Section::Plain {
                        return None;
                    }
                    kept(row).or_else(|| letter(row).map(RowMark::Letter))?
                }
                _ => return None,
            };
            Some((row.identity(), mark))
        })
        .collect()
}

/// A place's letter (`Mark`): its host's first, uppercased.
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
