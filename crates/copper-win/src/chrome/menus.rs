//! Right-click menus (`TabBar.swift` `TabMenu`): what a tab offers,
//! wherever it is drawn.

use copper_core::browser::TabId;
use copper_win_ui::Menu;

/// The tab menu's tag.
pub const TAB_MENU: u32 = 1;

/// The tab menu's items.
pub mod item {
    pub const PIN: u32 = 1;
    pub const UNPIN: u32 = 2;
    pub const DUPLICATE: u32 = 3;
    pub const COPY_ADDRESS: u32 = 4;
    pub const CLOSE: u32 = 5;
    pub const CLOSE_OTHERS: u32 = 6;
    pub const SAVE: u32 = 7;
    pub const UNSAVE: u32 = 8;
}

/// What the open menu is about.
#[derive(Default)]
pub struct Menus {
    pub tab: Option<TabId>,
}

/// What the tab menu needs to know about its tab.
#[derive(Clone, Copy, Debug)]
pub struct TabFacts {
    pub pinned: bool,
    pub blank: bool,
    pub saved: bool,
    pub several: bool,
}

/// The tab menu for a tab like `t`.
pub fn tab_menu(t: TabFacts) -> Menu {
    let mut m = Menu::new();
    m = if t.pinned {
        m.item(item::UNPIN, "&Unpin")
    } else {
        let m = m.item(item::PIN, "&Pin");
        if t.blank { m.disabled() } else { m }
    };
    m = m.separator().item(item::DUPLICATE, "&Duplicate");
    if t.blank {
        m = m.disabled();
    }
    m = m.item(item::COPY_ADDRESS, "Copy &Address");
    if t.blank {
        m = m.disabled();
    }
    m = m
        .separator()
        .item(item::CLOSE, "&Close Tab")
        .shortcut("Ctrl+W")
        .item(item::CLOSE_OTHERS, "Close &Other Tabs");
    if !t.several {
        m = m.disabled();
    }
    if !t.pinned {
        m = if t.saved {
            m.item(item::UNSAVE, "Move to &Today")
        } else {
            m.item(item::SAVE, "&Keep in Saved")
        };
    }
    m
}
