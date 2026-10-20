//! The agents lane's Windows half (`AgentsApp`): the MCP server
//! (copper-shell-core's `agents`, started here when `agent.json` says so),
//! the engine's backstage for tabs an agent works in while the user looks
//! at another, what the agent is doing said in a line, who is driving
//! which tab ([`hands`], drawn by [`marks`]), and passwords for
//! `browser_sign_in` / `browser_autofill`.

pub mod brain;
mod hands;
pub mod marks;

use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::{Shell, Win};
use copper_core::browser::TabId;
use copper_engine::{Backstage, Engine, LocalFuture};
use copper_mcp::drive::DriveEvent;
use copper_shell_core::agents::{self, AgentsApp};
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// What the window does for the agents.
#[derive(Default)]
struct App {
    /// Tabs whose pages are lent to the backstage.
    held: RefCell<HashSet<TabId>>,
    /// Tabs an agent is working in right now.
    driving: RefCell<HashSet<TabId>>,
}

impl AgentsApp<Win> for App {
    fn holds(&self, tab: &TabId) -> bool {
        self.held.borrow().contains(tab)
    }

    fn prepare(&self, shell: &Shell, tab: &TabId) -> bool {
        let mut changed = false;
        match shell.browser().tab(tab).map(|t| t.sleeping) {
            None => return false,
            Some(true) => {
                shell.with_browser(|b| b.set_sleeping(tab, false));
                changed = true;
            }
            Some(false) => {}
        }
        if shell.slot(tab).is_none() {
            if shell.ensure_page(tab).is_none() {
                return changed;
            }
            changed = true;
        }
        if shell.active_id().as_ref() == Some(tab) || self.held.borrow().contains(tab) {
            return changed;
        }
        let Some(slot) = shell.slot(tab) else {
            return changed;
        };
        // Off its hidden pane, then into the backstage, in one turn.
        let chrome = shell.view().chrome().clone();
        let lent = chrome
            .read({
                let tab = tab.clone();
                move |ui, st| st.stage.lend(ui, &tab)
            })
            .unwrap_or(false);
        if !lent {
            return changed;
        }
        let hwnd = chrome.window().hwnd();
        if let Err(e) = shell.engine().backstage().hold(&slot.page, &hwnd) {
            tracing::warn!("could not hold an agent's tab backstage: {e}");
            let tab = tab.clone();
            chrome.edit(move |ui, st| st.stage.take_back(ui, &tab));
            return changed;
        }
        self.held.borrow_mut().insert(tab.clone());
        true
    }

    fn release(&self, shell: &Shell, tab: &TabId) {
        if !self.held.borrow_mut().remove(tab) {
            return;
        }
        let_go(shell, tab);
    }

    fn release_all(&self, shell: &Shell) {
        let all: Vec<TabId> = self.held.borrow_mut().drain().collect();
        for tab in all {
            let_go(shell, &tab);
        }
    }

    fn announce(&self, text: &str) {
        if let Some(s) = crate::platform::current() {
            s.toast(text);
        }
    }

    fn drive(&self, event: &DriveEvent) {
        match event {
            DriveEvent::CallBegan { tab: Some(t), .. } | DriveEvent::RunBegan { tab: t, .. } => {
                self.driving.borrow_mut().insert(t.clone());
            }
            DriveEvent::CallEnded { tab: Some(t), .. } => {
                self.driving.borrow_mut().remove(t);
            }
            DriveEvent::RunFinished { .. } => self.driving.borrow_mut().clear(),
            _ => {}
        }
        marks::event(event);
    }

    fn clear_marks(&self) {
        self.driving.borrow_mut().clear();
        marks::clear();
    }

    fn sign_in(
        &self,
        shell: Shell,
        tab: TabId,
        args: Map<String, Value>,
    ) -> LocalFuture<Result<Value, String>> {
        Box::pin(copper_shell_core::forms::mcp::sign_in::<
            crate::forms::WinForms,
        >(shell, tab, args))
    }

    fn autofill(
        &self,
        shell: Shell,
        tab: TabId,
        args: Map<String, Value>,
    ) -> LocalFuture<Result<Value, String>> {
        Box::pin(copper_shell_core::forms::mcp::autofill::<Win>(
            shell, tab, args,
        ))
    }
}

/// The backstage lets `tab`'s page go; back onto its pane.
fn let_go(shell: &Shell, tab: &TabId) {
    if let Some(slot) = shell.slot(tab) {
        shell.engine().backstage().release(&slot.page);
    }
    let tab = tab.clone();
    shell
        .view()
        .chrome()
        .edit(move |ui, st| st.stage.take_back(ui, &tab));
}

/// Start the lane (the MCP server listens when `agent.json` says so) and
/// its commands.
pub fn register(shell: &Shell) {
    let server = agents::start::<Win>(shell, Rc::new(App::default()));
    marks::install(shell);
    brain::register(shell);
    let r = shell.view().commands();
    let s = server.clone();
    r.add(Command::new(
        "agents-toggle",
        "Let Agents Drive This Window",
        glyph::AGENT,
        move |shell| {
            let on = !s.enabled();
            s.set_enabled(on);
            shell.toast(if on {
                "Agents can drive this window"
            } else {
                "Agent server off"
            });
        },
    ));
    r.add(Command::new(
        "agents-settings",
        "Agents Settings",
        glyph::AGENT,
        |s| crate::settings::open(s, "agents"),
    ));
    // Ctrl+E: the Agent sidebar (an addon's page, or Copper's placeholder).
    r.add(Command::key("agent", crate::addons::toggle));
    // Settings › Agents follows the server (listening, port, token, Jev).
    server.watch(|_| {
        if let Some(s) = crate::platform::current() {
            crate::settings::changed(&s, "agents");
        }
        true
    });
}
