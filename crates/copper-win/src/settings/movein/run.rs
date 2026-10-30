//! Bringing a browser's things over (`Fork/Flow/*`; Linux
//! `forms/import.rs` `run`), one kind at a time: bookmarks merged into
//! Copper's, history taken in, each open window as a new space of sleeping
//! tabs, and passwords from the CSV the person exported and chose.
//!
//! The other browser's files are copied before they are read and never
//! written to (`copper-import`); its password store is never read — on
//! Windows passwords only come from a CSV it exported.

use crate::platform::Shell;
use copper_core::OpenOptions;
use copper_import::{Browser, ImportError, bookmarks, history, tabs};
use copper_shell_core::forms::rt;
use std::path::PathBuf;

/// How many places of history come over at most.
const HISTORY_LIMIT: usize = 50_000;

/// One kind of thing to bring over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tabs,
    Bookmarks,
    History,
    Passwords,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Tabs, Kind::Bookmarks, Kind::History, Kind::Passwords];

    /// Its name on the checklist.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Tabs => "Tabs and windows",
            Kind::Bookmarks => "Bookmarks",
            Kind::History => "History",
            Kind::Passwords => "Passwords",
        }
    }
}

/// Where one kind is.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Waiting,
    Working,
    /// Came over: what, in words.
    Arrived(String),
    /// Didn't: why.
    Missed(String),
}

/// What the person chose.
#[derive(Clone, Debug)]
pub struct Choice {
    pub browser: Browser,
    /// A profile's folder; `None`: every profile.
    pub profile: Option<String>,
    pub kinds: Vec<Kind>,
    /// The passwords CSV chosen (needed for [`Kind::Passwords`]).
    pub csv: Option<PathBuf>,
}

impl Choice {
    fn dirs(&self) -> Vec<PathBuf> {
        match &self.profile {
            Some(p) => vec![self.browser.root.join(p)],
            None => self.browser.profile_dirs(),
        }
    }
}

/// Bring `kind` over; what came of it, and how many tabs or bookmarks
/// came (the Chrome guide is offered when some did).
pub async fn one(shell: &Shell, c: &Choice, kind: Kind) -> (Step, usize) {
    let mut came = 0;
    let step = bring(shell, c, kind, &mut came).await;
    (step, came)
}

async fn bring(shell: &Shell, c: &Choice, kind: Kind, came: &mut usize) -> Step {
    let name = c.browser.name.clone();
    match kind {
        Kind::Bookmarks => {
            let dirs = c.dirs();
            let Some(read) = rt::blocking(move || bookmarks::read(&dirs)).await else {
                return Step::Missed(format!("Couldn't read {name}'s bookmarks"));
            };
            let (n, complete) = (read.count(), read.complete());
            *came = n;
            if !read.nodes.is_empty() {
                let roots = {
                    let mut b = shell.bookmarks_mut();
                    let mut roots = b.roots().to_vec();
                    bookmarks::merge(read.nodes, &mut roots);
                    b.replace(roots.clone());
                    roots
                };
                shell.saver().schedule(roots);
            }
            match (n, complete) {
                (0, false) => Step::Missed(format!("{name}'s bookmarks couldn't be read")),
                (n, true) => Step::Arrived(super::super::count(n, "bookmark", "bookmarks")),
                (n, false) => Step::Arrived(format!(
                    "{} — some files couldn't be read",
                    super::super::count(n, "bookmark", "bookmarks")
                )),
            }
        }
        Kind::History => {
            let dirs = c.dirs();
            let Some(read) = rt::blocking(move || history::read_all(&dirs, HISTORY_LIMIT)).await
            else {
                return Step::Missed(format!("Couldn't read {name}'s history"));
            };
            if !read.places.is_empty() {
                let mut h = shell.history_mut();
                for p in &read.places {
                    p.take_into(&mut h);
                }
                drop(h);
                shell.history_changed();
            }
            if !read.in_use.is_empty() && read.places.is_empty() {
                return Step::Missed(format!(
                    "{name} is in use — close the browser and try again"
                ));
            }
            Step::Arrived(super::super::count(
                read.places.len(),
                "page of history",
                "pages of history",
            ))
        }
        Kind::Tabs => {
            let (root, profile, source) = (c.browser.root.clone(), c.profile.clone(), name.clone());
            let read = rt::blocking(move || match profile {
                Some(p) => tabs::read_profile(&root.join(p), &source),
                None => tabs::read(&root, &source),
            })
            .await
            .unwrap_or_else(|| Err(ImportError::Unreadable("the session".into())));
            match read {
                Ok(spaces) => {
                    let (s, t) = open_spaces(shell, spaces);
                    *came = t;
                    Step::Arrived(format!(
                        "{} in {}",
                        super::super::count(t, "open tab", "open tabs"),
                        super::super::count(s, "new space", "new spaces")
                    ))
                }
                Err(e) => Step::Missed(e.to_string()),
            }
        }
        Kind::Passwords => match &c.csv {
            Some(path) => {
                let (line, worked) = crate::settings::csv::import_file(path.clone()).await;
                if worked {
                    Step::Arrived(line)
                } else {
                    Step::Missed(line)
                }
            }
            None => Step::Missed("No file chosen".into()),
        },
    }
}

/// Each window read becomes a new space of sleeping tabs (only web
/// pages; the browser's own pages stay behind).
fn open_spaces(shell: &Shell, spaces: Vec<tabs::ImportedSpace>) -> (usize, usize) {
    let mut count = (0, 0);
    shell.with_browser(|b| {
        for imp in spaces {
            let real: Vec<&tabs::OpenTab> = imp
                .tabs
                .iter()
                .filter(|t| t.url.starts_with("http://") || t.url.starts_with("https://"))
                .collect();
            if real.is_empty() {
                continue;
            }
            let id = b.add_space(&imp.space.name);
            let profile = imp.space.profile.clone();
            b.update_space(&id, |s| s.profile = profile);
            for t in real {
                let tab = b.open_tab(
                    &t.url,
                    OpenOptions {
                        title: t.title.clone(),
                        space: Some(id.clone()),
                        at_end: true,
                        ..Default::default()
                    },
                );
                b.set_sleeping(&tab, true);
                count.1 += 1;
            }
            count.0 += 1;
        }
    });
    count
}
