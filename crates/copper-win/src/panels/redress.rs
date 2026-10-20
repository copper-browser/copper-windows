//! What is hidden, on the documents already on screen.
//!
//! The curtain (shell-core) keeps each site's hidden things as user style
//! sheets, which WebKit applies to live documents as well as new ones. On
//! WebView2 a user sheet is a document-created script: adding or removing
//! one reaches only documents made afterwards, so on the page in front a
//! peeked or restored element stayed `display: none` (no outline, nothing
//! to scroll to) and a fresh hide did not show until a reload.
//!
//! [`redress`] closes that gap after every curtain change: on each live
//! page of the site it drops the creation-time rules for the site's
//! selectors and carries the current sheet (and the peek's outline) in a
//! sheet of its own (`redress.js`). New documents still get the curtain's
//! sheets from the engine, before their first paint.
//!
//! Remove once the engine applies user style sheet changes to live
//! documents (see the panels report).

use crate::platform::{Shell, Win};
use copper_engine::{JsonMap, JsonValue, PageView, World};
use copper_shell_core::curtain;
use copper_shell_core::util::site_key;

/// The page-world function body (`redress.js`).
const SCRIPT: &str = include_str!("redress.js");

/// Bring the live pages of `site` in line with what is hidden there now.
/// `gone` are selectors just taken off the list (restored, undone): their
/// creation-time rules are dropped too. `peek` is shown, outlined and
/// scrolled to on the page in front.
pub fn redress(shell: &Shell, site: &str, gone: &[String], peek: Option<&str>) {
    let mut selectors: Vec<String> = curtain::veils::<Win>(site)
        .into_iter()
        .map(|v| v.selector)
        .collect();
    selectors.extend(gone.iter().cloned());
    if let Some(p) = peek {
        selectors.push(p.to_string());
    }
    selectors.sort();
    selectors.dedup();
    let front = shell.active_id();
    let pages: Vec<_> = shell
        .pages_map()
        .borrow()
        .iter()
        .filter(|(id, _)| {
            shell
                .browser()
                .tab(id)
                .and_then(|t| site_key(&t.url))
                .as_deref()
                == Some(site)
        })
        .map(|(id, slot)| (Some(id) == front.as_ref(), slot.page.clone()))
        .collect();
    for (in_front, page) in pages {
        let spared = peek.filter(|_| in_front);
        let mut args = JsonMap::new();
        args.insert("css".into(), curtain::css::<Win>(site, spared).into());
        args.insert("selectors".into(), JsonValue::from(selectors.clone()));
        args.insert(
            "peek".into(),
            spared.map_or(JsonValue::Null, |p| p.to_string().into()),
        );
        let call = page.call_async(SCRIPT, &args, &World::Page);
        copper_win_rt::spawn_local(async move {
            if let Err(e) = call.await {
                tracing::debug!(error = %e, "curtain redress");
            }
        });
    }
}

/// A tab's picker just hid something: its site's live pages follow.
pub fn picked(shell: &Shell, tab: &copper_core::browser::TabId) {
    let site = shell.browser().tab(tab).and_then(|t| site_key(&t.url));
    if let Some(site) = site {
        redress(shell, &site, &[], None);
    }
}
