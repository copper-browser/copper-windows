//! Hidden tabs frozen (WebView2 `TrySuspend`, memory target low): what
//! WebKit does on its own for the Mac's hidden pages. A tab off screen for
//! a minute that the Mac's rules would keep awake for nothing (not pinned,
//! not playing, not on a call, not an agent's, not loading) stops running
//! script; showing it resumes it at once. Sleeping (closing the page) stays
//! the shell's sweep.

use crate::platform::Shell;
use copper_core::UnixTime;
use std::time::Duration;

/// Off screen this long before a tab is frozen.
const AFTER: f64 = 60.0;

pub fn start(shell: &Shell) {
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        loop {
            copper_win_rt::sleep(Duration::from_secs(20)).await;
            let Some(s) = weak.upgrade() else { return };
            let now = UnixTime::now().0;
            let idle: Vec<_> = {
                let b = s.browser();
                b.all_tabs()
                    .filter(|t| now - t.seen.0 > AFTER)
                    .map(|t| t.id.clone())
                    .collect()
            };
            let pages: Vec<_> = idle
                .into_iter()
                .filter(|id| s.keeps_awake(id).is_none())
                .filter_map(|id| s.page(&id))
                .filter(|p| !p.is_suspended())
                .collect();
            drop(s);
            for p in pages {
                // Declined (audio, locks, on screen now) is fine: next round.
                let _ = p.suspend().await;
            }
        }
    });
}
