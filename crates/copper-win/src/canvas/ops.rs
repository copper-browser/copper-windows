//! What the card does to canvases (`Canvases.rename`/`delete`/`leave`/
//! `answer`, Mac `Canvas.swift`): each call on the cloud's runtime, its
//! answer back on the main thread.

use super::copy::{self, Doing};
use super::door;
use super::inbox;
use super::state::{self, FieldKind, Mode};
use crate::platform::{Shell, Win};
use copper_cloud::{CanvasApi, Failure};
use copper_shell_core::canvas::store::CanvasStore;
use copper_shell_core::canvas::{self as core, host, registry};
use std::future::Future;

/// The cloud's canvas calls, when there is a cloud.
pub fn api() -> Option<CanvasApi> {
    copper_shell_core::cloud::get().map(|c| c.canvas.clone())
}

/// Run `f` on the cloud's runtime and wait for it here.
pub async fn cloud_call<T: Send + 'static>(
    f: impl Future<Output = Result<T, Failure>> + Send + 'static,
) -> Result<T, Failure> {
    copper_shell_core::cloud::spawn(f)
        .await
        .unwrap_or_else(|_| Err(Failure::new(0, "stopped", "Copper Cloud stopped")))
}

/// A canvas id fit for a path (copper-cloud's own `path_id` is private).
pub fn path_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Sharing needs the cloud (`Canvases.offline`).
fn offline() -> Failure {
    Failure::new(
        0,
        "not_signed_in",
        "Sign in at Settings › Cloud to share canvases",
    )
}

fn host_text() -> String {
    copy::cloud_host(
        copper_shell_core::cloud::get()
            .and_then(|c| c.client.link())
            .map(|l| l.host())
            .as_deref(),
    )
}

/// Say `f` on the card (the form stays, the working mark goes).
fn failed(shell: &Shell, f: &Failure, doing: Doing) {
    let text = copy::plain(f, &doing, &host_text());
    state::with(|c| {
        c.working = false;
        c.failure = Some(text);
    });
    door::render(shell);
}

fn working(shell: &Shell) {
    state::with(|c| {
        c.working = true;
        c.failure = None;
    });
    door::render(shell);
}

/// The form's action: Return or its button.
pub fn submit(shell: &Shell) {
    let (mode, name, email, busy) = state::with(|c| {
        (
            c.mode.clone(),
            c.texts.get(&FieldKind::Name).cloned().unwrap_or_default(),
            c.texts.get(&FieldKind::Email).cloned().unwrap_or_default(),
            c.working,
        )
    });
    if busy {
        return;
    }
    match mode {
        Mode::New if copy::submittable(&name) => create(shell, &name),
        Mode::Rename(id) if copy::submittable(&name) => rename(shell, &id, &name, true),
        Mode::Invite(id) if email.contains('@') => invite(shell, &id, &email),
        Mode::Share(_) => super::share::submit(shell),
        Mode::Delete(id) => remove(shell, &id),
        _ => {}
    }
}

/// New canvas: shared when signed in, on this computer otherwise; opened
/// at once.
pub fn create(shell: &Shell, name: &str) {
    working(shell);
    let weak = shell.downgrade();
    let name = copy::clean(name);
    copper_win_rt::spawn_local(async move {
        let made = core::create(&name).await;
        let Some(shell) = weak.upgrade() else { return };
        match made {
            Ok(e) => {
                door::close(&shell);
                core::show::<Win>(&shell, &e.id, true);
            }
            Err(text) => {
                state::with(|c| {
                    c.working = false;
                    c.failure = Some(text);
                });
                door::render(&shell);
            }
        }
    });
}

/// Rename canvas `id` — on the cloud first when it is shared. `card`: the
/// card asked (it goes back to the list; a failure is said there).
pub fn rename(shell: &Shell, id: &str, name: &str, card: bool) {
    let Some(e) = registry::entry(id).filter(|e| !registry::is_personal(e)) else {
        return;
    };
    let clean = copy::clean(name);
    if clean == e.name {
        if card {
            door::set_mode(shell, Mode::List);
        }
        return;
    }
    let done = {
        let id = id.to_string();
        let clean = clean.clone();
        move |shell: &Shell| {
            registry::rename_local(&id, &clean);
            for h in host::hosts_of::<Win>(&id) {
                h.renamed();
            }
            core::recount::<Win>();
            if card {
                state::with(|c| c.note = Some(format!("Renamed to “{clean}”")));
                door::set_mode(shell, Mode::List);
            }
            super::refresh_all(shell);
        }
    };
    if !registry::is_shared(&e) {
        done(shell);
        return;
    }
    let (Some(api), Some(remote)) = (api().filter(|_| registry::cloud_ready()), e.remote_id) else {
        if card {
            failed(shell, &offline(), Doing::Other);
        } else {
            shell.toast(&offline().message);
        }
        return;
    };
    if card {
        working(shell);
    }
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let n = clean.clone();
        let answer = cloud_call(async move { api.rename(&remote, &n).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        match answer {
            Ok(()) => done(&shell),
            Err(f) if card => failed(&shell, &f, Doing::Other),
            Err(f) => shell.toast(&copy::plain(&f, &Doing::Other, &host_text())),
        }
    });
}

/// Delete canvas `id` (the owner: for everyone) or leave it (a member).
pub fn remove(shell: &Shell, id: &str) {
    let Some(e) = registry::entry(id).filter(|e| !registry::is_personal(e)) else {
        return;
    };
    let name = e.name.clone();
    let leaving = registry::is_shared(&e) && !e.is_owner();
    if !registry::is_shared(&e) {
        forget(shell, id);
        state::with(|c| c.note = Some(format!("Deleted “{name}”")));
        door::set_mode(shell, Mode::List);
        return;
    }
    let (Some(api), Some(remote)) = (
        api().filter(|_| registry::cloud_ready()),
        e.remote_id.clone(),
    ) else {
        failed(shell, &offline(), Doing::Other);
        return;
    };
    working(shell);
    let weak = shell.downgrade();
    let id = id.to_string();
    copper_win_rt::spawn_local(async move {
        let answer = cloud_call(async move {
            if leaving {
                api.leave(&remote).await
            } else {
                api.delete(&remote).await
            }
        })
        .await;
        let Some(shell) = weak.upgrade() else { return };
        match answer {
            // Already gone on the server: gone here too.
            Ok(()) => {}
            Err(f) if f.status == 404 => {}
            Err(f) => return failed(&shell, &f, Doing::Other),
        }
        forget(&shell, &id);
        state::with(|c| {
            c.note = Some(if leaving {
                format!("Left “{name}”")
            } else {
                format!("Deleted “{name}”")
            })
        });
        door::set_mode(&shell, Mode::List);
    });
}

/// Gone from this computer: its tabs close, its entry and history go.
fn forget(shell: &Shell, id: &str) {
    let key = registry::entry(id).map(|e| registry::store_key(&e));
    core::removed::<Win>(id);
    registry::forget(id);
    if let Some(key) = key {
        let root = shell.store().paths().data.join("canvas");
        CanvasStore::open(&root, &key).erase();
    }
    inbox::forget_canvas(id);
    core::recount::<Win>();
    super::refresh_all(shell);
}

/// The Invite form: an email invite to canvas `id`.
fn invite(shell: &Shell, id: &str, email: &str) {
    let Some(e) = registry::entry(id) else { return };
    let (Some(api), Some(remote)) = (api().filter(|_| registry::cloud_ready()), e.remote_id) else {
        failed(shell, &offline(), Doing::Other);
        return;
    };
    working(shell);
    let weak = shell.downgrade();
    let address = email.trim().to_string();
    copper_win_rt::spawn_local(async move {
        let a = address.clone();
        let answer = cloud_call(async move { api.invite(&remote, &a).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        match answer {
            Ok(_) => {
                state::with(|c| c.note = Some(format!("Invited {address}")));
                door::set_mode(&shell, Mode::List);
            }
            Err(f) => failed(&shell, &f, Doing::Inviting(address)),
        }
    });
}

/// The Members view: read them.
pub fn load_members(shell: &Shell, id: &str) {
    let Some(e) = registry::entry(id) else { return };
    let (Some(api), Some(remote)) = (api().filter(|_| registry::cloud_ready()), e.remote_id) else {
        state::with(|c| c.members = Some(Err(offline().message)));
        return;
    };
    let weak = shell.downgrade();
    let id = id.to_string();
    copper_win_rt::spawn_local(async move {
        let answer = cloud_call(async move { api.members(&remote).await }).await;
        let Some(shell) = weak.upgrade() else { return };
        let mine = state::with(|c| {
            if c.mode != Mode::Members(id.clone()) {
                return false;
            }
            c.members = Some(answer.map_err(|f| copy::plain(&f, &Doing::Other, &host_text())));
            true
        });
        if mine {
            door::render(&shell);
        }
    });
}

/// Join or decline invitation `invite_id` (`Canvases.answer`); a join
/// opens the canvas.
pub fn answer(shell: &Shell, invite_id: &str, accept: bool) {
    let Some(api) = api().filter(|_| registry::cloud_ready()) else {
        state::with(|c| {
            c.answer_failed
                .insert(invite_id.to_string(), offline().message)
        });
        door::render(shell);
        return;
    };
    let account = copper_shell_core::cloud::get()
        .and_then(|c| c.client.account())
        .map(|a| a.user_id.to_ascii_lowercase())
        .unwrap_or_default();
    let canvas = inbox::invites()
        .into_iter()
        .find(|i| i.id == invite_id)
        .and_then(|i| i.canvas_id);
    state::with(|c| {
        c.answering.insert(invite_id.to_string(), accept);
        c.answer_failed.remove(invite_id);
    });
    door::render(shell);
    let weak = shell.downgrade();
    let invite_id = invite_id.to_string();
    copper_win_rt::spawn_local(async move {
        let i = invite_id.clone();
        let answer = cloud_call(async move {
            if accept {
                api.accept_invite(&i).await.map(Some)
            } else {
                api.decline_invite(&i).await.map(|()| None)
            }
        })
        .await;
        let Some(shell) = weak.upgrade() else { return };
        state::with(|c| c.answering.remove(&invite_id));
        match answer {
            Ok(info) => {
                inbox::drop_invite(&shell, &invite_id);
                let opened = info.and_then(|info| registry::upsert(&info, &account));
                core::refresh_soon::<Win>();
                if let Some(e) = opened {
                    door::close(&shell);
                    core::show::<Win>(&shell, &e.id, true);
                } else {
                    door::render(&shell);
                }
            }
            Err(f) if f.status == 404 => {
                inbox::drop_invite(&shell, &invite_id);
                core::refresh_soon::<Win>();
                // Already a member: open it anyway.
                match canvas.and_then(|c| registry::entry(&c)).filter(|_| accept) {
                    Some(e) => {
                        door::close(&shell);
                        core::show::<Win>(&shell, &e.id, true);
                    }
                    None => {
                        state::with(|c| c.note = Some(copy::CLOSED_INVITE.into()));
                        door::render(&shell);
                        shell.toast(copy::CLOSED_INVITE);
                    }
                }
            }
            Err(f) => {
                let text = copy::plain(
                    &f,
                    &if accept { Doing::Joining } else { Doing::Other },
                    &host_text(),
                );
                state::with(|c| c.answer_failed.insert(invite_id.clone(), text.clone()));
                super::pills::sync(&shell, None);
                door::render(&shell);
                shell.toast(&text);
            }
        }
    });
}
