//! "Change Letter" on a pinned tab (`Browser.swift` `editLetter` /
//! `letter(_:for:)`). The Mac types into the pin's square itself; here a
//! small pill drops into the top of the stage with one short field
//! holding the letter, selected. `Enter` keeps the first character typed
//! (empty leaves the letter as it was: a pin with nothing on it could
//! never be told apart), `Escape` leaves it alone.

use crate::chrome::glyph;
use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_win_ui::tokens::{Shadow, motion};
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Event, Insets, Node, NodeId, Prop, Role, Token, Weight};

/// The pill's nodes, and the pin it is changing.
pub struct Bar {
    pill: NodeId,
    title: NodeId,
    field: NodeId,
    tab: Option<TabId>,
}

const FIELD: f32 = 40.0;

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
            .padding(Insets::new(14.0, 8.0, 8.0, 8.0))
            .gap(10.0)
            .align(Align::Center)
            .fill(Token::Ground)
            .radius(super::toast::CAPSULE)
            .stroke(Token::Hairline, 1.0)
            .shadow(Shadow {
                alpha: 0.10,
                radius: 18.0,
                y: 5.0,
            })
            .layer()
            .hit()
            .role(Role::Group)
            .name("Change Letter")
            .hidden(true),
    );
    ui.tree.add(
        pill,
        Node::glyph(glyph::PIN, 11.0, 14.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    let title = ui.tree.add(
        pill,
        Node::text("Letter", 12.5)
            .ink(Token::Ink)
            .weight(Weight::Medium)
            .shrink(1.0)
            .max_width(260.0)
            .role(Role::None),
    );
    let field = ui
        .tree
        .add(pill, Node::field("Letter", 13.0).width(FIELD).height(24.0));
    Bar {
        pill,
        title,
        field,
        tab: None,
    }
}

/// Open the pill for `tab`'s letter.
pub fn open(shell: &Shell, tab: &TabId) {
    let (letter, label) = {
        let b = shell.browser();
        let Some(t) = b.tab(tab) else { return };
        let Some(pin) = t.pin.clone() else { return };
        (
            pin,
            shell.tab_view(tab).map(|v| v.label).unwrap_or_default(),
        )
    };
    let tab = tab.clone();
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, st| {
        let stage = st.stage.stage;
        let bar = st.letter.get_or_insert_with(|| build(ui, stage));
        let was_open = bar.tab.is_some();
        bar.tab = Some(tab);
        let title = if label.is_empty() {
            "Letter".to_string()
        } else {
            format!("Letter for {label}")
        };
        ui.tree.set_text(bar.title, &title);
        ui.set_field_text(bar.field, &letter, true);
        if !was_open {
            ui.tree.set_hidden(bar.pill, false);
            ui.tree.set_prop(bar.pill, Prop::Y, -8.0);
            ui.tree.set_prop(bar.pill, Prop::Opacity, 0.0);
            ui.tree.animate(bar.pill, Prop::Y, 0.0, motion::SETTLE);
            ui.tree.animate(bar.pill, Prop::Opacity, 1.0, motion::QUICK);
        }
        ui.focus(Some(bar.field));
    });
}

/// Put the pill away; `false` if it was not up.
pub fn close(shell: &Shell) -> bool {
    let chrome = shell.view().chrome().clone();
    let was = chrome
        .read(|ui, st| {
            let Some(bar) = st.letter.as_mut().filter(|b| b.tab.is_some()) else {
                return false;
            };
            bar.tab = None;
            ui.tree.set_hidden(bar.pill, true);
            true
        })
        .unwrap_or(false);
    if was {
        shell.view().focus_page();
    }
    was
}

/// The field's own events; `true` when they were the pill's.
pub fn field_event(shell: &Shell, id: NodeId, e: &Event) -> bool {
    let chrome = shell.view().chrome().clone();
    let Some(Some((tab, typed))) = chrome.read(|ui, st| {
        let bar = st.letter.as_ref().filter(|b| b.field == id)?;
        Some((
            bar.tab.clone(),
            ui.field_text(bar.field).unwrap_or("").to_string(),
        ))
    }) else {
        return false;
    };
    match e {
        Event::FieldSubmit(_) => {
            if let (Some(tab), Some(first)) = (tab, first_letter(&typed)) {
                shell.with_browser(|b| b.set_pin_letter(&tab, &first));
            }
            close(shell);
        }
        Event::FieldCancel(_) => {
            close(shell);
        }
        _ => {}
    }
    true
}

/// What a pin takes from what was typed: its first character, trimmed;
/// nothing for nothing.
pub fn first_letter(typed: &str) -> Option<String> {
    typed.trim().chars().next().map(|c| c.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_takes_one_letter() {
        assert_eq!(first_letter("  gmail "), Some("g".into()));
        assert_eq!(first_letter("   "), None);
        assert_eq!(first_letter("É"), Some("É".into()));
    }
}
