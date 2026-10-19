//! The toolkit's gallery: Copper's column of tabs with its sliding pill, the
//! address field (IME, selection, undo), the Ctrl+K palette, a context menu,
//! tooltips, a long scrolling list, a WebSlot standing in for the page, and
//! the light/dark switch.
//!
//! It logs the time to first frame and frame timings to stdout and to
//! `%TEMP%\copper-gallery.log`.
//!
//! Keys: Ctrl+K palette · Ctrl+L address · Ctrl+T new tab · Ctrl+W close
//! tab · Ctrl+Shift+L light/dark/system · Shift+F10 or the menu key for the
//! context menu · Tab / Shift+Tab focus.

fn main() {
    #[cfg(windows)]
    app::main();
    #[cfg(not(windows))]
    eprintln!("the gallery runs on Windows");
}

#[cfg(windows)]
mod app {
    use std::cell::RefCell;
    use std::fs::File;
    use std::io::Write as _;
    use std::rc::Rc;

    use copper_win_ui::input::Key;
    use copper_win_ui::menu::Menu;
    use copper_win_ui::tokens::{card, metrics, motion, side};
    use copper_win_ui::win::{FocusReason, Options, SlotHost, SlotMouse, SlotPointer, Ui, Window};
    use copper_win_ui::{
        Abs, Align, Appearance, Event, Insets, Justify, Node, NodeId, Prop, Role, Swatch, Token,
        Trim, Weight,
    };
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::DirectComposition::IDCompositionVisual;
    use windows::Win32::UI::WindowsAndMessaging::{HCURSOR, WM_LBUTTONDOWN};
    use windows::core::IUnknown;

    // Segoe Fluent Icons.
    const BACK: char = '\u{E72B}';
    const FORWARD: char = '\u{E72A}';
    const RELOAD: char = '\u{E72C}';
    const SEARCH: char = '\u{E721}';
    const PLUS: char = '\u{E710}';
    const SUN: char = '\u{E706}';
    const MOON: char = '\u{E708}';
    const AUTO: char = '\u{E7F4}';

    /// Context menus.
    const MENU_TAB: u32 = 1;
    const MENU_COLUMN: u32 = 2;
    /// Their items.
    const ITEM_RELOAD: u32 = 1;
    const ITEM_DUPLICATE: u32 = 2;
    const ITEM_CLOSE: u32 = 3;
    const ITEM_NEW: u32 = 4;
    const ITEM_LIGHT: u32 = 5;
    const ITEM_DARK: u32 = 6;
    const ITEM_SYSTEM: u32 = 7;

    /// Tabs the list opens with: enough to scroll.
    const SITES: &[&str] = &[
        "Office Commun",
        "Mac Studio — Apple",
        "driceroland · GitHub",
        "Are.na",
        "The Browser Company",
        "Hacker News",
        "Rust Programming Language",
        "Windows App SDK documentation",
        "DirectComposition — Win32 apps",
        "Segoe UI Variable — Typography",
        "Fluent 2 Design System",
        "WebView2 — Microsoft Edge Developer",
        "crates.io: Rust Package Registry",
        "docs.rs",
        "Mozilla Developer Network",
        "Can I use…",
        "Figma",
        "Linear",
        "Read the Docs",
        "Wikipedia, the free encyclopedia",
        "OpenStreetMap",
        "Bandcamp",
        "Letterboxd",
        "The Verge",
        "Ars Technica",
        "Arts & Letters Daily",
        "Kottke.org",
        "Craig Mod",
        "Robin Sloan",
        "Low-tech Magazine",
        "Works in Progress",
        "Dense Discovery",
    ];

    /// Commands the palette lists before tabs.
    const COMMANDS: &[(&str, &str)] = &[
        ("New Tab", "Ctrl+T"),
        ("Focus Address", "Ctrl+L"),
        ("Close Tab", "Ctrl+W"),
        ("Light Appearance", ""),
        ("Dark Appearance", ""),
        ("Follow Windows", "Ctrl+Shift+L"),
    ];

    /// Stdout and `%TEMP%\copper-gallery.log`.
    struct Log {
        file: Option<File>,
    }

    impl Log {
        fn open() -> Log {
            let path = std::env::temp_dir().join("copper-gallery.log");
            Log {
                file: File::create(path).ok(),
            }
        }

        fn line(&mut self, s: &str) {
            println!("{s}");
            if let Some(f) = &mut self.file {
                let _ = writeln!(f, "{s}");
            }
        }
    }

    /// The page's stand-in: logs what a real page would be told.
    struct Placeholder {
        log: Rc<RefCell<Log>>,
    }

    impl SlotHost for Placeholder {
        fn attach(&self, _hwnd: HWND, _visual: &IDCompositionVisual) {
            self.log.borrow_mut().line("slot: attached");
        }
        fn detach(&self) {
            self.log.borrow_mut().line("slot: detached");
        }
        fn set_bounds(&self, r: RECT) {
            self.log.borrow_mut().line(&format!(
                "slot: bounds {}x{} at {},{}",
                r.right - r.left,
                r.bottom - r.top,
                r.left,
                r.top
            ));
        }
        fn set_visible(&self, visible: bool) {
            self.log
                .borrow_mut()
                .line(&format!("slot: visible {visible}"));
        }
        fn set_scale(&self, scale: f64) {
            self.log.borrow_mut().line(&format!("slot: scale {scale}"));
        }
        fn mouse(&self, m: SlotMouse) {
            if m.kind == WM_LBUTTONDOWN {
                self.log
                    .borrow_mut()
                    .line(&format!("slot: click at {},{}", m.point.x, m.point.y));
            }
        }
        fn pointer(&self, p: SlotPointer) {
            self.log
                .borrow_mut()
                .line(&format!("slot: pointer {:#x} id {}", p.kind, p.pointer_id));
        }
        fn focus(&self, reason: FocusReason) {
            self.log
                .borrow_mut()
                .line(&format!("slot: focus {reason:?}"));
        }
        fn cursor(&self) -> Option<HCURSOR> {
            None
        }
        fn automation(&self) -> Option<IUnknown> {
            None
        }
    }

    /// One tab in the column.
    struct Tab {
        title: String,
        row: NodeId,
        label: NodeId,
    }

    /// One palette result.
    enum Pick {
        Command(usize),
        Tab(usize),
    }

    /// The gallery's nodes and state.
    struct Gallery {
        log: Rc<RefCell<Log>>,
        side: NodeId,
        address: NodeId,
        scroll: NodeId,
        list: NodeId,
        pill: NodeId,
        new_tab: NodeId,
        back: NodeId,
        forward: NodeId,
        reload: NodeId,
        theme: NodeId,
        theme_glyph: NodeId,
        slot: NodeId,
        heading: NodeId,
        button: NodeId,
        palette: NodeId,
        card: NodeId,
        query: NodeId,
        results: NodeId,
        tabs: Vec<Tab>,
        live: usize,
        picks: Vec<(Pick, NodeId)>,
        picked: usize,
        /// The tab the context menu is about.
        menu_tab: Option<usize>,
        /// Focus to return to when the palette closes.
        before_palette: Option<NodeId>,
        frames: u32,
    }

    fn add(ui: &mut Ui, parent: NodeId, n: Node) -> NodeId {
        ui.tree.add(parent, n)
    }

    /// A square glyph button in the column, with a tooltip.
    fn door(
        ui: &mut Ui,
        parent: NodeId,
        glyph: char,
        size: f32,
        glyph_size: f32,
        name: &str,
        tip: &str,
    ) -> (NodeId, NodeId) {
        let d = add(
            ui,
            parent,
            Node::stack()
                .size(size, size)
                .radius(side::door_radius(size))
                .hover_fill(Token::ColumnHover)
                .align(Align::Center)
                .justify(Justify::Center)
                .button(name)
                .tooltip(tip),
        );
        let g = add(
            ui,
            d,
            Node::glyph(glyph, glyph_size, size).ink(Token::ColumnMuted),
        );
        (d, g)
    }

    /// A letter mark: a rounded square with the site's initial.
    fn mark(ui: &mut Ui, parent: NodeId, title: &str, size: f32) -> NodeId {
        let letter: String = title
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let m = add(
            ui,
            parent,
            Node::stack()
                .size(size, size)
                .shrink(0.0)
                .radius(side::mark_radius(size))
                .fill(Token::Square)
                .align(Align::Center)
                .justify(Justify::Center),
        );
        add(
            ui,
            m,
            Node::text(&letter, side::mark_letter(size))
                .weight(Weight::Semibold)
                .ink(Swatch::Fade(Token::ColumnInk, side::MARK_LETTER_INK))
                .text_align(copper_win_ui::TextAlign::Center)
                .role(Role::None),
        );
        m
    }

    /// The pill's place for row `i` (list content coordinates).
    fn row_y(i: usize) -> f32 {
        i as f32 * (side::ROW + side::GAP)
    }

    /// Build the window's content.
    fn build(ui: &mut Ui, log: Rc<RefCell<Log>>) -> Gallery {
        let content = ui.content();
        // One ground under the column and the caption band, so they read as
        // one surface (and let Mica through together).
        let shell = add(
            ui,
            content,
            Node::row().absolute(Abs::FILL).fill(Token::ColumnGround),
        );

        // — the column —
        let side_n = add(
            ui,
            shell,
            Node::column()
                .width(metrics::SIDE)
                .shrink(0.0)
                .layer()
                .role(Role::Pane)
                .name("Tabs"),
        );
        let head = add(
            ui,
            side_n,
            Node::row()
                .height(side::HEAD)
                .padding(Insets::hv(6.0, 0.0))
                .gap(side::HELM_GAP)
                .align(Align::Center)
                .drag(),
        );
        let door_size = side::HELM_DOOR;
        let (back, _) = door(
            ui,
            head,
            BACK,
            door_size,
            side::HEAD_GLYPH,
            "Back",
            "Back (Alt+Left)",
        );
        let (forward, _) = door(
            ui,
            head,
            FORWARD,
            door_size,
            side::HEAD_GLYPH,
            "Forward",
            "Forward (Alt+Right)",
        );
        let (reload, _) = door(
            ui,
            head,
            RELOAD,
            door_size,
            side::HEAD_GLYPH,
            "Reload",
            "Reload (Ctrl+R)",
        );

        let bar = add(
            ui,
            side_n,
            Node::row().padding(Insets::new(
                side::INSET,
                0.0,
                side::INSET,
                side::ADDRESS_BELOW,
            )),
        );
        let address_box = add(
            ui,
            bar,
            Node::row()
                .height(side::ADDRESS_HEIGHT)
                .grow(1.0)
                .radius(side::ADDRESS_RADIUS)
                .fill(Token::Square)
                .padding(Insets::new(
                    side::ADDRESS_LEADING,
                    0.0,
                    side::ADDRESS_TRAILING,
                    0.0,
                ))
                .align(Align::Center),
        );
        let address = add(
            ui,
            address_box,
            Node::field("Search or enter address", side::ADDRESS_TEXT)
                .ink(Swatch::Fade(Token::ColumnInk, side::ADDRESS_INK))
                .fill_height()
                .name("Address"),
        );

        // Favourites: three to a row at this width.
        let pins = add(
            ui,
            side_n,
            Node::row()
                .padding(Insets::new(side::INSET, 0.0, side::INSET, side::PINS_BELOW))
                .gap(side::PIN_GAP),
        );
        let pw = side::pin_width(metrics::SIDE);
        let ph = side::pin_height(pw);
        for site in ["X", "Notion", "Figma"] {
            let pin = add(
                ui,
                pins,
                Node::stack()
                    .size(pw, ph)
                    .radius(side::pin_radius(pw, ph))
                    .fill(Token::Square)
                    .hover_fill(Token::ColumnHover)
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .button(site)
                    .tooltip(site),
            );
            mark(ui, pin, site, side::pin_mark(ph));
        }

        // The tabs: a long list in a scroller, the pill sliding under them.
        let scroll = add(
            ui,
            side_n,
            Node::scroll().grow(1.0).name("Open tabs").role(Role::List),
        );
        let list = add(
            ui,
            scroll,
            Node::column()
                .padding(Insets::new(
                    side::INSET,
                    side::SCROLL_TOP,
                    side::INSET,
                    side::SCROLL_BOTTOM,
                ))
                .gap(side::GAP),
        );
        let pill = add(
            ui,
            list,
            Node::new()
                .absolute(Abs {
                    left: Some(0.0),
                    top: Some(0.0),
                    right: Some(0.0),
                    bottom: None,
                })
                .height(side::ROW)
                .fill(Token::Pill)
                .radius(side::ROW_RADIUS)
                // A one-point ring and a soft lift (`Side.swift` 711–716).
                .stroke(Token::Rim, 1.0)
                .shadow(copper_win_ui::tokens::Shadow::new(
                    1.0,
                    side::PILL_SHADOW_RADIUS,
                    side::PILL_SHADOW_Y,
                ))
                .shadow_ink(Token::Lift),
        );
        let new_tab = add(
            ui,
            list,
            Node::row()
                .height(side::ROW)
                .padding(Insets::new(side::ROW_INSET, 0.0, side::ROW_TRAILING, 0.0))
                .gap(side::MARK_GAP)
                .align(Align::Center)
                .radius(side::ROW_RADIUS)
                .hover_fill(Token::ColumnHover)
                .button("New tab")
                .opacity(side::QUIET_REST),
        );
        add(
            ui,
            new_tab,
            Node::glyph(PLUS, side::QUIET_GLYPH, side::MARK).ink(Token::ColumnMuted),
        );
        add(
            ui,
            new_tab,
            Node::text("New tab", side::TITLE_SIZE)
                .ink(Token::ColumnMuted)
                .grow(1.0),
        );

        // The foot: spaces and the appearance switch.
        let foot = add(
            ui,
            side_n,
            Node::row()
                .padding(Insets::new(
                    side::FOOT_PAD_H,
                    side::FOOT_PAD_TOP,
                    side::FOOT_PAD_H,
                    side::FOOT_PAD_BOTTOM,
                ))
                .gap(side::CHIP_GAP)
                .align(Align::Center),
        );
        for (tag, name) in [(1u64, "Paper"), (2, "Copper")] {
            let chip = add(
                ui,
                foot,
                Node::stack()
                    .size(side::CHIP, side::CHIP)
                    .radius(side::CHIP_RADIUS)
                    .hover_fill(Token::ColumnHover)
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .button(name)
                    .tooltip(&format!("{name} space"))
                    .tag(tag),
            );
            add(
                ui,
                chip,
                Node::new().size(12.0, 12.0).radius(6.0).fill(if tag == 1 {
                    Token::ColumnMuted
                } else {
                    Token::Dot
                }),
            );
        }
        add(ui, foot, Node::new().grow(1.0));
        let (theme, theme_glyph) = door(
            ui,
            foot,
            AUTO,
            side::FOOT_DOOR,
            side::FOOT_GLYPH,
            "Appearance",
            "Appearance: System (Ctrl+Shift+L)",
        );

        // — the page —
        // Windows puts the caption buttons top-right, over where the Mac's
        // page starts; the page leaves them the band Copper leaves for its
        // lights when there is no strip (`metrics::BARE`), and the band
        // drags the window like a title bar.
        let page = add(ui, shell, Node::column().grow(1.0));
        add(
            ui,
            page,
            Node::new().height(metrics::BARE).shrink(0.0).drag(),
        );
        // The column's edge: the hairline at 0.6, one wide, beside the page.
        let well = add(
            ui,
            page,
            Node::row()
                .grow(1.0)
                .padding(Insets::new(1.0, 0.0, 0.0, 0.0))
                .fill(Swatch::Fade(Token::ColumnHairline, side::EDGE_ALPHA)),
        );
        let slot = add(
            ui,
            well,
            Node::slot()
                .grow(1.0)
                .fill(Token::Ground)
                .align(Align::Center)
                .justify(Justify::Center)
                .name("Page"),
        );
        let copy = add(
            ui,
            slot,
            Node::column().width(420.0).gap(14.0).align(Align::Start),
        );
        let heading = add(
            ui,
            copy,
            Node::text(SITES[0], 17.0)
                .weight(Weight::Medium)
                .ink(Token::Ink),
        );
        add(
            ui,
            copy,
            Node::text(
                "A WebSlot: the page's composition visual goes here, under the toolkit's own layers. \
                 This colour is the slot's placeholder; a page covers it.",
                15.0,
            )
            .trim(Trim::Wrap)
            .ink(Token::Muted),
        );
        let button = add(
            ui,
            copy,
            Node::row()
                .height(40.0)
                .padding(Insets::hv(22.0, 0.0))
                .radius(20.0)
                .fill(Token::Ink)
                .align(Align::Center)
                .button("Start a project"),
        );
        add(
            ui,
            button,
            Node::text("Start a project", 14.0)
                .ink(Token::Ground)
                .role(Role::None),
        );
        ui.set_slot(slot, Box::new(Placeholder { log: log.clone() }));

        // — the palette —
        let palette = add(
            ui,
            content,
            Node::column()
                .absolute(Abs::FILL)
                .align(Align::Center)
                .padding(Insets::new(24.0, ui.size().h * card::DROP, 24.0, 24.0))
                .fill(Swatch::Fade(Token::Black, card::SCRIM_OVER))
                .layer()
                .hit()
                .modal()
                .hidden(true),
        );
        let card_n = add(
            ui,
            palette,
            Node::column()
                .width(card::WIDTH)
                .shrink(1.0)
                .fill(Token::Ground)
                .radius(card::RADIUS)
                .shadow(card::EDGE_SHADOW)
                .shadow(card::LIFT_SHADOW)
                .layer()
                .hit()
                .role(Role::Group)
                .name("Command palette"),
        );
        let field_row = add(
            ui,
            card_n,
            Node::row()
                .height(card::FIELD_HEIGHT)
                .padding(Insets::hv(card::FIELD_PAD, 0.0))
                .gap(card::FIELD_SPACING)
                .align(Align::Center),
        );
        add(
            ui,
            field_row,
            Node::glyph(SEARCH, card::GLASS, card::GLASS + 4.0).ink(Token::Muted),
        );
        let query = add(
            ui,
            field_row,
            Node::field("Search tabs and commands", card::FIELD_TEXT)
                .fill_height()
                .name("Search tabs and commands"),
        );
        add(
            ui,
            card_n,
            Node::new()
                .height(1.0)
                .fill(Swatch::Fade(Token::Hairline, card::RULE_ALPHA)),
        );
        let results = add(
            ui,
            card_n,
            Node::column()
                .padding(Insets::all(card::LIST_PAD))
                .gap(card::ROW_SPACING)
                .role(Role::List)
                .name("Results"),
        );

        let mut g = Gallery {
            log,
            side: side_n,
            address,
            scroll,
            list,
            pill,
            new_tab,
            back,
            forward,
            reload,
            theme,
            theme_glyph,
            slot,
            heading,
            button,
            palette,
            card: card_n,
            query,
            results,
            tabs: Vec::new(),
            live: 0,
            picks: Vec::new(),
            picked: 0,
            menu_tab: None,
            before_palette: None,
            frames: 0,
        };
        for site in SITES {
            g.push_tab(ui, site);
        }
        g.select(ui, 0, false);
        g
    }

    impl Gallery {
        /// Add a tab at the end of the list (above "New tab").
        fn push_tab(&mut self, ui: &mut Ui, title: &str) -> usize {
            // The pill is the list's first child; the tabs follow it.
            let index = 1 + self.tabs.len();
            let row = ui.tree.insert(
                self.list,
                index,
                Node::row()
                    .height(side::ROW)
                    .shrink(0.0)
                    .padding(Insets::new(side::ROW_INSET, 0.0, side::ROW_TRAILING, 0.0))
                    .gap(side::MARK_GAP)
                    .align(Align::Center)
                    .radius(side::ROW_RADIUS)
                    .hover_fill(Token::ColumnHover)
                    .button(title)
                    .role(Role::ListItem)
                    .tooltip(title),
            );
            mark(ui, row, title, side::MARK);
            let label = add(
                ui,
                row,
                Node::text(title, side::TITLE_SIZE)
                    .ink(Token::ColumnInk)
                    .grow(1.0)
                    .role(Role::None),
            );
            self.tabs.push(Tab {
                title: title.to_owned(),
                row,
                label,
            });
            self.tabs.len() - 1
        }

        /// Make tab `i` the live one: the pill glides to it.
        fn select(&mut self, ui: &mut Ui, i: usize, animated: bool) {
            let Some(tab) = self.tabs.get(i) else {
                return;
            };
            if let Some(old) = self.tabs.get(self.live) {
                ui.tree.set_selected(old.row, false);
                ui.tree.set_weight(old.label, Weight::Regular);
            }
            ui.tree.set_selected(tab.row, true);
            ui.tree.set_weight(tab.label, Weight::Semibold);
            ui.tree.set_text(self.heading, &tab.title);
            let host = host_of(&tab.title);
            ui.set_field_text(self.address, &host, false);
            self.live = i;
            let y = row_y(i);
            if animated {
                ui.tree.animate(self.pill, Prop::Y, y, motion::GLIDE);
            } else {
                ui.tree.set_prop(self.pill, Prop::Y, y);
            }
            ui.tree.set_hidden(self.pill, false);
            // Keep the live row in sight.
            let top = side::SCROLL_TOP + y;
            ui.tree
                .scroll(self.scroll, |s| s.reveal(top, top + side::ROW, animated));
        }

        /// Open a new tab and make it live.
        fn new_tab(&mut self, ui: &mut Ui) {
            let n = self.tabs.len() + 1;
            let i = self.push_tab(ui, &format!("New Tab {n}"));
            self.select(ui, i, true);
            ui.focus(Some(self.address));
        }

        /// Close tab `i`; the next one (or the one before) becomes live.
        fn close_tab(&mut self, ui: &mut Ui, i: usize) {
            if i >= self.tabs.len() {
                return;
            }
            let tab = self.tabs.remove(i);
            ui.tree.remove(tab.row);
            if self.tabs.is_empty() {
                self.live = 0;
                ui.tree.set_hidden(self.pill, true);
                ui.tree.set_text(self.heading, "No tabs");
                ui.set_field_text(self.address, "", false);
                return;
            }
            let next = if i < self.live || (i == self.live && i == self.tabs.len()) {
                self.live.saturating_sub(1)
            } else {
                self.live.min(self.tabs.len() - 1)
            };
            // The row that was live is gone: nothing to deselect.
            self.live = usize::MAX;
            self.select(ui, next, true);
        }

        /// The tab whose row is `id` or holds it.
        fn tab_at(&self, ui: &Ui, id: NodeId) -> Option<usize> {
            self.tabs
                .iter()
                .position(|t| t.row == id || ui.tree.is_within(id, t.row))
        }

        fn cycle_appearance(&mut self, ui: &mut Ui) {
            let next = match ui.appearance() {
                Appearance::System => Appearance::Light,
                Appearance::Light => Appearance::Dark,
                Appearance::Dark => Appearance::System,
            };
            self.set_appearance(ui, next);
        }

        fn set_appearance(&mut self, ui: &mut Ui, a: Appearance) {
            ui.set_appearance(a);
            let glyph = match a {
                Appearance::Light => SUN,
                Appearance::Dark => MOON,
                Appearance::System => AUTO,
            };
            ui.tree.set_text(self.theme_glyph, &glyph.to_string());
            ui.tree.edit(self.theme, |n| {
                n.tooltip = Some(format!("Appearance: {} (Ctrl+Shift+L)", a.title()));
            });
            self.log.borrow_mut().line(&format!(
                "appearance: {} (dark: {})",
                a.title(),
                ui.is_dark()
            ));
        }

        // — the palette —

        fn palette_open(&self, ui: &Ui) -> bool {
            ui.tree.get(self.palette).is_some_and(|n| !n.style.hidden)
        }

        fn open_palette(&mut self, ui: &mut Ui) {
            if self.palette_open(ui) {
                ui.focus(Some(self.query));
                return;
            }
            self.before_palette = ui.tree.focused();
            ui.tree.set_hidden(self.palette, false);
            ui.tree.set_prop(self.palette, Prop::Opacity, 0.0);
            ui.tree
                .animate(self.palette, Prop::Opacity, 1.0, motion::QUICK);
            ui.tree.set_prop(self.card, Prop::Scale, card::ENTER_SCALE);
            ui.tree.animate(self.card, Prop::Scale, 1.0, motion::SETTLE);
            ui.set_field_text(self.query, "", false);
            self.filter(ui);
            ui.focus(Some(self.query));
        }

        fn close_palette(&mut self, ui: &mut Ui) {
            if !self.palette_open(ui) {
                return;
            }
            ui.tree.set_hidden(self.palette, true);
            let back = self
                .before_palette
                .take()
                .filter(|&id| ui.tree.contains(id));
            ui.focus(back);
        }

        /// Rebuild the results for the query.
        fn filter(&mut self, ui: &mut Ui) {
            let q = ui.field_text(self.query).unwrap_or_default().to_lowercase();
            ui.tree.clear_children(self.results);
            self.picks.clear();
            let matches = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);

            let commands: Vec<usize> = (0..COMMANDS.len())
                .filter(|&i| matches(COMMANDS[i].0))
                .collect();
            if !commands.is_empty() {
                self.heading_row(ui, "Commands");
                for i in commands.into_iter().take(card::LIMIT) {
                    let (title, hint) = COMMANDS[i];
                    let row = self.result_row(ui, title, "", hint);
                    self.picks.push((Pick::Command(i), row));
                }
            }
            let room = card::LIMIT.saturating_sub(self.picks.len()).max(3);
            let tabs: Vec<usize> = (0..self.tabs.len())
                .filter(|&i| matches(&self.tabs[i].title))
                .take(room)
                .collect();
            if !tabs.is_empty() {
                self.heading_row(ui, "Tabs");
                for i in tabs {
                    let title = self.tabs[i].title.clone();
                    let row = self.result_row(ui, &title, &host_of(&title), "Switch to Tab");
                    self.picks.push((Pick::Tab(i), row));
                }
            }
            if self.picks.is_empty() {
                add(
                    ui,
                    self.results,
                    Node::text("No matches", card::ROW_TITLE)
                        .ink(Token::Muted)
                        .height(card::ROW_HEIGHT)
                        .padding(Insets::hv(card::ROW_PAD, 0.0)),
                );
            }
            self.picked = 0;
            self.show_picked(ui);
        }

        fn heading_row(&self, ui: &mut Ui, title: &str) {
            add(
                ui,
                self.results,
                Node::text(title, card::HEADING)
                    .weight(Weight::Semibold)
                    .ink(Token::Muted)
                    .height(card::HEADING_HEIGHT)
                    .padding(Insets::hv(card::ROW_PAD, 0.0)),
            );
        }

        fn result_row(&self, ui: &mut Ui, title: &str, detail: &str, hint: &str) -> NodeId {
            let row = add(
                ui,
                self.results,
                Node::row()
                    .height(card::ROW_HEIGHT)
                    .padding(Insets::hv(card::ROW_PAD, 0.0))
                    .gap(card::ROW_GAP)
                    .align(Align::Center)
                    .radius(card::ROW_RADIUS)
                    .hover_fill(Token::Hover)
                    .selected_fill(Swatch::Fade(Token::Accent, card::PICKED_ALPHA))
                    .button(title)
                    .role(Role::ListItem),
            );
            mark(ui, row, title, card::ROW_MARK);
            add(
                ui,
                row,
                Node::text(title, card::ROW_TITLE)
                    .weight(Weight::Medium)
                    .ink(Token::Ink)
                    .shrink(1.0)
                    .role(Role::None),
            );
            if !detail.is_empty() {
                add(
                    ui,
                    row,
                    Node::text(detail, card::ROW_DETAIL)
                        .ink(Token::Muted)
                        .trim(Trim::Middle)
                        .shrink(2.0)
                        .role(Role::None),
                );
            }
            add(ui, row, Node::new().grow(1.0));
            if !hint.is_empty() {
                add(
                    ui,
                    row,
                    Node::text(hint, card::ROW_HINT)
                        .weight(Weight::Medium)
                        .ink(Token::Muted)
                        .shrink(0.0)
                        .role(Role::None),
                );
            }
            row
        }

        fn show_picked(&self, ui: &mut Ui) {
            for (i, (_, row)) in self.picks.iter().enumerate() {
                ui.tree.set_selected(*row, i == self.picked);
            }
        }

        fn move_pick(&mut self, ui: &mut Ui, forward: bool) {
            if self.picks.is_empty() {
                return;
            }
            let n = self.picks.len();
            self.picked = if forward {
                (self.picked + 1) % n
            } else {
                (self.picked + n - 1) % n
            };
            self.show_picked(ui);
        }

        fn run_pick(&mut self, ui: &mut Ui, i: usize) {
            let Some((pick, _)) = self.picks.get(i) else {
                return;
            };
            let pick = match pick {
                Pick::Command(c) => Pick::Command(*c),
                Pick::Tab(t) => Pick::Tab(*t),
            };
            self.close_palette(ui);
            match pick {
                Pick::Command(0) => self.new_tab(ui),
                Pick::Command(1) => ui.focus(Some(self.address)),
                Pick::Command(2) => self.close_tab(ui, self.live),
                Pick::Command(3) => self.set_appearance(ui, Appearance::Light),
                Pick::Command(4) => self.set_appearance(ui, Appearance::Dark),
                Pick::Command(_) => self.set_appearance(ui, Appearance::System),
                Pick::Tab(t) => self.select(ui, t, true),
            }
        }

        // — events —

        fn handle(&mut self, ui: &mut Ui, e: Event) {
            match e {
                Event::Click(id) => self.click(ui, id),
                Event::ContextMenu { target, at } => self.context_menu(ui, target, at),
                Event::FieldChanged(id) if id == self.query => self.filter(ui),
                Event::FieldSubmit(id) if id == self.query => self.run_pick(ui, self.picked),
                Event::FieldSubmit(id) if id == self.address => {
                    let text = ui.field_text(id).unwrap_or_default().to_owned();
                    self.log
                        .borrow_mut()
                        .line(&format!("address: go to {text:?}"));
                    if let Some(tab) = self.tabs.get_mut(self.live) {
                        tab.title.clone_from(&text);
                        let (row, label) = (tab.row, tab.label);
                        ui.tree.set_text(label, &text);
                        ui.tree.edit(row, |n| n.a11y.name.clone_from(&text));
                        ui.tree.set_text(self.heading, &text);
                    }
                    ui.focus_page(self.slot, FocusReason::Programmatic);
                }
                Event::FieldCancel(id) if id == self.query => self.close_palette(ui),
                Event::FieldCancel(id) if id == self.address => {
                    // Escape puts the live tab's address back, then leaves.
                    let host = self
                        .tabs
                        .get(self.live)
                        .map(|t| host_of(&t.title))
                        .unwrap_or_default();
                    if ui.field_text(id) == Some(host.as_str()) {
                        ui.focus_page(self.slot, FocusReason::Programmatic);
                    } else {
                        ui.set_field_text(id, &host, true);
                    }
                }
                Event::Key { key, mods, .. } => self.key(ui, key, mods),
                Event::MenuChosen { menu, item } => self.menu_chosen(ui, menu, item),
                Event::MenuDismissed { .. } => self.menu_tab = None,
                Event::Resized(size) => {
                    ui.tree
                        .edit(self.palette, |n| n.style.padding.top = size.h * card::DROP);
                    self.log
                        .borrow_mut()
                        .line(&format!("resized: {:.0}x{:.0} DIPs", size.w, size.h));
                }
                Event::ScaleChanged(s) => self.log.borrow_mut().line(&format!("scale: {s}")),
                Event::Appearance { dark } => self.log.borrow_mut().line(&format!("dark: {dark}")),
                Event::FirstFrame {
                    since_process_ms,
                    since_create_ms,
                } => {
                    self.log.borrow_mut().line(&format!(
                        "TTFF {since_process_ms:.1} ms from process start ({since_create_ms:.1} ms from Window::new), \
                         backdrop: {}, scale: {}",
                        ui.has_backdrop(),
                        ui.scale()
                    ));
                }
                Event::Frame {
                    cpu_ms,
                    layers,
                    interval_ms,
                } => {
                    self.frames += 1;
                    if self.frames <= 3 {
                        self.log.borrow_mut().line(&format!(
                            "frame {}: cpu {cpu_ms:.2} ms, {layers} layers redrawn, {interval_ms:.1} ms since the last",
                            self.frames
                        ));
                    }
                    if self.frames.is_multiple_of(240) {
                        let s = ui.stats();
                        self.log.borrow_mut().line(&format!(
                            "frames {}: cpu p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms, {} over budget",
                            s.frames, s.p50, s.p95, s.max, s.slow
                        ));
                    }
                }
                Event::CloseRequested => {
                    let s = ui.stats();
                    self.log.borrow_mut().line(&format!(
                        "closing after {} frames: cpu p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms, {} over budget",
                        s.frames, s.p50, s.p95, s.max, s.slow
                    ));
                    ui.close();
                }
                Event::Closed => copper_win_rt::quit(0),
                _ => {}
            }
        }

        fn click(&mut self, ui: &mut Ui, id: NodeId) {
            if let Some(i) = self.tab_at(ui, id) {
                if i == self.live {
                    ui.focus(Some(self.address));
                } else {
                    self.select(ui, i, true);
                }
            } else if id == self.new_tab {
                self.new_tab(ui);
            } else if id == self.theme {
                self.cycle_appearance(ui);
            } else if id == self.back || id == self.forward || id == self.reload {
                let what = if id == self.back {
                    "back"
                } else if id == self.forward {
                    "forward"
                } else {
                    "reload"
                };
                self.log.borrow_mut().line(&format!("helm: {what}"));
            } else if id == self.button {
                self.log.borrow_mut().line("page: Start a project");
            } else if id == self.palette {
                // The scrim, not the card.
                self.close_palette(ui);
            } else if let Some(i) = self.picks.iter().position(|(_, row)| *row == id) {
                self.run_pick(ui, i);
            } else if let Some(tag) = ui.tree.get(id).map(|n| n.tag).filter(|&t| t != 0) {
                ui.set_space(if tag == 2 {
                    Some(copper_win_ui::tokens::Theme::COPPER)
                } else {
                    None
                });
            }
        }

        fn key(&mut self, ui: &mut Ui, key: Key, mods: copper_win_ui::input::Mods) {
            if self.palette_open(ui) {
                match key {
                    Key::Up => return self.move_pick(ui, false),
                    Key::Down => return self.move_pick(ui, true),
                    Key::Escape => return self.close_palette(ui),
                    _ => {}
                }
            }
            match (key, mods.ctrl, mods.shift) {
                (Key::Letter('K'), true, false) => {
                    if self.palette_open(ui) {
                        self.close_palette(ui);
                    } else {
                        self.open_palette(ui);
                    }
                }
                (Key::Letter('L'), true, false) => {
                    self.close_palette(ui);
                    ui.focus(Some(self.address));
                    ui.tree.with_field(self.address, |f| f.edit.select_all());
                }
                (Key::Letter('L'), true, true) => self.cycle_appearance(ui),
                (Key::Letter('T'), true, false) => {
                    self.close_palette(ui);
                    self.new_tab(ui);
                }
                (Key::Letter('W'), true, false) => self.close_tab(ui, self.live),
                (Key::Escape, false, false) if !self.palette_open(ui) => {
                    ui.focus_page(self.slot, FocusReason::Programmatic);
                }
                _ => {}
            }
        }

        fn context_menu(&mut self, ui: &mut Ui, target: Option<NodeId>, at: copper_win_ui::Point) {
            if self.palette_open(ui) {
                return;
            }
            let tab = target.and_then(|t| self.tab_at(ui, t));
            let within_side =
                target.is_some_and(|t| t == self.side || ui.tree.is_within(t, self.side));
            if let Some(i) = tab {
                self.menu_tab = Some(i);
                let menu = Menu::new()
                    .item(ITEM_RELOAD, "&Reload")
                    .shortcut("Ctrl+R")
                    .item(ITEM_DUPLICATE, "&Duplicate")
                    .separator()
                    .item(ITEM_CLOSE, "&Close Tab")
                    .shortcut("Ctrl+W");
                ui.show_menu(MENU_TAB, &menu, at);
            } else if within_side
                || target.is_none()
                || target == Some(self.slot)
                || target.is_some_and(|t| ui.tree.is_within(t, self.slot))
            {
                let a = ui.appearance();
                let menu = Menu::new()
                    .item(ITEM_NEW, "&New Tab")
                    .shortcut("Ctrl+T")
                    .separator()
                    .item(ITEM_LIGHT, "&Light")
                    .checked(a == Appearance::Light)
                    .item(ITEM_DARK, "&Dark")
                    .checked(a == Appearance::Dark)
                    .item(ITEM_SYSTEM, "Follow &Windows")
                    .checked(a == Appearance::System);
                ui.show_menu(MENU_COLUMN, &menu, at);
            }
        }

        fn menu_chosen(&mut self, ui: &mut Ui, menu: u32, item: u32) {
            let tab = self.menu_tab.take();
            self.log
                .borrow_mut()
                .line(&format!("menu {menu}: item {item}"));
            match (item, tab) {
                (ITEM_RELOAD, Some(i)) => {
                    let title = self
                        .tabs
                        .get(i)
                        .map(|t| t.title.clone())
                        .unwrap_or_default();
                    self.log.borrow_mut().line(&format!("reload {title:?}"));
                }
                (ITEM_DUPLICATE, Some(i)) => {
                    if let Some(title) = self.tabs.get(i).map(|t| t.title.clone()) {
                        let j = self.push_tab(ui, &title);
                        self.select(ui, j, true);
                    }
                }
                (ITEM_CLOSE, Some(i)) => self.close_tab(ui, i),
                (ITEM_NEW, _) => self.new_tab(ui),
                (ITEM_LIGHT, _) => self.set_appearance(ui, Appearance::Light),
                (ITEM_DARK, _) => self.set_appearance(ui, Appearance::Dark),
                (ITEM_SYSTEM, _) => self.set_appearance(ui, Appearance::System),
                _ => {}
            }
        }
    }

    /// A made-up host for a title, for the address field and the palette.
    fn host_of(title: &str) -> String {
        let word: String = title
            .split([' ', '·', '—', ':', ','])
            .find(|w| !w.is_empty())
            .unwrap_or("copper")
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if word.is_empty() {
            "copper.page".to_owned()
        } else {
            format!("{word}.com")
        }
    }

    pub fn main() {
        let log = Rc::new(RefCell::new(Log::open()));
        if let Err(e) = copper_win_rt::init() {
            log.borrow_mut().line(&format!("runtime: {e}"));
            return;
        }
        let window = match Window::new(Options::default(), |_, _| {}) {
            Ok(w) => w,
            Err(e) => {
                log.borrow_mut().line(&format!("window: {e}"));
                return;
            }
        };
        let Some(gallery) = window.with(|ui| build(ui, log.clone())) else {
            return;
        };
        let gallery = RefCell::new(gallery);
        window.on_event(move |ui, e| gallery.borrow_mut().handle(ui, e));
        window.show();
        let code = copper_win_rt::run();
        log.borrow_mut().line(&format!("exit {code}"));
        std::process::exit(code);
    }
}
