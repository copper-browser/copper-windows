//! A tab's page frame (`PageFrame`): the page's pane on the stage, and the
//! status laid over it when the page could not be shown. The pane itself
//! is the stage's ([`crate::chrome::stage`]); the frame is the shell's
//! handle on its status, and its button reloads the tab (the stage's click
//! handling calls `Shell::reload`).
//!
//! What it says is the Mac's (`Stage.swift` `Trouble`, `Browser.swift`
//! `message(for:)`, copper-browser/Copper#181), as copper-shell-core words
//! it: "Can't connect to <host>", why in one line, `Try Again` (Ctrl+R) and
//! the engine's own words under Details. A page that stopped (its web process went
//! away) is brought back by itself as on the Mac (`Tab.swift` revive): at
//! once when it is on screen, checked 1.5 s later and tried once more if
//! it is still not there; when it is behind another tab, as soon as it is
//! picked. Only when that fails does it say `This page stopped.`.

use crate::chrome::Chrome;
use copper_core::browser::TabId;
use copper_shell_core::{PageFrame, PageStatus};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The button under a page that stopped.
pub const TRY_AGAIN: &str = "Try Again";
/// What a page that stopped says once it would not come back (Windows).
pub const STOPPED: &str = "This page stopped.";

/// How often a stopped page is brought back by itself before it says so.
const REVIVALS: u8 = 2;
/// A stop this long after the last starts the count again.
const FRESH: Duration = Duration::from_secs(30);
/// When a revived page is looked at again (`Tab.swift`).
const VERIFY: Duration = Duration::from_millis(1500);
/// A load that ends this soon after a download began is the download's.
const HANDOFF: Duration = Duration::from_secs(3);

/// One tab's frame.
pub struct WinFrame {
    chrome: Rc<Chrome>,
    tab: TabId,
    /// Revivals since the page last stopped (within [`FRESH`]).
    revived: Cell<u8>,
    last_stop: Cell<Option<Instant>>,
    /// Stopped while behind another tab: revive when picked.
    stale: Cell<bool>,
}

impl WinFrame {
    pub fn new(chrome: Rc<Chrome>, tab: TabId) -> WinFrame {
        WinFrame {
            chrome,
            tab,
            revived: Cell::new(0),
            last_stop: Cell::new(None),
            stale: Cell::new(false),
        }
    }

    /// The tab was picked: a page that stopped while it was behind
    /// another comes back now.
    pub fn picked(&self) {
        if self.stale.replace(false) {
            self.revive(false);
        }
    }

    /// Bring the stopped page back (`verify`: and look again in a moment).
    fn revive(&self, verify: bool) {
        let Some(shell) = self.chrome.shell() else {
            return;
        };
        self.revived.set(self.revived.get() + 1);
        tracing::info!(target: "copper::frame", tab = %self.tab.short(), n = self.revived.get(), "reviving a stopped page");
        let weak = shell.downgrade();
        let tab = self.tab.clone();
        // On the loop's next turn: never inside the engine's own event.
        copper_win_rt::spawn_local(async move {
            let Some(shell) = weak.upgrade() else { return };
            shell.reload(&tab);
            drop(shell);
            if !verify {
                return;
            }
            copper_win_rt::sleep(VERIFY).await;
            let Some(shell) = weak.upgrade() else { return };
            let Some(slot) = shell.slot(&tab) else { return };
            // Stopped again: its own report decides. Neither loading nor
            // loaded: nothing came, once more if allowed.
            let blank = !slot.crashed.get() && !slot.loading.get() && slot.progress.get() < 1.0;
            if blank && slot.frame.revived.get() < REVIVALS {
                slot.frame.revive(false);
            }
        });
    }

    /// The page stopped: whether to bring it back rather than say so.
    fn stopped(&self) -> bool {
        let now = Instant::now();
        if self
            .last_stop
            .replace(Some(now))
            .is_none_or(|t| now - t > FRESH)
        {
            self.revived.set(0);
        }
        if self.revived.get() >= REVIVALS {
            return false;
        }
        let Some(shell) = self.chrome.shell() else {
            return false;
        };
        if shell.active_id().as_ref() == Some(&self.tab) {
            self.revive(true);
        } else {
            self.stale.set(true);
        }
        true
    }
}

impl PageFrame for WinFrame {
    fn show_status(&self, status: &PageStatus) {
        let status = if status.title.starts_with("This page stopped") {
            if self.stopped() {
                return;
            }
            PageStatus {
                title: STOPPED.into(),
                description: String::new(),
                button: TRY_AGAIN.into(),
                detail: None,
                offline: false,
            }
        } else {
            let handoff = self
                .chrome
                .shell()
                .and_then(|s| s.view().download_began())
                .is_some_and(|t| t.elapsed() < HANDOFF);
            // A page that turned into a download ends its load as a
            // dropped connection while the file goes on arriving.
            if handoff && status.description == DIDNT_LOAD {
                return;
            }
            status.clone()
        };
        let tab = self.tab.clone();
        self.chrome
            .edit(move |ui, st| st.stage.set_status(ui, &tab, Some(status)));
    }

    fn hide_status(&self) {
        self.stale.set(false);
        let tab = self.tab.clone();
        self.chrome
            .edit(move |ui, st| st.stage.set_status(ui, &tab, None));
    }
}

/// copper-shell-core's line for a load that failed for no known reason
/// (also a dropped connection, and a load handed to a download).
const DIDNT_LOAD: &str = "The page didn't load.";
