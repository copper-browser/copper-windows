//! Move in's body as a Settings form: the browser and profile, what
//! comes over (switches before, each kind's progress after), and — when
//! passwords are ticked — where that browser's export is and the file
//! chosen.

use super::run::{Kind, Step};
use super::{Phase, State};
use crate::settings::form::{Form, Tone};
use copper_import::passwords_csv::{PasswordsVia, passwords_via};
use copper_import::{Browser, ProfileInfo};

impl State {
    pub(super) fn browser(&self) -> Option<&(Browser, Vec<ProfileInfo>)> {
        self.browsers.as_ref()?.get(self.chosen)
    }

    pub(super) fn kinds(&self) -> Vec<Kind> {
        Kind::ALL
            .into_iter()
            .zip(self.ticked)
            .filter(|(_, on)| *on)
            .map(|(k, _)| k)
            .collect()
    }

    /// Whether Bring it over can start: something ticked, and the file
    /// when passwords are.
    pub(super) fn ready(&self) -> bool {
        let kinds = self.kinds();
        self.browser().is_some()
            && !kinds.is_empty()
            && (!kinds.contains(&Kind::Passwords) || self.csv.is_some())
    }

    /// The body, as a Settings form.
    pub(super) fn form(&self) -> Form {
        let mut f = Form::default();
        let Some(browsers) = &self.browsers else {
            f.section("").note("Looking for other browsers…", None);
            return f;
        };
        if browsers.is_empty() {
            f.section("From")
                .note(
                    "No other browser found on this PC",
                    Some("Chrome, Edge, Brave, Vivaldi and Arc are looked for under your Windows account"),
                )
                .button(
                    "Passwords from a CSV…",
                    Some("An export from any browser or password manager"),
                    "csv-only",
                    "Choose File…",
                    false,
                );
            return f;
        }
        let running = self.phase != Phase::Choose;
        let from = f.section("From");
        let names: Vec<(String, String)> = browsers
            .iter()
            .enumerate()
            .map(|(i, (b, _))| (i.to_string(), b.name.clone()))
            .collect();
        let pairs: Vec<(&str, &str)> = names
            .iter()
            .map(|(v, l)| (v.as_str(), l.as_str()))
            .collect();
        from.pick("Browser", None, "browser", &pairs, &self.chosen.to_string())
            .disabled(running);
        if let Some((_, profiles)) = self.browser()
            && profiles.len() > 1
        {
            let mut opts = vec![("*".to_string(), "Every profile".to_string())];
            opts.extend(profiles.iter().map(|p| {
                (
                    p.folder.clone(),
                    p.name.clone().unwrap_or_else(|| p.folder.clone()),
                )
            }));
            let pairs: Vec<(&str, &str)> =
                opts.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
            from.pick(
                "Profile",
                None,
                "profile",
                &pairs,
                self.profile.as_deref().unwrap_or("*"),
            )
            .disabled(running);
        }
        let what = f.section("What comes over");
        for (i, kind) in Kind::ALL.into_iter().enumerate() {
            let step = self.steps.iter().find(|(k, _)| *k == kind).map(|(_, s)| s);
            match (self.phase, step) {
                (Phase::Choose, _) => {
                    what.switch(
                        kind.title(),
                        Some(detail(kind)),
                        &format!("kind:{i}"),
                        self.ticked[i],
                    );
                }
                (_, Some(step)) => {
                    let (word, tone) = match step {
                        Step::Waiting => ("Waiting".to_string(), Tone::Plain),
                        Step::Working => ("Bringing it over…".to_string(), Tone::Plain),
                        Step::Arrived(s) => (s.clone(), Tone::Good),
                        Step::Missed(s) => (s.clone(), Tone::Error),
                    };
                    what.note(kind.title(), Some(&word)).tone(tone);
                }
                (_, None) => {
                    what.note(kind.title(), Some("Not chosen")).disabled(true);
                }
            }
        }
        if self.phase == Phase::Choose {
            crate::settings::pages::not_yet(
                what,
                "Passkeys, sign-ins, site data and extensions",
                "Not yet on Windows — they stay where they are",
            );
        }
        if self.phase == Phase::Choose
            && self.ticked[3]
            && let Some((b, _)) = self.browser()
            && let PasswordsVia::Export(hint) = passwords_via(b)
        {
            let pw = f.section("Passwords");
            pw.buttons(
                hint.page,
                Some(hint.steps),
                &[("copy-page", "Copy address", false)],
            )
            .mono();
            let file = self
                .csv
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned());
            pw.button(
                file.as_deref().unwrap_or("The exported file"),
                Some(if file.is_some() {
                    "Read when you bring it over, then left as it is — delete it once you're done"
                } else {
                    "Copper reads only the file you choose"
                }),
                "choose",
                if file.is_some() {
                    "Change…"
                } else {
                    "Choose File…"
                },
                file.is_none(),
            );
        }
        f
    }
}

/// What each kind does, in a line.
fn detail(kind: Kind) -> &'static str {
    match kind {
        Kind::Tabs => "Each window becomes a new space, its tabs asleep until you open them",
        Kind::Bookmarks => "Added beside yours, folders and all",
        Kind::History => "So typed addresses complete as they did there",
        Kind::Passwords => "From a CSV the browser exports — Copper never reads its password store",
    }
}
