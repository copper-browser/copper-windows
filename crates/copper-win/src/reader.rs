//! Reading mode (Mac: `Reader.swift`, `Tab.toggleReader`): Ctrl+Alt+R
//! strips the page back to its article, set calmly in one column. Again
//! (or navigating) leaves it, by reloading: restoring the old markup would
//! give a page that looks right and does nothing.
//!
//! Ported from Linux `copper-app/src/features/reader.rs`. Only the type
//! below is Windows'; the rest is toolkit-free and could move to
//! copper-shell-core so both shells share one copy.

use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_engine::{PageView, World};
use copper_pagejs::tab::ReaderOutcome;
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static READING: RefCell<HashSet<TabId>> = RefCell::default();
}

/// The Mac's type in Windows' families (article.js asks for
/// `-apple-system`, which Chromium here doesn't know), and the article's
/// own `<h1>` dropped when it only repeats the title above it.
const WINDOWS_TYPE: &str = r#"(function () {
  var s = document.createElement('style');
  s.textContent = '#office-reader h1,#office-reader h2,#office-reader h3,' +
    '#office-reader .office-from,#office-reader figcaption{font-family:' +
    '"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif !important}';
  document.head.appendChild(s);
  var r = document.getElementById('office-reader');
  var hs = r ? r.querySelectorAll('h1') : [];
  if (hs.length > 1 && hs[1].textContent.trim() === hs[0].textContent.trim()) hs[1].remove();
  return true;
})()"#;

const NOTHING: &str = "Nothing to read on this page";

fn reading(tab: &TabId) -> bool {
    READING.with(|r| r.borrow().contains(tab))
}

/// The tab left reading mode (navigated, reloaded, closed).
pub fn forget(tab: &TabId) {
    READING.with(|r| r.borrow_mut().remove(tab));
}

/// Ctrl+Alt+R: the article, or back to the page.
pub fn toggle(shell: &Shell) {
    let Some(id) = shell.active_id() else { return };
    let url = shell
        .browser()
        .tab(&id)
        .map(|t| t.url.clone())
        .unwrap_or_default();
    if url.is_empty() || url.starts_with("about:") {
        shell.toast(NOTHING);
        return;
    }
    if reading(&id) {
        forget(&id);
        shell.reload_active();
        return;
    }
    let Some(page) = shell.ensure_page(&id) else {
        return;
    };
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let outcome = copper_shell_core::util::run(&page, &copper_pagejs::tab::reader()).await;
        let Some(shell) = weak.upgrade() else { return };
        match outcome {
            Ok(ReaderOutcome::Read) => {
                READING.with(|r| r.borrow_mut().insert(id.clone()));
                let _ = page.eval(WINDOWS_TYPE, &World::Page).await;
            }
            Ok(ReaderOutcome::None) => shell.toast(NOTHING),
            Err(e) => {
                tracing::warn!(error = %e, "reading mode");
                shell.toast(NOTHING);
            }
        }
    });
}
