//! `browser_handoff`'s bar (Mac `HandoffSlot`/`HandoffBar`; Linux
//! `agents/handoff.rs`): a capsule at the bottom centre of the stage,
//! "<agent> needs you on <tab>: <message>", with Show (only when the tab is
//! elsewhere: the person's click goes to it, never the agent's), Done and
//! Not now. One ask at a time, oldest first. The model is copper-shell-core's
//! [`copper_shell_core::agents::handoff`]; the capsule reuses the agent
//! marks' look ([`super::marks`]).

use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use copper_core::browser::TabId;
use copper_mcp::HandoffAnswer;
use copper_shell_core::agents::{Agents, handoff};
use copper_win_ui::tokens::Shadow;
use copper_win_ui::{Abs, Align, Insets, Justify, Node, NodeId, Role, Swatch, Token, Trim, Weight};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// What a click on the bar means.
#[derive(Clone, Debug)]
enum Target {
    Show(TabId),
    Answer(u64, HandoffAnswer),
}

thread_local! {
    /// The row over the stage that holds the capsule.
    static SLOT: Cell<Option<NodeId>> = const { Cell::new(None) };
    static TARGETS: RefCell<HashMap<NodeId, Target>> = RefCell::default();
    /// What is drawn, to redraw only when it changes.
    static DRAWN: RefCell<Option<handoff::Bar>> = const { RefCell::new(None) };
}

/// Build the slot (hidden) over the stage and follow the asks.
pub fn install(shell: &Shell, agents: &Agents<Win>) {
    shell.view().chrome().edit(|ui, st| {
        let slot = ui.tree.add(
            st.stage.stage,
            Node::row()
                .absolute(Abs {
                    left: Some(0.0),
                    top: None,
                    right: Some(0.0),
                    bottom: Some(16.0),
                })
                .justify(Justify::Center)
                .align(Align::Center)
                .hidden(true)
                .role(Role::Group),
        );
        SLOT.with(|s| s.set(Some(slot)));
    });
    handoff::follow(shell, agents, draw);
    shell.on_dark_changed(|_| {
        if let Some(s) = crate::platform::current() {
            DRAWN.with(|d| d.borrow_mut().take());
            draw(&s);
        }
    });
}

/// An ask came while Copper is in the background: flash its taskbar button
/// once (Mac: one Dock bounce). Never brought to the front.
pub fn attention() {
    use windows::Win32::UI::WindowsAndMessaging::{FLASHW_TRAY, FLASHWINFO, FlashWindowEx};
    if crate::canvas::notify::frontmost() {
        return;
    }
    let Some(s) = crate::platform::current() else {
        return;
    };
    let info = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd: s.view().chrome().window().hwnd(),
        dwFlags: FLASHW_TRAY,
        uCount: 1,
        dwTimeout: 0,
    };
    // SAFETY: a plain call on our own window with a filled-in struct.
    unsafe {
        let _ = FlashWindowEx(&info);
    }
}

/// A click on one of the bar's nodes; `true` if it was ours.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let Some(t) = TARGETS.with(|t| t.borrow().get(&id).cloned()) else {
        return false;
    };
    match t {
        Target::Show(tab) => {
            shell.select(&tab);
        }
        Target::Answer(ask, answer) => {
            if let Some(a) = copper_shell_core::agents::get::<Win>() {
                a.handoff().answer(ask, answer);
            }
        }
    }
    true
}

fn draw(shell: &Shell) {
    let Some(slot) = SLOT.with(Cell::get) else {
        return;
    };
    let bar = handoff::bar(shell);
    if DRAWN.with(|d| *d.borrow() == bar) {
        return;
    }
    DRAWN.with(|d| *d.borrow_mut() = bar.clone());
    shell.view().chrome().edit(move |ui, _| {
        TARGETS.with(|t| t.borrow_mut().clear());
        ui.tree.clear_children(slot);
        let Some(bar) = bar else {
            ui.tree.set_hidden(slot, true);
            return;
        };
        ui.tree.set_hidden(slot, false);
        let cap = ui.tree.add(
            slot,
            Node::row()
                .shrink(0.0)
                .padding(Insets::new(16.0, 9.0, 12.0, 9.0))
                .gap(12.0)
                .align(Align::Center)
                .radius(18.0)
                .fill(Token::Ground)
                .stroke(Token::Hairline, 1.0)
                .shadow(Shadow::new(0.12, 20.0, 6.0))
                .layer()
                .role(Role::Group)
                .name(&bar.label()),
        );
        ui.tree.add(
            cap,
            Node::glyph(glyph::PERSON, 11.0, 14.0)
                .ink(Token::Accent)
                .role(Role::None),
        );
        let words = ui.tree.add(
            cap,
            Node::row()
                .gap(0.0)
                .align(Align::Center)
                .max_width(520.0)
                .role(Role::None),
        );
        ui.tree.add(
            words,
            Node::text(&bar.lead(), 12.5)
                .weight(Weight::Semibold)
                .ink(Token::Ink)
                .shrink(0.0)
                .role(Role::None),
        );
        ui.tree.add(
            words,
            Node::text(&bar.rest(), 12.5)
                .ink(Token::Ink)
                .trim(Trim::Wrap)
                .role(Role::None),
        );
        let mut targets = Vec::new();
        if bar.elsewhere {
            let show = pill(ui, cap, "Show", Swatch::Clear, Token::Ink, Weight::Medium);
            ui.tree.edit(show, |n| {
                n.tooltip = Some("Go to the tab the agent handed you".into())
            });
            targets.push((show, Target::Show(bar.ask.tab.clone())));
        }
        let done = pill(
            ui,
            cap,
            "Done",
            Token::Accent.into(),
            Token::White,
            Weight::Regular,
        );
        let tip = format!("Tell {} the step is done", bar.ask.agent);
        ui.tree.edit(done, |n| {
            n.tooltip = Some(tip);
            n.look.hover = Some(Swatch::Fade(Token::Accent, 0.88));
        });
        targets.push((done, Target::Answer(bar.ask.id, HandoffAnswer::Done)));
        let later = pill(
            ui,
            cap,
            "Not now",
            Swatch::Clear,
            Token::Muted,
            Weight::Regular,
        );
        targets.push((later, Target::Answer(bar.ask.id, HandoffAnswer::NotNow)));
        TARGETS.with(|t| t.borrow_mut().extend(targets));
    });
}

/// A capsule button: its label, in `ink` on `fill`.
fn pill(
    ui: &mut copper_win_ui::win::Ui,
    parent: NodeId,
    label: &str,
    fill: Swatch,
    ink: Token,
    weight: Weight,
) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::stack()
            .height(24.0)
            .shrink(0.0)
            .padding(Insets::new(11.0, 0.0, 11.0, 0.0))
            .radius(12.0)
            .fill(fill)
            .hover_fill(Swatch::Fade(Token::Ink, 0.08))
            .press_fill(Swatch::Fade(Token::Ink, 0.14))
            .align(Align::Center)
            .justify(Justify::Center)
            .button(label),
    );
    ui.tree.add(
        b,
        Node::text(label, 12.0)
            .weight(weight)
            .ink(ink)
            .role(Role::None),
    );
    b
}
