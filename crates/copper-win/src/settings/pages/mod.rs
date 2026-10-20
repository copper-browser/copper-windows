//! The pages Settings opens with, in the Mac's words (`SettingsIndex.swift`
//! blurbs, `Settings.swift` rows). The rail places them by id
//! (`settings::index::GROUPS`), so the order here doesn't matter.
//!
//! One file per page so each can be owned and changed on its own. A page
//! another worker adds is one `mod` line and one `vec!` line here.
//!
//! What Copper for Windows can't do yet is said, faded, with no control
//! ([`not_yet`]) — never a switch that does nothing.

use super::Page;
use super::form::Section;
use crate::platform::Shell;
use std::rc::Rc;

mod about;
mod agents;
mod cloud;
mod downloads;
mod extensions;
mod general;
mod intelligence;
mod labs;
mod passwords;
mod privacy;
mod spaces;
mod tabs;
mod updates;
mod voice;

/// Every page.
pub fn all() -> Vec<Rc<dyn Page>> {
    vec![
        Rc::new(general::General),
        Rc::new(tabs::Tabs),
        Rc::new(spaces::Spaces),
        Rc::new(downloads::Downloads),
        Rc::new(extensions::Extensions),
        Rc::new(intelligence::Intelligence),
        Rc::new(voice::Voice),
        Rc::new(passwords::Passwords),
        Rc::new(privacy::Privacy),
        Rc::new(cloud::Cloud),
        Rc::new(agents::Agents),
        Rc::new(labs::Labs),
        Rc::new(updates::Updates),
        Rc::new(about::About),
    ]
}

/// What the pages put in place when Copper starts, before Settings is ever
/// opened (the downloads folder chosen last time).
pub fn start(shell: &Shell) {
    downloads::start(shell);
}

/// A switch setting's stored value as a bool.
pub(super) fn on(v: &str) -> bool {
    v == "on"
}

/// A row for something Copper for Windows can't do yet: its title, why,
/// faded and with nothing to press.
pub(super) fn not_yet<'a>(sec: &'a mut Section, title: &str, detail: &str) -> &'a mut Section {
    sec.note(title, Some(detail)).disabled(true)
}

pub(super) use super::count;

/// Draw page `id` again once something it shows has changed (an answer
/// from a vault, a dialog, the network).
pub(super) fn redraw(shell: &Shell, id: &str) {
    super::changed(shell, id);
}
