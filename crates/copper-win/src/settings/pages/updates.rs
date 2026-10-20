//! Updates (`SettingsFork.swift` 339–454): which Copper this is, and the
//! next one. The check is copper-shell-core's `updates` lane (every six
//! hours, and on Check now); Copper for Windows doesn't download or swap
//! itself, so a newer Copper is offered as the Windows installer on the
//! releases page — never as "ready" or "Update".

use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use crate::settings::Page;
use crate::settings::form::{Form, Pill};
use copper_shell_core::updates;

/// Updates: which Copper you have, and the next one.
pub(super) struct Updates;

/// `2026-10-12T22:01:03Z` → `Oct 12, 2026` (the Mac's abbreviated date);
/// anything else as it came.
pub(super) fn published(raw: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = raw.get(..10).unwrap_or("").split('-');
    let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return raw.to_string();
    };
    match (y.parse::<u32>(), m.parse::<usize>(), d.parse::<u32>()) {
        (Ok(y), Ok(m), Ok(d)) if (1..=12).contains(&m) => format!("{} {d}, {y}", MONTHS[m - 1]),
        _ => raw.to_string(),
    }
}

/// `3 minutes ago`, as the rest of Copper says it.
pub(super) fn ago(when: f64) -> String {
    copper_shell_core::format::ago::<crate::sys::calendar::WinCalendar>(
        when,
        copper_core::date::now_unix(),
    )
}

/// The host updates come from (`github.com`).
fn feed_host() -> String {
    url::Url::parse(crate::updates::FEED)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "github.com".into())
}

/// The headline under "You have …" (`latestLine`): a newer Copper already
/// known stays the headline when a re-check fails.
pub(super) fn latest_line(st: &updates::Status, newer: Option<&str>) -> String {
    if st.checking {
        return "Checking…".into();
    }
    if let (Some(v), Some(latest)) = (newer, &st.latest) {
        let line = format!("Latest Copper {v} · {}", published(&latest.published_at));
        return match st.error {
            None => line,
            Some(_) => line + " · couldn’t check again just now",
        };
    }
    if let Some(e) = &st.error {
        return e.clone();
    }
    if st.latest.is_none() {
        return "Not checked yet — Copper looks every six hours on its own".into();
    }
    "Up to date".into()
}

impl Page for Updates {
    fn id(&self) -> &'static str {
        "updates"
    }
    fn title(&self) -> &'static str {
        "Updates"
    }
    fn blurb(&self) -> &'static str {
        "Which Copper you have, and getting the next one."
    }
    fn glyph(&self) -> char {
        glyph::UPDATE
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let st = updates::status::<Win>();
        let newer = updates::available::<Win>();
        let mut f = Form::default();
        let sec = f.section("This Copper");
        sec.pills(
            &format!("You have {}", updates::version()),
            Some(&latest_line(&st, newer.as_deref())),
            vec![Pill {
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
        )
        .anchor("updates.check")
        .keywords(&["version", "check for updates"]);
        if let Some(v) = &newer {
            sec.button(
                &format!("Copper {v} is out"),
                Some("Download the Windows installer from Copper's releases page and run it — your tabs and settings stay"),
                "download",
                "Download",
                true,
            )
            .anchor("updates.install");
            if let Some(notes) = st
                .latest
                .as_ref()
                .and_then(|l| l.notes.as_deref())
                .filter(|n| !n.trim().is_empty())
            {
                sec.note(&format!("What’s new in {v}"), Some(notes))
                    .anchor("updates.notes");
            }
        }
        crate::addons::updates_section(&mut f);
        let mut note = format!("Updates come from {}", feed_host());
        if let Some(when) = st.checked {
            note += &format!(" · last checked {}", ago(when));
        }
        f.section("").uncarded().footnote(&note);
        f
    }

    fn act(&self, shell: &Shell, action: &str, _value: &str) {
        match action {
            a if a.starts_with("addon-") => crate::addons::updates_act(shell, a),
            "check" => updates::check::<Win>(shell, true),
            "download" => {
                if let Err(e) = crate::sys::shell::open(&updates::release_page::<Win>()) {
                    tracing::warn!("release page: {e}");
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_as_the_mac_writes_them() {
        assert_eq!(published("2026-10-12T22:01:03Z"), "Oct 12, 2026");
        assert_eq!(published("soon"), "soon");
    }
}
