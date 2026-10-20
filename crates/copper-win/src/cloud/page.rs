//! Settings › Cloud in the window: the page's state for the whole run, the
//! cloud read fresh for every draw, the nodes kept between draws, and the
//! watches and clock that redraw it.
//!
//! Settings asks for the page's body on every refresh
//! ([`crate::settings::Page::body`]); clicks and field events it doesn't
//! take come here, and what they mean is [`super::work`]'s.

use super::el::{Act, FieldId};
use super::model;
use super::mount::Mount;
use super::state::{Snap, State};
use super::view::{self, connect};
use super::work;
use crate::platform::Shell;
use crate::sys::calendar::WinCalendar;
use copper_cloud::JoinCode;
use copper_shell_core::cloud::{self, sso::SsoDone, sso::sign_in};
use copper_shell_core::format;
use copper_win_ui::win::Ui;
use copper_win_ui::{Event, NodeId};
use std::cell::RefCell;
use std::time::Duration;

/// Settings' id for the page.
pub const ID: &str = "cloud";

/// The page between draws.
struct Page {
    st: State,
    mount: Mount,
    /// The one-second clock runs (only while the page shows).
    ticking: bool,
    /// The last sign-in through the browser said aloud.
    told: Option<SsoDone>,
}

thread_local! {
    static PAGE: RefCell<Option<Page>> = const { RefCell::new(None) };
}

/// The page's state, made on first use (the name to create an account
/// with is this user's).
pub(super) fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    with_page(|p| f(&mut p.st))
}

fn with_page<R>(f: impl FnOnce(&mut Page) -> R) -> R {
    PAGE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let page = slot.get_or_insert_with(|| Page {
            st: State::new(crate::sys::user::name()),
            mount: Mount::default(),
            ticking: false,
            told: sign_in().state().signed_in,
        });
        f(page)
    })
}

/// Draw the page again, if Settings shows it: only the page's own list —
/// the rail and title around it stay as they are (the clock redraws once a
/// second).
pub fn refresh(shell: &Shell) {
    if crate::settings::showing(shell) != Some(ID) {
        return;
    }
    let Some(list) = with_page(|p| p.mount.list()) else {
        crate::settings::changed(shell, ID);
        return;
    };
    if let Some(draw) = body(shell) {
        shell.view().chrome().read(move |ui, _| draw(ui, list));
    }
}

/// What the cloud says now, as the page reads it.
pub fn snap(st: &State) -> Snap {
    let Some(c) = cloud::get() else {
        return Snap::default();
    };
    let status = c.client.status();
    let sync = c.sync.status();
    let link = c.client.link();
    let sso = sign_in().state();
    let sso_info = link.as_ref().and_then(|l| sign_in().known(&l.endpoint));
    // What the pasted join link's instance offers — only if it has said
    // already; pasting never asks.
    let join_info = match model::read_code(&st.code, link.as_ref()).code() {
        Some(JoinCode::Sso(j)) if !st.manual => sign_in().known(&j.endpoint),
        _ => None,
    };
    Snap {
        on: true,
        signed_in: status.signed_in,
        account: status.account.map(|a| (a.display_name, a.email)),
        reachable: status.reachable,
        device_name: status.device_name,
        sync_on: sync.on,
        enabled: sync.enabled.iter().copied().collect(),
        state: sync.state,
        last_sync: sync.last_sync,
        devices: sync
            .other_devices
            .into_iter()
            .filter(|d| !d.tabs.is_empty())
            .collect(),
        log: model::log_rows(&c.sync.log())
            .into_iter()
            .map(|(at, words)| (clock(at), words))
            .collect(),
        sso,
        sso_info,
        join_info,
        deletes_history: c.client.deletes_history(),
        now: copper_core::date::now_unix(),
        link,
    }
}

/// `15:41:07` in local time.
fn clock(unix: f64) -> String {
    format!(
        "{}:{:02}",
        format::clock::<WinCalendar>(unix),
        (unix as i64).rem_euclid(60)
    )
}

/// The text field `f` shows: the draft, or for the device name the saved
/// one until it is edited.
fn field_text(s: &Snap, st: &State, f: FieldId) -> String {
    match f {
        FieldId::DeviceName => st
            .device_name
            .clone()
            .unwrap_or_else(|| s.device_name.clone()),
        _ => st.draft(f).to_string(),
    }
}

// MARK: Settings' hooks

/// The page, laid into Settings' list.
#[allow(clippy::type_complexity)]
pub fn body(shell: &Shell) -> Option<Box<dyn FnOnce(&mut Ui, NodeId)>> {
    let snap = with(|st| snap(st));
    work::on_show(shell, &snap);
    let els = with(|st| view::page(&snap, st));
    let texts: Vec<(FieldId, String)> = with(|st| {
        [
            FieldId::Code,
            FieldId::Host,
            FieldId::Key,
            FieldId::Fingerprint,
            FieldId::Email,
            FieldId::Password,
            FieldId::Name,
            FieldId::DeviceName,
            FieldId::Site,
        ]
        .into_iter()
        .map(|f| (f, field_text(&snap, st, f)))
        .collect()
    });
    tick(shell);
    Some(Box::new(move |ui: &mut Ui, list: NodeId| {
        let fields = with_page(|p| {
            p.mount.sync(&mut ui.tree, list, &els);
            p.mount.fields()
        });
        for (f, id) in fields {
            let Some((_, want)) = texts.iter().find(|(g, _)| *g == f) else {
                continue;
            };
            if ui.field_text(id) != Some(want.as_str()) {
                ui.set_field_text(id, want, false);
            }
        }
    }))
}

/// A click Settings didn't take: the page's, if it is one of its controls.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let Some(act) = with_page(|p| p.mount.act(id).cloned()) else {
        return false;
    };
    work::run(shell, act);
    true
}

/// A field event: keep the draft as typed; Return does what the field is
/// for.
pub fn field(shell: &Shell, id: NodeId, e: &Event) -> bool {
    let Some(f) = with_page(|p| p.mount.field_of(id)) else {
        return false;
    };
    match e {
        Event::FieldChanged(_) => {
            let text = shell
                .view()
                .chrome()
                .read(move |ui, _| ui.field_text(id).map(str::to_string))
                .flatten()
                .unwrap_or_default();
            with(|st| st.set_draft(f, &text));
            if matches!(
                f,
                FieldId::Code | FieldId::Host | FieldId::Key | FieldId::Fingerprint
            ) {
                sign_in().clear_problem();
            }
            refresh(shell);
            true
        }
        Event::FieldSubmit(_) => {
            work::submit(shell, f);
            true
        }
        _ => false,
    }
}

/// The page went off screen: its nodes went with it; a device name typed
/// and not yet submitted is kept now.
pub fn left() {
    with_page(|p| {
        p.mount.forget();
        p.st.sso_asked = false;
        p.st.info_asked = false;
        p.st.devices_asked = false;
    });
    work::rename_device(None);
}

// MARK: keeping the page current

/// While the page shows: once a second, for the pairing countdown, "Copied"
/// and "2 minutes ago"; every third second a live code's state is asked.
fn tick(shell: &Shell) {
    if with_page(|p| std::mem::replace(&mut p.ticking, true)) {
        return;
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let mut n: u64 = 0;
        loop {
            copper_win_rt::sleep(Duration::from_secs(1)).await;
            let Some(shell) = weak.upgrade() else { break };
            if crate::settings::showing(&shell) != Some(ID) {
                break;
            }
            n += 1;
            if n.is_multiple_of(3) {
                work::check_code(&shell);
            }
            refresh(&shell);
        }
        with_page(|p| p.ticking = false);
    });
}

/// Redraw on every change the cloud announces: the link and account, the
/// sync engine, a sign-in through the browser (and say when one finishes).
pub fn watch(shell: &Shell) {
    let Some(c) = cloud::get() else { return };
    let mut status = c.client.watch_status();
    let mut sync = c.sync.watch();
    let mut sso = sign_in().watch();
    let w = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        while status.changed().await.is_ok() {
            match w.upgrade() {
                Some(s) => refresh(&s),
                None => break,
            }
        }
    });
    let w = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        while sync.changed().await.is_ok() {
            match w.upgrade() {
                Some(s) => refresh(&s),
                None => break,
            }
        }
    });
    let w = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        while sso.changed().await.is_ok() {
            let Some(s) = w.upgrade() else { break };
            let done = sso.borrow_and_update().signed_in.clone();
            let new = with_page(|p| {
                let new = done.is_some() && done != p.told;
                p.told = done.clone();
                new
            });
            if new && let Some(d) = done {
                s.toast(&d.text());
                with(|st| {
                    st.code.clear();
                    st.problem = None;
                });
            }
            refresh(&s);
        }
    });
}

/// "Pair another device with Copper Cloud" in the palette: the page, and a
/// code made at once unless one is live.
pub fn pair_from_palette(shell: &Shell) {
    crate::settings::open(shell, ID);
    let now = copper_core::date::now_unix();
    let live = with(|st| matches!(st.pair_card(now), model::PairCard::Live(_)));
    if !live {
        work::run(shell, Act::MakeCode);
    }
}

/// Fill the Connect field with a code a `copper://cloud/join` link brought,
/// and show it read — nothing is sent until a button is pressed.
pub fn prefill(code: &str) {
    with(|st| {
        st.manual = false;
        st.code = code.trim().to_string();
        st.problem = None;
        st.unknown = None;
    });
}

/// The node of field `f`, while the page shows it.
pub(super) fn node_of(f: FieldId) -> Option<NodeId> {
    with_page(|p| p.mount.node_of(f))
}

/// The fingerprint waiting to be trusted for what the Connect card holds.
pub(super) fn seen(s: &Snap, st: &State) -> Option<String> {
    connect::read(s, st)
        .code()
        .and_then(|c| connect::seen(s, st, c))
}
