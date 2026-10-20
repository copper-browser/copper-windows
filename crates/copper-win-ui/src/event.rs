//! What the toolkit tells the app.

use crate::geom::{Point, Size};
use crate::input::{Key, Mods};
use crate::tree::NodeId;

/// One thing that happened. Delivered to the app's handler after the
/// toolkit has updated the tree for it (hover, press and focus states are
/// already applied).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A pointer target was clicked, or a focused button was invoked from
    /// the keyboard (Enter, Space) or by UI Automation.
    Click(NodeId),
    /// A middle click (press and release on the same pointer target): a
    /// row's close, a link's new tab.
    MiddleClick(NodeId),
    /// A secondary click or the context-menu key. `target` is the innermost
    /// pointer target under the point (or the focused node, from the
    /// keyboard); `at` is in window DIPs.
    ContextMenu { target: Option<NodeId>, at: Point },
    /// A field's text changed (typing, paste, undo, an IME commit).
    FieldChanged(NodeId),
    /// Enter in a field.
    FieldSubmit(NodeId),
    /// Escape in a field.
    FieldCancel(NodeId),
    /// A key the focused node did not use (Ctrl+K, arrows in the palette).
    Key {
        key: Key,
        mods: Mods,
        focused: Option<NodeId>,
    },
    /// Keyboard focus moved inside the chrome (`None`: to the page or out).
    Focus(Option<NodeId>),
    /// The pointer target under the pointer changed (`None`: over nothing
    /// of the chrome's, or out of the window).
    Hover(Option<NodeId>),
    /// A press on a draggable node moved past the drag threshold. `at` is
    /// the pointer in window DIPs.
    DragStart { source: NodeId, at: Point },
    /// The drag moved; `target` is the innermost pointer target under it.
    DragMove {
        source: NodeId,
        target: Option<NodeId>,
        at: Point,
    },
    /// The button came up: dropped on `target`. No click follows.
    Drop {
        source: NodeId,
        target: Option<NodeId>,
        at: Point,
    },
    /// The drag ended without a drop (Escape, the window lost the mouse).
    DragCancel { source: NodeId },
    /// A menu item was chosen.
    MenuChosen { menu: u32, item: u32 },
    /// A menu closed without a choice.
    MenuDismissed { menu: u32 },
    /// The client area changed size, in DIPs.
    Resized(Size),
    /// The window moved to a monitor with another scale.
    ScaleChanged(f32),
    /// Light or dark changed (system switch or the override).
    Appearance { dark: bool },
    /// The window became (or stopped being) the foreground window.
    Activated(bool),
    /// The user asked to close the window (the close button, Alt+F4). The
    /// toolkit does nothing more until the app calls `close`.
    CloseRequested,
    /// The window was destroyed (after `close`); the last event it sends.
    Closed,
    /// The first frame reached the compositor and the window was shown.
    FirstFrame {
        /// From the process's creation, as Windows recorded it.
        since_process_ms: f32,
        /// From `Window::new`.
        since_create_ms: f32,
    },
    /// A frame was produced.
    Frame {
        /// CPU time spent on layout, painting and committing.
        cpu_ms: f32,
        /// Layers redrawn this frame.
        layers: u32,
        /// Time since the previous frame.
        interval_ms: f32,
        /// From the key, click or wheel turn this frame answers to the
        /// composition that picks it up (the first vertical blank after
        /// the commit, from the compositor's timing); `None` if no input
        /// led to it.
        input_to_screen_ms: Option<f32>,
    },
}
