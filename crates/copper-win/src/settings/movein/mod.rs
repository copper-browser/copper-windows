//! Move in (`Fork/Flow/FlowUI.swift`): a sheet of its own, 560 wide,
//! opened from Settings › General or Passwords (Settings goes first). Pick
//! the browser and its profile, tick what comes over — tabs and windows,
//! bookmarks, history, passwords — then Bring it over: each kind waits,
//! works, then arrives or says why it didn't.
//!
//! Passwords come only from a CSV that browser exported: ticking them
//! shows where its export is (`passwords_csv::export_hint`) before the
//! file is chosen, and Bring it over waits for the file. What Copper for
//! Windows can't bring yet (passkeys, sign-ins, site data, extensions) is
//! said once, faded. When tabs or bookmarks came from Chrome, the finished
//! sheet offers "Open the Chrome guide" (Mac Flow; shell-core's
//! `canvas::land_chrome_canvas`).
//!
//! The body is a Settings [`Form`] drawn with Settings' own pieces, so it
//! reads as the same thing.

mod body;
mod run;

use super::draw::{self, Drawn, Hit};
use super::form::{Control, act_of};
use super::look::Inks;
use crate::chrome::glyph;
use crate::panels::{self, Panel, plate::Plate};
use crate::platform::Shell;
use copper_import::passwords_csv::{PasswordsVia, passwords_via};
use copper_import::{Browser, ProfileInfo};
use copper_shell_core::forms::rt;
use copper_win_ui::win::Ui;
use copper_win_ui::{Menu, NodeId, Point, Token, Trim};
use run::{Choice, Kind, Step};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

/// The panel's id: `show-movein`.
pub const ID: &str = "movein";
/// The "page" its form is drawn as.
const PAGE: &str = "movein";
/// Its popup chooser (Settings' menus are `40..=49`).
const MENU: u32 = 41;
/// The body's height at most, under the lead and over the foot.
const BODY_MAX: f32 = 470.0;

/// Where the sheet is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Choose,
    Running,
    Done,
}

/// What the sheet holds between draws.
#[derive(Default)]
struct State {
    /// The browsers with a profile, each with its profiles; `None` until
    /// looked for.
    browsers: Option<Vec<(Browser, Vec<ProfileInfo>)>>,
    looking: bool,
    chosen: usize,
    profile: Option<String>,
    /// Ticked kinds, in [`Kind::ALL`] order.
    ticked: [bool; 4],
    csv: Option<PathBuf>,
    phase: Phase,
    steps: Vec<(Kind, Step)>,
    /// Tabs or bookmarks came from Chrome: "Open the Chrome guide".
    guide: Guide,
}

/// The Chrome guide's button on the finished sheet (Mac Flow).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Guide {
    #[default]
    None,
    Offered,
    Opening,
}

#[derive(Clone, Copy)]
struct Nodes {
    plate: Plate,
    list: NodeId,
    foot: NodeId,
}

/// The Move-in sheet.
#[derive(Default)]
pub struct MoveIn {
    nodes: Cell<Option<Nodes>>,
    st: RefCell<State>,
    drawn: RefCell<Drawn>,
    foot: RefCell<Vec<(NodeId, &'static str)>>,
    menu: RefCell<Option<Control>>,
}

thread_local! {
    static SHEET: RefCell<Option<Rc<MoveIn>>> = const { RefCell::new(None) };
}

/// Register the sheet.
pub fn register(shell: &Shell) {
    let m = Rc::new(MoveIn::default());
    SHEET.with(|s| *s.borrow_mut() = Some(m.clone()));
    shell.view().panels().add(shell, m);
}

/// Show it.
pub fn open(shell: &Shell) {
    panels::show(shell, ID);
}

/// The sheet, shown or not: a run carries on after it is closed, and is
/// shown as it stands when it opens again.
fn sheet(_shell: &Shell) -> Option<Rc<MoveIn>> {
    SHEET.with(|s| s.borrow().clone())
}

impl MoveIn {
    /// Look for the browsers and their profiles, off the UI thread.
    fn look(&self, shell: &Shell) {
        if std::mem::replace(&mut self.st.borrow_mut().looking, true) {
            return;
        }
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            let found = rt::blocking(|| {
                copper_import::config_dir()
                    .map(|c| copper_import::detect(&c))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|b| {
                        let p = copper_import::profile_infos(&b.root);
                        (b, p)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            let Some(s) = weak.upgrade() else { return };
            if let Some(m) = sheet(&s) {
                let mut st = m.st.borrow_mut();
                st.looking = false;
                st.browsers = Some(found);
            }
            panels::refresh(&s, ID);
        });
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        match action {
            "browser" => {
                let mut st = self.st.borrow_mut();
                st.chosen = value.parse().unwrap_or(0);
                st.profile = None;
                st.csv = None;
            }
            "profile" => {
                self.st.borrow_mut().profile = (value != "*").then(|| value.to_string());
            }
            "copy-page" => {
                let page = {
                    let st = self.st.borrow();
                    st.browser().and_then(|(b, _)| match passwords_via(b) {
                        PasswordsVia::Export(h) => Some(h.page),
                        PasswordsVia::Profile => None,
                    })
                };
                if let Some(p) = page {
                    shell.view().copy_text(p);
                    shell.toast("Address copied — paste it into that browser");
                }
            }
            "choose" => self.choose(shell),
            "csv-only" => {
                shell.view().panels().hide(shell);
                crate::settings::csv::import(shell, |s, said| s.toast(&said.0));
                return;
            }
            _ => {
                if let Some(i) = action
                    .strip_prefix("kind:")
                    .and_then(|i| i.parse::<usize>().ok())
                    && i < 4
                {
                    self.st.borrow_mut().ticked[i] = super::pages::on(value);
                }
            }
        }
        self.refresh(shell);
    }

    /// Ask for the exported CSV.
    fn choose(&self, shell: &Shell) {
        let Some(owner) = super::dialog::owner(shell) else {
            return;
        };
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            let path = super::dialog::ask(
                owner,
                super::dialog::Ask::OpenCsv {
                    title: "The passwords file the browser exported".into(),
                },
            )
            .await;
            let (Some(path), Some(s)) = (path, weak.upgrade()) else {
                return;
            };
            if let Some(m) = sheet(&s) {
                m.st.borrow_mut().csv = Some(path);
            }
            panels::refresh(&s, ID);
        });
    }

    /// Bring it over: each ticked kind in turn.
    fn start(&self, shell: &Shell) {
        let choice = {
            let mut st = self.st.borrow_mut();
            if !st.ready() || st.phase != Phase::Choose {
                return;
            }
            let Some((browser, _)) = st.browser().cloned() else {
                return;
            };
            let kinds = st.kinds();
            st.phase = Phase::Running;
            st.steps = kinds.iter().map(|k| (*k, Step::Waiting)).collect();
            Choice {
                browser,
                profile: st.profile.clone(),
                kinds,
                csv: st.csv.clone(),
            }
        };
        self.refresh(shell);
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            let mut came = 0;
            for kind in choice.kinds.clone() {
                let Some(s) = weak.upgrade() else { return };
                set_step(&s, kind, Step::Working);
                let (step, n) = run::one(&s, &choice, kind).await;
                came += n;
                set_step(&s, kind, step);
            }
            if let Some(s) = weak.upgrade()
                && let Some(m) = sheet(&s)
            {
                let mut st = m.st.borrow_mut();
                st.phase = Phase::Done;
                if choice.browser.name == "Chrome" && came > 0 {
                    st.guide = Guide::Offered;
                }
                drop(st);
                panels::refresh(&s, ID);
                s.toast("Moved in");
            }
        });
    }
}

/// "Open the Chrome guide": land the "Switching from Chrome" canvas
/// (Linux `forms/import.rs`), then put the sheet away.
fn open_guide(shell: &Shell) {
    let Some(m) = sheet(shell) else { return };
    if m.st.borrow().guide != Guide::Offered {
        return;
    }
    m.st.borrow_mut().guide = Guide::Opening;
    panels::refresh(shell, ID);
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let Some(s) = weak.upgrade() else { return };
        let landed = copper_shell_core::canvas::land_chrome_canvas(&s).await;
        tracing::info!(?landed, "move in: chrome guide");
        let Some(m) = sheet(&s) else { return };
        if landed.is_some() {
            m.st.borrow_mut().guide = Guide::None;
            if s.view().panels().showing() == Some(ID) {
                s.view().panels().hide(&s);
            }
        } else {
            m.st.borrow_mut().guide = Guide::Offered;
            panels::refresh(&s, ID);
            s.toast("Couldn't open the Chrome guide");
        }
    });
}

/// One kind moved on.
fn set_step(shell: &Shell, kind: Kind, step: Step) {
    if let Some(m) = sheet(shell) {
        if let Some(s) = m.st.borrow_mut().steps.iter_mut().find(|(k, _)| *k == kind) {
            s.1 = step;
        }
        panels::refresh(shell, ID);
    }
}

impl Panel for MoveIn {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> String {
        "Move in".into()
    }

    fn glyph(&self) -> char {
        glyph::DOWNLOAD
    }

    fn width(&self) -> f32 {
        560.0
    }

    fn build(&self, ui: &mut Ui, p: &Plate) {
        ui.tree.add(
            p.body,
            copper_win_ui::Node::text(
                "Bring your tabs, bookmarks and history over from Chrome, Edge, Brave, Vivaldi or Arc — and passwords from a CSV it exports. Nothing in the other browser changes.",
                13.0,
            )
            .ink(Token::Muted)
            .trim(Trim::Wrap),
        );
        let list = ui.tree.add(
            p.body,
            copper_win_ui::Node::scroll()
                .max_height(BODY_MAX)
                .gap(18.0)
                .shrink(1.0),
        );
        let foot = ui.tree.add(
            p.foot,
            copper_win_ui::Node::row()
                .gap(8.0)
                .grow(1.0)
                .justify(copper_win_ui::Justify::End),
        );
        p.show_foot(ui, true);
        self.nodes.set(Some(Nodes {
            plate: *p,
            list,
            foot,
        }));
        // A run still under way when the sheet was closed carries on and
        // is shown as it stands; otherwise it starts afresh.
        if self.st.borrow().phase != Phase::Running {
            *self.st.borrow_mut() = State {
                ticked: [true, true, true, false],
                ..State::default()
            };
        }
    }

    fn refresh(&self, shell: &Shell) {
        let Some(n) = self.nodes.get() else { return };
        if self.st.borrow().browsers.is_none() {
            self.look(shell);
        }
        let (form, phase, ready, guide) = {
            let st = self.st.borrow();
            (st.form(), st.phase, st.ready(), st.guide)
        };
        let ink = Inks::new(shell.is_dark());
        let made = shell.view().chrome().read(move |ui, _| {
            let mut drawn = Drawn::default();
            ui.tree.clear_children(n.list);
            draw::form(ui, n.list, PAGE, &form, &ink, &mut drawn);
            ui.tree.clear_children(n.foot);
            let mut foot = Vec::new();
            match phase {
                Phase::Choose => {
                    foot.push((
                        super::controls::pill(ui, n.foot, "Cancel", false, true),
                        "close",
                    ));
                    foot.push((
                        super::controls::pill(ui, n.foot, "Bring it over", true, ready),
                        "start",
                    ));
                }
                Phase::Running => {
                    super::controls::pill(ui, n.foot, "Bringing it over…", true, false);
                }
                Phase::Done => {
                    if guide != Guide::None {
                        let opening = guide == Guide::Opening;
                        let label = if opening {
                            "Opening the guide…"
                        } else {
                            "Open the Chrome guide"
                        };
                        foot.push((
                            super::controls::pill(ui, n.foot, label, false, !opening),
                            "guide",
                        ));
                    }
                    foot.push((
                        super::controls::pill(ui, n.foot, "Done", true, true),
                        "close",
                    ));
                }
            }
            n.plate.show_foot(ui, true);
            (drawn, foot)
        });
        if let Some((drawn, foot)) = made {
            *self.drawn.borrow_mut() = drawn;
            *self.foot.borrow_mut() = foot;
        }
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        let foot = self
            .foot
            .borrow()
            .iter()
            .find(|(f, _)| *f == id)
            .map(|(_, a)| *a);
        match foot {
            Some("close") => {
                shell.view().panels().hide(shell);
                return true;
            }
            Some("guide") => {
                open_guide(shell);
                return true;
            }
            Some(_) => {
                self.start(shell);
                return true;
            }
            None => {}
        }
        let hit = self
            .drawn
            .borrow()
            .hits
            .iter()
            .find(|(h, _)| *h == id)
            .map(|(_, h)| h.clone());
        match hit {
            Some(Hit::Act(_, control, option)) => {
                if let Some((action, value)) = act_of(&control, option) {
                    self.act(shell, &action, &value);
                }
                true
            }
            Some(Hit::Pick(_, control)) => {
                let Control::Pick {
                    options, chosen, ..
                } = &control
                else {
                    return true;
                };
                let mut menu = Menu::new();
                for (i, (value, label)) in options.iter().enumerate() {
                    menu = menu.item(i as u32, label).checked(value == chosen);
                }
                *self.menu.borrow_mut() = Some(control.clone());
                shell.view().chrome().read(move |ui, _| {
                    let r = ui.tree.window_rect(id);
                    ui.show_menu(MENU, &menu, Point::new(r.x, r.y + r.h + 4.0));
                });
                true
            }
            _ => false,
        }
    }

    fn menu(&self, shell: &Shell, menu: u32, item: u32) -> bool {
        if menu != MENU {
            return false;
        }
        if let Some(control) = self.menu.borrow_mut().take()
            && let Some((action, value)) = act_of(&control, Some(item as usize))
        {
            self.act(shell, &action, &value);
        }
        true
    }

    fn closed(&self) {
        self.nodes.set(None);
        *self.drawn.borrow_mut() = Drawn::default();
        self.foot.borrow_mut().clear();
        self.menu.borrow_mut().take();
    }
}
