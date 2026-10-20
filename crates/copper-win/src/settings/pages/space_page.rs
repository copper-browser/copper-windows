//! Settings › Spaces drawn the Mac's way (`Fork/SpacePage.swift`), on the
//! page's own-body hook: the spaces across the top as chips, the picked
//! one lifted out of the row (`SpacesSettingsPage.picker`, 46–112), and
//! under them the picked space — its mark, name and what it holds
//! (`header`, 263–281), a name field (`name`, 316–328), its profile, the
//! icon tiles (`icons`/`tile`, 334–399), the colour wells, its place in
//! the row and Delete. Every change lands as it is made.
//!
//! Not drawn: the live column preview and the Look tabs (gradient,
//! picture, scene) with their Adjust sliders — a space on Windows wears
//! one named colour (`SpaceColour`), which the wells pick.

use crate::palette::{spaces, symbols};
use crate::platform::Shell;
use crate::settings::controls;
use crate::settings::look::{self, Inks, metric};
use copper_shell_core::spaces::colour::{Rgb, SpaceColour, Theme};
use copper_win_ui::tokens::Shadow;
use copper_win_ui::win::Ui;
use copper_win_ui::{
    Align, Color, Event, Insets, Justify, Node, NodeId, Point, Role, Swatch, TextAlign, Token,
    Trim, Weight,
};
use std::cell::RefCell;

/// The picker's chips (`chip`, 76–112): 30 high, corner 8, 7 between the
/// mark's 20-square and the name.
const CHIP_H: f32 = 30.0;
const CHIP_RADIUS: f32 = 8.0;
const CHIP_MARK: f32 = 20.0;
/// The icon tiles (`tile`, 384–399): 34 × 30, corner 7, 5 apart.
const TILE_W: f32 = 34.0;
const TILE_H: f32 = 30.0;
const TILE_GAP: f32 = 5.0;
/// The colour wells: a 22 circle in a 30 target.
const WELL: f32 = 22.0;
const WELL_TARGET: f32 = 30.0;
/// The resting chips' and tiles' wash (`Palette.wash.opacity(0.7)`).
const WASH: f32 = 0.7;

/// What a click on the page does.
#[derive(Clone, Debug)]
enum Act {
    Pick(String),
    New,
    Icon(Option<String>),
    Colour(SpaceColour),
    /// The profile popup: its menu (`palette::spaces::profile_menu`).
    Profile,
    Left,
    Right,
    Delete,
}

/// One space as the page shows it.
#[derive(Clone, Debug, PartialEq)]
struct SpaceSnap {
    id: String,
    title: String,
    name: String,
    icon: Option<String>,
    colour: SpaceColour,
    /// Its theme's first colour.
    color: Color,
    profile: Option<String>,
}

/// Everything the page draws, read from the shell.
#[derive(Clone, Debug, PartialEq)]
struct Snap {
    editing: String,
    current: String,
    all: Vec<SpaceSnap>,
    tabs: usize,
    profiles: Vec<String>,
    /// New Profile… was chosen for the space shown: its name is asked.
    naming: bool,
    dark: bool,
    /// The page column's width (DIPs) the grid is laid out for.
    room: f32,
}

impl Snap {
    /// What, changed, means drawing the page again; names alone are
    /// patched in place so the name field keeps its caret.
    fn sig(&self) -> Snap {
        let mut s = self.clone();
        for sp in &mut s.all {
            sp.title.clear();
            sp.name.clear();
        }
        s
    }
    fn picked(&self) -> Option<&SpaceSnap> {
        self.all.iter().find(|s| s.id == self.editing)
    }
}

/// What the page drew last.
#[derive(Default)]
struct Drawn {
    sig: Option<Snap>,
    root: Option<NodeId>,
    hits: Vec<(NodeId, Act)>,
    name: Option<NodeId>,
    emoji: Option<NodeId>,
    /// The new profile's name, while New Profile… asks for one.
    profile: Option<NodeId>,
    /// Nodes showing a space's title, by space.
    titles: Vec<(String, NodeId)>,
}

thread_local! {
    static PAGE: RefCell<Drawn> = RefCell::default();
}

fn rgb(c: Rgb) -> Color {
    Color::rgb(c.r as f32, c.g as f32, c.b as f32)
}

fn first_colour(theme: &Theme) -> Color {
    theme
        .colors
        .first()
        .copied()
        .map(rgb)
        .unwrap_or(Color::grey(0.5))
}

/// A named colour's own colour (`theme(for:)`).
fn colour_of(c: SpaceColour) -> Color {
    first_colour(&c.hue().map(Theme::hue).unwrap_or_else(Theme::plain))
}

fn snap(shell: &Shell) -> Snap {
    let editing = spaces::editing(shell);
    let b = shell.browser();
    let all = b
        .spaces()
        .map(|s| SpaceSnap {
            id: s.id.clone(),
            title: symbols::title(s.icon.as_deref(), &s.name),
            name: s.name.clone(),
            icon: s.icon.clone(),
            colour: SpaceColour::of(s),
            color: first_colour(&Theme::of(s)),
            profile: s.profile.clone(),
        })
        .collect();
    let tabs = b.row(&editing).len();
    let current = b.current_space_id().to_string();
    let profiles = b.profiles();
    drop(b);
    let naming = spaces::naming_profile(&editing);
    Snap {
        editing,
        current,
        all,
        tabs,
        naming,
        profiles,
        dark: shell.is_dark(),
        room: 0.0,
    }
}

/// How many `w`-wide items, `gap` apart, fit on one line of a card in a
/// page column `room` wide (the card's 14 each side and its edge).
fn per_line(room: f32, w: f32, gap: f32) -> usize {
    let inner = room - 2.0 * 14.0 - 2.0;
    (((inner + gap) / (w + gap)).floor() as usize).clamp(4, 27)
}

// MARK: Settings' hooks

/// The page, drawn into Settings' list.
#[allow(clippy::type_complexity)]
pub(super) fn body(shell: &Shell) -> Option<Box<dyn FnOnce(&mut Ui, NodeId)>> {
    let s = snap(shell);
    Some(Box::new(move |ui: &mut Ui, list: NodeId| {
        draw(ui, list, &s)
    }))
}

/// A click Settings didn't take: the page's, if it is one of its controls.
pub(super) fn click(shell: &Shell, id: NodeId) -> bool {
    let Some(act) = PAGE.with(|p| {
        p.borrow()
            .hits
            .iter()
            .find(|(n, _)| *n == id)
            .map(|(_, a)| a.clone())
    }) else {
        return false;
    };
    let editing = spaces::editing(shell);
    match act {
        Act::Pick(id) => spaces::pick(&id),
        Act::New => spaces::new_space(shell),
        Act::Icon(icon) => shell.set_space_icon(&editing, icon.as_deref()),
        Act::Colour(c) => shell.set_space_colour(&editing, c),
        Act::Profile => {
            let at = shell
                .view()
                .chrome()
                .read(move |ui, _| {
                    let r = ui.tree.window_rect(id);
                    Point::new(r.x, r.y + r.h + 4.0)
                })
                .unwrap_or_default();
            spaces::profile_menu(shell, &editing, at);
            return true;
        }
        Act::Left => shell.nudge_space(&editing, -1),
        Act::Right => shell.nudge_space(&editing, 1),
        Act::Delete => spaces::ask_delete(shell, &editing),
    }
    crate::settings::changed(shell, "spaces");
    true
}

/// The name field renames as it is typed (`name`, 322–325: an empty name
/// is not taken); the emoji field takes the first emoji typed as the icon
/// (`icons`, 358–365).
pub(super) fn field(shell: &Shell, id: NodeId, e: &Event) -> bool {
    let (name, emoji, profile) = PAGE.with(|p| {
        let p = p.borrow();
        (p.name, p.emoji, p.profile)
    });
    if ![name, emoji, profile].contains(&Some(id)) {
        return false;
    }
    let text = shell
        .view()
        .chrome()
        .read(move |ui, _| ui.field_text(id).unwrap_or("").to_string())
        .unwrap_or_default();
    let editing = spaces::editing(shell);
    match e {
        Event::FieldChanged(_) if Some(id) == name => {
            shell.rename_space(&editing, &text);
            true
        }
        Event::FieldChanged(_) if Some(id) == emoji => {
            if let Some(first) = symbols::first_emoji(&text) {
                shell.set_space_icon(&editing, Some(&first));
                crate::settings::changed(shell, "spaces");
            }
            true
        }
        // `profile`, 703–708: Return names it, if there is a name.
        Event::FieldSubmit(_) if Some(id) == profile => {
            let named = text.trim();
            if !named.is_empty() {
                spaces::named_profile();
                shell.set_space_profile(&editing, Some(named));
                crate::settings::changed(shell, "spaces");
            }
            true
        }
        Event::FieldChanged(_) => true,
        Event::FieldSubmit(_) => {
            shell.view().chrome().read(|ui, _| ui.focus(None));
            true
        }
        _ => false,
    }
}

/// The page went off screen: what it drew is gone with the list.
pub(super) fn left() {
    spaces::named_profile();
    PAGE.with(|p| *p.borrow_mut() = Drawn::default());
}

// MARK: drawing

fn draw(ui: &mut Ui, list: NodeId, s: &Snap) {
    let mut s = s.clone();
    let r = ui.tree.window_rect(list);
    let pad = ui
        .tree
        .get(list)
        .map_or(0.0, |n| n.style.padding.left + n.style.padding.right);
    s.room = if r.w > 100.0 { r.w - pad } else { 446.0 };
    let s = &s;
    let sig = s.sig();
    let kept = PAGE.with(|p| {
        let p = p.borrow();
        p.sig.as_ref() == Some(&sig)
            && p.root
                .is_some_and(|r| ui.tree.children(list).first() == Some(&r))
    });
    if kept {
        // Only names changed: patch them where they show, and leave the
        // name field (and its caret) alone.
        PAGE.with(|p| {
            for (id, node) in &p.borrow().titles {
                if let Some(sp) = s.all.iter().find(|x| x.id == *id) {
                    ui.tree.set_text(*node, &sp.title);
                }
            }
        });
        return;
    }
    let (had_name, had_profile) = PAGE.with(|p| {
        let p = p.borrow();
        (p.name, p.profile)
    });
    let typing = had_name.is_some() && ui.tree.focused() == had_name;
    // New Profile… just chosen, or its name being typed: the field has
    // the caret (`focus = .profile`).
    let naming = had_profile.is_none() || ui.tree.focused() == had_profile;
    ui.tree.clear_children(list);
    let ink = Inks::new(s.dark);
    let mut d = Drawn::default();
    let root = ui.tree.add(list, Node::column().gap(18.0));
    d.root = Some(root);
    picker(ui, root, s, &mut d);
    if let Some(sp) = s.picked() {
        page(ui, root, s, sp, &ink, &mut d);
    }
    if typing && let Some(n) = d.name {
        ui.focus(Some(n));
    } else if naming && let Some(f) = d.profile {
        ui.focus(Some(f));
    }
    d.sig = Some(sig);
    PAGE.with(|p| *p.borrow_mut() = d);
}

/// A space's mark (`SpaceGlyph`, bare): its emoji, symbol or a dot, in
/// `ink`, about `size` across.
fn mark(ui: &mut Ui, parent: NodeId, sp: &SpaceSnap, size: f32, ink: Swatch) {
    let node = match symbols::mark(sp.icon.as_deref(), &sp.name) {
        symbols::Mark::Emoji(e) => Node::text(&e, size)
            .text_align(TextAlign::Center)
            .size(size + 4.0, size + 4.0),
        symbols::Mark::Symbol(g) => Node::glyph(g, size * 0.9, size + 4.0)
            .weight(Weight::Semibold)
            .ink(ink),
        symbols::Mark::Dot => Node::new().size(7.0, 7.0).radius(3.5).fill(ink),
    };
    ui.tree.add(parent, node.role(Role::None));
}

/// The column's ground for a space, off the column (`offColumn.ground`):
/// its colour faintly over the card.
fn tint_ground(c: Color, dark: bool) -> Swatch {
    Swatch::Fixed(Color {
        a: if dark { 0.26 } else { 0.16 },
        ..c
    })
}

/// The space's mark ink (`offColumn.mark`): its colour, deepened on white
/// and lifted on the dark card so it reads on either.
fn tint_mark(c: Color, dark: bool) -> Swatch {
    Swatch::Fixed(if dark {
        c.mix(Color::WHITE, 0.25)
    } else {
        c.mix(Color::BLACK, 0.15)
    })
}

/// Every space as a chip with its name; the picked one lifted the way the
/// rail lifts the page you are on; a plus for a new space.
fn picker(ui: &mut Ui, parent: NodeId, s: &Snap, d: &mut Drawn) {
    let row = ui.tree.add(
        parent,
        Node::row()
            .gap(6.0)
            .padding(Insets::hv(0.0, 2.0))
            .align(Align::Center)
            .role(Role::Group)
            .name("Your spaces"),
    );
    for sp in &s.all {
        let on = sp.id == s.editing;
        let mut chip = Node::row()
            .height(CHIP_H)
            .shrink(1.0)
            .padding(Insets::new(5.0, 0.0, 10.0, 0.0))
            .gap(7.0)
            .radius(CHIP_RADIUS)
            .align(Align::Center)
            .button(&sp.title)
            .selected(on);
        chip = if on {
            chip.fill(Token::Ground)
                .stroke(Token::Hairline, 1.0)
                .shadow(Shadow {
                    alpha: 0.08,
                    radius: 3.0,
                    y: 1.0,
                })
        } else {
            chip.fill(Swatch::Fade(Token::Wash, WASH))
                .hover_fill(Token::Hover)
        };
        let chip = ui.tree.add(row, chip);
        let well = ui.tree.add(
            chip,
            Node::stack()
                .size(CHIP_MARK, CHIP_MARK)
                .shrink(0.0)
                .radius(5.0)
                .fill(tint_ground(sp.color, s.dark))
                .align(Align::Center)
                .justify(Justify::Center)
                .role(Role::None),
        );
        mark(ui, well, sp, 13.0, tint_mark(sp.color, s.dark));
        let title = ui.tree.add(
            chip,
            Node::text(&sp.title, 12.0)
                .weight(if on { Weight::Medium } else { Weight::Regular })
                .ink(if on { Token::Ink } else { Token::Muted })
                .trim(Trim::End)
                .shrink(1.0)
                .role(Role::None),
        );
        d.titles.push((sp.id.clone(), title));
        if sp.id == s.current {
            ui.tree.add(
                chip,
                Node::new()
                    .size(4.0, 4.0)
                    .shrink(0.0)
                    .radius(2.0)
                    .fill(Swatch::Fade(Token::Muted, 0.6))
                    .tooltip("The space you are in")
                    .role(Role::None),
            );
        }
        d.hits.push((chip, Act::Pick(sp.id.clone())));
    }
    let plus = ui.tree.add(
        row,
        Node::stack()
            .size(CHIP_H, CHIP_H)
            .shrink(0.0)
            .radius(CHIP_RADIUS)
            .fill(Swatch::Fade(Token::Wash, WASH))
            .hover_fill(Token::Hover)
            .align(Align::Center)
            .justify(Justify::Center)
            .button("New Space")
            .tooltip("New Space"),
    );
    ui.tree.add(
        plus,
        Node::glyph(crate::chrome::glyph::PLUS, 12.0, 14.0)
            .ink(Token::Muted)
            .role(Role::None),
    );
    d.hits.push((plus, Act::New));
}

/// A caption over its content (`section`, 300–313): 12 semibold at the
/// ink's 0.8, the current choice's name trailing in 11 muted.
fn section(ui: &mut Ui, parent: NodeId, title: &str, trailing: Option<&str>) -> NodeId {
    let sec = ui.tree.add(parent, Node::column().gap(8.0));
    let head = ui.tree.add(sec, Node::row().align(Align::Center));
    ui.tree.add(
        head,
        Node::text(title, 12.0)
            .weight(Weight::Semibold)
            .ink(Swatch::Fade(Token::Ink, 0.8))
            .grow(1.0)
            .role(Role::None),
    );
    if let Some(t) = trailing {
        ui.tree
            .add(head, Node::text(t, 11.0).ink(Token::Muted).role(Role::None));
    }
    sec
}

/// One of Settings' hairline cards around a section, 14 in (`group`).
fn group(ui: &mut Ui, parent: NodeId, ink: &Inks) -> NodeId {
    let card = controls::card(ui, parent, ink);
    ui.tree.edit(card, |n| n.style.padding = Insets::all(14.0));
    card
}

/// The picked space: header, name, profile; then icon, colour, order and
/// Delete in cards down the page.
fn page(ui: &mut Ui, parent: NodeId, s: &Snap, sp: &SpaceSnap, ink: &Inks, d: &mut Drawn) {
    let top = ui.tree.add(parent, Node::column().gap(16.0));
    // The header (`header`, 263–281).
    let head = ui.tree.add(top, Node::row().gap(10.0).align(Align::Center));
    let well = ui.tree.add(
        head,
        Node::stack()
            .size(32.0, 32.0)
            .shrink(0.0)
            .radius(9.0)
            .fill(tint_ground(sp.color, s.dark))
            .align(Align::Center)
            .justify(Justify::Center)
            .role(Role::None),
    );
    mark(ui, well, sp, 18.0, tint_mark(sp.color, s.dark));
    let words = ui.tree.add(head, Node::column().gap(1.0).shrink(1.0));
    let title = ui.tree.add(
        words,
        Node::text(&sp.title, 14.0)
            .weight(Weight::Semibold)
            .ink(Token::Ink)
            .trim(Trim::End),
    );
    d.titles.push((sp.id.clone(), title));
    let you = if sp.id == s.current {
        " · the space you are in"
    } else {
        ""
    };
    ui.tree.add(
        words,
        Node::text(
            &format!("{} · {}{you}", symbols::tabs(s.tabs), sp.colour.name()),
            11.5,
        )
        .ink(Token::Muted)
        .trim(Trim::End),
    );

    // The name (`name`, 316–328): 32 high, 14 medium, corner 8.
    let sec = section(ui, top, "Name", None);
    let name_well = ui.tree.add(
        sec,
        Node::row()
            .height(32.0)
            .padding(Insets::hv(10.0, 0.0))
            .radius(8.0)
            .fill(Token::Wash)
            .align(Align::Center),
    );
    let name = ui.tree.add(
        name_well,
        Node::field("Name", 14.0)
            .weight(Weight::Medium)
            .fill_height()
            .grow(1.0)
            .name("Name"),
    );
    ui.set_field_text(name, &sp.name, false);
    d.name = Some(name);

    // The profile (`profile`, 678–716): a menu of Shared, the profiles in
    // use and New Profile… (no wider than 200), the new one's name when
    // asked, and what a profile is in a line.
    let sec = section(ui, top, "Profile", None);
    let label = if s.naming {
        "New Profile\u{2026}"
    } else {
        sp.profile.as_deref().unwrap_or("Shared")
    };
    let holder = ui.tree.add(sec, Node::row().max_width(200.0));
    let pop = controls::popup(ui, holder, "Profile", label, true);
    d.hits.push((pop, Act::Profile));
    if s.naming {
        let well = ui.tree.add(
            sec,
            Node::row()
                .height(28.0)
                .padding(Insets::hv(9.0, 0.0))
                .radius(8.0)
                .fill(Token::Wash)
                .align(Align::Center),
        );
        let f = ui.tree.add(
            well,
            Node::field("Profile name", 12.5)
                .fill_height()
                .grow(1.0)
                .name("Profile name"),
        );
        d.profile = Some(f);
    }
    ui.tree.add(
        sec,
        Node::text(
            "Its own sign-ins and cookies, shared with spaces of the same profile. Pages opened from now on use it.",
            11.0,
        )
        .trim(Trim::Wrap)
        .ink(Token::Muted),
    );

    let cards = ui.tree.add(parent, Node::column().gap(12.0));
    icons(ui, cards, s, sp, ink, d);
    colours(ui, cards, sp, ink, d);
    order(ui, cards, s, sp, ink, d);
}

/// None, the symbols and an emoji field (`icons`, 334–381).
fn icons(ui: &mut Ui, parent: NodeId, s: &Snap, sp: &SpaceSnap, ink: &Inks, d: &mut Drawn) {
    let now = symbols::symbol(sp.icon.as_deref());
    let emoji = sp
        .icon
        .as_deref()
        .filter(|i| !i.is_empty() && !i.starts_with("sf:"));
    // `controls`, 286: the symbol's name, else Emoji, else what the name
    // gives (an emoji it starts with) or None.
    let trailing = match (now, emoji) {
        (Some(sym), _) => symbols::symbol_name(sym),
        (None, Some(_)) => "Emoji".into(),
        (None, None) if symbols::emoji(None, &sp.name).is_some() => "From the name".into(),
        (None, None) => "None".into(),
    };
    let card = group(ui, parent, ink);
    let sec = section(ui, card, "Icon", Some(&trailing));
    let mark_ink = tint_mark(sp.color, s.dark);
    // As many to a line as the card has room for (`GridItem(.adaptive)`).
    let per_line = per_line(s.room, TILE_W, TILE_GAP);
    let grid = ui.tree.add(
        sec,
        Node::column().gap(TILE_GAP).role(Role::Group).name("Icon"),
    );
    let mut lines: Vec<NodeId> = Vec::new();
    let mut placed = 0usize;
    let mut tile = |ui: &mut Ui, selected: bool, name: &str, help: &str| -> NodeId {
        if placed.is_multiple_of(per_line) {
            lines.push(ui.tree.add(grid, Node::row().gap(TILE_GAP)));
        }
        placed += 1;
        let line = *lines.last().unwrap_or(&grid);
        let mut t = Node::stack()
            .size(TILE_W, TILE_H)
            .shrink(0.0)
            .radius(7.0)
            .align(Align::Center)
            .justify(Justify::Center)
            .button(name)
            .tooltip(help)
            .selected(selected);
        t = if selected {
            t.fill(tint_ground(sp.color, s.dark))
                .stroke(Swatch::Fixed(Color { a: 0.8, ..sp.color }), 1.5)
        } else {
            t.fill(Swatch::Fade(Token::Wash, WASH))
                .hover_fill(Token::Hover)
        };
        ui.tree.add(line, t)
    };
    let none = tile(
        ui,
        sp.icon.is_none(),
        "No icon",
        "No icon \u{2014} a dot, or the emoji the name starts with",
    );
    ui.tree.add(
        none,
        Node::new()
            .size(7.0, 7.0)
            .radius(3.5)
            .fill(mark_ink)
            .role(Role::None),
    );
    d.hits.push((none, Act::Icon(None)));
    for (sym, label, g) in symbols::SYMBOLS {
        let t = tile(ui, now == Some(sym), label, label);
        ui.tree.add(
            t,
            Node::glyph(g, 13.0, 16.0)
                .weight(Weight::Semibold)
                .ink(mark_ink)
                .role(Role::None),
        );
        d.hits.push((t, Act::Icon(Some(format!("sf:{sym}")))));
    }
    // The emoji line (`icons`, 352–379).
    let line = ui.tree.add(sec, Node::row().gap(8.0).align(Align::Center));
    if let Some(e) = emoji {
        ui.tree.add(
            line,
            Node::text(e, 17.0)
                .text_align(TextAlign::Center)
                .width(26.0)
                .role(Role::None),
        );
    }
    let f = controls::field(
        ui,
        line,
        "Emoji",
        "",
        "Type or paste an emoji",
        false,
        None,
        true,
    );
    if let Some(well) = ui.tree.parent(f) {
        ui.tree
            .edit(well, |n| n.style.height = copper_win_ui::Dim::Px(28.0));
    }
    d.emoji = Some(f);
}

/// The named colours as wells (`colours`): the chosen one ringed.
fn colours(ui: &mut Ui, parent: NodeId, sp: &SpaceSnap, ink: &Inks, d: &mut Drawn) {
    // Ten wells of 30 and 4 between: 336, within Settings' narrowest card.
    let card = group(ui, parent, ink);
    let sec = section(ui, card, "Colours", Some(sp.colour.name()));
    let row = ui
        .tree
        .add(sec, Node::row().gap(4.0).role(Role::Group).name("Colour"));
    for c in SpaceColour::ALL {
        let on = c == sp.colour;
        let mut target = Node::stack()
            .size(WELL_TARGET, WELL_TARGET)
            .shrink(0.0)
            .radius(WELL_TARGET / 2.0)
            .align(Align::Center)
            .justify(Justify::Center)
            .hover_fill(Token::Hover)
            .button(c.name())
            .tooltip(c.name())
            .selected(on);
        if on {
            target = target.stroke(Token::Ink, 1.5);
        }
        let t = ui.tree.add(row, target);
        ui.tree.add(
            t,
            Node::new()
                .size(WELL, WELL)
                .radius(WELL / 2.0)
                .fill(Swatch::Fixed(colour_of(c)))
                .stroke(Token::Hairline, 1.0)
                .role(Role::None),
        );
        d.hits.push((t, Act::Colour(c)));
    }
}

/// Its place in the row, and Delete (`footer`).
fn order(ui: &mut Ui, parent: NodeId, s: &Snap, sp: &SpaceSnap, ink: &Inks, d: &mut Drawn) {
    let index = s.all.iter().position(|x| x.id == sp.id).unwrap_or(0);
    let count = s.all.len();
    let card = group(ui, parent, ink);
    let row = ui.tree.add(card, Node::row().gap(8.0).align(Align::Center));
    ui.tree.add(
        row,
        Node::text("Order", metric::TITLE)
            .ink(Token::Ink)
            .grow(1.0)
            .role(Role::None),
    );
    let left = controls::pill(ui, row, "Move Left", false, index > 0);
    d.hits.push((left, Act::Left));
    let right = controls::pill(ui, row, "Move Right", false, index + 1 < count);
    d.hits.push((right, Act::Right));
    // The foot (`footer`, 720–730): its own card, a line saying what
    // Delete does, and the pill in red.
    if count > 1 {
        let card = controls::card(ui, parent, ink);
        let l = controls::line(
            ui,
            card,
            "Delete this space",
            Some("Asks first, and can move its tabs to the space beside it"),
            Swatch::Token(Token::Muted),
        );
        let del = controls::pill(ui, l.control, "Delete Space\u{2026}", false, true);
        let red = Swatch::Fixed(look::ERROR.get(s.dark));
        if let Some(&words) = ui.tree.children(del).first() {
            ui.tree.edit(words, |n| n.look.ink = red);
        }
        d.hits.push((del, Act::Delete));
    }
}
