//! The install window: "Copper", one line, Install. Options tucked under a
//! quiet disclosure; a progress bar while it works; "Copper is ready", and
//! Copper opens.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::card::{ButtonSpec, Card, Check, ChecksAt, LinkSpec, Scene, Target};
use super::window::{Controller, Ctx};
use crate::cli::InstallOptions;
use crate::exit;
use crate::install::{self, Failure};
use crate::log::log;
use crate::plan::{Plan, Step};
use crate::registration::{APP_NAME, DESCRIPTION};
use crate::sys::{process, shell};

/// How long "Copper is ready" stays up before Copper opens over it.
const READY_BEAT: Duration = Duration::from_millis(900);
/// An install takes a fraction of a second when WebView2 is already there.
/// The bar still runs its course, so the window never flicks from "Install"
/// to "ready" in one frame: at least this long from Install to the end.
const LEAST_WORKING: Duration = Duration::from_millis(1100);
/// And once the work is done, the full bar shows at least this long.
const FULL_BAR: Duration = Duration::from_millis(350);

const CHECK_DESKTOP: usize = 0;
const CHECK_LAUNCH: usize = 1;

pub enum Msg {
    Step(Step),
    Finished(Result<PathBuf, Failure>),
    /// The bar has had its time: say "Copper is ready".
    Ready,
    /// The ready beat is over: open Copper.
    Launch,
}

enum Phase {
    Ready,
    /// Installing, since this moment.
    Working(Instant),
    /// Installed; the bar is finishing its run before "Copper is ready".
    Finishing(PathBuf),
    Done(PathBuf),
    Failed,
}

pub struct Installer {
    options: InstallOptions,
    plan: Plan,
    phase: Phase,
    verb: &'static str,
    options_open: bool,
    cancel: Arc<AtomicBool>,
    close_when_done: bool,
    exit: i32,
}

fn ready(verb: &str, open: bool) -> Scene {
    Scene {
        title: APP_NAME.to_owned(),
        line: DESCRIPTION.to_owned(),
        buttons: vec![ButtonSpec::primary(verb)],
        link: Some(LinkSpec {
            label: "Options".to_owned(),
            disclosure: true,
        }),
        checks: open.then_some(ChecksAt::Below),
        progress: None,
    }
}

fn working(plan: &Plan, step: Step) -> Scene {
    Scene {
        title: APP_NAME.to_owned(),
        line: DESCRIPTION.to_owned(),
        buttons: Vec::new(),
        link: None,
        checks: None,
        progress: Some((plan.fraction(step), plan.label(step))),
    }
}

fn done(launching: bool) -> Scene {
    Scene {
        title: "Copper is ready".to_owned(),
        line: if launching {
            "Opening Copper…".to_owned()
        } else {
            "You'll find it in the Start menu.".to_owned()
        },
        buttons: if launching {
            Vec::new()
        } else {
            vec![ButtonSpec::primary("Open Copper")]
        },
        link: (!launching).then(|| LinkSpec {
            label: "Make Copper your default browser".to_owned(),
            disclosure: false,
        }),
        checks: None,
        progress: launching.then(|| (1.0, String::new())),
    }
}

fn failed(sentence: String) -> Scene {
    Scene {
        title: "Copper wasn't installed".to_owned(),
        line: sentence,
        buttons: vec![ButtonSpec::primary("Try again")],
        link: Some(LinkSpec {
            label: "View log".to_owned(),
            disclosure: false,
        }),
        checks: None,
        progress: None,
    }
}

impl Installer {
    /// The card and its controller for `options`.
    pub fn new(options: InstallOptions) -> (Card, Installer) {
        let verb = match (install::is_installed(), install::installed_version()) {
            (true, Some(v)) if v == crate::registration::VERSION => "Reinstall",
            (true, _) => "Update",
            (false, _) => "Install",
        };
        let checks = vec![
            Check::new("Add a desktop shortcut", options.desktop_shortcut),
            Check::new("Open Copper when done", options.launch),
        ];
        let card = Card::new(ready(verb, false), checks);
        let me = Installer {
            plan: install::plan(),
            options,
            phase: Phase::Ready,
            verb,
            options_open: false,
            cancel: Arc::new(AtomicBool::new(false)),
            close_when_done: false,
            exit: exit::CANCELLED,
        };
        (card, me)
    }

    fn start(&mut self, card: &mut Card, ctx: &mut Ctx<Msg>) {
        self.options.desktop_shortcut = card.checks[CHECK_DESKTOP].on;
        self.options.launch = card.checks[CHECK_LAUNCH].on;
        // WebView2 may have arrived since the window opened (a retry).
        self.plan = install::plan();
        self.cancel.store(false, Ordering::Relaxed);
        self.phase = Phase::Working(Instant::now());
        let first = if self.plan.webview2 {
            Step::DownloadWebView2 {
                done: 0,
                total: None,
            }
        } else {
            Step::Start
        };
        card.show(working(&self.plan, first));
        let (options, plan, cancel) = (self.options.clone(), self.plan, self.cancel.clone());
        ctx.spawn(move |poster| {
            let result = install::run(&options, plan, &cancel, |step| poster.send(Msg::Step(step)));
            poster.send(Msg::Finished(result));
        });
    }
}

impl Controller for Installer {
    type Msg = Msg;

    fn clicked(&mut self, target: Target, card: &mut Card, ctx: &mut Ctx<Msg>) {
        match (&self.phase, target) {
            (Phase::Ready, Target::Button(0)) | (Phase::Failed, Target::Button(0)) => {
                self.start(card, ctx)
            }
            (Phase::Ready, Target::Link) => {
                self.options_open = !self.options_open;
                card.show(ready(self.verb, self.options_open));
            }
            (Phase::Ready, Target::Check(i)) => card.toggle(i),
            (Phase::Done(exe), Target::Button(0)) => {
                if let Err(e) = process::launch(exe) {
                    log!("couldn't open Copper: {e:#}");
                }
                ctx.close();
            }
            (Phase::Done(_), Target::Link) => {
                if let Err(e) = crate::open_default_apps_settings() {
                    log!("couldn't open Default apps: {e:#}");
                }
            }
            (Phase::Failed, Target::Link) => {
                if let Err(e) = shell::open(&crate::log::path().to_string_lossy()) {
                    log!("couldn't open the log: {e:#}");
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, msg: Msg, card: &mut Card, ctx: &mut Ctx<Msg>) {
        match msg {
            Msg::Step(step) => {
                if matches!(self.phase, Phase::Working(_)) && !self.close_when_done {
                    card.show(working(&self.plan, step));
                }
            }
            Msg::Finished(Ok(exe)) => {
                self.exit = exit::SUCCESS;
                if !self.options.launch || self.close_when_done {
                    crate::prewarm(&exe);
                }
                if self.close_when_done {
                    ctx.close();
                    return;
                }
                let elapsed = match self.phase {
                    Phase::Working(since) => since.elapsed(),
                    _ => LEAST_WORKING,
                };
                card.show(working(&self.plan, Step::Done));
                self.phase = Phase::Finishing(exe);
                let wait = LEAST_WORKING.saturating_sub(elapsed).max(FULL_BAR);
                ctx.spawn(move |poster| {
                    std::thread::sleep(wait);
                    poster.send(Msg::Ready);
                });
            }
            Msg::Ready => {
                let Phase::Finishing(exe) = std::mem::replace(&mut self.phase, Phase::Ready) else {
                    return;
                };
                let launching = self.options.launch;
                card.show(done(launching));
                self.phase = Phase::Done(exe);
                if launching {
                    ctx.spawn(|poster| {
                        std::thread::sleep(READY_BEAT);
                        poster.send(Msg::Launch);
                    });
                }
            }
            Msg::Finished(Err(Failure::Cancelled)) => {
                self.exit = exit::CANCELLED;
                ctx.close();
            }
            Msg::Finished(Err(failure)) => {
                log!("install failed: {failure}");
                self.exit = exit::FAILED;
                if self.close_when_done {
                    ctx.close();
                    return;
                }
                self.phase = Phase::Failed;
                card.show(failed(failure.sentence()));
            }
            Msg::Launch => {
                if let Phase::Done(exe) = &self.phase {
                    if let Err(e) = process::launch(exe) {
                        log!("couldn't open Copper: {e:#}");
                    }
                    ctx.close();
                }
            }
        }
    }

    fn close_requested(&mut self, card: &mut Card, ctx: &mut Ctx<Msg>) {
        match self.phase {
            Phase::Working(_) => {
                // Stop at the next safe point; the worker reports back and
                // the window closes then.
                self.cancel.store(true, Ordering::Relaxed);
                self.close_when_done = true;
                let mut scene = card.scene().clone();
                if let Some((f, _)) = scene.progress {
                    scene.progress = Some((f, "Stopping…".to_owned()));
                }
                card.show(scene);
            }
            _ => ctx.close(),
        }
    }

    fn exit_code(&self) -> i32 {
        self.exit
    }
}
