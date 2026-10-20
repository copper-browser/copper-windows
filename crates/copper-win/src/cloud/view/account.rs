//! 2 · Your account on this instance (Mac: `CloudAccountForm`) and, signed
//! in, the Account card (`CloudAccountCard`): who you are, Sign out, and
//! this device's name.

use super::super::el::{
    Act, Choice, El, FieldId, Kind, PillLook, Tone, field, line, pill, problem, quiet, row,
    section, spacer, text,
};
use super::super::model::{self, MIN_PASSWORD, Mode};
use super::super::state::{Snap, State};
use super::sso_waiting;
use copper_cloud::sso::SsoProvider;
use copper_shell_core::cloud::sso::domain_hint;

/// The linked instance's single sign-on, when it offers one.
pub fn provider(s: &Snap) -> Option<&SsoProvider> {
    s.sso_info.as_ref()?.offered().first()
}

/// The password form shows: always without single sign-on; with it,
/// unless the instance keeps passwords for its admins.
pub fn passwords(s: &Snap, st: &State) -> bool {
    match (&s.sso_info, provider(s)) {
        (Some(info), Some(_)) => info.password_signin || st.use_password,
        _ => true,
    }
}

/// The sign-in step.
pub fn form(s: &Snap, st: &State) -> El {
    let mut rows = Vec::new();
    let endpoint = s.link.as_ref().map(|l| &l.endpoint);
    if let (Some(p), Some(ep)) = (provider(s), endpoint) {
        if s.sso.running_for(ep) {
            rows.push(sso_waiting(s));
        } else {
            let hint = domain_hint(&p.domains)
                .map(|h| format!("{h} — Copper opens your browser."))
                .unwrap_or_else(|| "Copper opens your browser to sign in.".into());
            rows.push(row(
                "sso",
                vec![
                    El::new("words", Kind::Stack { gap: 2.0 }).with(vec![
                        text(
                            "title",
                            &format!("Sign in with your {} account", p.title()),
                            13.0,
                            Tone::Ink,
                        ),
                        text("hint", &hint, 11.5, Tone::Muted),
                    ]),
                    spacer(),
                    pill("go", &p.button(), Act::SsoContinue, PillLook::Filled, true),
                ],
            ));
        }
    }
    if passwords(s, st) {
        rows.extend(password_rows(st));
    } else {
        rows.push(row(
            "password-door",
            vec![
                quiet(
                    "use-password",
                    "Sign in with a password instead",
                    Act::UsePassword,
                ),
                spacer(),
            ],
        ));
    }
    let words = st.account_problem.clone().or_else(|| {
        endpoint
            .and_then(|ep| s.sso.problem_for(ep))
            .map(str::to_string)
    });
    section(
        "signin",
        Some("2 · Your account on this instance"),
        rows,
        words.map(|w| problem("problem", &w)).into_iter().collect(),
    )
}

/// Sign in or create: the choice, email, password, name, the button.
fn password_rows(st: &State) -> Vec<El> {
    let create = st.mode == Mode::Create;
    let mut rows = vec![
        row(
            "mode",
            vec![El::new(
                "choice",
                Kind::Segmented {
                    options: vec![
                        Choice {
                            label: "Sign in".into(),
                            act: Act::Mode(Mode::SignIn),
                            on: !create,
                        },
                        Choice {
                            label: "Create account".into(),
                            act: Act::Mode(Mode::Create),
                            on: create,
                        },
                    ],
                    name: "Account".into(),
                    wide: true,
                },
            )],
        ),
        line(
            "email",
            "Email",
            None,
            vec![field(FieldId::Email, "you@example.com", Some(220.0), false)],
        ),
    ];
    let floor = format!("At least {MIN_PASSWORD} characters");
    rows.push(line(
        "password",
        "Password",
        create.then_some(floor.as_str()),
        vec![field(
            FieldId::Password,
            if create {
                "10 or more characters"
            } else {
                "password"
            },
            Some(220.0),
            false,
        )],
    ));
    if create {
        rows.push(line(
            "name",
            "Your name",
            Some("Shown beside your cursor on shared canvases"),
            vec![field(FieldId::Name, "Name", Some(220.0), false)],
        ));
    }
    let ready = model::account_ready(st.mode, &st.email, &st.password, &st.name);
    rows.push(row(
        "submit",
        vec![
            text("note", model::account_note(st.mode), 11.5, Tone::Muted),
            spacer(),
            pill(
                "go",
                model::account_button(st.mode, st.working),
                Act::AccountSubmit,
                if ready && !st.working {
                    PillLook::Filled
                } else {
                    PillLook::Plain
                },
                ready && !st.working,
            ),
        ],
    ));
    rows
}

/// Signed in: who, Sign out, and this device's name.
pub fn card(s: &Snap, st: &State) -> El {
    let (name, email) = s
        .account
        .clone()
        .unwrap_or_else(|| ("Signed in".into(), String::new()));
    let title = if name.trim().is_empty() {
        "Signed in".to_string()
    } else {
        name
    };
    let rows = vec![
        line(
            "who",
            &title,
            Some(email.as_str()).filter(|e| !e.is_empty()),
            vec![pill(
                "signout",
                "Sign out",
                Act::SignOut,
                PillLook::Plain,
                true,
            )],
        ),
        line(
            "device",
            "This device",
            Some("The name your other devices see it by"),
            vec![field(
                FieldId::DeviceName,
                "Device name",
                Some(170.0),
                false,
            )],
        ),
    ];
    section(
        "account",
        Some("Account"),
        rows,
        st.rename_problem
            .as_deref()
            .map(|w| problem("problem", w))
            .into_iter()
            .collect(),
    )
}
