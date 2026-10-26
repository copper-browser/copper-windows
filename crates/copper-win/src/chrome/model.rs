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
    /// Its icon as stored: an emoji, or `sf:<symbol>` (`palette::symbols`).
    pub icon: Option<String>,
    /// How many loose tabs it holds.
    pub count: usize,
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
    /// How many rows Clear would close (`crate::clear`).
    pub clearable: usize,
    /// `groups.json`: the folders Saved and Today rows are filed in.
    pub groups: Vec<copper_core::groups::TabGroup>,
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
                icon: s.icon.clone(),
                count: b.row(&s.id).len(),
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
                icon: None,
                count: 0,
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
            clearable: shell.clearable_tabs().len(),
            groups: copper_shell_core::agents::groups::all(shell),
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
    if let Some(l) = crate::canvas::address_label(url) {
        return l;
    }
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

/// The column's tint for a space: `Space.look` — its theme, else its hue,
/// else Copper's own (the Mac's default column; its picture is not drawn
/// here) — flattened the way `SpaceTheme.toned` does: the mean of its stops,
/// its intensity and tone, and the hue of its most saturated stop.
pub fn tint_of(space: &copper_core::session::Space) -> Option<Tint> {
    Some(tint(&colour::Theme::of(space)))
}

/// A shared-crate theme as the toolkit's tint.
fn tint(t: &colour::Theme) -> Tint {
    let stops = &t.colors;
    let mean = match stops.len() {
        0 => colour::Rgb::grey(0.62),
        1 => stops[0],
        n => {
            // The trapezoid over evenly spaced stops (`SpaceTheme.toned`).
            let k = (n - 1) as f64;
            let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
            for (i, s) in stops.iter().enumerate() {
                let w = if i == 0 || i == n - 1 { 0.5 } else { 1.0 } / k;
                r += s.r * w;
                g += s.g * w;
                b += s.b * w;
            }
            colour::Rgb::new(r, g, b)
        }
    };
    let mut each = [rgb(mean); 3];
    for (slot, c) in each.iter_mut().zip(stops) {
        *slot = rgb(*c);
    }
    Tint {
        color: rgb(mean),
        intensity: t.intensity as f32,
        tone: t.tone as f32,
        hue: t.hue_of().map(|h| h as f32),
        stops: each,
        count: stops.len().clamp(1, 3) as u8,
    }
}

/// The column's tint while a private tab is picked: copper-shell-core's
/// quiet violet-grey (`colour::Theme::private`, the Linux build's), on the
/// column and strip only. A Windows extension: the Mac marks a private
/// tab with `eye.slash` alone.
pub fn private_tint() -> Tint {
    tint(&colour::Theme::private())
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

    #[test]
    fn spaces_wear_their_whole_theme() {
        use copper_core::session::{ColorStop, Space, SpaceTheme};
        // No theme, no hue: Copper's own column, not paper.
        let home = Space::new("Home");
        let t = tint_of(&home).unwrap();
        assert!((t.intensity - 0.45).abs() < 1e-6 && t.hue.is_some());
        // A gradient is its mean, and its hue the fullest stop's.
        let stop = |r, g, b| ColorStop {
            r,
            g,
            b,
            ..Default::default()
        };
        let mut s = Space::new("Exowatt");
        s.theme = Some(SpaceTheme {
            colors: vec![stop(1.0, 0.0, 0.0), stop(0.0, 0.0, 1.0)],
            intensity: Some(0.6),
            tone: Some(-0.5),
            ..Default::default()
        });
        let t = tint_of(&s).unwrap();
        assert!((t.color.r - 0.5).abs() < 1e-6 && (t.color.b - 0.5).abs() < 1e-6);
        assert!((t.tone + 0.5).abs() < 1e-6);
        assert!(t.hue.is_some());
    }
}
