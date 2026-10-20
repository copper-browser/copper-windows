//! What the Passwords panel does with Credential Manager: read a password
//! to show or copy (only when asked), remove one, and add one typed in.
//! Each runs off the window's thread and comes back to the panel if it is
//! still on show.

use super::{ID, Manager, SHOWN_FOR, Shown, manager};
use crate::panels;
use crate::platform::Shell;
use crate::settings::sites::host_of;
use copper_creds::{CredentialId, Login, Source};
use copper_shell_core::forms::{self, rt};
use std::sync::Arc;

/// What a failed read of Credential Manager says.
pub(super) const UNREACHABLE: &str = "Copper couldn't reach Credential Manager";

impl Manager {
    /// Run `work` on the vault, then `then` here with its answer.
    pub(super) fn vault<T: Send + 'static, F>(
        shell: &Shell,
        work: impl FnOnce(Arc<forms::vault::Vault>) -> F + 'static,
        then: impl FnOnce(&Shell, &Manager, Result<T, String>) + 'static,
    ) where
        F: Future<Output = Result<T, String>> + Send + 'static,
    {
        let vault = forms::vault();
        let weak = shell.downgrade();
        copper_win_rt::spawn_local(async move {
            let r = match vault {
                Some(v) => rt::run(work(v))
                    .await
                    .unwrap_or_else(|| Err(UNREACHABLE.into())),
                None => Err(UNREACHABLE.into()),
            };
            let Some(s) = weak.upgrade() else { return };
            if let Some(m) = manager(&s) {
                then(&s, &m, r);
                panels::refresh(&s, ID);
            }
        });
    }

    pub(super) fn show(&self, shell: &Shell, l: Login) {
        let ticket = {
            let mut st = self.st.borrow_mut();
            st.ticket += 1;
            st.ticket
        };
        let id = keychain(&l);
        Self::vault(
            shell,
            move |v| async move { v.secret(&id).await },
            move |s, m, r| match r {
                Ok(secret) => {
                    let mut st = m.st.borrow_mut();
                    if st.ticket != ticket {
                        return;
                    }
                    st.error = None;
                    st.shown = Some(Shown {
                        host: l.host,
                        user: l.user,
                        secret,
                        ticket,
                    });
                    drop(st);
                    let weak = s.downgrade();
                    copper_win_rt::spawn_local(async move {
                        copper_win_rt::sleep(SHOWN_FOR).await;
                        let Some(s) = weak.upgrade() else { return };
                        if let Some(m) = manager(&s) {
                            let mut st = m.st.borrow_mut();
                            if st.shown.as_ref().is_some_and(|x| x.ticket == ticket) {
                                st.shown = None;
                                drop(st);
                                panels::refresh(&s, ID);
                            }
                        }
                    });
                }
                Err(e) => {
                    m.st.borrow_mut().error = Some(format!("Couldn't read that password: {e}"))
                }
            },
        );
    }

    /// Copy: the password to the clipboard, read only now.
    pub(super) fn copy(&self, shell: &Shell, l: Login) {
        let id = keychain(&l);
        Self::vault(
            shell,
            move |v| async move { v.secret(&id).await },
            |s, m, r| match r {
                Ok(secret) => {
                    s.view().copy_text(&secret);
                    s.toast("Password copied");
                }
                Err(e) => {
                    m.st.borrow_mut().error = Some(format!("Couldn't read that password: {e}"))
                }
            },
        );
    }

    /// Remove: the sign-in out of Credential Manager, then the list again.
    pub(super) fn remove(&self, shell: &Shell, l: Login) {
        let (host, user) = (l.host.clone(), l.user.clone());
        Self::vault(
            shell,
            move |v| async move { v.forget(&host, &user).await },
            move |s, m, r| match r {
                Ok(()) => {
                    s.toast(&format!("Password removed for {}", l.host));
                    m.st.borrow_mut().logins = None;
                }
                Err(e) => {
                    m.st.borrow_mut().error =
                        Some(format!("Credential Manager wouldn't remove it: {e}"))
                }
            },
        );
    }
    /// Add / Cancel: the form opens empty with the caret in Site, or
    /// closes, forgetting what was typed, with the caret back in search.
    pub(super) fn toggle_add(&self, shell: &Shell) {
        let adding = {
            let mut st = self.st.borrow_mut();
            st.adding = !st.adding;
            st.site.clear();
            st.user.clear();
            st.password.clear();
            st.adding
        };
        if let Some(n) = self.nodes.get() {
            shell.view().chrome().edit(move |ui, _| {
                for f in [n.site, n.user, n.password] {
                    ui.set_field_text(f, "", false);
                }
                ui.focus(Some(if adding { n.site } else { n.hunt }));
            });
        }
        self.controls(shell);
    }

    /// Add's Save: the typed sign-in into Credential Manager.
    pub(super) fn save(&self, shell: &Shell) {
        let (host, user, password) = {
            let mut st = self.st.borrow_mut();
            let host = host_of(&st.site);
            if host.is_empty() || st.password.is_empty() || st.saving {
                return;
            }
            st.saving = true;
            (host, st.user.trim().to_string(), st.password.clone())
        };
        let said = host.clone();
        Self::vault(
            shell,
            move |v| async move { v.save(Source::Keychain, &host, &user, &password).await },
            move |s, m, r| {
                let mut st = m.st.borrow_mut();
                st.saving = false;
                match r {
                    Ok(()) => {
                        st.adding = false;
                        st.site.clear();
                        st.user.clear();
                        st.password.clear();
                        st.logins = None;
                        st.error = None;
                        drop(st);
                        s.toast(&format!("Password saved for {said}"));
                    }
                    Err(e) => st.error = Some(format!("Credential Manager refused it: {e}")),
                }
            },
        );
        self.controls(shell);
    }
}

/// Credential Manager's id for a listed sign-in.
fn keychain(l: &Login) -> CredentialId {
    CredentialId::Keychain {
        host: l.host.clone(),
        user: l.user.clone(),
    }
}
