//! `--download-ask-check`: Save As before a download writes anything
//! (`WebView2Engine::set_download_ask`), and Mac-style free names when it
//! does not ask. The Save As dialog is answered like a user would: Save
//! with the suggested name, Save to a typed full path, Cancel, and the
//! close box (that one on a `Profile::download` fetch).

use std::path::{Path, PathBuf};
use std::time::Duration;

use copper_engine::{DownloadEvent, DownloadState, EventStream, JsonValue, PageView, Profile};

use crate::checks::{act, centre, js, pass, until_js};
use crate::os::{self, Answer};
use crate::run::{Ctx, wait_load, within};

/// 64 KiB of `x` from a blob link with a download name (a page download).
const DOWNLOAD_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8">
<title>downloads</title><style>body { font: 22px "Segoe UI", sans-serif; margin: 48px; }</style>
</head><body><p><a id="dl" download="smoke file.bin">Download smoke file.bin</a></p>
<script>document.getElementById('dl').href =
  URL.createObjectURL(new Blob([new Uint8Array(65536).fill(120)]));</script></body></html>"#;
const NAME: &str = "smoke file.bin";
const BYTES: u64 = 65536;
/// What `Profile::download` fetches.
const FETCH_URL: &str = "https://example.com/";

/// How long a dialog may take to show, and a download to end.
const DIALOG: Duration = Duration::from_secs(10);
const DOWNLOAD: Duration = Duration::from_secs(30);

/// Wait until the page has drawn two frames: a click before its first
/// frame hits nothing (on a fresh install the page commits while the
/// browser is still busy setting the profile up).
async fn painted(ctx: &Ctx) {
    let _ = js(
        ctx,
        "window.__painted = false; requestAnimationFrame(() => \
         requestAnimationFrame(() => { window.__painted = true; })); true",
    )
    .await;
    let _ = until_js(
        ctx,
        "window.__painted === true",
        &JsonValue::Bool(true),
        DIALOG,
    )
    .await;
}

pub async fn download_ask(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    let dir = ctx.profile.download_dir();
    // Earlier runs' files would turn the free names below.
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    log.at(&format!("download folder {}", dir.display()));
    ctx.page.load_html(DOWNLOAD_HTML, None);
    if let Err(e) = wait_load(&log, &mut ctx.events).await {
        return pass(&log, false, &format!("downloads: page failed: {e}"));
    }
    let Some(link) = centre(ctx, "dl").await else {
        return pass(&log, false, "downloads: no link");
    };
    let mut downloads = ctx.profile.downloads();
    let owner = ctx.window.hwnd;
    let mut ok = true;
    ctx.engine.set_download_ask(true, owner);

    // 1. Save with the suggested name, in the download folder.
    painted(ctx).await;
    let clicked = act(ctx, copper_engine::InputEvent::click(link.0, link.1)).await;
    log.at(&format!("clicked the link: {clicked:?}"));
    let (dialog, name, disabled) = shown(owner).await;
    log.at(&format!(
        "dialog {dialog:?}: name box {name:?}, owner disabled {disabled}"
    ));
    ok &= pass(
        &log,
        dialog.is_some() && plain_name(name.as_deref()) && disabled,
        "download ask: Save As shows the suggested name, owner disabled",
    );
    let saved = match dialog {
        Some(d) => os::answer(d, owner, Answer::Save).await,
        None => false,
    };
    let states = one_download(&mut downloads).await;
    let want = dir.join(NAME);
    log.at(&format!("saved (closed {saved}): {}", show_states(&states)));
    ok &= pass(
        &log,
        started_then(&states, &want) && file_size(&want) == Some(BYTES),
        &format!("download ask: Save writes {}", want.display()),
    );
    ok &= pass(
        &log,
        os::enabled(owner),
        "download ask: owner enabled again",
    );

    // 2. Save to a full path typed in the box (another folder, another name).
    let picked = dir.join("picked").join("chosen name.bin");
    let _ = std::fs::create_dir_all(picked.parent().unwrap_or(&dir));
    let _ = act(ctx, copper_engine::InputEvent::click(link.0, link.1)).await;
    let (dialog, name, _) = shown(owner).await;
    // The file is there now: the dialog still offers the page's name.
    ok &= pass(
        &log,
        plain_name(name.as_deref()),
        &format!("download ask: second Save As offers the page's name ({name:?})"),
    );
    let mut typed = false;
    if let Some(d) = dialog {
        typed = os::wait_foreground(d, Duration::from_secs(5)).await;
        os::type_over(&picked.to_string_lossy());
        copper_win_rt::sleep(Duration::from_millis(300)).await;
        log.at(&format!("typed: name box {:?}", os::file_name(d)));
        os::enter();
        if !os::wait_closed(d, owner, Duration::from_secs(5)).await {
            os::answer(d, owner, Answer::Save).await;
        }
    }
    let states = one_download(&mut downloads).await;
    log.at(&format!(
        "typed path (foreground {typed}): {}",
        show_states(&states)
    ));
    ok &= pass(
        &log,
        started_then(&states, &picked) && file_size(&picked) == Some(BYTES),
        &format!("download ask: Save goes to exactly {}", picked.display()),
    );

    // 3. Cancel: nothing written, only `Failed { cancelled: true }`.
    let before = listing(&dir);
    let _ = act(ctx, copper_engine::InputEvent::click(link.0, link.1)).await;
    let (dialog, _, _) = shown(owner).await;
    if let Some(d) = dialog {
        os::answer(d, owner, Answer::Cancel).await;
    }
    let states = one_download(&mut downloads).await;
    log.at(&format!("cancelled: {}", show_states(&states)));
    ok &= pass(
        &log,
        dialog.is_some() && only_cancelled(&states) && listing(&dir) == before,
        "download ask: Cancel writes nothing, reports only Failed { cancelled: true }",
    );

    // 4. A `Profile::download` fetch, closed with the close box; the
    // fetch queue goes on (the next fetch, not asked, finishes).
    let fetch = ctx.profile.download(FETCH_URL);
    let (dialog, name, _) = shown(owner).await;
    log.at(&format!("fetch {fetch:?}: dialog {dialog:?} name {name:?}"));
    if let Some(d) = dialog {
        os::answer(d, owner, Answer::Close).await;
    }
    let states = one_download(&mut downloads).await;
    log.at(&format!("fetch closed: {}", show_states(&states)));
    ok &= pass(
        &log,
        dialog.is_some() && only_cancelled(&states) && listing(&dir) == before,
        "download ask: Profile::download asks too; close writes nothing",
    );
    ctx.engine.set_download_ask(false, owner);
    let again = ctx.profile.download(FETCH_URL);
    let states = one_download(&mut downloads).await;
    log.at(&format!(
        "fetch, not asked {again:?}: {}",
        show_states(&states)
    ));
    ok &= pass(
        &log,
        os::owned_dialog(owner).is_none() && finished(&states).is_some(),
        "download ask: off again, fetches save without asking",
    );

    // 5. Not asking: Mac-style free names next to the first file.
    let mut paths = Vec::new();
    for _ in 0..2 {
        let _ = act(ctx, copper_engine::InputEvent::click(link.0, link.1)).await;
        let states = one_download(&mut downloads).await;
        paths.push(finished(&states));
    }
    log.at(&format!("free names: {paths:?}"));
    ok &= pass(
        &log,
        paths
            == [
                Some(dir.join("smoke file 2.bin")),
                Some(dir.join("smoke file 3.bin")),
            ],
        "downloads: free names are `smoke file 2.bin`, `smoke file 3.bin`",
    );
    ok
}

/// The name box holds the page's name (the dialog may hide the extension).
fn plain_name(name: Option<&str>) -> bool {
    name.is_some_and(|n| n == NAME || Some(n) == NAME.strip_suffix(".bin"))
}

/// The dialog the owner shows next: (dialog, its name box, owner disabled).
async fn shown(
    owner: windows::Win32::Foundation::HWND,
) -> (
    Option<windows::Win32::Foundation::HWND>,
    Option<String>,
    bool,
) {
    let Some(dialog) = os::wait_dialog(owner, DIALOG).await else {
        return (None, None, false);
    };
    // The name box is filled in as the dialog comes up.
    copper_win_rt::sleep(Duration::from_millis(500)).await;
    (Some(dialog), os::file_name(dialog), !os::enabled(owner))
}

/// The states of the next download, to its end (or the time limit).
async fn one_download(events: &mut EventStream<DownloadEvent>) -> Vec<DownloadState> {
    let mut id = None;
    let mut states = Vec::new();
    let collect = async {
        while let Some(ev) = events.next().await {
            if *id.get_or_insert(ev.id) != ev.id {
                continue;
            }
            let end = matches!(
                ev.state,
                DownloadState::Finished { .. } | DownloadState::Failed { .. }
            );
            if !matches!(ev.state, DownloadState::Progress { .. }) {
                states.push(ev.state);
            }
            if end {
                break;
            }
        }
    };
    let _ = within(collect, DOWNLOAD).await;
    states
}

/// Started, Destination `path`, … Finished at `path`.
fn started_then(states: &[DownloadState], path: &Path) -> bool {
    matches!(states.first(), Some(DownloadState::Started { .. }))
        && matches!(states.get(1), Some(DownloadState::Destination { path: p, .. }) if p == path)
        && finished(states).as_deref() == Some(path)
}

fn finished(states: &[DownloadState]) -> Option<PathBuf> {
    states.iter().find_map(|s| match s {
        DownloadState::Finished { path, .. } => Some(path.clone()),
        _ => None,
    })
}

fn only_cancelled(states: &[DownloadState]) -> bool {
    matches!(
        states,
        [DownloadState::Failed {
            cancelled: true,
            ..
        }]
    )
}

fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

/// Every file under `dir` (a cancelled download must add none, not even a
/// partial one).
fn listing(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn show_states(states: &[DownloadState]) -> String {
    states
        .iter()
        .map(|s| match s {
            DownloadState::Started { .. } => "Started".to_owned(),
            DownloadState::Destination { path, .. } => format!("Destination({})", path.display()),
            DownloadState::Finished { path, bytes } => {
                format!("Finished({}, {bytes} B)", path.display())
            }
            DownloadState::Failed { error, cancelled } => {
                format!("Failed({error}, cancelled {cancelled})")
            }
            DownloadState::Progress { .. } => "Progress".to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" → ")
}
