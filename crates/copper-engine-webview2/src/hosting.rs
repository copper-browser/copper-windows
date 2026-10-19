//! Values of the visual-hosting API (CONTRACT §2), as thin wrappers over the
//! WebView2 enums so the toolkit and app never need `webview2-com`.
//!
//! The numbers are WebView2's, which are themselves the Win32 ones:
//! [`MouseEventKind`] is the `WM_*` mouse message, [`VirtualKeys`] the
//! `MK_*` flags of its `wParam`, [`PointerEventKind`] the `WM_POINTER*`
//! message. So the app can forward a window message as it is.

/// `COREWEBVIEW2_MOUSE_EVENT_KIND` (= the `WM_*` message).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MouseEventKind {
    HorizontalWheel = 0x020E,
    LeftButtonDoubleClick = 0x0203,
    LeftButtonDown = 0x0201,
    LeftButtonUp = 0x0202,
    /// `WM_MOUSELEAVE`: the pointer left the slot.
    Leave = 0x02A3,
    MiddleButtonDoubleClick = 0x0209,
    MiddleButtonDown = 0x0207,
    MiddleButtonUp = 0x0208,
    Move = 0x0200,
    RightButtonDoubleClick = 0x0206,
    RightButtonDown = 0x0204,
    RightButtonUp = 0x0205,
    Wheel = 0x020A,
    XButtonDoubleClick = 0x020D,
    XButtonDown = 0x020B,
    XButtonUp = 0x020C,
    NonClientRightButtonDown = 0x00A4,
    NonClientRightButtonUp = 0x00A5,
}

impl MouseEventKind {
    const ALL: [MouseEventKind; 18] = [
        Self::HorizontalWheel,
        Self::LeftButtonDoubleClick,
        Self::LeftButtonDown,
        Self::LeftButtonUp,
        Self::Leave,
        Self::MiddleButtonDoubleClick,
        Self::MiddleButtonDown,
        Self::MiddleButtonUp,
        Self::Move,
        Self::RightButtonDoubleClick,
        Self::RightButtonDown,
        Self::RightButtonUp,
        Self::Wheel,
        Self::XButtonDoubleClick,
        Self::XButtonDown,
        Self::XButtonUp,
        Self::NonClientRightButtonDown,
        Self::NonClientRightButtonUp,
    ];

    /// The kind for a window message, if it is a mouse message WebView2 takes.
    pub fn from_window_message(msg: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|k| *k as i32 as u32 == msg)
    }

    /// Whether `mouse_data` carries a wheel delta (else an X button or 0).
    pub fn is_wheel(self) -> bool {
        matches!(self, Self::Wheel | Self::HorizontalWheel)
    }
}

/// `COREWEBVIEW2_MOUSE_EVENT_VIRTUAL_KEYS` (= the `MK_*` flags).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VirtualKeys(pub i32);

impl VirtualKeys {
    pub const NONE: Self = Self(0);
    pub const LEFT_BUTTON: Self = Self(0x1);
    pub const RIGHT_BUTTON: Self = Self(0x2);
    pub const SHIFT: Self = Self(0x4);
    pub const CONTROL: Self = Self(0x8);
    pub const MIDDLE_BUTTON: Self = Self(0x10);
    pub const X_BUTTON1: Self = Self(0x20);
    pub const X_BUTTON2: Self = Self(0x40);
    const MASK: i32 = 0x7F;

    /// From a mouse message's `wParam` (its low word holds the `MK_*` flags).
    pub fn from_wparam(wparam: usize) -> Self {
        Self((wparam & 0xFFFF) as i32 & Self::MASK)
    }

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for VirtualKeys {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// `COREWEBVIEW2_POINTER_EVENT_KIND` (= the `WM_POINTER*` message).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum PointerEventKind {
    Activate = 0x024B,
    Down = 0x0246,
    Enter = 0x0249,
    Leave = 0x024A,
    Up = 0x0247,
    Update = 0x0245,
}

impl PointerEventKind {
    /// The kind for a `WM_POINTER*` message WebView2 takes.
    pub fn from_window_message(msg: u32) -> Option<Self> {
        [
            Self::Activate,
            Self::Down,
            Self::Enter,
            Self::Leave,
            Self::Up,
            Self::Update,
        ]
        .into_iter()
        .find(|k| *k as i32 as u32 == msg)
    }
}

/// `COREWEBVIEW2_MOVE_FOCUS_REASON`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FocusReason {
    /// Focus the page where it last had focus.
    Programmatic = 0,
    /// Tab into the page: its first focusable element.
    Next = 1,
    /// Shift+Tab into the page: its last focusable element.
    Previous = 2,
}

/// The page wants focus to leave it (Tab past its last element): the
/// toolkit moves focus to the next (or previous) shell control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FocusExit {
    Next,
    Previous,
}

/// `COREWEBVIEW2_KEY_EVENT_KIND`: which window message the key came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyEventKind {
    KeyDown,
    KeyUp,
    SystemKeyDown,
    SystemKeyUp,
}

/// A key the focused page received, offered to the shell first
/// (`AcceleratorKeyPressed`). The shell returns `true` from its handler to
/// take it (the page never sees it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AcceleratorKey {
    pub kind: KeyEventKind,
    /// Windows virtual-key code (`VK_*`).
    pub virtual_key: u32,
    /// The message's `lParam` (repeat count, scan code, flags).
    pub lparam: i32,
    pub repeat_count: u32,
    pub scan_code: u32,
    pub is_extended_key: bool,
    pub is_menu_key_down: bool,
    pub was_key_down: bool,
    pub is_key_released: bool,
    /// Modifier state when the key arrived (`GetKeyState`).
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

/// Page state changes `copper_engine::PageEvent` has no variant for, from
/// `WebView2Page::changes`.
#[derive(Clone, Debug, PartialEq)]
pub enum PageChange {
    /// The status text WebView2 would show (a hovered link's URL, mostly).
    StatusText(Option<String>),
    Muted(bool),
    /// The zoom factor (the user pinched or pressed Ctrl+/−).
    Zoom(f64),
    /// The back/forward list changed (`WebView2Page::history`).
    History,
    /// The page entered or left full screen (`requestFullscreen`). The
    /// shell sizes the page's slot to the monitor and back.
    Fullscreen(bool),
    /// The page was suspended (`true`) or resumed.
    Suspended(bool),
    /// The page's webview got (`true`) or lost keyboard focus.
    Focus(bool),
}

/// The last frame of a page that was hidden, for the shell to draw while the
/// page wakes up (a suspended or slept tab paints nothing until it has run
/// again).
#[derive(Clone, PartialEq, Eq)]
pub struct Cover {
    pub jpeg: Vec<u8>,
    /// Physical pixels.
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Debug for Cover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cover")
            .field("bytes", &self.jpeg.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

/// Width and height from a baseline or progressive JPEG's `SOFn` marker.
pub(crate) fn jpeg_size(jpeg: &[u8]) -> Option<(u32, u32)> {
    if jpeg.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    while i + 4 <= jpeg.len() {
        if jpeg[i] != 0xFF {
            return None;
        }
        let marker = jpeg[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]));
        let sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if sof {
            let h = u16::from_be_bytes([*jpeg.get(i + 5)?, *jpeg.get(i + 6)?]);
            let w = u16::from_be_bytes([*jpeg.get(i + 7)?, *jpeg.get(i + 8)?]);
            return Some((u32::from(w), u32::from(h)));
        }
        i += 2 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_sizes() {
        // SOI, APP0 (len 4), SOF0 (len 11: precision, h=2, w=3, 1 component).
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00,
            0x02, 0x00, 0x03, 0x01, 0x01, 0x11, 0x00,
        ];
        assert_eq!(jpeg_size(&jpeg), Some((3, 2)));
        assert_eq!(jpeg_size(b"\x89PNG"), None);
        assert_eq!(jpeg_size(&jpeg[..10]), None);
    }

    #[test]
    fn window_messages_map() {
        assert_eq!(
            MouseEventKind::from_window_message(0x0201),
            Some(MouseEventKind::LeftButtonDown)
        );
        assert_eq!(
            MouseEventKind::from_window_message(0x02A3),
            Some(MouseEventKind::Leave)
        );
        assert_eq!(MouseEventKind::from_window_message(0x0100), None);
        assert!(MouseEventKind::Wheel.is_wheel());
        assert!(!MouseEventKind::Move.is_wheel());
        assert_eq!(
            PointerEventKind::from_window_message(0x0246),
            Some(PointerEventKind::Down)
        );
        assert_eq!(PointerEventKind::from_window_message(0x0200), None);
        let k = VirtualKeys::from_wparam(0x0078_0009);
        assert!(k.contains(VirtualKeys::LEFT_BUTTON | VirtualKeys::CONTROL));
        assert!(!k.contains(VirtualKeys::SHIFT));
    }
}
