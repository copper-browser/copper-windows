//! Where a popover hangs from its door: platform-free, so it is tested
//! on any host.

use copper_win_ui::{Abs, Rect, Size};

/// A popover's gap from its door.
pub const GAP: f32 = 6.0;
/// How close a popover may come to the window's edges.
pub const MARGIN: f32 = 8.0;

/// Where a popover `width` wide goes for a door at `anchor` in a window of
/// `window`: under the door when there is room for `tall`, else over it;
/// centred on the door and kept inside the window. Returns the absolute
/// placement.
pub fn popover(anchor: Rect, window: Size, width: f32, tall: f32) -> Abs {
    let left = (anchor.x + anchor.w / 2.0 - width / 2.0)
        .min(window.w - width - MARGIN)
        .max(MARGIN);
    let below = anchor.bottom() + GAP;
    if below + tall + MARGIN <= window.h || anchor.y < window.h / 2.0 {
        Abs::at(left, below)
    } else {
        // Hung over the door: pinned by its bottom edge.
        Abs {
            left: Some(left),
            top: None,
            right: None,
            bottom: Some((window.h - anchor.y + GAP).max(MARGIN)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn a_popover_hangs_under_its_door_centred_and_inside_the_window() {
        let w = Size {
            w: 1200.0,
            h: 800.0,
        };
        let a = popover(rect(1100.0, 13.0, 26.0, 26.0), w, 340.0, 300.0);
        assert_eq!(a.top, Some(13.0 + 26.0 + GAP));
        assert_eq!(a.left, Some(1200.0 - 340.0 - MARGIN));
        let a = popover(rect(0.0, 13.0, 26.0, 26.0), w, 280.0, 300.0);
        assert_eq!(a.left, Some(MARGIN));
        let a = popover(rect(500.0, 100.0, 26.0, 26.0), w, 280.0, 300.0);
        assert_eq!(a.left, Some(513.0 - 140.0));
    }

    #[test]
    fn a_popover_from_the_foot_of_the_column_hangs_over_its_door() {
        let w = Size {
            w: 1200.0,
            h: 800.0,
        };
        let a = popover(rect(20.0, 760.0, 28.0, 28.0), w, 280.0, 420.0);
        assert_eq!(a.top, None);
        assert_eq!(a.bottom, Some(800.0 - 760.0 + GAP));
    }
}
