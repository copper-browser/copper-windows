//! The pages Settings opens with. General is whole; the others say and
//! set what Copper for Windows does today, in the Mac's words
//! (`SettingsIndex.swift` blurbs).

//!
//! One file per page so each can be owned and changed on its own.

use super::Page;
use std::rc::Rc;

mod addons;
mod agents;
mod cloud;
mod extensions;
mod general;
mod passwords;
mod privacy;
mod updates;

/// Every page, in the rail's order.
pub fn all() -> Vec<Rc<dyn Page>> {
    vec![
        Rc::new(general::General),
        Rc::new(privacy::Privacy),
        Rc::new(passwords::Passwords),
        Rc::new(cloud::Cloud),
        Rc::new(agents::Agents),
        Rc::new(updates::Updates),
        Rc::new(extensions::Extensions),
        Rc::new(addons::Addons),
    ]
}

/// A switch setting's stored value as a bool.
pub(super) fn on(v: &str) -> bool {
    v == "on"
}
