//! Finding a word on the page (`Find.swift`): a pill in the stage's top
//! corner — the field, up, down, close — and the engine's in-page find
//! behind it. `Ctrl+F` opens it (or takes focus back to it, the words left
//! as they are), `Enter` / `Down` / `F3` / `Ctrl+G` look onward,
//! `Shift+Enter` / `Up` / `Shift+F3` / `Ctrl+Shift+G` back, `Escape` or the
//! cross closes it and forgets the words. As on the Mac the bar stays up
//! when another tab is picked, and looks again in that tab's page.
//! The bar is built the first time it is wanted, not at start-up.

use crate::chrome::{Chrome, glyph};
use crate::commands::Command;
use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_engine::{FindOptions, PageView};
use copper_win_ui::input::{Key, Mods};
use copper_win_ui::tokens::{Shadow, motion};
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Event, Insets, Node, NodeId, Prop, Role, Swatch, Token};
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
    /// Bumped by every look from the top: an answer for an older one (or
    /// for another tab's page) is dropped.
    generation: u64,
}

const FIELD_WIDTH: f32 = 160.0;
const STEP: f32 = 24.0;
const STEP_GLYPH: f32 = 9.0;
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
            .placeholder_ink(Swatch::Fade(Token::Ink, 0.3))
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
        generation: 0,
    }
}

/// A tab with a page worth looking in (not a blank one).
fn has_page(shell: &Shell) -> bool {
    if shell.active_page().is_none() {
        return false;
    }
    shell
        .browser()
        .active()
        .is_some_and(|t| !t.url.is_empty() && !t.url.starts_with("about:blank"))
}

/// The find commands.
pub fn register(shell: &Shell) {
    let r = shell.view().commands();
    r.add(Command::new("find", "Find on Page…", glyph::SEARCH, open).when(has_page));
    r.add(Command::key("find-next", |s| look(s, true)));
    r.add(Command::key("find-previous", |s| look(s, false)));
}

/// Open the bar (or focus it again). The words already in it stay as
/// they are, unselected (`Find.swift`).
pub fn open(shell: &Shell) {
    if !has_page(shell) {
        return;
    }
    let chrome = shell.view().chrome().clone();
    chrome.edit(|ui, st| {
        let stage = st.stage.stage;
        let bar = st.find.get_or_insert_with(|| build(ui, stage));
        if !bar.open {
            bar.open = true;
            ui.tree.set_hidden(bar.pill, false);
            // In from the top, fading up (`.move(edge: .top)` + opacity).
            ui.tree.set_prop(bar.pill, Prop::Y, -8.0);
            ui.tree.set_prop(bar.pill, Prop::Opacity, 0.0);
            ui.tree.animate(bar.pill, Prop::Y, 0.0, motion::SETTLE);
            ui.tree.animate(bar.pill, Prop::Opacity, 1.0, motion::QUICK);
        }
        ui.focus(Some(bar.field));
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
            bar.missed = false;
            bar.generation += 1;
            ui.tree
                .edit(bar.pill, |n| n.look.stroke = Swatch::Token(Token::Hairline));
            ui.tree.set_hidden(bar.pill, true);
            // The words go with it (`closeFind` empties the needle).
            ui.set_field_text(bar.field, "", false);
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

/// Look for `text` from the top of the shown page, and show whether it is
/// there. Any look still in flight is overtaken.
fn search(shell: &Shell, text: String) {
    let chrome = shell.view().chrome().clone();
    let Some(generation) = chrome
        .read(|_, st| {
            st.find.as_mut().map(|b| {
                b.generation += 1;
                b.generation
            })
        })
        .flatten()
    else {
        return;
    };
    let page = shell.active_page().filter(|_| has_page(shell));
    let Some(page) = page.filter(|_| !text.is_empty()) else {
        if let Some(p) = shell.active_page() {
            p.find_finish();
        }
        set_missed(&chrome, generation, false);
        return;
    };
    let weak = Rc::downgrade(&chrome);
    let found = page.find(
        &text,
        FindOptions {
            wrap_around: true,
            ..FindOptions::default()
        },
    );
    copper_win_rt::spawn_local(async move {
        let missed = !matches!(found.await, Ok(n) if n > 0);
        if let Some(c) = weak.upgrade() {
            set_missed(&c, generation, missed);
        }
    });
}

/// Show whether look `generation` found anything — if it is still the
/// latest look.
fn set_missed(chrome: &Rc<Chrome>, generation: u64, missed: bool) {
    chrome.edit(move |ui, st| {
        let Some(bar) = st.find.as_mut() else { return };
        if bar.generation != generation || bar.missed == missed {
            return;
        }
        bar.missed = missed;
        let rim = if missed {
            // SwiftUI's `.red` at .35.
            Swatch::Fixed(copper_win_ui::Color::rgba(1.0, 0.23, 0.19, 0.35))
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

/// Up and Down in the bar's field look back and onward; `true` if the
/// key was the bar's.
pub fn key(shell: &Shell, key: Key, mods: Mods, focused: Option<NodeId>) -> bool {
    if mods != Mods::NONE || !matches!(key, Key::Up | Key::Down) {
        return false;
    }
    let chrome = shell.view().chrome().clone();
    let mine = chrome
        .read(|_, st| {
            st.find
                .as_ref()
                .is_some_and(|b| b.open && Some(b.field) == focused)
        })
        .unwrap_or(false);
    if mine {
        look(shell, key == Key::Down);
    }
    mine
}

/// Another tab is shown. The bar stays (`App.swift` keeps it over the
/// stage) and looks for the same words in that tab's page; a blank tab
/// finds nothing and shows no miss.
pub fn tab_changed(shell: &Shell) {
    if let Some(text) = needle(shell.view().chrome()) {
        search(shell, text);
    }
}

/// The shown page went somewhere new: what was found went with the old
/// document, and a miss there says nothing about this one.
pub fn page_moved(shell: &Shell, tab: &TabId) {
    if shell.active_id().as_ref() != Some(tab) {
        return;
    }
    let chrome = shell.view().chrome().clone();
    let generation = chrome
        .read(|_, st| {
            st.find.as_mut().filter(|b| b.open).map(|b| {
                b.generation += 1;
                b.generation
            })
        })
        .flatten();
    if let Some(g) = generation {
        set_missed(&chrome, g, false);
    }
}
