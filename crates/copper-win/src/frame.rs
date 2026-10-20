//! A tab's page frame (`PageFrame`): the page's pane on the stage, and the
//! status laid over it when the page could not be shown. The pane itself
//! is the stage's ([`crate::chrome::stage`]); the frame is the shell's
//! handle on its status, and its button reloads the tab (the stage's click
//! handling calls `Shell::reload`).
//!
//! What it says is the Mac's (`Stage.swift` `Trouble`, `Browser.swift`
//! `message(for:)`): one sentence and a quiet `Try again`, never the
//! engine's own error text. A page that stopped (its web process went
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

/// The Mac's sentence for a page that never came, and the button under it.
pub const TRY_AGAIN: &str = "Try again";
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
        let shown = match trouble(status) {
            Trouble::Stopped => {
                if self.stopped() {
                    return;
                }
                STOPPED.to_string()
            }
            Trouble::Failed(s) => {
                let handoff = self
                    .chrome
                    .shell()
                    .and_then(|s| s.view().download_began())
                    .is_some_and(|t| t.elapsed() < HANDOFF);
                // A page that turned into a download ends its load as a
                // dropped connection while the file goes on arriving.
                if handoff && s == ABORTED {
                    return;
                }
                s.to_string()
            }
        };
        let tab = self.tab.clone();
        let status = PageStatus {
            title: shown,
            description: String::new(),
            button: TRY_AGAIN.into(),
        };
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

/// What the shell's status is about.
#[derive(Debug, PartialEq, Eq)]
pub enum Trouble {
    /// The web process went away.
    Stopped,
    /// The page did not load: the Mac's sentence for why.
    Failed(&'static str),
}

/// The Mac's fallback.
const DIDNT_LOAD: &str = "The page didn't load.";
/// What a dropped connection says (also a load handed to a download).
const ABORTED: &str = DIDNT_LOAD;

/// The shell's status (copper-shell-core `crash_status` /
/// `failure_status`, whose description is the engine's error as a
/// sentence) in the Mac's words (`Browser.swift` `message(for:)`).
pub fn trouble(s: &PageStatus) -> Trouble {
    if s.title.starts_with("This page stopped") {
        return Trouble::Stopped;
    }
    if s.title.starts_with("You\u{2019}re offline") || s.title.starts_with("You're offline") {
        return Trouble::Failed("No connection.");
    }
    let e = s
        .description
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    Trouble::Failed(match e.as_str() {
        "host name not resolved" => "No site at that address.",
        "disconnected" | "connection reset" => "No connection.",
        "timeout" => "The site took too long to answer.",
        "cannot connect" | "server unreachable" => "The site refused the connection.",
        e if e.starts_with("certificate") || e.starts_with("client certificate") => {
            "The connection isn't secure."
        }
        _ => DIDNT_LOAD,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(title: &str, description: &str) -> PageStatus {
        PageStatus {
            title: title.into(),
            description: description.into(),
            button: "Try Again".into(),
        }
    }

    #[test]
    fn speaks_the_macs_sentences() {
        let t = |d: &str| trouble(&failed("Couldn\u{2019}t open a.example", d));
        assert_eq!(
            t("Host name not resolved."),
            Trouble::Failed("No site at that address.")
        );
        assert_eq!(t("Disconnected."), Trouble::Failed("No connection."));
        assert_eq!(
            t("Timeout."),
            Trouble::Failed("The site took too long to answer.")
        );
        assert_eq!(
            t("Cannot connect."),
            Trouble::Failed("The site refused the connection.")
        );
        assert_eq!(
            t("Certificate expired."),
            Trouble::Failed("The connection isn't secure.")
        );
        assert_eq!(
            t("Something odd."),
            Trouble::Failed("The page didn't load.")
        );
        assert_eq!(
            trouble(&failed(
                "You\u{2019}re offline",
                "Copper will load this page once you are connected."
            )),
            Trouble::Failed("No connection.")
        );
        assert_eq!(
            trouble(&failed("This page stopped", "Its process was ended.")),
            Trouble::Stopped
        );
    }
}
