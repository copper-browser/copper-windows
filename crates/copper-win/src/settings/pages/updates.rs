//! Settings › About › Updates (`SettingsFork.swift` `UpdatesPage`; a page
//! of its own until Copper#115): which Copper this is, and the next one. The check is copper-shell-core's `updates` lane (every six
//! hours, and on Check now). The installed copy downloads and verifies the
//! new one itself (`self_update`) and offers Restart to Update; a portable
//! copy is offered the installer on the releases page.

use crate::platform::{Shell, Win};
use crate::settings::form::{Form, Pill};
use copper_shell_core::updates;

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

/// The Updates section, and addons waiting for theirs, on `f`.
pub(super) fn section(f: &mut Form) {
    let st = updates::status::<Win>();
    let newer = updates::available::<Win>();
    let sec = f.section("Updates");
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
    let me = crate::self_update::status();
    if let Some(ready) = &me.ready {
        sec.button(
            &format!("Copper {ready} is ready"),
            Some("Copper restarts and your tabs come back. Quitting installs it too"),
            "restart-update",
            "Restart to Update",
            true,
        )
        .anchor("updates.install");
    } else if let Some(v) = newer.as_ref().filter(|_| me.enabled && me.busy) {
        sec.note(&format!("Downloading Copper {v}…"), None)
            .anchor("updates.install");
    } else if let Some(v) = &newer {
        let detail = match &me.error {
            Some(e) if me.enabled => format!(
                "Couldn’t get it ready: {e}. Check now tries again, or download the installer and run it"
            ),
            _ => "Download the Windows installer from Copper's releases page and run it — your tabs and settings stay".to_owned(),
        };
        sec.button(
            &format!("Copper {v} is out"),
            Some(&detail),
            "download",
            "Download",
            true,
        )
        .anchor("updates.install");
    }
    if let Some(v) = &newer
        && let Some(notes) = st
            .latest
            .as_ref()
            .and_then(|l| l.notes.as_deref())
            .filter(|n| !n.trim().is_empty())
    {
        sec.note(&format!("What’s new in {v}"), Some(notes))
            .anchor("updates.notes");
    }
    if let Some(o) = &me.outcome {
        let when = ago(o.at.unix());
        let (title, detail) = if o.ok {
            (format!("Updated to Copper {}", o.detail), when)
        } else {
            (
                "The last update didn’t install".to_owned(),
                format!("{} · {when}", o.detail),
            )
        };
        sec.note(&title, Some(&detail)).anchor("updates.outcome");
    }
    let mut note = format!("Updates come from {}", feed_host());
    if let Some(when) = st.checked {
        note += &format!(" · last checked {}", ago(when));
    }
    crate::addons::updates_section(f);
    f.section("").uncarded().footnote(&note);
}

/// A control in the section was used; `false` if `action` isn't one.
pub(super) fn act(shell: &Shell, action: &str) -> bool {
    match action {
        a if a.starts_with("addon-") => crate::addons::updates_act(shell, a),
        "check" => updates::check::<Win>(shell, true),
        "restart-update" => crate::self_update::restart(shell),
        "download" => {
            if let Err(e) = crate::sys::shell::open(&updates::release_page::<Win>()) {
                tracing::warn!("release page: {e}");
            }
        }
        _ => return false,
    }
    true
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
