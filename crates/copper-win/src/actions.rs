//! The shell's own commands (Linux `actions.rs` `register_core`): tabs,
//! the field and the palette, the page, spaces, the window. Features add
//! theirs when they start.

use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::Shell;
use copper_core::browser::TabId;
use copper_core::settings::{Look, TabsPosition};
use copper_engine::{PageView, World};
use copper_shell_core::OpenTab;
use copper_shell_core::palette::complete::Purpose;

fn with_active(s: &Shell, f: impl FnOnce(&Shell, &TabId)) {
    if let Some(id) = s.active_id() {
        f(s, &id);
    }
}

fn has_tab(s: &Shell) -> bool {
    s.active_id().is_some()
}

fn has_page(s: &Shell) -> bool {
    s.active_page().is_some()
}

fn several_spaces(s: &Shell) -> bool {
    s.browser().spaces().count() > 1
}

/// `Ctrl+Shift+N` (`Browser.swift` `newShyTab`): a private tab at once,
/// blank, its address ready for typing, and a word on what it is.
fn new_private_tab(s: &Shell) {
    // Already on a blank private tab: just the address again.
    let blank_private = s.active_is_private()
        && s.browser()
            .active()
            .is_some_and(|t| t.url.is_empty() || t.url.starts_with("about:blank"));
    if blank_private {
        crate::palette::open(s, Purpose::Edit, true);
        return;
    }
    s.open(
        "about:blank",
        OpenTab {
            title: "New Tab".into(),
            foreground: true,
            private: true,
            ..OpenTab::default()
        },
    );
    s.toast("A tab that keeps nothing");
    crate::palette::open(s, Purpose::Edit, true);
}

/// `Ctrl+Shift+M` (`Browser.swift` `pauseMedia`): whatever is making
/// noise in this tab stops. (The page's own frames and any it can reach;
/// WKWebView's `pauseAllMediaPlayback` also reaches other sites' frames.)
fn pause_media(s: &Shell) {
    let Some(page) = s.active_page() else { return };
    const PAUSE: &str = "(() => { const all = (d) => { for (const m of d.querySelectorAll('video, audio')) { try { m.pause(); } catch (_) {} } for (const f of d.querySelectorAll('iframe')) { try { if (f.contentDocument) all(f.contentDocument); } catch (_) {} } }; all(document); })()";
    let done = page.eval(PAUSE, &World::isolated("copper-media"));
    copper_win_rt::spawn_local(async move {
        let _ = done.await;
    });
    s.toast("Paused");
}

/// Register the core commands.
pub fn register(shell: &Shell) {
    let r = shell.view().commands();
    let cmds = [
        // Tabs.
        Command::new("new-tab", "New Tab", glyph::PLUS, |s| {
            crate::palette::open(s, Purpose::Launch, false)
        }),
        Command::new(
            "new-private-tab",
            "New Private Tab",
            glyph::PRIVATE,
            new_private_tab,
        ),
        Command::new("close-tab", "Close Tab", glyph::CLOSE, |s| s.close_active()).when(has_tab),
        Command::new("reopen-tab", "Reopen Closed Tab", glyph::HISTORY, |s| {
            s.reopen();
        })
        .when(|s| !s.browser().closed_tabs().is_empty()),
        Command::key("next-tab", |s| s.cycle_tab(true)),
        Command::key("previous-tab", |s| s.cycle_tab(false)),
        Command::new("pin-tab", "Pin or Unpin Tab", glyph::PIN, |s| {
            with_active(s, |s, id| s.toggle_pin(id))
        })
        .when(has_tab),
        Command::new("duplicate-tab", "Duplicate Tab", glyph::COPY, |s| {
            with_active(s, crate::chrome::events::duplicate)
        })
        .when(has_page),
        Command::new("copy-address", "Copy Address", glyph::COPY, |s| {
            if let Some(url) = s.browser().active().map(|t| t.url.clone()) {
                s.view().copy_text(&url);
                s.toast("Address copied");
            }
        })
        .when(has_tab),
        // The field and the palette.
        Command::key("focus-field", crate::palette::edit_address),
        Command::key("palette", |s| {
            crate::palette::open(s, Purpose::Palette, false)
        }),
        Command::key("search", |s| {
            crate::palette::open(s, Purpose::Launch, false)
        }),
        Command::key("escape", |s| s.view().escape(s)),
        // The page.
        Command::key("reload", |s| s.reload_active()),
        Command::new("pause-media", "Pause Media", glyph::AUDIO, pause_media).when(has_page),
        Command::new("hard-reload", "Reload Without Cache", glyph::RELOAD, |s| {
            s.hard_reload_active()
        })
        .when(has_page),
        Command::key("stop", |s| s.stop_active()),
        Command::key("back", |s| s.back()),
        Command::key("forward", |s| s.forward()),
        Command::new("zoom-in", "Zoom In", glyph::SEARCH, |s| s.zoom(Some(true))).when(has_page),
        Command::new("zoom-out", "Zoom Out", glyph::SEARCH, |s| {
            s.zoom(Some(false))
        })
        .when(has_page),
        Command::new("zoom-reset", "Actual Size", glyph::SEARCH, |s| s.zoom(None)).when(has_page),
        Command::new("inspector", "Developer Tools", glyph::GEAR, |s| {
            s.show_inspector()
        })
        .when(has_page),
        // Spaces.
        Command::new("next-space", "Next Space", glyph::FORWARD, |s| {
            s.cycle_space(true)
        })
        .when(several_spaces),
        Command::new("previous-space", "Previous Space", glyph::BACK, |s| {
            s.cycle_space(false)
        })
        .when(several_spaces),
        // New Space and Delete Space are the palette's (`palette::register`).
        // The window.
        Command::new(
            "toggle-layout",
            "Move Tabs to the Top or Side",
            glyph::SIDEBAR,
            |s| {
                let next = match s.tabs_position() {
                    TabsPosition::Left => TabsPosition::Top,
                    TabsPosition::Top => TabsPosition::Left,
                };
                s.set_tabs_position(next);
            },
        ),
        Command::new(
            "toggle-sidebar",
            "Hide or Show Sidebar",
            glyph::SIDEBAR,
            |s| s.view().toggle_fold(),
        )
        .when(|s| s.tabs_position() == TabsPosition::Left),
        Command::new(
            "fullscreen",
            "Enter or Exit Full Screen",
            glyph::FORWARD,
            |s| s.view().toggle_fullscreen(),
        ),
        Command::new("look-light", "Light Appearance", glyph::GEAR, |s| {
            s.set_look(Look::Light)
        })
        .when(|s| s.look() != Look::Light),
        Command::new("look-dark", "Dark Appearance", glyph::GEAR, |s| {
            s.set_look(Look::Dark)
        })
        .when(|s| s.look() != Look::Dark),
        Command::new("look-system", "Match System Appearance", glyph::GEAR, |s| {
            s.set_look(Look::System)
        })
        .when(|s| s.look() != Look::System),
        Command::new("settings", "Settings", glyph::GEAR, |s| {
            crate::settings::open(s, "general")
        }),
        Command::new("quit", "Quit Copper", glyph::CLOSE, crate::app::quit),
        // What sites were allowed (Settings › Permissions on the Mac).
        Command::new(
            "forget-capture",
            "Forget Camera and Microphone Choices",
            glyph::VIDEO,
            crate::chrome::permit::forget_capture,
        )
        .when(|s| crate::chrome::permit::has_choices(s, crate::chrome::permit::CAPTURE)),
        Command::new(
            "forget-permits",
            "Forget Location and Notification Choices",
            glyph::LOCATION,
            crate::chrome::permit::forget_others,
        )
        .when(|s| crate::chrome::permit::has_choices(s, crate::chrome::permit::PERMIT)),
    ];
    for c in cmds {
        r.add(c);
    }
    for n in 1..=9usize {
        r.add(Command::key(&format!("select-tab-{n}"), move |s| {
            s.select_nth(if n == 9 { None } else { Some(n - 1) })
        }));
    }
}

/// "Switch to …" for every other space, made fresh for the palette.
pub fn space_commands(shell: &Shell) -> Vec<(String, String)> {
    let b = shell.browser();
    let current = b.current_space_id().to_string();
    b.spaces()
        .filter(|sp| sp.id != current)
        .map(|sp| {
            (
                format!("space-{}", sp.id),
                format!("Switch to {}", crate::chrome::model::title_of(&sp.name)),
            )
        })
        .collect()
}
