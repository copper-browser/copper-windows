//! App-rendered context menus: what a page's right click (or Menu key, or
//! Shift+F10) asks for, with WebView2's own menu as data
//! (`WebView2Page::set_context_menu_handler`).
//!
//! Platform-free: the types and the mapping of WebView2's kind values; the
//! Windows side reads the event into them and answers through
//! `ContextMenuReply`.

/// What was clicked (`COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextKind {
    /// The page itself (also a link or an editable field: see the
    /// request's `link_url` and `editable`).
    Page,
    Image,
    SelectedText,
    Audio,
    Video,
}

impl ContextKind {
    /// From WebView2's target kind; an unknown kind is the page.
    pub(crate) fn from_webview2(kind: i32) -> Self {
        match kind {
            1 => ContextKind::Image,
            2 => ContextKind::SelectedText,
            3 => ContextKind::Audio,
            4 => ContextKind::Video,
            _ => ContextKind::Page,
        }
    }
}

/// What kind of entry an item is (`COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextItemKind {
    Command,
    CheckBox,
    Radio,
    Separator,
    /// Opens `children`.
    Submenu,
}

impl ContextItemKind {
    /// From WebView2's item kind; an unknown kind is a command.
    pub(crate) fn from_webview2(kind: i32) -> Self {
        match kind {
            1 => ContextItemKind::CheckBox,
            2 => ContextItemKind::Radio,
            3 => ContextItemKind::Separator,
            4 => ContextItemKind::Submenu,
            _ => ContextItemKind::Command,
        }
    }
}

/// One entry of WebView2's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMenuItem {
    /// What to pass to `ContextMenuReply::run` to run it.
    pub command: i32,
    /// WebView2's stable, unlocalized name: `"back"`, `"reload"`,
    /// `"copy"`, `"paste"`, `"copyImage"`, `"saveImageAs"`,
    /// `"copyImageLocation"`, `"openImageInNewWindow"`,
    /// `"copyLinkLocation"`, `"saveLinkAs"`, `"inspectElement"`… (spell
    /// suggestions and other generated items have their own); `"other"`
    /// for separators and items WebView2 does not name.
    pub name: String,
    /// The localized label, as a Win32 menu takes it: `&` marks the access
    /// key that follows, `&&` is a literal ampersand.
    pub label: String,
    /// The shortcut shown next to the label (`"Ctrl+C"`), if any.
    pub shortcut: Option<String>,
    pub kind: ContextItemKind,
    pub enabled: bool,
    /// For a check box or radio item.
    pub checked: bool,
    /// A submenu's items, in order (empty otherwise).
    pub children: Vec<ContextMenuItem>,
}

/// A page asks for its context menu. Coordinates are physical pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMenuRequest {
    pub kind: ContextKind,
    /// The link under the pointer.
    pub link_url: Option<String>,
    pub link_text: Option<String>,
    /// The image's, audio's or video's source.
    pub source_url: Option<String>,
    /// The selected text, if any.
    pub selection: Option<String>,
    /// On an editable field (`paste` and friends apply).
    pub editable: bool,
    /// In a child frame rather than the main document.
    pub in_frame: bool,
    pub page_url: String,
    /// The child frame's URL, when `in_frame`.
    pub frame_url: Option<String>,
    /// Where to show the menu, relative to the page's slot (its top-left
    /// corner), in physical pixels. For the Menu key and Shift+F10, WebView2
    /// places it by the focused element or selection.
    pub point: (i32, i32),
    /// WebView2's own menu for this target, in order.
    pub items: Vec<ContextMenuItem>,
}

/// `Some(text)` unless `text` is empty.
pub(crate) fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(ContextKind::from_webview2(0), ContextKind::Page);
        assert_eq!(ContextKind::from_webview2(1), ContextKind::Image);
        assert_eq!(ContextKind::from_webview2(2), ContextKind::SelectedText);
        assert_eq!(ContextKind::from_webview2(3), ContextKind::Audio);
        assert_eq!(ContextKind::from_webview2(4), ContextKind::Video);
        assert_eq!(ContextKind::from_webview2(99), ContextKind::Page);
        assert_eq!(ContextItemKind::from_webview2(0), ContextItemKind::Command);
        assert_eq!(ContextItemKind::from_webview2(1), ContextItemKind::CheckBox);
        assert_eq!(ContextItemKind::from_webview2(2), ContextItemKind::Radio);
        assert_eq!(
            ContextItemKind::from_webview2(3),
            ContextItemKind::Separator
        );
        assert_eq!(ContextItemKind::from_webview2(4), ContextItemKind::Submenu);
        assert_eq!(ContextItemKind::from_webview2(-1), ContextItemKind::Command);
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(non_empty(String::new()), None);
        assert_eq!(non_empty("x".into()), Some("x".into()));
    }
}
