//! 1 · Connect to an instance (Mac: `CloudConnectCard`): one field for any
//! code — a link code connects and leaves signing in for step 2, a pairing
//! code connects, signs in and turns on sync on one press, a join link does
//! the same through the company's sign-in. Or the address, key and
//! fingerprint by hand. Read locally as typed; nothing leaves this device
//! until the button is pressed.

use super::super::el::{
    Act, El, FieldId, Kind, PillLook, Tone, block, field, line, mono, pill, problem, quiet, row,
    section, spacer, text,
};
use super::super::model::{self, KindIcon, Read};
use super::super::state::{Snap, State};
use super::{glyph, sso_waiting};
use copper_cloud::JoinCode;
use copper_shell_core::cloud::sso::domain_hint;
use copper_win_ui::Weight;

/// What the card's field (or fields) hold now.
pub fn read(s: &Snap, st: &State) -> Read {
    if st.manual {
        model::read_manual(&st.host, &st.key, &st.fingerprint)
    } else {
        model::read_code(&st.code, s.link.as_ref())
    }
}

/// The text a code is known by while a certificate waits to be trusted
/// (the pasted code, or the hand-entered fields together).
pub fn typed(st: &State) -> String {
    if st.manual {
        format!(
            "{}\n{}\n{}",
            st.host.trim(),
            st.key.trim(),
            st.fingerprint.trim()
        )
    } else {
        st.code.trim().to_string()
    }
}

/// "Google": what the join link's instance calls its provider, once it has
/// said; a name made from the id until then (no request just for this).
pub fn provider_name(s: &Snap, code: &JoinCode) -> String {
    match code {
        JoinCode::Sso(link) => s
            .join_info
            .as_ref()
            .and_then(|i| i.provider(&link.provider))
            .map(|p| p.title())
            .unwrap_or_else(|| copper_cloud::sso::provider_title(&link.provider)),
        _ => String::new(),
    }
}

/// The fingerprint waiting to be compared and trusted for what is typed:
/// the one a pairing or link code's instance showed, or the join link's.
pub fn seen(s: &Snap, st: &State, code: &JoinCode) -> Option<String> {
    match code {
        JoinCode::Sso(link) if link.fingerprint.is_none() && !s.sso.phase.busy() => {
            s.sso.seen_for(&link.endpoint).map(str::to_string)
        }
        JoinCode::Sso(_) => None,
        _ => st.seen_for(&typed(st)).map(str::to_string),
    }
}

/// The card.
pub fn card(s: &Snap, st: &State) -> El {
    let parsed = read(s, st);
    let code = parsed.code();
    let mut rows = Vec::new();
    if st.manual {
        rows.push(line(
            "host",
            "Address",
            Some("host:port, or https://…"),
            vec![field(
                FieldId::Host,
                "cloud.example.com:443",
                Some(200.0),
                false,
            )],
        ));
        rows.push(line(
            "key",
            "Key",
            Some("The k= part of the link code: the instance key, or your access key"),
            vec![field(FieldId::Key, "key", Some(200.0), false)],
        ));
        rows.push(line(
            "fingerprint",
            "Certificate fingerprint",
            Some("SHA-256, for a self-signed instance. Empty trusts this device's own roots."),
            vec![field(FieldId::Fingerprint, "optional", Some(200.0), true)],
        ));
    } else {
        let mut kids = vec![
            text(
                "explain",
                "Paste the link code or join link from your Copper Cloud administrator — or a pairing code from a device that is already signed in.",
                13.0,
                Tone::Ink,
            ),
            text(
                "server",
                "Running your own server? `copper-cloud link-code` prints one.",
                11.5,
                Tone::Muted,
            ),
            El::new(
                "entry",
                Kind::Row {
                    gap: 6.0,
                    pad: (0.0, 0.0),
                },
            )
            .with(vec![
                field(
                    FieldId::Code,
                    "copper-cloud://host:port/#k=…, #sso=… or #p=…",
                    None,
                    true,
                ),
                El::new(
                    "paste",
                    Kind::Icon {
                        glyph: glyph::PASTE,
                        act: Act::Paste,
                        name: "Paste link code".into(),
                    },
                ),
            ]),
        ];
        if let Some(c) = code {
            kids.push(kind_line(s, c));
        }
        rows.push(block("code", 11.0, 8.0, kids));
    }
    if let Some(c) = code {
        let endpoint = model::endpoint(c);
        let pin = model::pin(c);
        rows.push(line(
            "instance",
            "Instance",
            Some(if seen(s, st, c).is_some() {
                model::UNTRUSTED_LINE
            } else {
                model::trust_line(endpoint, pin.is_some())
            }),
            vec![mono("host", &endpoint.host_text(), 12.0, Tone::Ink).one_line()],
        ));
        if let Some(fp) = pin {
            rows.push(block(
                "pin",
                0.0,
                0.0,
                vec![
                    mono("pairs", &model::pair_lines(fp), 10.5, Tone::Muted),
                    text(
                        "compare",
                        "Compare with `copper-cloud doctor` on the server",
                        11.0,
                        Tone::Muted,
                    ),
                ],
            ));
        }
        if let Some(seen) = seen(s, st, c) {
            rows.push(block(
                "untrusted",
                10.0,
                6.0,
                vec![
                    text("question", &model::untrusted_text(c), 11.5, Tone::Muted),
                    mono("seen", &model::pair_lines(&seen), 10.5, Tone::Ink),
                    text(
                        "compare",
                        "Compare it with `copper-cloud doctor` on the server. Trusting it pins this instance to that certificate from now on.",
                        11.5,
                        Tone::Muted,
                    ),
                ],
            ));
        }
    }
    rows.push(submit_row(s, st, code));
    let words = st
        .problem
        .clone()
        .or_else(|| sso_problem(s, code))
        .or_else(|| match &parsed {
            Read::Bad(hint) => Some(hint.to_string()),
            _ => None,
        });
    let after = words.map(|w| problem("problem", &w)).into_iter().collect();
    section("connect", Some("1 · Connect to an instance"), rows, after)
}

/// The last single sign-on's refusal, while that join link is the one
/// pasted.
fn sso_problem(s: &Snap, code: Option<&JoinCode>) -> Option<String> {
    match code {
        Some(JoinCode::Sso(link)) => s.sso.problem_for(&link.endpoint).map(str::to_string),
        _ => None,
    }
}

/// Which code it is, and what pressing the button will do with it.
fn kind_line(s: &Snap, code: &JoinCode) -> El {
    let provider = provider_name(s, code);
    let hint = match code {
        JoinCode::Sso(link) => s
            .join_info
            .as_ref()
            .and_then(|i| i.provider(&link.provider))
            .and_then(|p| domain_hint(&p.domains)),
        _ => None,
    };
    let (icon, title, detail) = model::kind(code, &provider, hint.as_deref(), s.enabled.len());
    let g = match icon {
        KindIcon::Link => glyph::LINK,
        KindIcon::Pairing => glyph::KEY,
        KindIcon::Join => glyph::SHIELD,
    };
    let mut words = vec![text("title", &title, 11.5, Tone::Ink).weight(Weight::Medium)];
    if let Some(d) = detail {
        words.push(text("detail", &d, 11.0, Tone::Muted));
    }
    El::new(
        "kind",
        Kind::Row {
            gap: 6.0,
            pad: (0.0, 2.0),
        },
    )
    .with(vec![
        El::new(
            "glyph",
            Kind::Glyph {
                glyph: g,
                size: 10.5,
                tone: Tone::Ink,
            },
        ),
        El::new("words", Kind::Stack { gap: 2.0 }).with(words),
    ])
}

/// The card's last row: the way to the other entry, and the button — or,
/// while the join link's sign-in runs, what it waits for and Cancel.
fn submit_row(s: &Snap, st: &State, code: Option<&JoinCode>) -> El {
    if let Some(JoinCode::Sso(link)) = code
        && s.sso.running_for(&link.endpoint)
    {
        return sso_waiting(s);
    }
    let trusting = code.is_some_and(|c| seen(s, st, c).is_some());
    let provider = code.map(|c| provider_name(s, c)).unwrap_or_default();
    let label = model::connect_button(code, st.connecting, trusting, &provider);
    let ready = code.is_some() && !st.connecting;
    row(
        "submit",
        vec![
            quiet(
                "toggle",
                if st.manual {
                    "Paste a code instead"
                } else {
                    "Enter it by hand"
                },
                Act::ToggleManual,
            ),
            spacer(),
            pill(
                "go",
                &label,
                Act::Submit,
                if ready {
                    PillLook::Filled
                } else {
                    PillLook::Plain
                },
                ready,
            ),
        ],
    )
}
