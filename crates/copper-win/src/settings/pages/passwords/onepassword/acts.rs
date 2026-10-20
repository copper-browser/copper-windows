//! What the 1Password card's buttons do: each runs `op` on the forms lane
//! through [`run`], busy until it answers, and forgets what was typed
//! once the step that needed it is done.

use super::*;

/// Mark the card busy and run `work` on 1Password; `then` gets its answer
/// (with what `op` should remember) once the card is no longer busy.
fn run<F>(
    shell: &Shell,
    waiting: bool,
    work: impl FnOnce(Arc<forms::vault::Vault>) -> F + 'static,
    then: impl FnOnce(&Shell, Result<(), String>) + 'static,
) where
    F: Future<Output = (Result<(), String>, Remembered)> + Send + 'static,
{
    if with(|c| std::mem::replace(&mut c.busy, true)) {
        return;
    }
    with(|c| c.waiting = waiting);
    redraw(shell, "passwords");
    live::run(shell, work, move |s, answer| {
        with(|c| {
            c.busy = false;
            c.waiting = false;
            c.dismissed = None;
        });
        let r = match answer {
            Some((r, remembered)) => {
                forms::remember_op(s, &remembered);
                r
            }
            None => Err("Copper couldn't reach 1Password".into()),
        };
        if r.is_ok() {
            with(|c| c.synced = Some(copper_core::date::now_unix()));
        }
        then(s, r);
        redraw(s, "passwords");
    });
}

/// The chosen account's user id, when there is a choice to make.
fn chosen_account() -> Option<String> {
    let accounts = live::with(|l| l.op.accounts.clone());
    with(|c| c.chosen.clone())
        .filter(|id| accounts.iter().any(|(a, _)| a == id))
        .or_else(|| accounts.first().map(|(a, _)| a.clone()))
}

/// Nothing secret stays typed once it has been used.
fn forget_typed() {
    with(|c| {
        c.password.clear();
        c.secret_key.clear();
        c.token.clear();
    });
}

/// The card's actions (`op.*`); `false` if `action` isn't one.
pub fn act(shell: &Shell, action: &str, value: &str) -> bool {
    let Some(what) = action.strip_prefix("op.") else {
        return false;
    };
    match what {
        "address" => with(|c| c.address = value.to_string()),
        "email" => with(|c| c.email = value.to_string()),
        "key" => with(|c| c.secret_key = value.to_string()),
        "password" => with(|c| c.password = value.to_string()),
        "token" => with(|c| c.token = value.to_string()),
        "dismiss" => {
            let msg = live::with(|l| l.snap.op_problem.clone());
            with(|c| c.dismissed = msg);
        }
        "copy" => {
            shell.view().copy_text(INSTALL);
            shell.toast("Install command copied");
        }
        "path" => {
            let mode = Mode::parse(value);
            with(|c| c.path = mode);
        }
        "account" if value == "+new" => with(|c| c.adding = true),
        "account" => with(|c| c.chosen = Some(value.to_string())),
        "cancel" => with(|c| {
            c.adding = false;
            c.password.clear();
            c.secret_key.clear();
        }),
        "use-password" => with(|c| c.path = Some(Mode::Password)),
        "open-app" => match live::op_app() {
            Some(app) => {
                if let Err(e) = crate::sys::shell::open(&app.to_string_lossy()) {
                    tracing::warn!("opening 1Password: {e}");
                    shell.toast("Couldn't open 1Password");
                }
            }
            None => shell.toast("The 1Password app isn't installed"),
        },
        "check" => run(
            shell,
            false,
            |v| async move {
                let mut op = v.op.lock().await;
                let remembered = op.remembered();
                op.start(remembered).await;
                (Ok(()), op.remembered())
            },
            |_, _| {},
        ),
        "app" => {
            let account = live::with(|l| l.op.accounts.len() > 1)
                .then(chosen_account)
                .flatten();
            run(
                shell,
                true,
                move |v| async move {
                    let mut op = v.op.lock().await;
                    let r = op.unlock_with_app(account.as_deref()).await;
                    (r.map_err(|e| e.message), op.remembered())
                },
                |_, _| {},
            );
        }
        "signin" => {
            let password = with(|c| c.password.clone());
            let account = chosen_account();
            run(
                shell,
                false,
                move |v| async move {
                    let mut op = v.op.lock().await;
                    let r = op.sign_in(&password, account.as_deref()).await;
                    (r.map_err(|e| e.message), op.remembered())
                },
                |_, r| {
                    if r.is_ok() {
                        forget_typed();
                    }
                },
            );
        }
        "add" => {
            let (address, email, key, password) = with(|c| {
                (
                    c.address.clone(),
                    c.email.clone(),
                    c.secret_key.clone(),
                    c.password.clone(),
                )
            });
            if email.trim().is_empty() || key.trim().is_empty() || password.is_empty() {
                return true;
            }
            let address = if address.trim().is_empty() {
                DEFAULT_ADDRESS.to_string()
            } else {
                address
            };
            run(
                shell,
                false,
                move |v| async move {
                    let mut op = v.op.lock().await;
                    let r = op.add_account(&address, &email, &key, &password).await;
                    (r.map_err(|e| e.message), op.remembered())
                },
                |_, r| {
                    if r.is_ok() {
                        forget_typed();
                        with(|c| c.adding = false);
                    }
                },
            );
        }
        "connect" => {
            let token = with(|c| c.token.clone());
            run(
                shell,
                false,
                move |v| async move {
                    let mut op = v.op.lock().await;
                    let r = op.connect(&token).await;
                    (r.map_err(|e| e.message), op.remembered())
                },
                |_, r| {
                    if r.is_ok() {
                        forget_typed();
                    }
                },
            );
        }
        "unlock" => {
            let (password, mode) = (with(|c| c.password.clone()), live::with(|l| l.snap.op_mode));
            run(
                shell,
                matches!(mode, Some(Mode::App) | None),
                move |v| async move {
                    let mut op = v.op.lock().await;
                    let p = (!password.is_empty()).then_some(password.as_str());
                    let r = op.unlock(p).await;
                    (r.map_err(|e| e.message), op.remembered())
                },
                |_, r| {
                    if r.is_ok() {
                        forget_typed();
                    }
                },
            );
        }
        "lock" => run(
            shell,
            false,
            |v| async move {
                let mut op = v.op.lock().await;
                op.lock().await;
                (Ok(()), op.remembered())
            },
            |_, _| {},
        ),
        "signout" => run(
            shell,
            false,
            |v| async move {
                let mut op = v.op.lock().await;
                op.sign_out().await;
                (Ok(()), op.remembered())
            },
            |s, _| {
                if forms::chosen_backend(s) == Source::OnePassword {
                    forms::set_backend(s, Source::Keychain);
                }
            },
        ),
        "sync" => run(
            shell,
            false,
            |v| async move {
                let mut op = v.op.lock().await;
                let r = op.sync().await;
                (r.map_err(|e| e.message), op.remembered())
            },
            |s, r| {
                if r.is_ok() {
                    s.toast("1Password synced");
                }
            },
        ),
        "vault" => {
            let id = value.to_string();
            let keep = id.clone();
            shell.update_settings(move |s| s.set(SAVE_VAULT, keep.as_str()));
            run(
                shell,
                false,
                move |v| async move {
                    let mut op = v.op.lock().await;
                    op.set_save_vault(Some(id));
                    (Ok(()), op.remembered())
                },
                |_, _| {},
            );
        }
        "stay" => {
            let on = crate::settings::pages::on(value);
            shell.update_settings(|s| s.set(keys::OP_STAY, on));
        }
        _ => return false,
    }
    true
}
