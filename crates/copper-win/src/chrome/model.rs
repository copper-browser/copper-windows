//! What the chrome shows, read from the shell in one go: the edits that
//! draw it take this by value and never reach back into the shell.

use crate::platform::{Icon, Shell};
use copper_core::browser::TabId;
use copper_engine::PageView;
use copper_shell_core::Visible;
use copper_shell_core::spaces::colour;
use copper_win_ui::Color;
use copper_win_ui::tokens::Theme as Tint;

/// Back, forward, reload or stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nav {
    pub back: bool,
    pub forward: bool,
    pub loading: bool,
    /// There is a page to act on.
    pub page: bool,
}

/// One space, as its chip draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceView {
    pub id: String,
    pub title: String,
    pub letter: String,
    pub color: Color,
    pub current: bool,
}

/// Everything the column and the strip draw.
#[derive(Clone, Debug)]
pub struct Model {
    pub visible: Visible<Icon>,
    pub active: Option<TabId>,
    pub nav: Nav,
    /// The address field's text at rest: the host, or empty.
    pub host: String,
    pub space: SpaceView,
    pub spaces: Vec<SpaceView>,
    /// The column's colour; `None` for paper.
    pub tint: Option<Tint>,
}

impl Model {
    /// Read it all from `shell`.
    pub fn of(shell: &Shell) -> Model {
        let visible = shell.visible();
        let active = shell.active_id();
        let nav = Model::nav_of(shell);
        let host = active
            .as_ref()
            .and_then(|id| shell.tab_view(id))
            .map(|t| shown_host(&t.host, &t.url))
            .unwrap_or_default();
        let b = shell.browser();
        let current = b.current_space_id().to_string();
        let spaces: Vec<SpaceView> = b
            .spaces()
            .map(|s| SpaceView {
                id: s.id.clone(),
                title: title_of(&s.name),
                letter: s.letter(),
                color: chip_color(s),
                current: s.id == current,
            })
            .collect();
        let space = spaces
            .iter()
            .find(|s| s.current)
            .cloned()
            .unwrap_or_else(|| SpaceView {
                id: current.clone(),
                title: "Space".into(),
                letter: "S".into(),
                color: Color::grey(0.5),
                current: true,
            });
        let tint = tint_of(b.current_space());
        drop(b);
        Model {
            visible,
            active,
            nav,
            host,
            space,
            spaces,
            tint,
        }
    }
}

impl Model {
    /// Back, forward and reload / stop for the selected tab.
    pub fn nav_of(shell: &Shell) -> Nav {
        let page = shell.active_page();
        let loading = shell
            .active_id()
            .and_then(|id| shell.slot(&id))
            .is_some_and(|s| s.loading.get());
        Nav {
            back: page.as_ref().is_some_and(|p| p.can_go_back()),
            forward: page.as_ref().is_some_and(|p| p.can_go_forward()),
            loading,
            page: page.is_some(),
        }
    }
}

/// A space's name, or what stands in for none.
pub fn title_of(name: &str) -> String {
    let t = name.trim();
    if t.is_empty() {
        "Space".into()
    } else {
        t.to_string()
    }
}

/// The host the address shows at rest (`Side.swift`'s `SideAddress`): no
/// `www.`; a page with no host (a file, `about:`) shows its address.
pub fn shown_host(host: &str, url: &str) -> String {
    let h = host.strip_prefix("www.").unwrap_or(host);
    if !h.is_empty() {
        return h.to_string();
    }
    if url.starts_with("about:blank") || url.is_empty() {
        return String::new();
    }
    url.to_string()
}

fn rgb(c: colour::Rgb) -> Color {
    Color::rgb(c.r as f32, c.g as f32, c.b as f32)
}

/// The colour a space's chip wears.
fn chip_color(space: &copper_core::session::Space) -> Color {
    colour::Theme::of(space)
        .colors
        .first()
        .copied()
        .map(rgb)
        .unwrap_or(Color::grey(0.5))
}

/// The column's tint for a space: a space never given a colour or a theme
/// is the app's paper (what the Mac draws under the default space);
/// otherwise its theme's first colour at its intensity.
pub fn tint_of(space: &copper_core::session::Space) -> Option<Tint> {
    if space.theme.is_none() && space.hue.is_none() {
        return None;
    }
    let t = colour::Theme::of(space);
    let c = t.colors.first().copied()?;
    Some(Tint {
        color: rgb(c),
        intensity: t.intensity as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_at_rest() {
        assert_eq!(
            shown_host("www.github.com", "https://www.github.com/"),
            "github.com"
        );
        assert_eq!(shown_host("", "about:blank"), "");
        assert_eq!(shown_host("", "file:///C:/x.html"), "file:///C:/x.html");
        assert_eq!(title_of("  "), "Space");
    }
}
