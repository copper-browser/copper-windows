//! The lanes (`docs/APP.md` › Lanes), started in the order the shell
//! expects: shield, curtain, updates, forms, cloud, canvas, agents. Each
//! lives in copper-shell-core; this wires each to the window and to every
//! page as it is made.

use crate::platform::{Shell, Win};
use copper_core::browser::TabId;
use copper_engine::{LoadState, PageEvent, PageView};
use copper_engine_webview2::WebView2Page;
use copper_shell_core::{curtain, shield};
use std::rc::Rc;

/// Start every lane and hook every page.
pub fn register(shell: &Shell) {
    shield::start(shell);
    curtain::start(shell);
    // The hide-anything picker's state shows in the palette's offers; a
    // change redraws the chrome that reads it.
    curtain::on_change::<Win>(Rc::new(|s: &Shell| s.view().sync(s)));
    crate::updates::register(shell);
    crate::forms::register(shell);
    crate::cloud::register(shell);
    crate::canvas::register(shell);
    crate::agents::register(shell);
    crate::panels::register(shell);
    crate::settings::register(shell);
    crate::find::register(shell);
    crate::palette::register(shell);
    page_commands(shell);
    shell.on_page_created(page_created);
    let pages: Vec<(TabId, WebView2Page)> = shell
        .pages_map()
        .borrow()
        .iter()
        .map(|(id, slot)| (id.clone(), slot.page.clone()))
        .collect();
    for (id, page) in pages {
        page_created(shell, &id, &page);
    }
}

/// The page commands the lanes bring.
fn page_commands(shell: &Shell) {
    use crate::chrome::glyph;
    use crate::commands::Command;
    let r = shell.view().commands();
    r.add(
        Command::new(
            "hide-anything",
            "Hide Something on This Page",
            glyph::HIDDEN,
            curtain::toggle,
        )
        .when(|s| curtain::here(s).is_some()),
    );
    r.add(
        Command::new(
            "shield-here",
            "Pause or Resume Shield Here",
            glyph::SHIELD,
            |s| {
                let Some(site) = s.browser().active().map(|t| t.url.clone()).and_then(|u| {
                    url::Url::parse(&u)
                        .ok()
                        .and_then(|u| u.host_str().map(str::to_string))
                }) else {
                    return;
                };
                let off = !shield::paused_on(s, &site);
                shield::toggle_here(s, &site, off);
            },
        )
        .when(|s| shield::enabled(s) && s.active_page().is_some()),
    );
}

/// A tab's page was just made, before its first load: the block list, the
/// hidden-element style sheets and the picker; then its events, for the
/// picker's messages and its moves.
pub fn page_created(shell: &Shell, id: &TabId, page: &WebView2Page) {
    shield::protect(shell, page);
    curtain::dress(shell, id, page);
    let mut events = page.events();
    let weak = shell.downgrade();
    let tab = id.clone();
    let page_id = page.page_id();
    copper_win_rt::spawn_local(async move {
        while let Some(ev) = events.next().await {
            let Some(shell) = weak.upgrade() else { break };
            // The tab may have a newer page (after a crash or a sleep);
            // this one's events no longer speak for it.
            if shell.slot(&tab).map(|s| s.page.page_id()) != Some(page_id) {
                break;
            }
            match ev {
                PageEvent::ScriptMessage { name, body } if name == curtain::HANDLER => {
                    curtain::message(&shell, &tab, &body);
                    // WebView2's user sheets reach new documents only:
                    // the page on screen is told itself.
                    crate::panels::redress::picked(&shell, &tab);
                }
                PageEvent::Load(LoadState::Committed { .. }) => {
                    curtain::navigated(&shell, &tab);
                    crate::find::page_moved(&shell, &tab);
                }
                _ => {}
            }
        }
        curtain::page_gone::<Win>(page_id);
    });
}
