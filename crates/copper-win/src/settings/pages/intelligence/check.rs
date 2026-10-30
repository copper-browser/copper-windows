//! Intelligence's Check: the words and the run are copper-mcp's
//! ([`copper_mcp::intelligence::check`]); here, running it off the UI
//! thread and what a key row says.

use super::*;

/// Ask Jev and the gateway one small question each and keep the verdicts
/// ([`brain::check::run`] does its requests on the cloud runtime).
pub(super) fn run_check(shell: &Shell) {
    if CHECK.with(|c| std::mem::replace(&mut c.borrow_mut().running, true)) {
        return;
    }
    let eff = effective(shell);
    let net = brain::Net::new(
        cloud::runtime().handle().clone(),
        copper_shell_core::updates::version(),
    );
    redraw(shell, "intelligence");
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let done = brain::check::run(&net, &eff).await;
        CHECK.with(|c| *c.borrow_mut() = done);
        if let Some(s) = weak.upgrade() {
            redraw(&s, "intelligence");
        }
    });
}

/// What a key row says: where the key came from, and whether it works.
pub(super) fn key_detail(
    eff: &Effective,
    field: &str,
    plain: &str,
    ready: bool,
    refused: bool,
) -> (String, Tone) {
    if eff.sources.get(field).map(String::as_str) == Some("cloud") {
        return ("Provided by Copper Cloud".into(), Tone::Plain);
    }
    if refused {
        return (
            format!("{plain} Turned away by the last check."),
            Tone::Error,
        );
    }
    if ready {
        return (format!("{plain} Set."), Tone::Plain);
    }
    (plain.to_string(), Tone::Plain)
}
