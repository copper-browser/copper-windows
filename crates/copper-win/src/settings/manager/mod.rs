//! The Passwords panel (`Passwords.swift`): every sign-in Copper keeps in
//! Windows Credential Manager, by site — opened from Settings › Passwords ›
//! Your passwords, or the palette. A 620-wide plate: search and Add at the
//! head, the sites in a list at most 400 high, and a foot pinned under it
//! for bringing passwords in (each browser's own CSV export, or any CSV)
//! and exporting them.
//!
//! A password is read from Credential Manager only when Show or Copy is
//! clicked, never to list; a shown one hides again after 15 seconds, when
//! the panel closes, or on Hide. Credential Manager asks for nothing —
//! the sign-ins are your Windows account's — so nothing here claims a
//! Windows Hello check it doesn't make.

mod acts;
mod foot;
mod rows;

use super::sites::{self, host_of};
use crate::chrome::glyph;
use crate::panels::{
    self, Panel,
    plate::{self, Plate},
};
use crate::platform::Shell;
use copper_creds::Login;
use copper_shell_core::forms::{self, rt};
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Event, Node, NodeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// The panel's id: `show-passwords`.
pub const ID: &str = "passwords";
/// How long a shown password stays shown.
const SHOWN_FOR: Duration = Duration::from_secs(15);

/// What a click in the panel means.
#[derive(Clone, Debug)]
pub(super) enum Hit {
    Toggle(String),
    Show(Login),
    Hide,
    Copy(Login),
    Remove(Login),
    Add,
    Save,
    /// The foot's own ([`foot::Hit`]).
    Foot(foot::Hit),
}

#[derive(Clone, Copy)]
struct Nodes {
    plate: Plate,
    hunt: NodeId,
    add: NodeId,
    form: NodeId,
    site: NodeId,
    user: NodeId,
    password: NodeId,
    save: NodeId,
    list: NodeId,
    foot: NodeId,
}

/// A password on show: whose, what, and which Show it came from.
struct Shown {
    host: String,
    user: String,
    secret: String,
    ticket: u64,
}

/// What the panel holds between draws.
#[derive(Default)]
struct State {
    /// Credential Manager's sign-ins; `None` until read.
    logins: Option<Result<Vec<Login>, String>>,
    reading: bool,
    open: Option<String>,
    adding: bool,
    site: String,
    user: String,
    password: String,
    saving: bool,
    shown: Option<Shown>,
    ticket: u64,
    /// A failure to show, copy, remove or save, said in the list.
    error: Option<String>,
}

/// The Passwords panel.
#[derive(Default)]
pub struct Manager {
    nodes: Cell<Option<Nodes>>,
    hits: RefCell<HashMap<NodeId, Hit>>,
    st: RefCell<State>,
    foot: RefCell<foot::State>,
}

/// Register the panel.
pub fn register(shell: &Shell) {
    let m = Rc::new(Manager::default());
    MANAGER.with(|g| *g.borrow_mut() = Some(m.clone()));
    shell.view().panels().add(shell, m);
}

/// Show it (Settings goes first: one plate at a time).
pub fn open(shell: &Shell) {
    panels::show(shell, ID);
}

/// Read the list again, then draw it.
fn reload(shell: &Shell) {
    let Some(m) = manager(shell) else { return };
    {
        let mut st = m.st.borrow_mut();
        if st.reading {
            return;
        }
        st.reading = true;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let read = match forms::vault() {
            Some(v) => rt::run(async move { v.logins().await })
                .await
                .unwrap_or_else(|| Err(acts::UNREACHABLE.into())),
            None => Err(acts::UNREACHABLE.into()),
        };
        let Some(s) = weak.upgrade() else { return };
        if let Some(m) = manager(&s) {
            let mut st = m.st.borrow_mut();
            st.reading = false;
            st.logins = Some(read);
        }
        panels::refresh(&s, ID);
    });
}

/// The panel, when it is the one on show.
fn manager(shell: &Shell) -> Option<Rc<Manager>> {
    MANAGER
        .with(|m| m.borrow().clone())
        .filter(|_| shell.view().panels().showing() == Some(ID))
}

thread_local! {
    static MANAGER: RefCell<Option<Rc<Manager>>> = const { RefCell::new(None) };
}

impl Manager {
    fn text(shell: &Shell, id: NodeId) -> String {
        shell
            .view()
            .chrome()
            .read(move |ui, _| ui.field_text(id).map(str::to_string))
            .flatten()
            .unwrap_or_default()
    }

    /// The head's Add/Cancel and the form's Save, which change as it is
    /// typed in without redrawing the boxes.
    fn controls(&self, shell: &Shell) {
        let Some(n) = self.nodes.get() else { return };
        let (adding, ready) = {
            let st = self.st.borrow();
            (
                st.adding,
                !host_of(&st.site).is_empty() && !st.password.is_empty() && !st.saving,
            )
        };
        let made = shell.view().chrome().read(move |ui, _| {
            ui.tree.clear_children(n.add);
            let add = super::controls::pill(
                ui,
                n.add,
                if adding { "Cancel" } else { "Add" },
                !adding,
                true,
            );
            ui.tree.set_hidden(n.form, !adding);
            ui.tree.clear_children(n.save);
            let save = super::controls::pill(ui, n.save, "Save", true, ready);
            (add, save)
        });
        if let Some((add, save)) = made {
            let mut h = self.hits.borrow_mut();
            h.retain(|_, v| !matches!(v, Hit::Add | Hit::Save));
            h.insert(add, Hit::Add);
            h.insert(save, Hit::Save);
        }
    }
}

impl Panel for Manager {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> String {
        "Passwords".into()
    }

    fn glyph(&self) -> char {
        glyph::KEY
    }

    fn width(&self) -> f32 {
        620.0
    }

    fn build(&self, ui: &mut Ui, p: &Plate) {
        let head = ui
            .tree
            .add(p.body, Node::row().gap(10.0).align(Align::Center));
        let hunt_box = ui.tree.add(head, Node::column().grow(1.0).shrink(1.0));
        let hunt = plate::hunt(ui, hunt_box, "Search sites and accounts");
        let add = ui.tree.add(head, Node::row().shrink(0.0));
        let form = ui.tree.add(p.body, Node::column().hidden(true));
        let card = plate::card(ui, form);
        let fields = ui.tree.add(
            card,
            Node::column()
                .gap(8.0)
                .padding(copper_win_ui::Insets::all(14.0)),
        );
        let field = |ui: &mut Ui, name: &str, secret: bool| {
            super::controls::field(ui, fields, name, "", name, secret, None, true)
        };
        let site = field(ui, "Site", false);
        let user = field(ui, "Username", false);
        let password = field(ui, "Password", true);
        let save = ui
            .tree
            .add(fields, Node::row().justify(copper_win_ui::Justify::End));
        let list = ui
            .tree
            .add(p.body, Node::scroll().max_height(400.0).shrink(1.0));
        let foot = ui
            .tree
            .add(p.foot, Node::column().gap(10.0).grow(1.0).shrink(1.0));
        p.show_foot(ui, true);
        ui.focus(Some(hunt));
        self.nodes.set(Some(Nodes {
            plate: *p,
            hunt,
            add,
            form,
            site,
            user,
            password,
            save,
            list,
            foot,
        }));
        *self.st.borrow_mut() = State::default();
        *self.foot.borrow_mut() = foot::State::default();
        self.hits.borrow_mut().clear();
    }

    fn refresh(&self, shell: &Shell) {
        let Some(n) = self.nodes.get() else { return };
        if self.st.borrow().logins.is_none() {
            reload(shell);
        }
        foot::detect(shell);
        self.controls(shell);
        let hunt = Self::text(shell, n.hunt);
        let st = self.st.borrow();
        let logins = st.logins.clone();
        let open = st.open.clone();
        let error = st.error.clone();
        let shown = st
            .shown
            .as_ref()
            .map(|x| (x.host.clone(), x.user.clone(), x.secret.clone()));
        drop(st);
        let foot_model = self.foot.borrow().model(&logins);
        let made = shell.view().chrome().read(move |ui, _| {
            let mut made = Vec::new();
            ui.tree.clear_children(n.list);
            let card = plate::card(ui, n.list);
            if let Some(e) = &error {
                rows::note(ui, card, e);
                plate::rule(ui, card);
            }
            match &logins {
                None => {
                    rows::note(ui, card, "Reading Credential Manager…");
                }
                Some(Err(e)) => {
                    rows::note(ui, card, &format!("Credential Manager couldn't be read: {e}"));
                }
                Some(Ok(all)) => {
                    let found = sites::sites(all, &hunt);
                    if found.is_empty() {
                        rows::note(
                            ui,
                            card,
                            if all.is_empty() {
                                "Nothing kept yet. Sign in somewhere and say yes, or bring yours in below."
                            } else {
                                "Nothing matches."
                            },
                        );
                    }
                    let shown = shown.as_ref().map(|(h, u, s)| rows::Shown {
                        host: h,
                        user: u,
                        secret: s,
                    });
                    for (i, s) in found.iter().enumerate() {
                        if i > 0 {
                            plate::rule(ui, card);
                        }
                        rows::site(ui, card, s, open.as_deref() == Some(&s.host), shown.as_ref(), &mut made);
                    }
                }
            }
            ui.tree.clear_children(n.foot);
            for (id, h) in foot::draw(ui, n.foot, &foot_model) {
                made.push((id, Hit::Foot(h)));
            }
            n.plate.show_foot(ui, true);
            made
        });
        if let Some(made) = made {
            let mut h = self.hits.borrow_mut();
            h.retain(|_, v| matches!(v, Hit::Add | Hit::Save));
            h.extend(made);
        }
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        let Some(hit) = self.hits.borrow().get(&id).cloned() else {
            return false;
        };
        match hit {
            Hit::Toggle(host) => {
                let mut st = self.st.borrow_mut();
                st.open = if st.open.as_deref() == Some(&host) {
                    None
                } else {
                    Some(host)
                };
                st.shown = None;
            }
            Hit::Show(l) => self.show(shell, l),
            Hit::Hide => self.st.borrow_mut().shown = None,
            Hit::Copy(l) => self.copy(shell, l),
            Hit::Remove(l) => self.remove(shell, l),
            Hit::Add => {
                self.toggle_add(shell);
                return true;
            }
            Hit::Save => self.save(shell),
            Hit::Foot(h) => foot::click(shell, h),
        }
        self.refresh(shell);
        true
    }

    fn field(&self, shell: &Shell, id: NodeId, e: &Event) -> bool {
        let Some(n) = self.nodes.get() else {
            return false;
        };
        if id == n.hunt {
            match e {
                Event::FieldChanged(_) => self.refresh(shell),
                Event::FieldCancel(_) => {
                    shell.view().panels().hide(shell);
                }
                _ => {}
            }
            return true;
        }
        if ![n.site, n.user, n.password].contains(&id) {
            return false;
        }
        match e {
            Event::FieldChanged(_) => {
                let text = Self::text(shell, id);
                let mut st = self.st.borrow_mut();
                if id == n.site {
                    st.site = text;
                } else if id == n.user {
                    st.user = text;
                } else {
                    st.password = text;
                }
                drop(st);
                self.controls(shell);
            }
            Event::FieldSubmit(_) => self.save(shell),
            Event::FieldCancel(_) => self.toggle_add(shell),
            _ => {}
        }
        true
    }

    fn closed(&self) {
        self.nodes.set(None);
        self.hits.borrow_mut().clear();
        // Nothing typed or shown outlives the panel.
        *self.st.borrow_mut() = State::default();
        *self.foot.borrow_mut() = foot::State::default();
    }
}
