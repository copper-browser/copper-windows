//! Where pages sit in the rail and what search knows about them
//! (`Fork/SettingsIndex.swift`): the rail's groups, each page's own other
//! words, and the search entries a page's [`Form`] gives. Platform-free so
//! it is tested on any host.

use super::form::Form;
use super::search::Entry;

/// The rail's groups and their pages, in order (`SettingsIndex.swift`
/// 32–60, with the Windows rail's Spaces, Addons, Agents and Cloud). A
/// registered page this table doesn't name goes at the end of `Copper`.
pub const GROUPS: &[(&str, &[&str])] = &[
    (
        "Browsing",
        &[
            "general",
            "tabs",
            "spaces",
            "downloads",
            "extensions",
            "addons",
        ],
    ),
    ("AI & agents", &["intelligence", "agents", "voice"]),
    ("Data & privacy", &["passwords", "privacy", "cloud"]),
    ("Copper", &["labs", "updates", "about"]),
];

/// The page Settings opens at the first time.
pub const FIRST: &str = "general";

/// The registered `pages` grouped as the rail shows them; empty groups are
/// left out.
pub fn rail<'a>(pages: &[&'a str]) -> Vec<(&'static str, Vec<&'a str>)> {
    let mut out: Vec<(&'static str, Vec<&'a str>)> = GROUPS
        .iter()
        .map(|(g, ids)| {
            (
                *g,
                ids.iter()
                    .filter_map(|id| pages.iter().find(|p| *p == id).copied())
                    .collect(),
            )
        })
        .collect();
    let known = |p: &&str| GROUPS.iter().any(|(_, ids)| ids.contains(p));
    if let Some(last) = out.last_mut() {
        last.1.extend(pages.iter().filter(|p| !known(p)).copied());
    }
    out.retain(|(_, ids)| !ids.is_empty());
    out
}

/// The registered pages in rail order, flat (what ↑/↓ and search walk).
pub fn order<'a>(pages: &[&'a str]) -> Vec<&'a str> {
    rail(pages).into_iter().flat_map(|(_, ids)| ids).collect()
}

/// A page's other words (`Page.keywords`).
pub fn page_keywords(id: &str) -> &'static [&'static str] {
    match id {
        "general" => &["preferences", "basics", "options"],
        "tabs" => &["tab bar", "vertical tabs", "sidebar"],
        "spaces" => &["workspaces", "profiles", "space look"],
        "intelligence" => &[
            "ai",
            "model",
            "llm",
            "claude",
            "anthropic",
            "openai",
            "gpt",
            "keys",
        ],
        "agents" => &["mcp", "proxy", "bot", "bots", "automation", "agent"],
        "voice" => &[
            "voice",
            "dictation",
            "dictate",
            "speech",
            "transcription",
            "push to talk",
            "hold to talk",
            "microphone",
            "mic",
            "talk",
        ],
        "cloud" => &["sync", "account", "canvas", "devices", "backup"],
        "updates" => &["update", "upgrade", "version", "release"],
        "extensions" => &["add-ons", "plugins", "chrome web store"],
        "passwords" => &[
            "pw",
            "pwd",
            "logins",
            "credential manager",
            "autofill",
            "credentials",
            "password manager",
        ],
        "downloads" => &["files", "download folder"],
        "privacy" => &["security", "tracking", "data", "clear"],
        "labs" => &[
            "experiments",
            "experimental",
            "flights",
            "beta",
            "beta features",
            "preview",
            "previews",
        ],
        "addons" => &["addon", "add-on", "agent sidebar", "sidebar app", "web app"],
        "about" => &["version", "credits", "help", "shortcuts"],
        _ => &[],
    }
}

/// The anchor at the top of every page (`Page.topAnchor`).
pub fn top_anchor(page: &str) -> String {
    format!("{page}.top")
}

/// The entry for page `id` as a whole.
pub fn page_entry(id: &'static str, title: &str, blurb: &str) -> Entry {
    Entry {
        page: id,
        page_title: title.to_string(),
        section: String::new(),
        title: title.to_string(),
        subtitle: blurb.to_string(),
        keywords: page_keywords(id).iter().map(|s| s.to_string()).collect(),
        anchor: top_anchor(id),
        is_page: true,
    }
}

/// One entry per titled line of `form`: what search finds on a page that
/// has no index of its own. A section with an anchor of its own lands its
/// lines on the card.
pub fn entries_of(id: &'static str, title: &str, form: &Form) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut heading = String::new();
    for s in &form.sections {
        if !s.continues {
            heading = s.title.clone();
        }
        for l in &s.lines {
            if l.title.trim().is_empty() {
                continue;
            }
            out.push(Entry {
                page: id,
                page_title: title.to_string(),
                section: heading.clone(),
                title: l.title.clone(),
                subtitle: l.detail.clone().unwrap_or_default(),
                keywords: l.keywords.clone(),
                anchor: l
                    .anchor
                    .clone()
                    .or_else(|| s.anchor.clone())
                    .unwrap_or_else(|| l.title.clone()),
                is_page: false,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rail_groups_what_is_registered() {
        let pages = [
            "general",
            "privacy",
            "passwords",
            "updates",
            "custom",
            "about",
        ];
        let r = rail(&pages);
        assert_eq!(r[0], ("Browsing", vec!["general"]));
        assert_eq!(r[1], ("Data & privacy", vec!["passwords", "privacy"]));
        assert_eq!(r[2], ("Copper", vec!["updates", "about", "custom"]));
        assert_eq!(r.len(), 3);
        assert_eq!(
            order(&pages),
            vec![
                "general",
                "passwords",
                "privacy",
                "updates",
                "about",
                "custom"
            ]
        );
    }

    #[test]
    fn a_form_gives_one_entry_per_titled_line() {
        let mut f = Form::default();
        f.section("Copper on this PC")
            .button(
                "Open links from other apps",
                Some("Make Copper the default"),
                "d",
                "Make default",
                true,
            )
            .anchor("general.default")
            .keywords(&["default browser"]);
        f.more()
            .card_anchor("bitwarden")
            .note("Bitwarden", Some("Not connected"))
            .note("", None);
        let e = entries_of("general", "General", &f);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].anchor, "general.default");
        assert_eq!(e[0].section, "Copper on this PC");
        assert_eq!(e[1].anchor, "bitwarden");
        // A continued card keeps the section's heading.
        assert_eq!(e[1].section, "Copper on this PC");
    }

    #[test]
    fn a_page_entry_carries_its_words() {
        let e = page_entry("passwords", "Passwords", "Saved sign-ins");
        assert!(e.is_page);
        assert_eq!(e.anchor, "passwords.top");
        assert!(e.keywords.iter().any(|k| k == "credential manager"));
    }
}
