//! Copper updating itself, end to end: `COPPER_SELFTEST=update` (or
//! `update-on-quit`, `update-at-launch`, `update-refuse`) on the installed
//! copy, with `COPPER_UPDATE_FEED` pointing at a test feed that offers a
//! newer build (`tools/update-e2e.ps1` runs them all on a Windows machine). Results go
//! to `update-a.txt` (before) and `update-b.txt` (the Copper that started
//! after the update) in `COPPER_SELFTEST_OUT` (default
//! `%TEMP%\copper-update-selftest`): `PASS`/`FAIL` lines, then a count.
//!
//! - Before: the feed names a newer Copper; it is downloaded, its size and
//!   SHA-256 checked and staged. `update` then runs Restart to Update;
//!   `update-on-quit` just quits (the staged update installs then);
//!   `update-at-launch` stays open for the test to end it (no quit: the
//!   next start installs it); `update-refuse` expects a feed whose hash doesn't match to be refused
//!   with nothing staged.
//! - After (the same variable, inherited or set again): the pending update
//!   settled as "Updated to" this version, and nothing is left staged.

use crate::platform::{Shell, Win};
use crate::self_update;
use copper_shell_core::updates;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const MODES: [&str; 4] = [
    "update",
    "update-on-quit",
    "update-at-launch",
    "update-refuse",
];

struct Run {
    file: PathBuf,
    log: String,
    failures: usize,
}

impl Run {
    fn check(&mut self, ok: bool, what: &str) {
        let line = format!("{} {what}", if ok { "PASS" } else { "FAIL" });
        tracing::info!(target: "copper::selftest", "{line}");
        let _ = writeln!(self.log, "{line}");
        self.failures += usize::from(!ok);
        let _ = std::fs::write(&self.file, &self.log);
    }

    fn finish(mut self) {
        let _ = writeln!(self.log, "update selftest: {} failure(s)", self.failures);
        let _ = std::fs::write(&self.file, &self.log);
    }
}

/// Start when `COPPER_SELFTEST` is one of [`MODES`].
pub fn maybe_start(shell: &Shell) {
    let Some(mode) = std::env::var("COPPER_SELFTEST")
        .ok()
        .filter(|m| MODES.contains(&m.as_str()))
    else {
        return;
    };
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(Duration::from_millis(1500)).await;
        let Some(shell) = weak.upgrade() else { return };
        let out = std::env::var_os("COPPER_SELFTEST_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("copper-update-selftest"));
        let _ = std::fs::create_dir_all(&out);
        let me = self_update::status();
        let name = if me.settled {
            "update-b.txt"
        } else {
            "update-a.txt"
        };
        let mut r = Run {
            file: out.join(name),
            log: format!("Copper {} ({mode})\n", updates::version()),
            failures: 0,
        };
        r.check(me.enabled, "this is the installed copy");
        if me.settled {
            after(&mut r, &me);
            r.finish();
            crate::app::quit(&shell);
            return;
        }
        let restart = before(&shell, &mut r, &mode).await;
        r.finish();
        if mode == "update-at-launch" {
            // Left open for the test to end it, as a crash would.
            return;
        }
        if restart {
            self_update::restart(&shell);
        } else {
            crate::app::quit(&shell);
        }
    });
}

/// The Copper that started after the update.
fn after(r: &mut Run, me: &self_update::Status) {
    let version = updates::version();
    let o = me.outcome.as_ref();
    r.check(
        o.is_some_and(|o| o.ok && o.detail == version),
        &format!("the update landed: running {version}, outcome {o:?}"),
    );
    r.check(me.ready.is_none(), "nothing is left staged");
}

/// Up to the update: `true` to Restart to Update.
async fn before(shell: &Shell, r: &mut Run, mode: &str) -> bool {
    updates::check::<Win>(shell, false);
    let answered = updates::wait_checked::<Win>(Duration::from_secs(60)).await;
    let st = updates::status::<Win>();
    r.check(
        answered && st.error.is_none(),
        &format!("the feed answered ({:?})", st.error),
    );
    let Some(newer) = updates::available::<Win>() else {
        r.check(false, "the feed names a newer Copper");
        return false;
    };
    r.check(true, &format!("the feed names Copper {newer}"));
    // The check's end started the download; wait for it to settle.
    let start = Instant::now();
    let me = loop {
        copper_win_rt::sleep(Duration::from_millis(250)).await;
        let me = self_update::status();
        let done = me.ready.is_some() || (!me.busy && me.error.is_some());
        if done || start.elapsed() > Duration::from_secs(300) {
            break me;
        }
    };
    if mode == "update-refuse" {
        r.check(
            me.ready.is_none() && me.error.is_some(),
            &format!(
                "a download that doesn't match the feed is refused: {:?}",
                me.error
            ),
        );
        return false;
    }
    r.check(
        me.ready.as_deref() == Some(newer.as_str()),
        &format!(
            "Copper {newer} downloaded, size and SHA-256 verified, staged in {:.0} s ({:?})",
            start.elapsed().as_secs_f64(),
            me.error
        ),
    );
    me.ready.is_some() && mode == "update"
}
