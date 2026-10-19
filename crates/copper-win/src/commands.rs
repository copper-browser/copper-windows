//! The command registry (Linux `actions.rs`): every keyboard shortcut and
//! every palette command is a named command run with the shell. The
//! shortcuts themselves are [`crate::keys`]'s table; this holds what each
//! id does, its name and glyph for the palette, and when it is offered.
//!
//! Features add their own ([`Registry::add`]); the palette lists those
//! marked for it; [`run`] runs any by id.

use crate::chrome::glyph;
use crate::platform::Shell;
use copper_shell_core::palette::complete::CommandRow;
use std::cell::RefCell;
use std::rc::Rc;

type Run = Rc<dyn Fn(&Shell)>;
type When = Rc<dyn Fn(&Shell) -> bool>;

/// One command.
#[derive(Clone)]
pub struct Command {
    pub id: String,
    /// What the palette calls it; empty: not in the palette.
    pub name: String,
    pub glyph: char,
    run: Run,
    when: Option<When>,
}

impl Command {
    /// A command the palette lists as `name`.
    pub fn new(id: &str, name: &str, glyph: char, run: impl Fn(&Shell) + 'static) -> Command {
        Command {
            id: id.to_string(),
            name: name.to_string(),
            glyph,
            run: Rc::new(run),
            when: None,
        }
    }

    /// A command only a key runs (not listed in the palette).
    pub fn key(id: &str, run: impl Fn(&Shell) + 'static) -> Command {
        Command::new(id, "", glyph::GEAR, run)
    }

    /// Offered (and run) only while `f` says so.
    pub fn when(mut self, f: impl Fn(&Shell) -> bool + 'static) -> Command {
        self.when = Some(Rc::new(f));
        self
    }

    pub fn available(&self, shell: &Shell) -> bool {
        self.when.as_ref().is_none_or(|w| w(shell))
    }
}

/// Every command, by id (later registrations replace earlier ones).
#[derive(Default)]
pub struct Registry {
    list: RefCell<Vec<Command>>,
}

impl Registry {
    pub fn add(&self, c: Command) {
        let mut l = self.list.borrow_mut();
        match l.iter_mut().find(|x| x.id == c.id) {
            Some(x) => *x = c,
            None => l.push(c),
        }
    }

    pub fn get(&self, id: &str) -> Option<Command> {
        self.list.borrow().iter().find(|c| c.id == id).cloned()
    }

    /// The palette's commands, as it ranks them.
    pub fn palette_rows(&self, shell: &Shell) -> Vec<CommandRow> {
        let list = self.list.borrow().clone();
        list.into_iter()
            .filter(|c| !c.name.is_empty() && c.available(shell))
            .map(|c| CommandRow {
                shortcut: crate::keys::shortcut(&c.id),
                icon: c.glyph.to_string(),
                name: c.name,
                id: c.id,
            })
            .collect()
    }

    /// The glyph for command `id` (the palette's row mark).
    pub fn glyph(&self, id: &str) -> Option<char> {
        self.list
            .borrow()
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.glyph)
    }
}

/// Run command `id`; `false` if there is none (or it is not available).
/// Never called with the window's state borrowed.
pub fn run(shell: &Shell, id: &str) -> bool {
    let Some(c) = shell.view().commands().get(id) else {
        tracing::debug!("no command {id}");
        return false;
    };
    if !c.available(shell) {
        return false;
    }
    let started = std::time::Instant::now();
    (c.run)(shell);
    tracing::debug!(target: "copper::perf", "{id}: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
    true
}
