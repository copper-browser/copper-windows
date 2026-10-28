//! What the window's events do. Each runs from the chrome's drain, with
//! nothing of the window borrowed: it reads what it needs through
//! [`Chrome::read`], lets go, then calls the shell (which may call the view
//! back, which edits the window at once).

use super::column::Hit as ColumnHit;
use super::menus::{self, TabFacts, item};
use super::stage::Hit as StageHit;
use super::strip::Hit as StripHit;
use super::{Chrome, State};
use crate::keys::{self, Chord};
use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_engine_webview2::WebView2Page;
use copper_engine_webview2::hosting::{FocusExit, KeyEventKind, PageChange};
use copper_shell_core::OpenTab;
use copper_shell_core::agents::groups;
use copper_shell_core::palette::complete::Purpose;
use copper_win_ui::input::Key;
use copper_win_ui::input::Mods;
use copper_win_ui::win::Ui;
use copper_win_ui::{Event, NodeId, Point};
use std::rc::Rc;

/// One event, from the drain.
pub fn handle(chrome: &Rc<Chrome>, e: Event) {
    let Some(shell) = chrome.shell() else {
        if matches!(e, Event::Closed) {
            copper_win_rt::quit(0);
        }
        return;
    };
    match e {
        Event::Click(id) => click(chrome, &shell, id),
        Event::MiddleClick(id) => {
            if let Some(tab) = tab_at(chrome, id) {
                shell.close(&tab);
            }
        }
        Event::ContextMenu { target, at } => context_menu(chrome, &shell, target, at),
        Event::MenuChosen { menu, item } => menu_chosen(chrome, &shell, menu, item),
        Event::MenuDismissed { .. } => {
            chrome.edit(|_, st| {
                st.menus.tab = None;
                super::pagemenu::dismissed(st);
            });
        }
        Event::FieldChanged(id) if is_card_field(chrome, id) => crate::palette::changed(&shell),
        Event::FieldSubmit(id) if is_card_field(chrome, id) => {
            crate::palette::submit(&shell, alt_down())
        }
        Event::FieldCancel(id) if is_card_field(chrome, id) => crate::palette::cancel(&shell),
        Event::FieldChanged(id) | Event::FieldSubmit(id) | Event::FieldCancel(id)
            if crate::palette::strip_field(&shell, id, &e) => {}
        Event::FieldChanged(id) | Event::FieldSubmit(id) | Event::FieldCancel(id) => {
            if super::letter::field_event(&shell, id, &e) {
                return;
            }
            if crate::canvas::field_event(&shell, id, &e) {
                return;
            }
            crate::find::field_event(&shell, id, &e);
            crate::panels::field_event(&shell, id, &e);
        }
        Event::Key { key, mods, focused } => key_event(chrome, &shell, key, mods, focused),
        Event::Hover(target) => crate::peek::hover(&shell, target),
        Event::Resized(_) => {
            chrome.edit(|ui, st| st.card.resized(ui));
            if shell.view().layout() == copper_core::settings::TabsPosition::Top {
                shell.view().sync(&shell);
            }
        }
        Event::Appearance { .. } => shell.dark_changed(),
        // A link's address does not outlive the window's attention.
        Event::Activated(false) => chrome.edit(|ui, st| st.stage.set_hover(ui, None)),
        // Settings may have changed where downloads go.
        Event::Activated(true) => shell.view().apply_download_ask(&shell),
        Event::CloseRequested => crate::app::quit(&shell),
        Event::Closed => copper_win_rt::quit(0),
        _ => {}
    }
}

fn is_card_field(chrome: &Chrome, id: NodeId) -> bool {
    chrome.read(|_, st| st.card.field == id).unwrap_or(false)
}

/// Alt held right now (Alt+Return opens in a new tab).
fn alt_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_MENU};
    // SAFETY: reads the calling thread's key state.
    unsafe { GetKeyState(i32::from(VK_MENU.0)) < 0 }
}

/// What a click means, read from whichever part of the chrome owns the
/// node.
enum Clicked {
    Answer(Box<dyn FnOnce(bool)>, bool),
    Row(usize),
    Scrim,
    Column(ColumnHit),
    Strip(StripHit),
    Stage(StageHit),
    Unfold,
    Other(NodeId),
}

fn click(chrome: &Rc<Chrome>, shell: &Shell, id: NodeId) {
    if super::welcome::click(shell, id) {
        return;
    }
    let Some(what) = chrome.read(|ui, st| {
        if let Some((a, yes)) = st.sheet.answered(ui, id) {
            return Clicked::Answer(a, yes);
        }
        if st.card.is_open() {
            if let Some(i) = st.card.row_at(ui, id) {
                return Clicked::Row(i);
            }
            if st.card.is_scrim(id) {
                return Clicked::Scrim;
            }
        }
        if id == st.ids.unfold {
            return Clicked::Unfold;
        }
        if let Some(h) = st.column.hit(id) {
            return Clicked::Column(h);
        }
        if let Some(h) = st.strip.hit(id) {
            return Clicked::Strip(h);
        }
        if let Some(h) = st.stage.hit(id) {
            return Clicked::Stage(h);
        }
        Clicked::Other(id)
    }) else {
        return;
    };
    match what {
        Clicked::Answer(answer, yes) => {
            shell.view().overlay_changed();
            answer(yes);
        }
        Clicked::Row(i) => crate::palette::fire(shell, i),
        Clicked::Scrim => crate::palette::close(shell),
        Clicked::Unfold => shell.view().toggle_fold(),
        Clicked::Column(h) => column_click(chrome, shell, h),
        Clicked::Strip(h) => strip_click(shell, h),
        Clicked::Stage(StageHit::StatusButton(tab)) => shell.reload(&tab),
        Clicked::Other(id) => {
            if crate::palette::click(shell, id) || crate::peek::click(shell, id) {
                return;
            }
            if !crate::panels::click(shell, id)
                && !crate::forms::click(shell, id)
                && !crate::addons::click(shell, id)
                && !crate::canvas::click(shell, id)
                && !crate::clear::click(shell, id)
            {
                crate::find::click(shell, id);
            }
        }
    }
}

fn column_click(chrome: &Rc<Chrome>, shell: &Shell, h: ColumnHit) {
    match h {
        ColumnHit::Fold => shell.view().toggle_fold(),
        ColumnHit::Back => shell.back(),
        ColumnHit::Forward => shell.forward(),
        ColumnHit::Reload => {
            if super::model::Model::nav_of(shell).loading {
                shell.stop_active();
            } else {
                shell.reload_active();
            }
        }
        ColumnHit::Address => crate::palette::edit_address(shell),
        ColumnHit::NewTab => crate::palette::open(shell, Purpose::Launch, false),
        ColumnHit::Tab(id) if super::drag::was_dragged(&id) => {}
        ColumnHit::Tab(id) => {
            // The live row again: its address, ready to edit (`Side.swift`).
            if shell.active_id().as_ref() == Some(&id) {
                crate::palette::edit_address(shell);
            } else {
                chrome.watch("tab switch");
                shell.select(&id);
            }
        }
        ColumnHit::Close(id) => {
            shell.close(&id);
        }
        ColumnHit::Settings => {
            crate::commands::run(shell, "settings");
        }
        ColumnHit::Bookmarks => {
            crate::commands::run(shell, "bookmarks-door");
        }
        ColumnHit::Canvas => crate::canvas::door::toggle(shell),
        ColumnHit::Folder(id) => groups::toggle(shell, &id),
        h => crate::palette::spaces::column_hit(chrome, shell, h),
    }
}

fn strip_click(shell: &Shell, h: StripHit) {
    match h {
        StripHit::Back => shell.back(),
        StripHit::Forward => shell.forward(),
        StripHit::Reload => {
            if super::model::Model::nav_of(shell).loading {
                shell.stop_active();
            } else {
                shell.reload_active();
            }
        }
        StripHit::NewTab => crate::palette::open(shell, Purpose::Launch, false),
        StripHit::Tab(id) => {
            if shell.active_id().as_ref() == Some(&id) {
                crate::palette::edit_address(shell);
            } else {
                shell.view().chrome().watch("tab switch");
                shell.select(&id);
            }
        }
        StripHit::Close(id) => {
            shell.close(&id);
        }
    }
}

/// The tab a node in the column or the strip belongs to.
fn tab_at(chrome: &Chrome, id: NodeId) -> Option<TabId> {
    chrome
        .read(|ui, st| st.column.tab_at(ui, id).or_else(|| st.strip.tab_at(ui, id)))
        .flatten()
}

fn context_menu(chrome: &Rc<Chrome>, shell: &Shell, target: Option<NodeId>, at: Point) {
    if crate::canvas::context_menu(shell, target, at) {
        return;
    }
    if crate::palette::spaces::context_menu(chrome, shell, target, at) {
        return;
    }
    let Some(tab) = target.and_then(|t| tab_at(chrome, t)) else {
        crate::panels::context_menu(shell, target, at);
        return;
    };
    let (facts, canvas) = {
        let b = shell.browser();
        let Some(t) = b.tab(&tab) else { return };
        let canvas = crate::canvas::tab_menu_items(shell, t);
        let facts = TabFacts {
            pinned: t.is_pinned(),
            blank: t.url.is_empty() || t.url.starts_with("about:blank"),
            saved: shell.tab_view(&tab).is_some_and(|v| v.saved),
            several: shell.visible_ids().len() > 1,
            filed: t.group.as_deref().is_some_and(|g| {
                groups::all(shell)
                    .iter()
                    .any(|f| f.id.eq_ignore_ascii_case(g))
            }),
        };
        (facts, canvas)
    };
    let mut menu = menus::tab_menu(facts);
    menu.items.splice(0..0, canvas);
    chrome.edit(move |ui, st| {
        st.menus.tab = Some(tab);
        ui.show_menu(menus::TAB_MENU, &menu, at);
    });
}

fn menu_chosen(chrome: &Rc<Chrome>, shell: &Shell, menu: u32, chosen: u32) {
    if crate::palette::spaces::menu_chosen(shell, menu, chosen) {
        return;
    }
    if menu == crate::forms::PICKER_MENU {
        crate::forms::picked(chosen);
        return;
    }
    if crate::canvas::menu_chosen(shell, menu, chosen) {
        return;
    }
    if menu == super::pagemenu::PAGE_MENU {
        super::pagemenu::chosen(shell, chosen);
        return;
    }
    if menu != menus::TAB_MENU {
        crate::panels::menu_chosen(shell, menu, chosen);
        return;
    }
    let Some(Some(tab)) = chrome.read(|_, st| st.menus.tab.take()) else {
        return;
    };
    match chosen {
        item::PIN => {
            shell.pin(&tab);
        }
        item::UNPIN => {
            shell.unpin(&tab);
        }
        item::CHANGE_LETTER => super::letter::open(shell, &tab),
        item::DUPLICATE => duplicate(shell, &tab),
        item::COPY_ADDRESS => {
            let url = shell.browser().tab(&tab).map(|t| t.url.clone());
            if let Some(url) = url {
                shell.view().copy_text(&url);
                shell.toast("Address copied");
            }
        }
        item::CLOSE => {
            shell.close(&tab);
        }
        item::CLOSE_OTHERS => {
            for id in shell.visible_ids() {
                let pinned = shell.browser().tab(&id).is_some_and(|t| t.is_pinned());
                if id != tab && !pinned {
                    shell.close(&id);
                }
            }
        }
        item::SAVE => shell.set_saved(&tab, true),
        item::UNSAVE => shell.set_saved(&tab, false),
        item::TAKE_OUT => groups::take_out(shell, &tab),
        _ => {}
    }
}

/// A second tab on the same page, right after it.
pub fn duplicate(shell: &Shell, tab: &TabId) {
    let url = shell.browser().tab(tab).map(|t| t.url.clone());
    if let Some(url) = url.filter(|u| !u.is_empty()) {
        shell.open(
            &url,
            OpenTab {
                foreground: true,
                after: Some(tab.clone()),
                ..Default::default()
            },
        );
    }
}

/// Keys the chrome's focused node did not use.
fn key_event(chrome: &Rc<Chrome>, shell: &Shell, key: Key, mods: Mods, focused: Option<NodeId>) {
    if super::welcome::key_event(shell, key, mods) {
        return;
    }
    let in_card = focused.is_some()
        && focused
            == chrome
                .read(|_, st| st.card.is_open().then_some(st.card.field))
                .flatten();
    if in_card && crate::palette::key(shell, key, mods) {
        return;
    }
    if in_card && !mods.ctrl && !mods.alt {
        match key {
            Key::Down => return crate::palette::step(shell, 1),
            Key::Up => return crate::palette::step(shell, -1),
            Key::Tab => return crate::palette::jump(shell, !mods.shift),
            // Only at the very end of the text (the field is `app_keys`):
            // take the inline completion (`Browser.acceptCompletion`).
            Key::Right | Key::End if !mods.shift && crate::palette::accept(shell) => return,
            _ => {}
        }
    }
    if crate::panels::key(shell, key, mods, focused) {
        return;
    }
    if crate::find::key(shell, key, mods, focused) {
        return;
    }
    let chord = Chord::new(key, mods);
    if let Some(id) = crate::panels::hidden::takes(shell, chord)
        .or_else(|| crate::clear::takes(shell, chord))
        .or_else(|| keys::command(chord))
    {
        crate::commands::run(shell, id);
    }
}

/// Wire a new page to the chrome: Copper's keys while the page has focus,
/// Tab out of the page into the chrome, full screen and the status text.
pub fn hook_page(shell: &Shell, tab: &TabId, page: &WebView2Page) {
    super::permit::watch(shell, tab, page);
    super::pagemenu::watch(shell, tab, page);
    let weak = shell.downgrade();
    page.on_accelerator_key(move |k| {
        if !matches!(k.kind, KeyEventKind::KeyDown | KeyEventKind::SystemKeyDown) {
            return false;
        }
        let Some(shell) = weak.upgrade() else {
            return false;
        };
        let chord = Chord::new(
            keys::key_of(k.virtual_key),
            Mods {
                ctrl: k.ctrl,
                shift: k.shift,
                alt: k.alt,
            },
        );
        let Some(id) = crate::panels::hidden::takes(&shell, chord)
            .or_else(|| crate::clear::takes(&shell, chord))
            .or_else(|| keys::page_takes(chord, shell.view().chrome().has_overlay()))
        else {
            return false;
        };
        // Run on the loop's next turn: never inside WebView2's callback.
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            if let Some(s) = weak.upgrade() {
                if crate::panels::key(&s, chord.key, chord.mods, None) {
                    return;
                }
                crate::commands::run(&s, id);
            }
        });
        true
    });
    let weak = shell.downgrade();
    page.on_focus_exit(move |exit| {
        let Some(shell) = weak.upgrade() else { return };
        let chrome = shell.view().chrome().clone();
        chrome.edit(move |ui, st| {
            // Out of the page, the address is next either way round (the
            // column's doors come before it, the page after).
            let _: FocusExit = exit;
            ui.focus(Some(st.column.address));
        });
    });
    let weak = shell.downgrade();
    let tab = tab.clone();
    let mut changes = page.changes();
    copper_win_rt::spawn_local(async move {
        while let Some(c) = changes.next().await {
            let Some(shell) = weak.upgrade() else { return };
            match c {
                PageChange::Fullscreen(on) => shell.view().page_fullscreen(&shell, &tab, on),
                PageChange::Focus(on) => {
                    tracing::debug!(target: "copper::focus", tab = %tab.short(), on, "page focus");
                    // A page that takes the keyboard by itself (a new
                    // blank tab's first load) does not take it from the
                    // open card: typing goes on into the card's field.
                    if on {
                        shell.view().chrome().edit(|ui, st| {
                            if st.card.is_open() {
                                ui.focus(Some(st.card.field));
                            }
                        });
                    }
                }
                PageChange::History if shell.active_id().as_ref() == Some(&tab) => {
                    let nav = super::model::Model::nav_of(&shell);
                    shell.view().chrome().edit(move |ui, st| {
                        st.column.sync_nav(ui, nav);
                        st.strip.sync_nav(ui, nav);
                    });
                }
                _ => {}
            }
        }
    });
}

thread_local! {
    /// Whether the parts were last laid out for a page that has the screen.
    static LAID_IMMERSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Show and hide the parts for the layout: the column or the strip, the
/// band over the page, the folded column's door; and nothing but the page
/// while it is full screen.
///
/// A page taking the screen has it at once, on the black ground
/// (`Ui::set_blackout`), so no pale band shows as the window grows; when
/// it gives the screen back the column or strip fades back in over the
/// Mac's `easeOut(duration: 0.12)` (`App.swift` 292–349).
pub fn apply_layout(ui: &mut Ui, st: &mut State) {
    let side = st.side && !st.immersed;
    let column = side && !st.folded;
    let strip = !st.side && !st.immersed;
    let back = LAID_IMMERSED.with(|c| c.replace(st.immersed)) && !st.immersed;
    for (part, shown) in [(st.column.side, column), (st.strip.strip, strip)] {
        let was = ui.tree.get(part).is_some_and(|n| !n.style.hidden);
        ui.tree.set_hidden(part, !shown);
        if back && shown && !was {
            ui.tree.set_prop(part, copper_win_ui::Prop::Opacity, 0.0);
            ui.tree.animate(
                part,
                copper_win_ui::Prop::Opacity,
                1.0,
                copper_win_ui::tokens::motion::IMMERSE,
            );
        }
    }
    // The band is the caption buttons' room over the page.
    ui.tree.set_hidden(st.ids.band, !side || st.full);
    ui.tree.set_hidden(st.ids.unfold, !(side && st.folded));
    let edge = if column { 1.0 } else { 0.0 };
    ui.tree.edit(st.ids.well, |n| n.style.padding.left = edge);
}
