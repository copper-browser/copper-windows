//! About (`Settings.swift` 518–605): which Copper this is, the same update
//! status as Settings › Updates, a way to report a problem, and the
//! keyboard shortcuts — Copper for Windows' own keys, read from
//! [`crate::keys`], never the Mac's ⌘ strings.
//!
//! There is no Diagnostics card: Copper for Windows keeps no crash
//! reports yet, and the Mac shows the card only when there is one.

use super::updates::{ago, latest_line};
use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use crate::settings::Page;
use crate::settings::form::Form;
use copper_shell_core::updates;

/// About: version, feedback and the shortcuts.
pub(super) struct About;

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
        "Version, feedback and the keyboard shortcuts."
    }
    fn glyph(&self) -> char {
        glyph::INFO
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let mut f = Form::default();
        f.section("")
            .card_anchor("about.identity")
            .note(
                "Copper for Windows",
                Some(&format!(
                    "by Collin and Felipe · version {}",
                    updates::version()
                )),
            )
            .footnote("Built on Search by Office Commun");

        let st = updates::status::<Win>();
        let newer = updates::available::<Win>();
        let sec = f.section("Version");
        match &newer {
            Some(v) => {
                sec.button(
                    &format!("Copper {v} is out"),
                    Some("Settings › Updates has what’s new and the download"),
                    "show-update",
                    "Show",
                    true,
                );
            }
            None => {
                let detail = match (&st.error, st.checked) {
                    _ if st.checking => "Checking…".to_string(),
                    (Some(e), _) => e.clone(),
                    (None, Some(when)) if st.latest.is_some() => {
                        format!("Up to date · checked {}", ago(when))
                    }
                    _ => latest_line(&st, None),
                };
                sec.pills(
                    "Updates",
                    Some(&detail),
                    vec![crate::settings::form::Pill {
                        action: "check".into(),
                        label: if st.checking {
                            "Checking…"
                        } else {
                            "Check now"
                        }
                        .into(),
                        filled: false,
                        enabled: !st.checking,
                    }],
                );
            }
        }
        sec.anchor("about.version");
        sec.button(
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
        for (does, commands) in SHORTCUTS {
            let keys = keys_of(commands);
            if !keys.is_empty() {
                sec.value(does, None, &keys).keywords(&[
                    "keyboard shortcut",
                    "shortcut",
                    "hotkey",
                    "keys",
                ]);
            }
        }
        f
    }

    fn act(&self, shell: &Shell, action: &str, _value: &str) {
        match action {
            "check" => updates::check::<Win>(shell, true),
            "show-update" => crate::settings::open(shell, "updates"),
            "feedback" => {
                if let Err(e) = crate::sys::shell::open(&feedback_url()) {
                    tracing::warn!("feedback page: {e}");
                }
            }
            _ => {}
        }
    }
}
