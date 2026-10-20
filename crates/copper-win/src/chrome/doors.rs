//! The doors the panels hang from, built with the chrome as slots of its
//! own: the downloads door at the end of the tab strip and inside the
//! column's address (`DownloadsDoor`, `Side.swift` 868–880), and the
//! bookmarks door at the strip's far end (`TabBar.swift` 107–122; the
//! column's foot has its own).
//!
//! The chrome only builds them, hidden where the Mac hides them; what
//! they show (the ring, the check, the dot) and what a click opens are
//! the Downloads and Bookmarks panels' (`panels/door.rs`), which find the
//! nodes here.

use super::build::{door, tip};
use super::glyph;
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Justify, Node, NodeId, Role, Token};

/// The tab strip's doors.
pub const STRIP: f32 = 26.0;
/// The column address's door.
pub const COLUMN: f32 = 24.0;
/// The pictures the downloads rings are drawn into (the panels' block of
/// image ids).
const RING_STRIP: u32 = 0x7061_1001;
const RING_COLUMN: u32 = 0x7061_1002;
/// The bookmarks door's glyph.
const BOOKMARK_GLYPH: f32 = 13.0;

/// A downloads door's nodes: the button, the ring picture over it, the
/// arrow (or check, or warning) glyph and the unseen dot.
#[derive(Clone, Copy, Debug)]
pub struct DownloadsDoor {
    pub door: NodeId,
    pub ring: NodeId,
    pub glyph: NodeId,
    pub dot: NodeId,
    /// The picture id its ring is drawn into.
    pub image: u32,
    /// Its side (DIPs).
    pub size: f32,
}

/// The tab strip's downloads door, hidden until there is a download.
pub fn strip_downloads(ui: &mut Ui, strip: NodeId) -> DownloadsDoor {
    downloads(ui, strip, STRIP, RING_STRIP)
}

/// The column address's downloads door, hidden until there is one.
pub fn column_downloads(ui: &mut Ui, address: NodeId) -> DownloadsDoor {
    downloads(ui, address, COLUMN, RING_COLUMN)
}

/// The tab strip's bookmarks door.
pub fn bookmarks(ui: &mut Ui, strip: NodeId) -> NodeId {
    door(
        ui,
        strip,
        glyph::BOOKMARK,
        STRIP,
        BOOKMARK_GLYPH,
        "Bookmarks",
        &tip("Bookmarks", "bookmarks-door"),
    )
    .0
}

/// One downloads door, hidden.
fn downloads(ui: &mut Ui, parent: NodeId, size: f32, image: u32) -> DownloadsDoor {
    let door = ui.tree.add(
        parent,
        Node::stack()
            .size(size, size)
            .shrink(0.0)
            .radius(size * 0.3)
            .hover_fill(Token::ColumnHover)
            .press_fill(Token::ColumnHover)
            .selected_fill(Token::ColumnHover)
            .align(Align::Center)
            .justify(Justify::Center)
            .hidden(true)
            .button("Downloads")
            .tooltip("Downloads"),
    );
    let ring = ui.tree.add(
        door,
        Node::image(image, size)
            .absolute(Abs::at(0.0, 0.0))
            .hidden(true)
            .role(Role::None),
    );
    let glyph = ui.tree.add(
        door,
        Node::glyph(
            glyph::ARROW_DOWN,
            if size < STRIP { 9.5 } else { 10.0 },
            size,
        )
        .ink(Token::Muted)
        .role(Role::None),
    );
    let dot = ui.tree.add(
        door,
        Node::new()
            .size(5.0, 5.0)
            .radius(2.5)
            .fill(Token::Ink)
            .absolute(Abs {
                left: None,
                top: Some(2.0),
                right: Some(2.0),
                bottom: None,
            })
            .hidden(true)
            .role(Role::None),
    );
    DownloadsDoor {
        door,
        ring,
        glyph,
        dot,
        image,
        size,
    }
}
