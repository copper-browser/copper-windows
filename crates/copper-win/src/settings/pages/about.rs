//! About (`Settings.swift` `about`): which Copper this is, its updates
//! ([`super::updates`], a page of its own until Copper#115), a way to
//! report a problem, and the keyboard shortcuts folded into one row —
//! Copper for Windows' own keys, read from [`crate::keys`], never the
//! Mac's ⌘ strings.
//!
//! There is no Diagnostics card: Copper for Windows keeps no crash
//! reports yet, and the Mac shows the card only when there is one.

use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::fold;
use crate::settings::form::Form;
use copper_shell_core::updates;

/// About: version, feedback and the shortcuts.
pub(super) struct About;

/// The fold the shortcuts are in.
const SHORTCUTS_FOLD: &str = "about.shortcuts.more";

/// Where problems are reported: a new issue on Copper for Windows' page.
const ISSUES: &str = "https://github.com/copper-browser/copper-windows/issues/new";

/// What each shortcut does, and the commands whose keys say it (`SettingsShortcuts.all`,
/// those Copper for Windows has).
const SHORTCUTS: &[(&str, &[&str])] = &[
    ("Address", &["focus-field"]),
    ("Switch tab", &["palette"]),
    (
        "New, close, reopen tab",
        &["new-tab", "close-tab", "reopen-tab"],
    ),
    ("New private tab", &["new-private-tab"]),
    ("New canvas", &["new-canvas"]),
    ("Bookmark this page", &["bookmark-page"]),
    ("History", &["show-history"]),
    ("Downloads", &["show-downloads"]),
    ("Tabs in a sidebar", &["toggle-layout"]),
    ("Fold the sidebar away", &["toggle-sidebar"]),
    ("Find on the page", &["find"]),
    ("Reading mode", &["reader"]),
    ("Hard reload", &["hard-reload"]),
    ("Inspect element", &["inspector"]),
    ("Hide something on this site", &["hide-anything"]),
    ("The agent", &["agent"]),
    ("Settings", &["settings"]),
];

/// `Ctrl+T  Ctrl+W  Ctrl+Shift+T`: each command's first chord.
fn keys_of(commands: &[&str]) -> String {
    commands
        .iter()
        .filter_map(|c| crate::keys::shortcut(c))
        .collect::<Vec<_>>()
        .join("  ")
}

/// A new issue with the version already in it.
fn feedback_url() -> String {
    let body = format!("Copper for Windows {}\n\n", updates::version());
    let mut u = url::Url::parse(ISSUES).expect("a valid address");
    u.query_pairs_mut().append_pair("body", &body);
    u.to_string()
}

impl Page for About {
    fn id(&self) -> &'static str {
        "about"
    }
    fn title(&self) -> &'static str {
        "About"
    }
    fn blurb(&self) -> &'static str {
        "Updates, feedback and the keyboard shortcuts."
    }
    fn glyph(&self) -> char {
        glyph::INFO
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let mut f = Form::default();
        f.section("")
            .card_anchor("about.identity")
            .note("Copper for Windows", Some("by Collin and Felipe"))
            .footnote("Built on Search by Office Commun");

        super::updates::section(&mut f);

        f.section("Help")
            .button(
                "Found something wrong?",
                Some("Opens a new issue on Copper’s page, with the version already in it"),
                "feedback",
                "Send Feedback",
                false,
            )
            .anchor("about.feedback")
            .keywords(&["bug", "report", "issue", "github"]);

        let sec = f
            .section("Keyboard shortcuts")
            .card_anchor("about.shortcuts");
        // The list was a card of rows ending every About; folded, it is
        // one row, and search still finds each keystroke.
        let shown: Vec<(&str, String)> = SHORTCUTS
            .iter()
            .map(|(does, commands)| (*does, keys_of(commands)))
            .filter(|(_, keys)| !keys.is_empty())
            .collect();
        fold::holds(SHORTCUTS_FOLD, &["about.shortcuts"]);
        let open = fold::is_open(SHORTCUTS_FOLD);
        sec.button(
            "Every keystroke Copper answers to",
            Some(&format!("{} shortcuts", shown.len())),
            &format!("{}{SHORTCUTS_FOLD}", fold::ACTION),
            if open { "Hide" } else { "Show" },
            false,
        )
        .anchor(SHORTCUTS_FOLD)
        .keywords(&[
            "shortcuts",
            "hotkeys",
            "keys",
            "keyboard",
            "key bindings",
            "cheat sheet",
        ]);
        for (does, keys) in shown.iter().filter(|_| open) {
            sec.value(does, None, keys).keywords(&[
                "keyboard shortcut",
                "shortcut",
                "hotkey",
                "keys",
            ]);
        }
        f
    }

    fn act(&self, shell: &Shell, action: &str, _value: &str) {
        if !super::updates::act(shell, action)
            && action == "feedback"
            && let Err(e) = crate::sys::shell::open(&feedback_url())
        {
            tracing::warn!("feedback page: {e}");
        }
    }
}
