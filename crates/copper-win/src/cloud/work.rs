//! What Settings › Cloud's controls do. Every request runs on the cloud's
//! runtime ([`cloud::run_then`]) and its answer comes back to the window's
//! thread, into the page's state, and the page draws again; nothing here
//! waits. The protocol — connecting, pinning, pairing, single sign-on,
//! sync, deleting history — is copper-cloud's and copper-shell-core's.

use super::el::{Act, FieldId};
use super::model::{self, Outcome, PairCard};
use super::page::{self, refresh, with};
use super::state::{Minted, Snap};
use super::view::connect;
use crate::platform::{Shell, WinLoop};
use copper_cloud::{Failure, JoinCode};
use copper_shell_core::cloud::sso::{Opener, SsoTarget, sign_in};
use copper_shell_core::cloud::{self, Cloud};
use copper_shell_core::{OpenTab, Question, ShellView};
use std::sync::Arc;

/// How long "Copied" shows.
const COPIED_FOR: f64 = 2.0;

/// Carry out `act`.
pub fn run(shell: &Shell, act: Act) {
    let Some(c) = cloud::get() else { return };
    match act {
        Act::Submit => connect(shell, c),
        Act::ToggleManual => with(|st| {
            st.manual = !st.manual;
            st.problem = None;
            st.unknown = None;
        }),
        Act::Paste => paste(shell),
        Act::DismissOffer => sign_in().dismiss_offer(),
        Act::SsoCancel => sign_in().cancel(),
        Act::ToggleDetails => with(|st| st.details = !st.details),
        Act::Disconnect => ask_disconnect(shell, c),
        Act::Mode(m) => with(|st| {
            st.mode = m;
            st.account_problem = None;
        }),
        Act::AccountSubmit => submit_account(shell, c),
        Act::SsoContinue => continue_linked(c),
        Act::UsePassword => with(|st| st.use_password = true),
        Act::Domain(d) => {
            if c.sync.is_on() {
                let sync = c.sync.clone();
                let on = !sync.syncs(d);
                cloud::spawn(async move { sync.set(d, on).await });
            } else {
                let saved = c.sync.enabled();
                with(|st| {
                    let mut chosen = st.chosen(&saved);
                    if !chosen.remove(&d) {
                        chosen.insert(d);
                    }
                    st.chosen = Some(chosen);
                });
            }
        }
        Act::TurnOn => turn_on(shell, c),
        Act::SyncNow => cloud::sync_now(),
        Act::TurnOff => {
            let sync = c.sync.clone();
            cloud::spawn(async move { sync.turn_off().await });
        }
        Act::Device(domain) => with(|st| {
            if !st.open_devices.remove(&domain) {
                st.open_devices.insert(domain);
            }
        }),
        Act::OpenTab(url) => {
            shell.open(
                &url,
                OpenTab {
                    foreground: true,
                    ..Default::default()
                },
            );
        }
        Act::SignOut => sign_out(shell, c),
        Act::MakeCode => make_code(shell, c),
        Act::CopyCode => copy_code(shell),
        Act::RevokeCode => ask_revoke(shell),
        Act::Span(span) => with(|st| st.span = span),
        Act::DeleteHistory => ask_delete(shell),
        Act::ToggleLog => with(|st| st.log_open = !st.log_open),
    }
    refresh(shell);
}

/// Return in field `f`: what the field is for.
pub fn submit(shell: &Shell, f: FieldId) {
    let next = match f {
        FieldId::Code | FieldId::Host | FieldId::Key | FieldId::Fingerprint => {
            return run(shell, Act::Submit);
        }
        FieldId::Email => Some(FieldId::Password),
        FieldId::Password if with(|st| st.mode) == model::Mode::Create => Some(FieldId::Name),
        FieldId::Password | FieldId::Name => return run(shell, Act::AccountSubmit),
        FieldId::DeviceName => {
            rename_device(Some(shell));
            return;
        }
        FieldId::Site => None,
    };
    if let Some(next) = next {
        focus(shell, next);
    }
}

/// Put the keyboard in field `f`.
fn focus(shell: &Shell, f: FieldId) {
    if let Some(id) = page::node_of(f) {
        shell.view().chrome().read(move |ui, _| ui.focus(Some(id)));
    }
}

/// What the page starts on showing: the linked instance's sign-in options
/// (for "Continue with …"), whether the cloud deletes history, the other
/// devices' tabs. Each once a showing.
pub fn on_show(shell: &Shell, s: &Snap) {
    let Some(c) = cloud::get() else { return };
    let Some(link) = s.link.clone() else { return };
    if !s.signed_in && !with(|st| std::mem::replace(&mut st.sso_asked, true)) {
        let w = shell.downgrade();
        cloud::run_then::<WinLoop, _, _>(
            async move {
                sign_in()
                    .discover(&link.endpoint, link.fingerprint.as_deref(), true)
                    .await
            },
            move |_| {
                if let Some(s) = w.upgrade() {
                    refresh(&s);
                }
            },
        );
    }
    if s.signed_in
        && s.deletes_history.is_none()
        && !with(|st| st.info_asked || std::mem::replace(&mut st.info_asking, true))
    {
        let client = c.client.clone();
        let w = shell.downgrade();
        cloud::run_then::<WinLoop, _, _>(
            async move { client.load_info(false).await.is_some() },
            move |_| {
                with(|st| {
                    st.info_asking = false;
                    st.info_asked = true;
                });
                if let Some(s) = w.upgrade() {
                    refresh(&s);
                }
            },
        );
    }
    if s.signed_in && s.sync_on && !with(|st| std::mem::replace(&mut st.devices_asked, true)) {
        let sync = c.sync.clone();
        cloud::spawn(async move { sync.refresh_devices().await });
    }
}

// MARK: 1. connect

/// The Connect button: connect a link code, pair with a pairing code, or
/// sign in through the browser with a join link — trusting the fingerprint
/// a person compared, when one is waiting.
fn connect(shell: &Shell, c: &'static Cloud) {
    let s = with(|st| page::snap(st));
    let (code, trusting, typed, busy) = with(|st| {
        (
            connect::read(&s, st).code().cloned(),
            page::seen(&s, st),
            connect::typed(st),
            st.connecting,
        )
    });
    let Some(code) = code else { return };
    if busy {
        return;
    }
    if let JoinCode::Sso(link) = code {
        with(|st| st.problem = None);
        drop(sign_in().start(c, SsoTarget::Join(link), trusting, opener()));
        return;
    }
    with(|st| {
        st.connecting = true;
        st.problem = None;
    });
    let client = c.client.clone();
    let pairing = matches!(code, JoinCode::Pairing(_));
    let w = shell.downgrade();
    cloud::run_then::<WinLoop, _, _>(
        async move {
            match code {
                JoinCode::Pairing(p) => client.pair(&p, trusting.as_deref()).await,
                JoinCode::Link(l) => client.connect(l, trusting.as_deref()).await.map(|_| ()),
                JoinCode::Sso(_) => Ok(()),
            }
        },
        move |r| {
            with(|st| st.connecting = false);
            match r {
                Ok(()) => {
                    c.start();
                    if pairing {
                        cloud::spawn(cloud::sync_after_sign_in(c));
                    }
                    with(|st| {
                        st.code.clear();
                        st.host.clear();
                        st.key.clear();
                        st.fingerprint.clear();
                        st.manual = false;
                        st.unknown = None;
                        st.sso_asked = false;
                    });
                }
                Err(f) => with(|st| match f.seen.clone() {
                    Some(seen) if f.code == "untrusted" => st.unknown = Some((typed, seen)),
                    _ => st.problem = Some(f.message),
                }),
            }
            if let Some(s) = w.upgrade() {
                refresh(&s);
            }
        },
    );
}

/// Paste: the clipboard into the Connect field.
fn paste(shell: &Shell) {
    let text = shell
        .view()
        .chrome()
        .read(|ui, _| super::clip::text(ui.hwnd()))
        .flatten();
    tracing::debug!(
        target: "copper::cloud",
        chars = text.as_ref().map_or(0, |t| t.chars().count()),
        "paste"
    );
    if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
        page::prefill(&text);
        sign_in().clear_problem();
    }
}

/// The system's browser, for a sign-in: `ShellExecuteW` on a thread of its
/// own with COM up (the cloud's runtime threads never start COM). Only the
/// host and path are logged — never the state or the PKCE challenge.
pub fn opener() -> Opener {
    Arc::new(|address: &str| {
        let shown = url::Url::parse(address)
            .map(|u| format!("{}{}", u.host_str().unwrap_or(""), u.path()))
            .unwrap_or_default();
        tracing::info!(target: "copper::cloud", at = %shown, "opening the browser to sign in");
        let address = address.to_string();
        std::thread::spawn(move || {
            use windows::Win32::System::Com::{
                COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
            };
            // SAFETY: this thread's own apartment, ended below if it began.
            let com =
                unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
            let r = crate::sys::shell::open(&address);
            if com.is_ok() {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
            r
        })
        .join()
        .unwrap_or_else(|_| Err("Copper couldn't open your browser".into()))
    })
}

/// Disconnect, once confirmed: this device's open tabs leave the server,
/// the session ends and the link is forgotten. Nothing is deleted.
fn ask_disconnect(shell: &Shell, c: &'static Cloud) {
    let host = c
        .client
        .link()
        .map(|l| l.endpoint.host_text())
        .unwrap_or_else(|| "this instance".into());
    let w = shell.downgrade();
    shell.view().ask(
        shell,
        Question {
            title: format!("Disconnect from {host}?"),
            detail: Some(
                "Copper signs out, stops syncing and forgets the instance. Nothing is deleted."
                    .into(),
            ),
            deny: "Cancel".into(),
            allow: "Disconnect".into(),
        },
        Box::new(move |yes| {
            if !yes {
                return;
            }
            let sync = c.sync.clone();
            let client = c.client.clone();
            cloud::run_then::<WinLoop, _, _>(
                async move {
                    sync.signing_out().await;
                    client.disconnect();
                },
                move |_| {
                    with(|st| {
                        st.details = false;
                        st.minted = None;
                        st.outcome = None;
                        st.sso_asked = false;
                        st.use_password = false;
                    });
                    if let Some(s) = w.upgrade() {
                        refresh(&s);
                    }
                },
            );
        }),
    );
}

// MARK: 2. account

/// Sign in or create the account with email and password.
fn submit_account(shell: &Shell, c: &'static Cloud) {
    let (mode, email, password, name, ready) = with(|st| {
        (
            st.mode,
            st.email.trim().to_string(),
            st.password.clone(),
            st.name.trim().to_string(),
            !st.working && model::account_ready(st.mode, &st.email, &st.password, &st.name),
        )
    });
    if !ready {
        return;
    }
    with(|st| {
        st.working = true;
        st.account_problem = None;
    });
    let client = c.client.clone();
    let w = shell.downgrade();
    cloud::run_then::<WinLoop, _, _>(
        async move {
            match mode {
                model::Mode::Create => client.sign_up(&email, &password, &name).await,
                model::Mode::SignIn => client.sign_in(&email, &password).await,
            }
        },
        move |r| {
            with(|st| {
                st.working = false;
                match r {
                    Ok(_) => {
                        st.password.clear();
                        st.chosen = None;
                    }
                    Err(f) => st.account_problem = Some(f.message),
                }
            });
            if let Some(s) = w.upgrade() {
                refresh(&s);
            }
        },
    );
}

/// "Continue with …" on the linked instance.
fn continue_linked(c: &'static Cloud) {
    let Some(link) = c.client.link() else { return };
    let Some(provider) = sign_in().first_provider(&link.endpoint) else {
        return;
    };
    with(|st| st.account_problem = None);
    drop(sign_in().start(
        c,
        SsoTarget::Linked {
            endpoint: link.endpoint,
            provider: provider.id,
        },
        None,
        opener(),
    ));
}

/// Sign out: this device's open tabs leave the server first; the code this
/// device made goes with the account.
fn sign_out(shell: &Shell, c: &'static Cloud) {
    let sync = c.sync.clone();
    let w = shell.downgrade();
    cloud::run_then::<WinLoop, _, _>(async move { sync.sign_out().await }, move |_| {
        with(|st| {
            st.minted = None;
            st.outcome = None;
            st.device_name = None;
            st.erase_result = None;
            st.erase_problem = None;
        });
        if let Some(s) = w.upgrade() {
            refresh(&s);
        }
    });
}

/// Keep a device name typed in Settings: on Return (`shell`) or when the
/// page closes (`None`: nothing to redraw, a refusal is only logged).
pub fn rename_device(shell: Option<&Shell>) {
    let Some(c) = cloud::get() else { return };
    let typed = with(|st| st.device_name.clone());
    let Some(name) = typed.map(|t| t.trim().to_string()) else {
        return;
    };
    if name.is_empty() || name == c.client.device_name() {
        with(|st| st.device_name = None);
        return;
    }
    let client = c.client.clone();
    let w = shell.map(|s| s.downgrade());
    cloud::run_then::<WinLoop, _, _>(async move { client.rename_device(&name).await }, move |r| {
        with(|st| match r {
            Ok(()) => {
                st.device_name = None;
                st.rename_problem = None;
            }
            Err(f) => {
                tracing::info!(target: "copper::cloud", code = %f.code, "device not renamed");
                st.rename_problem = Some(f.message);
            }
        });
        if let Some(s) = w.and_then(|w| w.upgrade()) {
            refresh(&s);
        }
    });
}

// MARK: 3. sync

/// "Turn on sync" with the switches chosen.
fn turn_on(shell: &Shell, c: &'static Cloud) {
    let saved = c.sync.enabled();
    let chosen: Vec<_> = with(|st| st.chosen(&saved)).into_iter().collect();
    if chosen.is_empty() || with(|st| std::mem::replace(&mut st.starting, true)) {
        return;
    }
    let sync = c.sync.clone();
    let w = shell.downgrade();
    cloud::run_then::<WinLoop, _, _>(async move { sync.turn_on(&chosen).await }, move |_| {
        with(|st| {
            st.starting = false;
            st.chosen = None;
        });
        if let Some(s) = w.upgrade() {
            refresh(&s);
        }
    });
}

// MARK: pairing

/// Make a one-time code (revoking the last one first, if it is unused).
fn make_code(shell: &Shell, c: &'static Cloud) {
    if !c.client.is_signed_in() || with(|st| st.pair_working) {
        return;
    }
    let old = with(|st| {
        st.pair_working = true;
        st.pair_problem = None;
        st.minted
            .as_ref()
            .filter(|_| st.outcome.is_none())
            .map(|m| m.id.clone())
    });
    let client = c.client.clone();
    let w = shell.downgrade();
    cloud::run_then::<WinLoop, _, _>(
        async move {
            if let Some(id) = old {
                let _ = client.revoke_pairing_code(&id).await;
            }
            client.mint_pairing_code(None).await
        },
        move |r| {
            with(|st| {
                st.pair_working = false;
                match r {
                    Ok(m) => {
                        let expires = copper_core::date::IsoDate::parse(&m.expires_at)
                            .map(|d| d.unix() as f64)
                            .unwrap_or_else(|| {
                                copper_core::date::now_unix() + model::PAIR_LIFETIME
                            });
                        st.minted = Some(Minted {
                            id: m.id.clone(),
                            code: m.code.clone(),
                            link: m.link.clone(),
                            expires,
                        });
                        st.outcome = None;
                        st.copied_until = 0.0;
                    }
                    Err(f) => st.pair_problem = Some(f.message),
                }
            });
            if let Some(s) = w.upgrade() {
                refresh(&s);
            }
        },
    );
}

/// Copy the whole code, on one line.
fn copy_code(shell: &Shell) {
    let Some(link) = with(|st| st.minted.as_ref().map(|m| m.link.clone())) else {
        return;
    };
    let copied = shell
        .view()
        .chrome()
        .read(move |ui, _| ui.copy_text(&link))
        .unwrap_or(false);
    if copied {
        with(|st| st.copied_until = copper_core::date::now_unix() + COPIED_FOR);
    }
}

/// Revoke the live code, once confirmed.
fn ask_revoke(shell: &Shell) {
    let w = shell.downgrade();
    shell.view().ask(
        shell,
        Question {
            title: "Revoke this pairing code?".into(),
            detail: Some(
                "Nobody will be able to use it. A device already paired with it stays signed in."
                    .into(),
            ),
            deny: "Cancel".into(),
            allow: "Revoke".into(),
        },
        Box::new(move |yes| {
            let (Some(c), Some(id)) = (
                cloud::get(),
                with(|st| st.minted.as_ref().map(|m| m.id.clone())),
            ) else {
                return;
            };
            if !yes {
                return;
            }
            let client = c.client.clone();
            cloud::run_then::<WinLoop, _, _>(
                async move { client.revoke_pairing_code(&id).await },
                move |r| {
                    with(|st| {
                        st.outcome = Some(Outcome::Revoked);
                        st.pair_problem = r.err().map(|f: Failure| f.message);
                    });
                    if let Some(s) = w.upgrade() {
                        refresh(&s);
                    }
                },
            );
        }),
    );
}

/// While a code is live: has it been used? Gone from the unused codes
/// before it expired means another device took it.
pub fn check_code(shell: &Shell) {
    let now = copper_core::date::now_unix();
    let Some(id) = with(|st| match st.pair_card(now) {
        PairCard::Live(_) if !st.pair_working => st.minted.as_ref().map(|m| m.id.clone()),
        _ => None,
    }) else {
        return;
    };
    let Some(c) = cloud::get() else { return };
    let client = c.client.clone();
    let w = shell.downgrade();
    cloud::run_then::<WinLoop, _, _>(async move { client.pairing_codes().await }, move |r| {
        let Ok(list) = r else { return };
        let open = list
            .iter()
            .any(|p| p.id.eq_ignore_ascii_case(&id) && p.used_at.is_none());
        if open {
            return;
        }
        let changed = with(|st| {
            let same = st.minted.as_ref().is_some_and(|m| m.id == id);
            let live = matches!(
                st.pair_card(copper_core::date::now_unix()),
                PairCard::Live(_)
            );
            if same && live && st.outcome.is_none() {
                st.outcome = Some(Outcome::Used);
                true
            } else {
                false
            }
        });
        if changed && let Some(s) = w.upgrade() {
            refresh(&s);
        }
    });
}

// MARK: delete history on cloud

/// Delete history on cloud, once the person has said yes to the exact
/// range and site.
fn ask_delete(shell: &Shell) {
    let (span, site, busy) = with(|st| (st.span, model::typed_site(&st.site), st.erasing));
    let Ok(site) = site else { return };
    if busy {
        return;
    }
    let w = shell.downgrade();
    let question = model::delete_question(span, site.as_deref());
    shell.view().ask(
        shell,
        Question {
            title: question,
            detail: Some(model::DELETE_MESSAGE.into()),
            deny: "Cancel".into(),
            allow: "Delete".into(),
        },
        Box::new(move |yes| {
            if !yes {
                return;
            }
            let Some(c) = cloud::get() else { return };
            let selector = model::selector(span, site.as_deref(), copper_core::date::now_unix());
            with(|st| {
                st.erasing = true;
                st.looking = selector.scans();
                st.erase_result = None;
                st.erase_problem = None;
            });
            if let Some(s) = w.upgrade() {
                refresh(&s);
            }
            let sync = c.sync.clone();
            let w = w.clone();
            cloud::run_then::<WinLoop, _, _>(
                async move { sync.delete_history(&selector).await },
                move |r| {
                    with(|st| {
                        st.erasing = false;
                        st.looking = false;
                        match r {
                            Ok(o) => st.erase_result = Some(model::deleted_text(o.deleted, o.cut)),
                            Err(f) => st.erase_problem = Some(f.message),
                        }
                    });
                    if let Some(s) = w.upgrade() {
                        refresh(&s);
                    }
                },
            );
        }),
    );
}
