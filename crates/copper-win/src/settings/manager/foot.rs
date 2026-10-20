//! The Passwords panel's foot (`Passwords.swift` foot): bringing passwords
//! in, how many there are, and Export….
//!
//! On the Mac each browser's pill reads that browser's passwords directly.
//! On Windows Copper never reads another browser's password store, so a
//! browser's pill shows where its own export is (`passwords_via` →
//! `export_hint`): the page to paste into that browser and what to do
//! there, then Choose File… for the CSV it wrote. `CSV file…` goes
//! straight to the file — any browser's or password manager's export.

use super::{ID, manager};
use crate::panels;
use crate::platform::Shell;
use crate::settings::csv;
use copper_creds::Login;
use copper_import::passwords_csv::{ExportHint, PasswordsVia, passwords_via};
use copper_shell_core::forms::rt;
use copper_win_ui::win::Ui;
use copper_win_ui::{Align, Node, NodeId, Swatch, Token, Trim};

/// What a click in the foot means.
#[derive(Clone, Debug)]
pub enum Hit {
    Browser(usize),
    CopyPage,
    Choose,
    Export,
}

/// What the foot keeps between draws.
#[derive(Default)]
pub struct State {
    /// The browsers on this PC with a profile, and where each exports
    /// passwords; `None` until looked for.
    browsers: Option<Vec<(String, ExportHint)>>,
    detecting: bool,
    /// The browser whose export is shown.
    hint: Option<usize>,
    /// The file being read.
    importing: Option<String>,
    /// What the last import or export came to.
    said: Option<(String, bool)>,
}

/// What one draw of the foot shows.
pub struct Model {
    browsers: Vec<String>,
    hint: Option<(String, ExportHint)>,
    importing: Option<String>,
    count: Option<usize>,
    said: Option<(String, bool)>,
}

impl State {
    /// The foot as it stands, with `logins` counted.
    pub fn model(&self, logins: &Option<Result<Vec<Login>, String>>) -> Model {
        let browsers = self.browsers.clone().unwrap_or_default();
        Model {
            hint: self.hint.and_then(|i| browsers.get(i).cloned()),
            browsers: browsers.into_iter().map(|(n, _)| n).collect(),
            importing: self.importing.clone(),
            count: match logins {
                Some(Ok(l)) => Some(l.len()),
                _ => None,
            },
            said: self.said.clone(),
        }
    }
}

/// Look for the browsers once per showing (their folders, off the UI
/// thread); `done` draws again with them.
pub fn detect(shell: &Shell) {
    let Some(m) = manager(shell) else { return };
    {
        let mut f = m.foot.borrow_mut();
        if f.browsers.is_some() || f.detecting {
            return;
        }
        f.detecting = true;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let found = rt::blocking(|| {
            copper_import::config_dir()
                .map(|base| copper_import::detect(&base))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|b| match passwords_via(&b) {
                    PasswordsVia::Export(hint) => Some((b.name, hint)),
                    PasswordsVia::Profile => None,
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let Some(s) = weak.upgrade() else { return };
        if let Some(m) = manager(&s) {
            let mut f = m.foot.borrow_mut();
            f.detecting = false;
            f.browsers = Some(found);
        }
        panels::refresh(&s, ID);
    });
}

/// Draw the foot into `parent`; the pressable parts, with what they mean.
pub fn draw(ui: &mut Ui, parent: NodeId, m: &Model) -> Vec<(NodeId, Hit)> {
    let mut hits = Vec::new();
    let busy = m.importing.is_some();
    let bring = ui
        .tree
        .add(parent, Node::row().gap(8.0).align(Align::Center));
    ui.tree
        .add(bring, Node::text("Bring in from", 12.0).ink(Token::Muted));
    for (i, name) in m.browsers.iter().enumerate() {
        let chosen = m.hint.as_ref().is_some_and(|(n, _)| n == name);
        let b = crate::settings::controls::pill(ui, bring, name, chosen, !busy);
        hits.push((b, Hit::Browser(i)));
    }
    let csv = crate::settings::controls::pill(ui, bring, "CSV file…", false, !busy);
    hits.push((csv, Hit::Choose));
    ui.tree.add(bring, Node::new().grow(1.0));
    match (&m.importing, m.count) {
        (Some(file), _) => {
            ui.tree.add(
                bring,
                Node::text(&format!("Reading {file}…"), 11.5)
                    .ink(Token::Muted)
                    .trim(Trim::Middle)
                    .shrink(1.0),
            );
        }
        (None, Some(n)) => {
            ui.tree.add(
                bring,
                Node::text(&crate::settings::count(n, "password", "passwords"), 12.0)
                    .ink(Token::Muted)
                    .shrink(0.0),
            );
            if n > 0 {
                let e = crate::settings::controls::pill(ui, bring, "Export…", false, true);
                hits.push((e, Hit::Export));
            }
        }
        (None, None) => {}
    }
    if let Some((name, hint)) = &m.hint {
        let row = ui
            .tree
            .add(parent, Node::row().gap(8.0).align(Align::Center));
        let words = ui
            .tree
            .add(row, Node::column().gap(2.0).grow(1.0).shrink(1.0));
        ui.tree.add(
            words,
            Node::text(&format!("{name}: {}", hint.page), 12.0)
                .ink(Token::Ink)
                .trim(Trim::Middle),
        );
        ui.tree.add(
            words,
            Node::text(hint.steps, 11.5)
                .ink(Token::Muted)
                .trim(Trim::Wrap),
        );
        let copy = crate::settings::controls::pill(ui, row, "Copy address", false, true);
        hits.push((copy, Hit::CopyPage));
        let choose = crate::settings::controls::pill(ui, row, "Choose File…", true, !busy);
        hits.push((choose, Hit::Choose));
    }
    if let Some((line, worked)) = &m.said {
        ui.tree.add(
            parent,
            Node::text(line, 11.5)
                .ink(if *worked {
                    Swatch::Token(Token::Ink)
                } else {
                    Swatch::Fixed(copper_win_ui::Color::rgba(0.85, 0.2, 0.2, 0.85))
                })
                .trim(Trim::Wrap),
        );
    }
    ui.tree.add(
        parent,
        Node::text(
            "Copper reads only the file you choose and changes nothing in the other browser; everything lands in Windows Credential Manager, under your Windows account.",
            11.5,
        )
        .ink(Token::Muted)
        .trim(Trim::Wrap),
    );
    hits
}

/// A click in the foot; `relist` reads the sign-ins again and draws.
pub fn click(shell: &Shell, h: Hit) {
    let Some(m) = manager(shell) else { return };
    match h {
        Hit::Browser(i) => {
            let mut f = m.foot.borrow_mut();
            f.hint = if f.hint == Some(i) { None } else { Some(i) };
        }
        Hit::CopyPage => {
            let page = {
                let f = m.foot.borrow();
                f.hint
                    .and_then(|i| f.browsers.as_ref()?.get(i).map(|(_, h)| h.page))
            };
            if let Some(page) = page {
                shell.view().copy_text(page);
                shell.toast("Address copied — paste it into that browser");
            }
        }
        Hit::Choose => csv::import_reading(
            shell,
            |s, name| {
                if let Some(m) = manager(s) {
                    m.foot.borrow_mut().importing = Some(name.to_string());
                }
                panels::refresh(s, ID);
            },
            finished,
        ),
        Hit::Export => csv::export(shell, finished),
    }
}

/// An import or export is done: say so, and read the list again.
fn finished(shell: &Shell, said: csv::Said) {
    shell.toast(&said.0);
    if let Some(m) = manager(shell) {
        {
            let mut f = m.foot.borrow_mut();
            f.importing = None;
            f.said = Some(said);
        }
        m.st.borrow_mut().logins = None;
    }
    panels::refresh(shell, ID);
}
