//! Finding a word on the page (`Find.swift`): a pill in the stage's top
//! corner — the field, up, down, close — and the engine's in-page find
//! behind it. `Ctrl+F` opens it (or takes focus back to it), `Enter` /
//! `F3` look onward, `Shift+Enter` / `Shift+F3` back, `Escape` closes it.
//! The bar is built the first time it is wanted, not at start-up.

use crate::chrome::{Chrome, glyph};
use crate::commands::Command;
use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_engine::{FindOptions, PageView};
use copper_win_ui::tokens::Shadow;
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Event, Insets, Node, NodeId, Role, Swatch, Token};
use std::rc::Rc;

/// The bar's nodes.
pub struct Bar {
    pill: NodeId,
    field: NodeId,
    up: NodeId,
    down: NodeId,
    close: NodeId,
    open: bool,
    /// The last look found nothing: the rim turns red.
    missed: bool,
}

const FIELD_WIDTH: f32 = 160.0;
const STEP: f32 = 24.0;
const STEP_GLYPH: f32 = 10.0;
const CHEVRON_UP: char = '\u{E70E}';
const CHEVRON_DOWN: char = '\u{E70D}';

fn build(ui: &mut Ui, stage: NodeId) -> Bar {
    let pill = ui.tree.add(
        stage,
        Node::row()
            .absolute(Abs {
                left: None,
                top: Some(12.0),
                right: Some(14.0),
                bottom: None,
            })
            .padding(Insets::new(16.0, 8.0, 8.0, 8.0))
            .gap(6.0)
            .align(Align::Center)
            .fill(Token::Ground)
            .radius(20.0)
            .stroke(Token::Hairline, 1.0)
            .shadow(Shadow {
                alpha: 0.10,
                radius: 18.0,
                y: 5.0,
            })
            .layer()
            .hit()
            .role(Role::Group)
            .name("Find on page")
            .hidden(true),
    );
    let field = ui.tree.add(
        pill,
        Node::field("Find on page", 12.5)
            .width(FIELD_WIDTH)
            .height(STEP),
    );
    let step = |ui: &mut Ui, ch: char, name: &str| {
        let b = ui.tree.add(
            pill,
            Node::new()
                .size(STEP, STEP)
                .radius(6.0)
                .hover_fill(Token::Hover)
                .align(Align::Center)
                .button(name),
        );
        ui.tree.add(
            b,
            Node::glyph(ch, STEP_GLYPH, STEP)
                .ink(Token::Muted)
                .role(Role::None),
        );
        b
    };
    let up = step(ui, CHEVRON_UP, "Previous match");
    let down = step(ui, CHEVRON_DOWN, "Next match");
    let close = step(ui, glyph::CLOSE, "Close");
    Bar {
        pill,
        field,
        up,
        down,
        close,
        open: false,
        missed: false,
    }
}

/// The find commands.
pub fn register(shell: &Shell) {
    let r = shell.view().commands();
    let has_page = |s: &Shell| s.active_page().is_some();
    r.add(Command::new("find", "Find on Page", glyph::SEARCH, open).when(has_page));
    r.add(Command::key("find-next", |s| look(s, true)));
    r.add(Command::key("find-previous", |s| look(s, false)));
}

/// Open the bar (or focus it again), its text selected.
pub fn open(shell: &Shell) {
    if shell.active_page().is_none() {
        return;
    }
    let chrome = shell.view().chrome().clone();
    chrome.edit(|ui, st| {
        let stage = st.stage.stage;
        let bar = st.find.get_or_insert_with(|| build(ui, stage));
        bar.open = true;
        ui.tree.set_hidden(bar.pill, false);
        ui.focus(Some(bar.field));
        let field = bar.field;
        ui.edit_field(field, |e| e.select_all());
    });
}

/// Close the bar and clear the page's highlights; `false` if it was not
/// open.
pub fn close(shell: &Shell) -> bool {
    let chrome = shell.view().chrome().clone();
    let was = chrome
        .read(|ui, st| {
            let Some(bar) = st.find.as_mut().filter(|b| b.open) else {
                return false;
            };
            bar.open = false;
            ui.tree.set_hidden(bar.pill, true);
            true
        })
        .unwrap_or(false);
    if was {
        if let Some(p) = shell.active_page() {
            p.find_finish();
        }
        shell.view().focus_page();
    }
    was
}

/// The needle in the bar, if it is open.
fn needle(chrome: &Chrome) -> Option<String> {
    chrome
        .read(|ui, st| {
            let bar = st.find.as_ref().filter(|b| b.open)?;
            ui.field_text(bar.field).map(str::to_string)
        })
        .flatten()
}

/// Look for the needle again, onward or back.
fn look(shell: &Shell, forward: bool) {
    let chrome = shell.view().chrome().clone();
    if needle(&chrome).is_none() {
        open(shell);
        return;
    }
    if let Some(p) = shell.active_page() {
        if forward {
            p.find_next();
        } else {
            p.find_previous();
        }
    }
}

/// Look for `text` from the top of the page, and show whether it is there.
fn search(shell: &Shell, text: String) {
    let Some(page) = shell.active_page() else {
        return;
    };
    if text.is_empty() {
        page.find_finish();
        set_missed(shell.view().chrome(), false);
        return;
    }
    let chrome = Rc::downgrade(shell.view().chrome());
    let found = page.find(
        &text,
        FindOptions {
            wrap_around: true,
            ..FindOptions::default()
        },
    );
    copper_win_rt::spawn_local(async move {
        let missed = !matches!(found.await, Ok(n) if n > 0);
        if let Some(c) = chrome.upgrade() {
            set_missed(&c, missed);
        }
    });
}

fn set_missed(chrome: &Rc<Chrome>, missed: bool) {
    chrome.edit(move |ui, st| {
        let Some(bar) = st.find.as_mut() else { return };
        if bar.missed == missed {
            return;
        }
        bar.missed = missed;
        let rim = if missed {
            Swatch::Fixed(copper_win_ui::Color::rgba(0.86, 0.15, 0.15, 0.35))
        } else {
            Swatch::Token(Token::Hairline)
        };
        ui.tree.edit(bar.pill, |n| n.look.stroke = rim);
    });
}

/// A field event the card did not take: the bar's, if it is.
pub fn field_event(shell: &Shell, id: NodeId, e: &Event) {
    let chrome = shell.view().chrome().clone();
    let mine = chrome
        .read(|_, st| st.find.as_ref().is_some_and(|b| b.field == id))
        .unwrap_or(false);
    if !mine {
        return;
    }
    match e {
        Event::FieldChanged(_) => {
            let text = needle(&chrome).unwrap_or_default();
            search(shell, text);
        }
        Event::FieldSubmit(_) => {
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
            // SAFETY: reads this thread's key state.
            let back = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
            look(shell, !back);
        }
        Event::FieldCancel(_) => {
            close(shell);
        }
        _ => {}
    }
}

/// A click the rest of the chrome did not take; `true` if it was the
/// bar's.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let chrome = shell.view().chrome().clone();
    let which = chrome
        .read(|_, st| {
            let b = st.find.as_ref()?;
            if id == b.up {
                Some(0)
            } else if id == b.down {
                Some(1)
            } else if id == b.close {
                Some(2)
            } else {
                None
            }
        })
        .flatten();
    match which {
        Some(0) => look(shell, false),
        Some(1) => look(shell, true),
        Some(2) => {
            close(shell);
        }
        _ => return false,
    }
    true
}

/// Another tab is shown: the bar belongs to the page it was opened on.
pub fn tab_changed(shell: &Shell) {
    let chrome = shell.view().chrome().clone();
    chrome.edit(|ui, st| {
        if let Some(bar) = st.find.as_mut().filter(|b| b.open) {
            bar.open = false;
            ui.tree.set_hidden(bar.pill, true);
        }
    });
}

/// The shown page went somewhere new: what was found is gone.
pub fn page_moved(shell: &Shell, tab: &TabId) {
    if shell.active_id().as_ref() == Some(tab) {
        close(shell);
    }
}
