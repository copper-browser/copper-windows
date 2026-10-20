//! The first time (`Welcome.swift` `WelcomePanel`): four short pages over
//! the whole window — what this is, what to bring over, how to hold it,
//! and whether links from other apps should come here. Nothing is asked
//! twice, every page can be skipped, and `welcomed` is written only by
//! Skip or Start browsing.
//!
//! Windows words where the Mac's would be untrue: "your PC", Settings
//! instead of macOS's dialog, `Ctrl` instead of `⌘`. Bringing things over
//! reads the other browser's bookmarks only — Chrome and Edge seal their
//! passwords to themselves on Windows (a CSV export comes in from
//! Settings › Passwords) and their history is a database Copper does not
//! read — so those two say so instead of offering a switch that does
//! nothing.
//!
//! It is shown only at a genuine first start: not when the start brought
//! addresses to open (a link from another app goes straight to its page),
//! not for scripted runs, and not in a named test world (`COPPER_WORLD`)
//! unless `COPPER_WELCOME=1` asks for it there.

use super::bring;
use super::glyph;
use crate::platform::Shell;
use copper_core::settings::{Glyph, TabsPosition};
use copper_win_ui::input::{Key, Mods};
use copper_win_ui::tokens::motion;
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Abs, Align, Insets, Justify, Node, NodeId, Prop, Role, Swatch, TextAlign, Token, Trim, Weight,
};
use std::time::Duration;

/// The panel's nodes and what has been chosen on it.
pub struct Welcome {
    root: NodeId,
    holder: NodeId,
    page_node: Option<NodeId>,
    dots: Vec<NodeId>,
    back: NodeId,
    skip: NodeId,
    go: NodeId,
    go_text: NodeId,
    page: usize,
    /// The shown page's buttons.
    hits: Vec<(NodeId, Hit)>,
    sources: Vec<bring::Source>,
    source: usize,
    bookmarks: bool,
    bringing: bool,
    brought: Option<String>,
    is_default: bool,
    asked: bool,
    sidebar: bool,
    glyph: Glyph,
}

/// What a button on the panel does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Back,
    Skip,
    Go,
    Source(usize),
    Bookmarks,
    Bring,
    Way(bool),
    Wear(Glyph),
    MakeDefault,
}

const PAGES: usize = 4;
/// The panel's widest, and its edges (`.frame(maxWidth: 520)`,
/// `.padding(40)`).
const WIDEST: f32 = 520.0;
const EDGE: f32 = 40.0;
/// How far a page slides as it comes and goes.
const TRAVEL: f32 = 40.0;
/// The picture number of the app's icon (far from the favicons').
const ICON_IMAGE: u32 = u32::MAX - 16;
/// The icon's own PNG, the 256 frame of `copper.ico`.
const ICO: &[u8] = include_bytes!("../../assets/copper.ico");

/// Whether this start is the first (see the module's note).
pub fn wanted(shell: &Shell, opening: bool) -> bool {
    if opening || shell.options().scripted() {
        return false;
    }
    let world = std::env::var("COPPER_WORLD").unwrap_or_default();
    if !world.trim().is_empty() && std::env::var("COPPER_WELCOME").as_deref() != Ok("1") {
        return false;
    }
    !shell.settings().welcomed()
}

/// Put the panel over everything.
pub fn show(shell: &Shell) {
    let sidebar = shell.tabs_position() == TabsPosition::Left;
    let glyph = shell.settings().glyph();
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, st| {
        if st.welcome.is_some() {
            return;
        }
        if let Some(p) = icon() {
            ui.set_image(ICON_IMAGE, p.bgra, p.w, p.h);
        }
        let content = ui.content();
        let mut w = build(ui, content, sidebar, glyph);
        w.render(ui, None, 0.0);
        ui.focus(Some(w.go));
        st.welcome = Some(w);
    });
    shell.view().overlay_changed();
    // Which browsers there are to bring from is a look at the disk: off
    // the window's thread.
    std::thread::spawn(|| {
        let found = bring::installed();
        let is_default = crate::sys::privacy::is_default_browser();
        copper_win_rt::post(move || {
            let Some(shell) = crate::platform::current() else {
                return;
            };
            shell.view().chrome().edit(move |ui, st| {
                if let Some(w) = st.welcome.as_mut() {
                    w.sources = found;
                    w.is_default = is_default;
                    if w.page == 1 || w.page == 3 {
                        w.refresh(ui);
                    }
                }
            });
        });
    });
}

/// Whether the panel is up.
pub fn showing(st: &super::State) -> bool {
    st.welcome.is_some()
}

/// The app's icon, decoded from the PNG frame of `copper.ico`.
fn icon() -> Option<crate::picture::Picture> {
    let count = u16::from_le_bytes([*ICO.get(4)?, *ICO.get(5)?]) as usize;
    for i in 0..count {
        let e = ICO.get(6 + 16 * i..6 + 16 * (i + 1))?;
        let size = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let at = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        let data = ICO.get(at..at + size)?;
        if data.starts_with(b"\x89PNG") {
            let p = crate::picture::decode_png(data)?;
            return Some(crate::picture::fit(p, 144));
        }
    }
    None
}

fn build(ui: &mut Ui, content: NodeId, sidebar: bool, glyph: Glyph) -> Welcome {
    let root = ui.tree.add(
        content,
        Node::column()
            .absolute(Abs::FILL)
            .padding(Insets::all(EDGE))
            .align(Align::Center)
            .fill(Token::Ground)
            .layer()
            .drag()
            .modal()
            .role(Role::Pane)
            .name("Welcome to Copper"),
    );
    ui.tree.add(root, Node::new().grow(1.0).shrink(1.0));
    let holder = ui.tree.add(
        root,
        Node::stack().fill_width().max_width(WIDEST).shrink(0.0),
    );
    ui.tree.add(root, Node::new().grow(1.0).shrink(1.0));
    let foot = ui.tree.add(
        root,
        Node::row()
            .fill_width()
            .max_width(WIDEST)
            .gap(14.0)
            .align(Align::Center)
            .shrink(0.0),
    );
    let dot_row = ui.tree.add(foot, Node::row().gap(6.0).role(Role::None));
    let dots = (0..PAGES)
        .map(|_| {
            ui.tree.add(
                dot_row,
                Node::new()
                    .size(6.0, 6.0)
                    .radius(3.0)
                    .fill(Swatch::Fade(Token::Faint, 0.6))
                    .role(Role::None),
            )
        })
        .collect();
    ui.tree.add(foot, Node::new().grow(1.0));
    let back = plain(ui, foot, "Back");
    let skip = plain(ui, foot, "Skip");
    let (go, go_text) = big(ui, foot, "Continue", true);
    Welcome {
        root,
        holder,
        page_node: None,
        dots,
        back,
        skip,
        go,
        go_text,
        page: 0,
        hits: Vec::new(),
        sources: Vec::new(),
        source: 0,
        bookmarks: true,
        bringing: false,
        brought: None,
        is_default: false,
        asked: false,
        sidebar,
        glyph,
    }
}

/// `Button(…).buttonStyle(.plain)`: 13, muted, no plate.
fn plain(ui: &mut Ui, parent: NodeId, label: &str) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::row()
            .height(28.0)
            .padding(Insets::hv(4.0, 0.0))
            .radius(6.0)
            .align(Align::Center)
            .button(label),
    );
    ui.tree.add(
        b,
        Node::text(label, 13.0)
            .ink(Token::Muted)
            .hover_ink(Token::Ink)
            .role(Role::None),
    );
    b
}

/// `Big`: a capsule, 13 medium, 16 × 9 in; filled is ink on the ground.
fn big(ui: &mut Ui, parent: NodeId, label: &str, filled: bool) -> (NodeId, NodeId) {
    let mut n = Node::row()
        .padding(Insets::hv(16.0, 9.0))
        .radius(super::toast::CAPSULE)
        .align(Align::Center)
        .shrink(0.0)
        .button(label);
    n = if filled {
        n.fill(Token::Ink)
            .hover_fill(Swatch::Fade(Token::Ink, 0.86))
    } else {
        n.fill(Token::Wash).hover_fill(Token::Hover)
    };
    let b = ui.tree.add(parent, n);
    let t = ui.tree.add(
        b,
        Node::text(label, 13.0)
            .weight(Weight::Medium)
            .ink(if filled { Token::Ground } else { Token::Ink })
            .role(Role::None),
    );
    (b, t)
}

impl Welcome {
    /// What `id` is on the panel.
    pub fn hit(&self, id: NodeId) -> Option<Hit> {
        if id == self.back {
            return Some(Hit::Back);
        }
        if id == self.skip {
            return Some(Hit::Skip);
        }
        if id == self.go {
            return Some(Hit::Go);
        }
        self.hits.iter().find(|(n, _)| *n == id).map(|(_, h)| *h)
    }

    /// Draw the page `self.page`, sliding in from `from` (0: in place).
    fn render(&mut self, ui: &mut Ui, keep: Option<Hit>, from: f32) {
        if let Some(old) = self.page_node.take() {
            if from == 0.0 {
                ui.tree.remove(old);
            } else {
                // The old page leaves the other way and is let go after.
                ui.tree.animate(old, Prop::X, -from, motion::GLIDE);
                ui.tree.animate(old, Prop::Opacity, 0.0, motion::GLIDE);
                ui.tree.edit(old, |n| n.flags.inert = true);
                let hwnd = ui.hwnd();
                copper_win_rt::spawn_local(async move {
                    copper_win_rt::sleep(Duration::from_millis(450)).await;
                    if let Some(shell) = crate::platform::current() {
                        shell.view().chrome().edit(move |ui, _| ui.tree.remove(old));
                    }
                    copper_win_ui::win::request_frame(hwnd);
                });
            }
        }
        self.hits.clear();
        let page = ui.tree.add(
            self.holder,
            Node::column().fill_width().layer().role(Role::Group),
        );
        match self.page {
            0 => self.welcome(ui, page),
            1 => self.bring(ui, page),
            2 => self.hold(ui, page),
            _ => self.links(ui, page),
        }
        if from != 0.0 {
            ui.tree.set_prop(page, Prop::X, from);
            ui.tree.set_prop(page, Prop::Opacity, 0.0);
            ui.tree.animate(page, Prop::X, 0.0, motion::GLIDE);
            ui.tree.animate(page, Prop::Opacity, 1.0, motion::GLIDE);
        }
        self.page_node = Some(page);
        // The foot.
        for (i, d) in self.dots.iter().enumerate() {
            let fill = if i == self.page {
                Swatch::Token(Token::Ink)
            } else {
                Swatch::Fade(Token::Faint, 0.6)
            };
            ui.tree.edit(*d, |n| n.look.fill = fill);
        }
        ui.tree.set_hidden(self.back, self.page == 0);
        ui.tree.set_hidden(self.skip, self.page + 1 >= PAGES);
        let label = if self.page + 1 < PAGES {
            "Continue"
        } else {
            "Start browsing"
        };
        ui.tree.set_text(self.go_text, label);
        ui.tree.edit(self.go, |n| n.a11y.name = label.to_string());
        if let Some(k) = keep
            && let Some((id, _)) = self.hits.iter().find(|(_, h)| *h == k)
        {
            ui.focus(Some(*id));
        }
    }

    /// The same page drawn again for a change on it (keeping focus).
    fn refresh(&mut self, ui: &mut Ui) {
        let keep = ui
            .tree
            .focused()
            .and_then(|f| self.hits.iter().find(|(n, _)| *n == f).map(|(_, h)| *h));
        self.render(ui, keep, 0.0);
    }

    fn welcome(&mut self, ui: &mut Ui, page: NodeId) {
        ui.tree.edit(page, |n| n.style.align = Align::Center);
        ui.tree.edit(page, |n| n.style.gap = 22.0);
        ui.tree.add(
            page,
            Node::image(ICON_IMAGE, 72.0)
                .role(Role::Image)
                .name("Copper"),
        );
        let words = ui.tree.add(
            page,
            Node::column().gap(10.0).align(Align::Center).fill_width(),
        );
        ui.tree.add(
            words,
            Node::text("Copper", 34.0)
                .weight(Weight::Medium)
                .ink(Token::Ink),
        );
        ui.tree.add(
            words,
            Node::text(
                "A browser that works for you. Spaces, an agent at your side, and the engine already in your PC.",
                14.5,
            )
            .trim(Trim::Wrap)
            .text_align(TextAlign::Center)
            .ink(Token::Muted)
            .max_width(400.0),
        );
    }

    fn bring(&mut self, ui: &mut Ui, page: NodeId) {
        ui.tree.edit(page, |n| n.style.gap = 22.0);
        heading(
            ui,
            page,
            "Bring things over.",
            "Bookmarks go into the menu, and passwords come in from a CSV in Settings. Nothing in the other browser changes.",
        );
        if self.sources.is_empty() {
            ui.tree.add(
                page,
                Node::text(
                    "No other browser found on this PC — nothing to bring.",
                    13.0,
                )
                .trim(Trim::Wrap)
                .fill_width()
                .ink(Token::Faint),
            );
            return;
        }
        let list = ui.tree.add(page, Node::column().gap(14.0).fill_width());
        if self.sources.len() > 1 {
            let names: Vec<(Hit, &str)> = self
                .sources
                .iter()
                .enumerate()
                .map(|(i, s)| (Hit::Source(i), s.name))
                .collect();
            let chosen = Hit::Source(self.source.min(self.sources.len() - 1));
            segmented(ui, list, "Bring in from", &names, chosen, &mut self.hits);
        } else {
            ui.tree.add(
                list,
                Node::text(&format!("From {}", self.sources[0].name), 13.0).ink(Token::Muted),
            );
        }
        self.choice(
            ui,
            list,
            "Passwords",
            "Export them from that browser as a CSV, then Settings › Passwords › Import",
            None,
        );
        self.choice(
            ui,
            list,
            "Bookmarks",
            "Folders and all, behind the bookmark button",
            Some(self.bookmarks),
        );
        self.choice(
            ui,
            list,
            "History",
            "Not read on Windows — the address field learns as you go",
            None,
        );
        let row = ui.tree.add(
            page,
            Node::row().gap(12.0).align(Align::Center).fill_width(),
        );
        let label = if self.bringing {
            "Bringing…"
        } else {
            "Bring them in"
        };
        let (b, _) = big(ui, row, label, true);
        let off = self.bringing || self.brought.is_some() || !self.bookmarks;
        if off {
            ui.tree.edit(b, |n| {
                n.flags.disabled = true;
                n.transform.opacity = 0.4;
            });
        } else {
            self.hits.push((b, Hit::Bring));
        }
        if self.bringing {
            ui.tree.add(
                row,
                Node::ring(10.0, 1.4).ink(Swatch::Fade(Token::Muted, 0.7)),
            );
        }
        if let Some(said) = &self.brought {
            ui.tree.add(row, Node::text(said, 13.0).ink(Token::Muted));
        }
    }

    /// `Choice`: a title and a line under it, and its switch (`None`: no
    /// switch, the line says why).
    fn choice(&mut self, ui: &mut Ui, parent: NodeId, title: &str, detail: &str, on: Option<bool>) {
        let row = ui.tree.add(
            parent,
            Node::row().gap(12.0).align(Align::Center).fill_width(),
        );
        let words = ui
            .tree
            .add(row, Node::column().gap(2.0).grow(1.0).shrink(1.0));
        ui.tree.add(words, Node::text(title, 13.5).ink(Token::Ink));
        ui.tree.add(
            words,
            Node::text(detail, 11.5)
                .trim(Trim::Wrap)
                .fill_width()
                .ink(Token::Faint),
        );
        let Some(on) = on else { return };
        let track = ui.tree.add(
            row,
            Node::row()
                .size(34.0, 20.0)
                .shrink(0.0)
                .padding(Insets::all(2.0))
                .radius(10.0)
                .align(Align::Center)
                .justify(if on { Justify::End } else { Justify::Start })
                .fill(if on {
                    Swatch::Token(Token::Accent)
                } else {
                    Swatch::Fade(Token::Ink, 0.16)
                })
                .button(title)
                .selected(on),
        );
        ui.tree.add(
            track,
            Node::new()
                .size(16.0, 16.0)
                .radius(8.0)
                .fill(Token::White)
                .role(Role::None),
        );
        self.hits.push((track, Hit::Bookmarks));
    }

    fn hold(&mut self, ui: &mut Ui, page: NodeId) {
        ui.tree.edit(page, |n| n.style.gap = 22.0);
        heading(
            ui,
            page,
            "Two ways to hold it.",
            "Titles across the top, or down the side. The grey slides to the tab you pick either way, and you can change your mind with Ctrl+Shift+S.",
        );
        let ways = ui.tree.add(page, Node::row().gap(12.0).fill_width());
        let a = way(ui, ways, "Tab strip", false, !self.sidebar);
        self.hits.push((a, Hit::Way(false)));
        let b = way(ui, ways, "Sidebar", true, self.sidebar);
        self.hits.push((b, Hit::Way(true)));
        let wear = ui
            .tree
            .add(page, Node::row().gap(12.0).align(Align::Center));
        ui.tree
            .add(wear, Node::text("Tabs wear", 13.0).ink(Token::Muted));
        segmented(
            ui,
            wear,
            "Tabs wear",
            &[
                (Hit::Wear(Glyph::Letters), "Letters"),
                (Hit::Wear(Glyph::Icons), "Icons"),
            ],
            Hit::Wear(self.glyph),
            &mut self.hits,
        );
    }

    fn links(&mut self, ui: &mut Ui, page: NodeId) {
        ui.tree.edit(page, |n| n.style.gap = 22.0);
        heading(
            ui,
            page,
            "Links from other apps.",
            "A click in Mail, in Slack, in a PDF — Windows sends it to whichever browser is the default. It can be this one.",
        );
        let row = ui
            .tree
            .add(page, Node::row().gap(12.0).align(Align::Center));
        if self.is_default {
            let done = ui.tree.add(row, Node::row().gap(8.0).align(Align::Center));
            ui.tree.add(
                done,
                Node::glyph(glyph::CHECK, 11.0, 14.0)
                    .ink(Token::Ink)
                    .role(Role::None),
            );
            ui.tree.add(
                done,
                Node::text("Copper is the default browser", 13.0).ink(Token::Ink),
            );
        } else {
            let (b, _) = big(ui, row, "Make Copper the default", true);
            self.hits.push((b, Hit::MakeDefault));
            if self.asked {
                ui.tree.add(
                    row,
                    Node::text(
                        "Windows opens Settings to change your default browser.",
                        13.0,
                    )
                    .ink(Token::Faint),
                );
            }
        }
        let keys = ui.tree.add(page, Node::column().gap(8.0).fill_width());
        ui.tree.add(
            keys,
            Node::text("THREE THINGS WORTH KNOWING", 11.0)
                .weight(Weight::Medium)
                .ink(Token::Faint),
        );
        ui.tree.edit(keys, |n| n.style.padding.top = 6.0);
        key(
            ui,
            keys,
            "Ctrl+T",
            "A new tab. Type a place, or words to search.",
        );
        key(ui, keys, "Ctrl+K", "Every open tab, by name.");
        key(
            ui,
            keys,
            "Ctrl+,",
            "Settings, including passwords and updates.",
        );
    }
}

/// A page's heading: 26 medium, and 14 muted under it, 8 apart.
fn heading(ui: &mut Ui, page: NodeId, title: &str, line: &str) {
    let h = ui
        .tree
        .add(page, Node::column().gap(8.0).fill_width().role(Role::None));
    ui.tree.add(
        h,
        Node::text(title, 26.0)
            .weight(Weight::Medium)
            .ink(Token::Ink),
    );
    ui.tree.add(
        h,
        Node::text(line, 14.0)
            .trim(Trim::Wrap)
            .fill_width()
            .ink(Token::Muted),
    );
}

/// `Segmented`: the options side by side on a wash, the chosen one raised.
fn segmented(
    ui: &mut Ui,
    parent: NodeId,
    name: &str,
    options: &[(Hit, &str)],
    chosen: Hit,
    hits: &mut Vec<(NodeId, Hit)>,
) {
    let row = ui.tree.add(
        parent,
        Node::row()
            .padding(Insets::all(2.0))
            .gap(2.0)
            .radius(8.0)
            .fill(Token::Wash)
            .role(Role::Group)
            .name(name),
    );
    for (hit, label) in options {
        let on = *hit == chosen;
        let b = ui.tree.add(
            row,
            Node::row()
                .height(24.0)
                .padding(Insets::hv(12.0, 0.0))
                .radius(6.0)
                .align(Align::Center)
                .fill(if on {
                    Swatch::Token(Token::Ground)
                } else {
                    Swatch::Clear
                })
                .stroke(
                    if on {
                        Swatch::Token(Token::Hairline)
                    } else {
                        Swatch::Clear
                    },
                    1.0,
                )
                .hover_fill(if on { Token::Ground } else { Token::Hover })
                .button(label)
                .selected(on),
        );
        ui.tree.add(
            b,
            Node::text(label, 12.0)
                .ink(if on { Token::Ink } else { Token::Muted })
                .role(Role::None),
        );
        hits.push((b, *hit));
    }
}

/// `Way`: one of the two layouts as a small drawing of the window.
fn way(ui: &mut Ui, parent: NodeId, title: &str, sidebar: bool, chosen: bool) -> NodeId {
    let b = ui.tree.add(
        parent,
        Node::column()
            .width(0.0)
            .grow(1.0)
            .gap(10.0)
            .padding(Insets::all(10.0))
            .radius(14.0)
            .fill(if chosen {
                Swatch::Token(Token::Wash)
            } else {
                Swatch::Clear
            })
            .hover_fill(if chosen { Token::Wash } else { Token::Hover })
            .stroke(
                if chosen {
                    Swatch::Fade(Token::Ink, 0.35)
                } else {
                    Swatch::Token(Token::Hairline)
                },
                1.0,
            )
            .button(title)
            .selected(chosen),
    );
    let picture = ui.tree.add(
        b,
        Node::row()
            .fill_width()
            .height(110.0)
            .radius(8.0)
            .fill(Token::Ground)
            .stroke(Token::Hairline, 1.0)
            .clip()
            .role(Role::None),
    );
    let bar = |ui: &mut Ui, parent: NodeId, first: bool, w: Option<f32>| {
        let mut n = Node::new()
            .height(8.0)
            .radius(3.0)
            .fill(if first { Token::Wash } else { Token::Hover })
            .role(Role::None);
        n = match w {
            Some(w) => n.width(w),
            None => n.fill_width(),
        };
        ui.tree.add(parent, n);
    };
    if sidebar {
        let side = ui.tree.add(
            picture,
            Node::column()
                .width(62.0)
                .fill_height()
                .gap(4.0)
                .padding(Insets::new(8.0, 20.0, 8.0, 8.0))
                .role(Role::None),
        );
        for i in 0..4 {
            bar(ui, side, i == 0, None);
        }
        ui.tree.add(
            picture,
            Node::new()
                .width(1.0)
                .fill_height()
                .fill(Token::Hairline)
                .role(Role::None),
        );
        ui.tree.add(picture, Node::new().grow(1.0));
        caption(ui, picture);
    } else {
        let strip = ui.tree.add(
            picture,
            Node::row()
                .grow(1.0)
                .gap(3.0)
                .padding(Insets::all(8.0))
                .role(Role::None),
        );
        for i in 0..4 {
            bar(ui, strip, i == 0, Some(30.0));
        }
        caption(ui, picture);
    }
    ui.tree.add(
        b,
        Node::text(title, 13.0)
            .weight(if chosen {
                Weight::Medium
            } else {
                Weight::Regular
            })
            .ink(if chosen { Token::Ink } else { Token::Muted })
            .role(Role::None),
    );
    b
}

/// The caption buttons' corner, top right (where Windows puts them; the
/// Mac draws its lights top left).
fn caption(ui: &mut Ui, picture: NodeId) {
    let c = ui.tree.add(
        picture,
        Node::row()
            .absolute(Abs {
                left: None,
                top: Some(9.0),
                right: Some(8.0),
                bottom: None,
            })
            .gap(5.0)
            .role(Role::None),
    );
    for _ in 0..3 {
        ui.tree.add(
            c,
            Node::new()
                .size(6.0, 6.0)
                .stroke(Token::Faint, 1.0)
                .role(Role::None),
        );
    }
}

/// `Key`: a keycap and what it does.
fn key(ui: &mut Ui, parent: NodeId, keys: &str, what: &str) {
    let row = ui
        .tree
        .add(parent, Node::row().gap(10.0).align(Align::Center));
    let cap = ui.tree.add(
        row,
        Node::row()
            .min_width(44.0)
            .padding(Insets::hv(7.0, 3.0))
            .radius(6.0)
            .justify(Justify::Center)
            .fill(Token::Wash)
            .role(Role::None),
    );
    ui.tree.add(
        cap,
        Node::text(keys, 12.0)
            .weight(Weight::Medium)
            .ink(Token::Ink)
            .role(Role::None),
    );
    ui.tree.add(row, Node::text(what, 13.0).ink(Token::Muted));
}

/// A click on the panel; `false` when the panel is not up.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let chrome = shell.view().chrome().clone();
    let Some(Some(hit)) = chrome.read(|_, st| st.welcome.as_ref().map(|w| w.hit(id))) else {
        return false;
    };
    if let Some(hit) = hit {
        act(shell, hit);
    }
    true
}

/// Enter anywhere on the panel but a button is Continue
/// (`.keyboardShortcut(.defaultAction)`). Every other key stays on the
/// panel. `false` when the panel is not up.
pub fn key_event(shell: &Shell, key: Key, mods: Mods) -> bool {
    let chrome = shell.view().chrome().clone();
    if !chrome.read(|_, st| showing(st)).unwrap_or(false) {
        return false;
    }
    // (Enter on a focused button is that button's click.)
    if key == Key::Enter && !mods.ctrl && !mods.alt {
        act(shell, Hit::Go);
    }
    true
}

fn act(shell: &Shell, hit: Hit) {
    let chrome = shell.view().chrome().clone();
    match hit {
        Hit::Skip => finish(shell),
        Hit::Go => {
            let last = chrome
                .read(|_, st| st.welcome.as_ref().is_some_and(|w| w.page + 1 >= PAGES))
                .unwrap_or(true);
            if last {
                finish(shell);
            } else {
                chrome.edit(|ui, st| {
                    if let Some(w) = st.welcome.as_mut() {
                        w.page += 1;
                        w.render(ui, None, TRAVEL);
                        ui.focus(Some(w.go));
                    }
                });
            }
        }
        Hit::Back => chrome.edit(|ui, st| {
            if let Some(w) = st.welcome.as_mut()
                && w.page > 0
            {
                w.page -= 1;
                w.render(ui, None, -TRAVEL);
                ui.focus(Some(if w.page == 0 { w.go } else { w.back }));
            }
        }),
        Hit::Source(i) => chrome.edit(move |ui, st| {
            if let Some(w) = st.welcome.as_mut() {
                w.source = i;
                w.refresh(ui);
            }
        }),
        Hit::Bookmarks => chrome.edit(|ui, st| {
            if let Some(w) = st.welcome.as_mut() {
                w.bookmarks = !w.bookmarks;
                w.refresh(ui);
            }
        }),
        Hit::Bring => bring_in(shell),
        Hit::Way(sidebar) => {
            shell.set_tabs_position(if sidebar {
                TabsPosition::Left
            } else {
                TabsPosition::Top
            });
            chrome.edit(move |ui, st| {
                if let Some(w) = st.welcome.as_mut() {
                    w.sidebar = sidebar;
                    w.refresh(ui);
                }
            });
        }
        Hit::Wear(g) => {
            shell.update_settings(|s| s.set_glyph(g));
            shell.view().sync(shell);
            chrome.edit(move |ui, st| {
                if let Some(w) = st.welcome.as_mut() {
                    w.glyph = g;
                    w.refresh(ui);
                }
            });
        }
        Hit::MakeDefault => make_default(shell),
    }
}

/// Read the chosen browser's bookmarks off the window's thread, then file
/// them.
fn bring_in(shell: &Shell) {
    let chrome = shell.view().chrome().clone();
    let Some(Some(source)) = chrome.read(|ui, st| {
        let w = st.welcome.as_mut()?;
        let s = w.sources.get(w.source).cloned()?;
        w.bringing = true;
        w.refresh(ui);
        Some(s)
    }) else {
        return;
    };
    std::thread::spawn(move || {
        let found = bring::read(&source);
        copper_win_rt::post(move || {
            let Some(shell) = crate::platform::current() else {
                return;
            };
            let taken = bring::take(found, &mut shell.bookmarks_mut());
            let list = shell.bookmarks().roots().to_vec();
            shell.saver().schedule(list);
            let said = match taken {
                1 => "1 bookmark".to_string(),
                n => format!("{n} bookmarks"),
            };
            shell.view().chrome().edit(move |ui, st| {
                if let Some(w) = st.welcome.as_mut() {
                    w.bringing = false;
                    w.brought = Some(said);
                    w.refresh(ui);
                }
            });
        });
    });
}

/// Open Windows' default-apps page for Copper, then watch for Windows'
/// answer while the panel is up (never claiming it before Windows does).
fn make_default(shell: &Shell) {
    if let Err(e) = crate::sys::shell::open(copper_setup::registration::DEFAULT_APPS_URI) {
        tracing::warn!("default apps: {e}");
    }
    let chrome = shell.view().chrome().clone();
    chrome.edit(|ui, st| {
        if let Some(w) = st.welcome.as_mut() {
            w.asked = true;
            w.refresh(ui);
        }
    });
    copper_win_rt::spawn_local(async move {
        for _ in 0..240 {
            copper_win_rt::sleep(Duration::from_millis(500)).await;
            let Some(shell) = crate::platform::current() else {
                return;
            };
            let up = shell
                .view()
                .chrome()
                .read(|_, st| showing(st))
                .unwrap_or(false);
            if !up {
                return;
            }
            if crate::sys::privacy::is_default_browser() {
                shell.view().chrome().edit(|ui, st| {
                    if let Some(w) = st.welcome.as_mut() {
                        w.is_default = true;
                        w.refresh(ui);
                    }
                });
                return;
            }
        }
    });
}

/// Skip or Start browsing: remember, and fade the panel away
/// (`Motion.settle`). The layout and the tabs' marks chosen are written
/// down too, so a later start reads them rather than defaults.
fn finish(shell: &Shell) {
    let glyph = shell.settings().glyph();
    let position = shell.tabs_position();
    shell.update_settings(|s| {
        s.set(copper_core::settings::keys::WELCOMED, true);
        s.set_glyph(glyph);
        s.set_tabs_position(position);
    });
    let chrome = shell.view().chrome().clone();
    chrome.edit(|ui, st| {
        let Some(w) = st.welcome.take() else { return };
        ui.tree.edit(w.root, |n| n.flags.inert = true);
        ui.tree.edit(w.root, |n| n.flags.modal = false);
        ui.tree.animate(w.root, Prop::Opacity, 0.0, motion::SETTLE);
        let root = w.root;
        copper_win_rt::spawn_local(async move {
            copper_win_rt::sleep(Duration::from_millis(450)).await;
            if let Some(shell) = crate::platform::current() {
                shell.view().chrome().edit(move |ui, _| {
                    ui.tree.remove(root);
                    ui.remove_image(ICON_IMAGE);
                });
            }
        });
    });
    shell.view().overlay_changed();
    shell.view().focus_page();
}
