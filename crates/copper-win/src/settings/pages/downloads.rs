//! Downloads (`Settings.swift` 467–483, 567–583): where downloaded files
//! go. One card, no caption; the folder is chosen in Windows' own folder
//! dialog, owned by the window, starting at the folder in use.
//!
//! The choice is `downloads` in settings.json, and applies to every
//! profile from the next download on ([`start`] puts it in place at
//! launch).

use super::{not_yet, redraw};
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::dialog::{self, Ask};
use crate::settings::form::{Form, Tone};
use std::path::{Path, PathBuf};

/// Downloads: where files go.
pub(super) struct Downloads;

/// Put the chosen folder in place at launch, when it is still there.
pub(super) fn start(shell: &Shell) {
    let dir = shell.settings().downloads_dir().map(PathBuf::from);
    if let Some(dir) = dir.filter(|d| d.is_dir()) {
        shell.downloads().set_folder(shell, &dir);
    }
}

/// The folder chosen in settings, and where downloads really go.
fn folders(shell: &Shell) -> (Option<PathBuf>, PathBuf) {
    let chosen = shell.settings().downloads_dir().map(PathBuf::from);
    (chosen, shell.downloads().folder(shell))
}

/// Ask Windows for a folder, starting at `start`; keep the one chosen.
fn choose(shell: &Shell, start: &Path) {
    let Some(owner) = dialog::owner(shell) else {
        return;
    };
    let weak = shell.downgrade();
    let start = start.to_path_buf();
    copper_win_rt::spawn_local(async move {
        let picked = dialog::ask(
            owner,
            Ask::Folder {
                title: "Save downloads to".into(),
                start: Some(start),
            },
        )
        .await;
        let (Some(dir), Some(shell)) = (picked, weak.upgrade()) else {
            return;
        };
        let text = dir.to_string_lossy().into_owned();
        shell.update_settings(|s| s.set_downloads_dir(&text));
        shell.downloads().set_folder(&shell, &dir);
        redraw(&shell, "downloads");
    });
}

impl Page for Downloads {
    fn id(&self) -> &'static str {
        "downloads"
    }
    fn title(&self) -> &'static str {
        "Downloads"
    }
    fn blurb(&self) -> &'static str {
        "Where downloaded files go."
    }
    fn glyph(&self) -> char {
        glyph::DOWNLOAD
    }

    fn draw(&self, shell: &Shell) -> Form {
        let (chosen, used) = folders(shell);
        let mut f = Form::default();
        let sec = f.section("");
        match chosen.filter(|c| !c.is_dir()) {
            // The folder chosen went away: downloads still land somewhere.
            Some(gone) => {
                let said = format!(
                    "{} isn't there anymore — files go to {} until you choose another",
                    gone.display(),
                    used.display()
                );
                sec.button("Save to", Some(&said), "change", "Change…", false)
                    .tone(Tone::Error);
            }
            None => {
                sec.button(
                    "Save to",
                    Some(&used.display().to_string()),
                    "change",
                    "Change…",
                    false,
                )
                .mono();
            }
        }
        sec.anchor("downloads.folder")
            .keywords(&["folder", "location", "path"]);
        not_yet(
            sec,
            "Ask where to save each file",
            "Not yet on Windows — every file goes to the folder above",
        )
        .anchor("downloads.ask");
        f
    }

    fn act(&self, shell: &Shell, action: &str, _value: &str) {
        if action == "change" {
            let (_, used) = folders(shell);
            choose(shell, &used);
        }
    }
}
