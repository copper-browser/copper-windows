//! The uninstall window: one question, one switch ("Also delete my browsing
//! data", off), Cancel and Uninstall.

use super::card::{ButtonSpec, Card, Check, ChecksAt, LinkSpec, Scene, Target};
use super::window::{Controller, Ctx};
use crate::cli::UninstallOptions;
use crate::exit;
use crate::log::log;
use crate::sys::shell;
use crate::uninstall::{self, Pending};

const TITLE: &str = "Uninstall Copper?";
const LINE: &str = "Copper will be removed from this PC.";

pub enum Msg {
    Progress(f32),
    Finished(Pending, Result<(), String>),
}

enum Phase {
    Ready,
    Working,
    Finished,
}

pub struct Uninstaller {
    options: UninstallOptions,
    phase: Phase,
    /// Handed back to the caller, which finishes it as the process exits.
    pub pending: Option<Pending>,
    exit: i32,
}

fn ready() -> Scene {
    Scene {
        title: TITLE.to_owned(),
        line: LINE.to_owned(),
        buttons: vec![
            ButtonSpec::secondary("Cancel"),
            ButtonSpec::primary("Uninstall"),
        ],
        link: None,
        checks: Some(ChecksAt::Above),
        progress: None,
    }
}

fn working(f: f32) -> Scene {
    Scene {
        title: TITLE.to_owned(),
        line: LINE.to_owned(),
        buttons: Vec::new(),
        link: None,
        checks: None,
        progress: Some((f, "Removing Copper…".to_owned())),
    }
}

impl Uninstaller {
    pub fn new(options: UninstallOptions) -> (Card, Uninstaller) {
        let checks = vec![Check::new(
            "Also delete my browsing data",
            options.delete_data,
        )];
        let card = Card::new(ready(), checks);
        (
            card,
            Uninstaller {
                options,
                phase: Phase::Ready,
                pending: None,
                exit: exit::CANCELLED,
            },
        )
    }

    fn start(&mut self, card: &mut Card, ctx: &mut Ctx<Msg>) {
        self.options.delete_data = card.checks[0].on;
        self.phase = Phase::Working;
        card.show(working(0.0));
        let options = self.options.clone();
        ctx.spawn(move |poster| {
            let (pending, result) = uninstall::run(&options, |f| poster.send(Msg::Progress(f)));
            poster.send(Msg::Finished(pending, result.map_err(|e| format!("{e:#}"))));
        });
    }
}

impl Controller for Uninstaller {
    type Msg = Msg;

    fn clicked(&mut self, target: Target, card: &mut Card, ctx: &mut Ctx<Msg>) {
        match (&self.phase, target) {
            (Phase::Ready, Target::Check(i)) => card.toggle(i),
            (Phase::Ready, Target::Button(0)) => ctx.close(),
            (Phase::Ready, Target::Button(1)) => self.start(card, ctx),
            (Phase::Finished, Target::Button(0)) => ctx.close(),
            (Phase::Finished, Target::Link) => {
                if let Err(e) = shell::open(&crate::log::path().to_string_lossy()) {
                    log!("couldn't open the log: {e:#}");
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, msg: Msg, card: &mut Card, _ctx: &mut Ctx<Msg>) {
        match msg {
            Msg::Progress(f) => card.show(working(f)),
            Msg::Finished(pending, result) => {
                self.pending = Some(pending);
                self.phase = Phase::Finished;
                let scene = match result {
                    Ok(()) => {
                        self.exit = exit::SUCCESS;
                        Scene {
                            title: "Copper is uninstalled".to_owned(),
                            line: if self.options.delete_data {
                                "Your browsing data was deleted too.".to_owned()
                            } else {
                                "Your browsing data is still on this PC.".to_owned()
                            },
                            buttons: vec![ButtonSpec::primary("Close")],
                            link: None,
                            checks: None,
                            progress: None,
                        }
                    }
                    Err(e) => {
                        log!("uninstall failed: {e}");
                        self.exit = exit::FAILED;
                        Scene {
                            title: "Copper wasn't fully removed".to_owned(),
                            line: "Some of it couldn't be deleted. Restarting Windows and uninstalling again usually clears it.".to_owned(),
                            buttons: vec![ButtonSpec::primary("Close")],
                            link: Some(LinkSpec {
                                label: "View log".to_owned(),
                                disclosure: false,
                            }),
                            checks: None,
                            progress: None,
                        }
                    }
                };
                card.show(scene);
            }
        }
    }

    fn close_requested(&mut self, _card: &mut Card, ctx: &mut Ctx<Msg>) {
        // Mid-uninstall the window stays: half-removed is worse than a few
        // more seconds. Everything else closes.
        if !matches!(self.phase, Phase::Working) {
            ctx.close();
        }
    }

    fn exit_code(&self) -> i32 {
        self.exit
    }
}
