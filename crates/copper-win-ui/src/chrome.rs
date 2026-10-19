//! Windows 11 caption buttons — the stand-in for the Mac's traffic lights.
//!
//! Three 46-wide buttons in the window's top-right corner, as tall as the
//! band Copper leaves at the top of a window ([`metrics::BARE`]), drawn with
//! Segoe Fluent Icons and the platform's own hover and press fills
//! ([`crate::tokens::caption`]). The window maps them to `HTMINBUTTON`,
//! `HTMAXBUTTON` (which brings up Snap layouts) and `HTCLOSE`.

use crate::layout::{Abs, Align, Justify};
use crate::node::{Node, Role};
use crate::theme::{Swatch, Token};
use crate::tokens::{caption, metrics};
use crate::tree::{NodeId, Tree};

/// The caption buttons' nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caption {
    pub bar: NodeId,
    pub min: NodeId,
    pub max: NodeId,
    pub close: NodeId,
}

/// Which caption button a node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionButton {
    Minimize,
    Maximize,
    Close,
}

impl Caption {
    /// Add the buttons to `parent`, pinned to its top-right corner.
    pub fn build(tree: &mut Tree, parent: NodeId) -> Caption {
        let bar = tree.add(
            parent,
            Node::row()
                .layer()
                .height(metrics::BARE)
                .justify(Justify::End)
                .align(Align::Stretch)
                .role(Role::TitleBar)
                .name("Window controls")
                .absolute(Abs {
                    right: Some(0.0),
                    top: Some(0.0),
                    ..Abs::default()
                }),
        );
        let button = |glyph: char, name: &str| {
            let mut n = Node::glyph(glyph, caption::GLYPH, caption::BUTTON_WIDTH)
                .height(metrics::BARE)
                .ink(Token::CaptionGlyph)
                .hover_fill(Token::CaptionHover)
                .press_fill(Token::CaptionPress)
                .role(Role::Button)
                .name(name);
            n.flags.hit = true;
            n
        };
        let min = tree.add(bar, button(caption::MINIMIZE, "Minimize"));
        let max = tree.add(bar, button(caption::MAXIMIZE, "Maximize"));
        let close = tree.add(
            bar,
            button(caption::CLOSE_GLYPH, "Close")
                .hover_fill(Token::Close)
                .press_fill(Swatch::Fade(Token::Close, caption::CLOSE_PRESS_ALPHA))
                .hover_ink(Token::White),
        );
        Caption {
            bar,
            min,
            max,
            close,
        }
    }

    /// Show Restore instead of Maximize while maximized.
    pub fn set_maximized(&self, tree: &mut Tree, maximized: bool) {
        let (glyph, name) = if maximized {
            (caption::RESTORE, "Restore")
        } else {
            (caption::MAXIMIZE, "Maximize")
        };
        let mut buf = [0u8; 4];
        tree.set_text(self.max, glyph.encode_utf8(&mut buf));
        tree.repaint(self.max, |n| {
            n.a11y.name.clear();
            n.a11y.name.push_str(name);
        });
    }

    /// Which button `id` is, if any.
    pub fn which(&self, id: NodeId) -> Option<CaptionButton> {
        if id == self.min {
            Some(CaptionButton::Minimize)
        } else if id == self.max {
            Some(CaptionButton::Maximize)
        } else if id == self.close {
            Some(CaptionButton::Close)
        } else {
            None
        }
    }

    /// The whole bar's width.
    pub fn width() -> f32 {
        3.0 * caption::BUTTON_WIDTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Point, Size};
    use crate::layout::NoContent;

    #[test]
    fn buttons_sit_top_right() {
        let mut t = Tree::new();
        let root = t.add_root(Node::stack());
        let c = Caption::build(&mut t, root);
        t.layout(Size::new(1200.0, 800.0), &mut NoContent);
        let close = t.window_rect(c.close);
        assert_eq!(close.right(), 1200.0);
        assert_eq!(close.h, metrics::BARE);
        assert_eq!(t.pick(Point::new(1199.0, 1.0)).target, Some(c.close));
        assert_eq!(
            c.which(
                t.pick(Point::new(1200.0 - 46.0 - 20.0, 10.0))
                    .target
                    .unwrap()
            ),
            Some(CaptionButton::Maximize)
        );
        c.set_maximized(&mut t, true);
        assert_eq!(t.get(c.max).unwrap().a11y.name, "Restore");
    }
}
