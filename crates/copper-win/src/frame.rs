//! A tab's page frame (`PageFrame`): the page's pane on the stage, and the
//! status laid over it when the page could not be shown. The pane itself
//! is the stage's ([`crate::chrome::stage`]); the frame is the shell's
//! handle on its status, and its button reloads the tab (the stage's click
//! handling calls `Shell::reload`).

use crate::chrome::Chrome;
use copper_core::browser::TabId;
use copper_shell_core::{PageFrame, PageStatus};
use std::rc::Rc;

/// One tab's frame.
pub struct WinFrame {
    chrome: Rc<Chrome>,
    tab: TabId,
}

impl WinFrame {
    pub fn new(chrome: Rc<Chrome>, tab: TabId) -> WinFrame {
        WinFrame { chrome, tab }
    }
}

impl PageFrame for WinFrame {
    fn show_status(&self, status: &PageStatus) {
        let tab = self.tab.clone();
        let status = status.clone();
        self.chrome
            .edit(move |ui, st| st.stage.set_status(ui, &tab, Some(status)));
    }

    fn hide_status(&self) {
        let tab = self.tab.clone();
        self.chrome
            .edit(move |ui, st| st.stage.set_status(ui, &tab, None));
    }
}
