//! Favicons as the window's pictures.
//!
//! The shell hands the view decoded icons ([`Icon`], shared); the toolkit
//! draws pictures by number. Each distinct icon gets a number once, its
//! pixels are given to the window once, and the number is retired when
//! the shell lets go of the icon.

use crate::platform::Icon;
use copper_win_ui::win::Ui;
use std::rc::{Rc, Weak};

/// The numbers handed out, by icon.
#[derive(Default)]
pub struct Images {
    live: Vec<(Weak<crate::picture::Picture>, u32)>,
    next: u32,
}

impl Images {
    /// `icon`'s number, giving its pixels to the window the first time.
    pub fn id(&mut self, ui: &mut Ui, icon: &Icon) -> u32 {
        if let Some((_, id)) = self
            .live
            .iter()
            .find(|(w, _)| std::ptr::eq(w.as_ptr(), Rc::as_ptr(icon)))
        {
            return *id;
        }
        self.sweep(ui);
        self.next = self.next.wrapping_add(1).max(1);
        let id = self.next;
        ui.set_image(id, icon.bgra.clone(), icon.w, icon.h);
        self.live.push((Rc::downgrade(icon), id));
        id
    }

    /// Retire the numbers of icons nobody holds any more.
    pub fn sweep(&mut self, ui: &mut Ui) {
        self.live.retain(|(w, id)| {
            let alive = w.strong_count() > 0;
            if !alive {
                ui.remove_image(*id);
            }
            alive
        });
    }
}
