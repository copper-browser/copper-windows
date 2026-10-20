//! What the Bitwarden card's buttons do: each runs `bw` on the forms lane
//! through [`run`], busy until it answers; a sign-in that needs a
//! two-step code asks for it and goes again with it.

use super::*;

/// Mark the card busy and run `work` on Bitwarden; `then` gets its
/// answer with the card no longer busy.
fn run<F>(
    shell: &Shell,
    work: impl FnOnce(std::sync::Arc<copper_shell_core::forms::vault::Vault>) -> F + 'static,
    then: impl FnOnce(&Shell, Result<(), String>) + 'static,
) where
    F: Future<Output = Result<(), String>> + Send + 'static,
{
    if with(|c| std::mem::replace(&mut c.busy, true)) {
        return;
    }
    with(|c| c.error = None);
    redraw(shell, "passwords");
    live::run(shell, work, move |s, answer| {
        with(|c| c.busy = false);
        then(
            s,
            answer.unwrap_or_else(|| Err("Copper couldn't reach Bitwarden".into())),
        );
        redraw(s, "passwords");
    });
}

/// Keep a failure for the card to show.
fn failed(e: String) {
    with(|c| c.error = Some(e));
}

/// The card's actions (`bw.*`); `false` if `action` isn't one.
pub fn act(shell: &Shell, action: &str, value: &str) -> bool {
    let Some(what) = action.strip_prefix("bw.") else {
        return false;
    };
    match what {
        "server" => with(|c| c.server = Some(value.to_string())),
        "email" => with(|c| c.email = value.to_string()),
        "password" => with(|c| c.password = value.to_string()),
        "code" => with(|c| c.code = value.to_string()),
        "dismiss" => with(|c| c.error = None),
        "copy" => {
            shell.view().copy_text(INSTALL);
            shell.toast("Install command copied");
        }
        "check" => run(
            shell,
            |v| async move {
                v.bw.lock().await.refresh_status().await;
                Ok(())
            },
            |_, _| {},
        ),
        "signin" => sign_in(shell, None),
        "continue" => {
            let (method, code) = with(|c| {
                let m = match c.step {
                    Step::Code(m) => m,
                    _ => 0,
                };
                (m, c.code.trim().to_string())
            });
            if !code.is_empty() {
                sign_in(shell, Some((method, code)));
            }
        }
        "cancel" => with(|c| {
            c.step = Step::Credentials;
            c.password.clear();
            c.code.clear();
            c.error = None;
        }),
        "unlock" => {
            let password = with(|c| c.password.clone());
            if password.is_empty() {
                return true;
            }
            run(
                shell,
                move |v| async move { v.bw.lock().await.unlock(&password).await.map_err(|e| e.0) },
                |_, r| match r {
                    Ok(()) => finished(),
                    Err(e) => failed(e),
                },
            );
        }
        "lock" => run(
            shell,
            |v| async move {
                v.bw.lock().await.lock().await;
                Ok(())
            },
            |_, _| {},
        ),
        "signout" => run(
            shell,
            |v| async move {
                v.bw.lock().await.logout().await;
                Ok(())
            },
            |_, _| finished(),
        ),
        "sync" => run(
            shell,
            |v| async move { v.bw.lock().await.sync().await.map_err(|e| e.0) },
            |s, r| match r {
                Ok(()) => s.toast("Bitwarden synced"),
                Err(e) => failed(e),
            },
        ),
        "stay" => {
            let on = crate::settings::pages::on(value);
            shell.update_settings(|s| s.set(keys::BW_STAY, on));
            run(
                shell,
                move |v| async move {
                    v.bw.lock().await.set_stay_unlocked(on);
                    Ok(())
                },
                |_, _| {},
            );
        }
        "autolock" => {
            let minutes: u32 = value.parse().unwrap_or(0);
            shell.update_settings(|s| s.set(AUTOLOCK, minutes));
        }
        _ => {
            if let Some(m) = what.strip_prefix("method:") {
                let m = m.parse().unwrap_or(0);
                with(|c| {
                    c.step = Step::Code(m);
                    c.code.clear();
                });
            } else {
                return false;
            }
        }
    }
    true
}

/// Signed in or unlocked: nothing typed is kept.
fn finished() {
    with(|c| {
        c.step = Step::Credentials;
        c.password.clear();
        c.code.clear();
        c.error = None;
    });
}

/// Email and master password (and a code, once one is asked for) to `bw`,
/// pointing it at the server typed first when that changed. What comes
/// back decides the next step: open, a method to choose, or a failure.
fn sign_in(shell: &Shell, code: Option<(u8, String)>) {
    let (typed, email, password) =
        with(|c| (c.server.clone(), c.email.clone(), c.password.clone()));
    if email.trim().is_empty() || password.is_empty() {
        return;
    }
    let server = typed
        .or_else(|| shell.settings().string(keys::BW_SERVER).map(str::to_string))
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty());
    if let Some(s) = &server {
        let s = s.clone();
        shell.update_settings(move |st| st.set(keys::BW_SERVER, s.as_str()));
    }
    let asked_code = code.is_some();
    run(
        shell,
        move |v| async move {
            let mut bw = v.bw.lock().await;
            if let Some(server) = server {
                let current = bw
                    .cli_server()
                    .unwrap_or(DEFAULT_SERVER)
                    .trim_end_matches('/');
                if current != server {
                    bw.configure(&server).await.map_err(|e| e.0)?;
                }
            }
            let (method, code) = match &code {
                Some((m, c)) => (Some(*m), Some(c.as_str())),
                None => (None, None),
            };
            bw.login(&email, &password, method, code)
                .await
                .map_err(|e| e.0)
        },
        move |_, r| match r {
            Ok(()) => finished(),
            Err(e) if e == NEEDS_CODE && !asked_code => with(|c| c.step = Step::Method),
            Err(e) => failed(e),
        },
    );
}
