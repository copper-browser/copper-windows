//! The bars that rise from the bottom of the window to say one thing
//! (`App.swift` `bars`): the announcement (a toast, gone after long enough
//! to read it) and above it a question waiting for an answer.

use copper_win_ui::tokens::{Shadow, motion};
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Align, Insets, Justify, Node, NodeId, Prop, Role, Token};
use std::time::Duration;

/// A corner past any height: the toolkit clamps it to half the height,
/// SwiftUI's `Capsule()`.
pub const CAPSULE: f32 = 999.0;

/// How long an announcement stays: long enough to read, 1.7–6 s
/// (`Fork/Announcements.swift`).
pub fn seconds(text: &str) -> Duration {
    let s = (text.chars().count() as f64 / 15.0).clamp(1.7, 6.0);
    Duration::from_secs_f64(s)
}

/// What the downloads listener saw just before the shell announces it:
/// the shell (shared with Linux) words a finished or failed download its
/// own way, and the Mac's words are put in their place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadNews {
    /// Finished, saved as this file name (empty when unknown).
    Finished(String),
    /// Failed (not cancelled).
    Failed,
}

/// The Mac's words for a download announcement (`Browser.swift`
/// 2604–2642): `Saved {file}` or `Download finished`, and `Download
/// failed`. `None`: `text` is not about `news`, keep it.
pub fn download_words(text: &str, news: &DownloadNews) -> Option<String> {
    match news {
        DownloadNews::Finished(name) if !name.is_empty() => text
            .starts_with(&format!("{name} is in "))
            .then(|| format!("Saved {name}")),
        DownloadNews::Finished(_) => text
            .contains(" is in ")
            .then(|| "Download finished".to_string()),
        DownloadNews::Failed => text
            .ends_with(" failed")
            .then(|| "Download failed".to_string()),
    }
}

/// The pill's look: the page's ground in a capsule, a hairline, a soft
/// shadow (`App.swift` 519–531).
fn pill(n: Node) -> Node {
    n.fill(Token::Ground)
        .stroke(Token::Hairline, 1.0)
        .radius(CAPSULE)
        .shadow(Shadow::new(0.10, 18.0, 6.0))
        .layer()
}

/// The bars and the toast in them.
pub struct Toasts {
    bars: NodeId,
    toast: NodeId,
    text: NodeId,
    /// Bumped by every toast, so an older one's timer leaves a newer alone.
    pub generation: u64,
}

pub fn build(ui: &mut Ui, parent: NodeId) -> Toasts {
    let bars = ui.tree.add(
        parent,
        Node::column()
            .absolute(Abs {
                left: Some(0.0),
                top: None,
                right: Some(0.0),
                bottom: Some(30.0),
            })
            .align(Align::Center)
            .justify(Justify::End)
            .gap(8.0),
    );
    let toast = ui.tree.add(
        bars,
        pill(Node::row())
            .padding(Insets::hv(15.0, 9.0))
            .hidden(true)
            .role(Role::ToolTip),
    );
    let text = ui.tree.add(toast, Node::text("", 12.0).ink(Token::Ink));
    Toasts {
        bars,
        toast,
        text,
        generation: 0,
    }
}

impl Toasts {
    /// The bars' stack: the question sheet goes in it too.
    pub fn bars(&self) -> NodeId {
        self.bars
    }

    /// Show `text`; returns its generation, for [`Toasts::hide`].
    pub fn show(&mut self, ui: &mut Ui, text: &str) -> u64 {
        self.generation += 1;
        ui.tree.set_text(self.text, text);
        ui.announce(text);
        let was_hidden = ui.tree.get(self.toast).is_some_and(|n| n.style.hidden);
        ui.tree.set_hidden(self.toast, false);
        if was_hidden {
            ui.tree.set_prop(self.toast, Prop::Y, 12.0);
            ui.tree.set_prop(self.toast, Prop::Opacity, 0.0);
        }
        ui.tree.animate(self.toast, Prop::Y, 0.0, motion::SETTLE);
        ui.tree
            .animate(self.toast, Prop::Opacity, 1.0, motion::QUICK);
        self.generation
    }

    /// Take toast `generation` away (if it is still the one showing).
    pub fn hide(&mut self, ui: &mut Ui, generation: u64) {
        if generation == self.generation {
            ui.tree.set_hidden(self.toast, true);
        }
    }
}

/// The ask bar's look, for [`super::sheet`]: the same pill, with a
/// primary (ink) and a quiet button.
pub fn ask_pill() -> Node {
    Node::row()
        .fill(Token::Ground)
        .stroke(Token::Hairline, 1.0)
        .radius(CAPSULE)
        .shadow(Shadow::new(0.12, 20.0, 6.0))
        .layer()
        .padding(Insets::new(16.0, 9.0, 10.0, 9.0))
        .gap(12.0)
        .align(Align::Center)
}

/// A capsule button: `primary` in ink, else quiet.
pub fn capsule(label: &str, primary: bool) -> (Node, Node) {
    let b = Node::row()
        .padding(Insets::hv(11.0, 5.0))
        .radius(CAPSULE)
        .align(Align::Center)
        .button(label);
    let b = if primary {
        b.fill(Token::Ink)
    } else {
        b.hover_fill(Token::Hover)
    };
    // `App.swift` sets both answers in 12 pt regular.
    let t = Node::text(label, 12.0)
        .ink(if primary { Token::Ground } else { Token::Muted })
        .role(Role::None);
    (b, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_time_is_bounded() {
        assert_eq!(seconds("Paused"), Duration::from_secs_f64(1.7));
        assert_eq!(seconds(&"x".repeat(45)), Duration::from_secs_f64(3.0));
        assert_eq!(seconds(&"x".repeat(400)), Duration::from_secs_f64(6.0));
    }

    #[test]
    fn downloads_speak_the_macs_words() {
        let saved = DownloadNews::Finished("report 2.pdf".into());
        assert_eq!(
            download_words("report 2.pdf is in Downloads", &saved).as_deref(),
            Some("Saved report 2.pdf")
        );
        assert_eq!(download_words("Address copied", &saved), None);
        assert_eq!(
            download_words("x is in Downloads", &DownloadNews::Finished(String::new())).as_deref(),
            Some("Download finished")
        );
        assert_eq!(
            download_words("report.pdf failed", &DownloadNews::Failed).as_deref(),
            Some("Download failed")
        );
        assert_eq!(download_words("Paused", &DownloadNews::Failed), None);
    }
}
