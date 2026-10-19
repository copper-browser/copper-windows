//! Regression check: the process must exit cleanly while tasks are still
//! parked on long timers. Before `shutdown`, the TLS destructors dropped the
//! task table, a task's `Sleep` reached for the destroyed timer table, and
//! the process aborted with 0xC0000409. Prints "clean exit" and returns 0.

#[cfg(windows)]
fn main() {
    use std::time::Duration;
    copper_win_rt::init().expect("init");
    for _ in 0..3 {
        copper_win_rt::spawn_local(async {
            copper_win_rt::sleep(Duration::from_secs(600)).await;
        });
    }
    copper_win_rt::spawn_local(async {
        copper_win_rt::sleep(Duration::from_millis(50)).await;
        copper_win_rt::quit(0);
    });
    let code = copper_win_rt::run();
    println!("clean exit (loop returned {code})");
    std::process::exit(code);
}

#[cfg(not(windows))]
fn main() {
    println!("copper-win-rt runs on Windows only");
}
