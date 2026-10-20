//! The rest of a signed-in page: Delete history on cloud
//! (`CloudHistoryDeleteCard`), Pair another device (`CloudPairCard`), the
//! instance line (`CloudConnection`, also the folded Connect step) and the
//! Log (`CloudLogCard`).

use super::super::el::{
    Act, Choice, El, FieldId, Kind, PillLook, Tone, block, caption, card, field, line, mono, pill,
    problem, quiet, row, section, spacer, text,
};
use super::super::model::{self, PairCard, Span, TrustIcon};
use super::super::state::{Snap, State};
use super::glyph;
use copper_cloud::sync::delete::UNSUPPORTED;
use copper_win_ui::Weight;

/// The instance this device is linked to, in one line — where, how its
/// certificate is trusted, a way into the details — and, opened, the whole
/// fingerprint and Disconnect. `opener` is "Change…" while signing in is
/// still to do, "Details" after; `lead` is said before the host when no
/// caption is over it.
pub fn connection(s: &Snap, st: &State, opener: &str, lead: Option<&str>) -> El {
    let Some(link) = s.link.as_ref() else {
        return El::new("connection", Kind::Stack { gap: 0.0 });
    };
    let pin = link.fingerprint.as_deref();
    let (icon, label) = model::trust_capsule(&link.endpoint, pin);
    let mut head = vec![El::new(
        "dot",
        Kind::Dot {
            dot: s.reachable.then_some(model::Dot::Green),
            help: if s.reachable {
                "Reachable"
            } else {
                "Not reached yet"
            }
            .into(),
        },
    )];
    if let Some(l) = lead {
        head.push(text("lead", l, 12.5, Tone::Muted).one_line());
    }
    head.push(mono("host", &link.endpoint.host_text(), 12.5, Tone::Ink).one_line());
    head.push(
        El::new(
            "trust",
            Kind::Capsule {
                help: model::trust_help(&link.endpoint, pin),
            },
        )
        .with(vec![
            El::new(
                "icon",
                Kind::Glyph {
                    glyph: match icon {
                        TrustIcon::Warning => glyph::WARNING,
                        TrustIcon::Seal => glyph::SEAL,
                        TrustIcon::Lock => glyph::LOCK,
                    },
                    size: 8.5,
                    tone: Tone::Muted,
                },
            ),
            if pin.is_some() {
                mono("label", &label, 10.5, Tone::Muted).one_line()
            } else {
                text("label", &label, 10.5, Tone::Muted).one_line()
            },
        ]),
    );
    head.push(spacer());
    head.push(quiet(
        "open",
        if st.details { "Hide" } else { opener },
        Act::ToggleDetails,
    ));
    let mut rows = vec![row("line", head)];
    if st.details {
        let mut words = vec![text(
            "trust",
            model::trust_details(&link.endpoint, pin.is_some()),
            11.5,
            Tone::Ink,
        )];
        if let Some(fp) = pin {
            words.push(mono("pairs", &model::pair_lines(fp), 10.5, Tone::Muted));
            words.push(text(
                "compare",
                "Compare it with `copper-cloud doctor` on the server.",
                11.0,
                Tone::Muted,
            ));
        }
        rows.push(block("details", 10.0, 4.0, words));
        rows.push(line(
            "disconnect",
            "Disconnect",
            Some("Signs out and forgets this instance, to connect to another. Nothing is deleted."),
            vec![pill(
                "go",
                "Disconnect",
                Act::Disconnect,
                PillLook::Red,
                true,
            )],
        ));
    }
    card("connection", rows)
}

/// Delete history on cloud: what the cloud can do, the range, a site, the
/// button; the result stays after Settings closes.
pub fn history(s: &Snap, st: &State) -> El {
    const TITLE: &str = "Delete history on cloud";
    let rows = match s.deletes_history {
        Some(true) => {
            let bad = model::typed_site(&st.site).is_err();
            let status =
                model::delete_status(bad, st.erasing, st.looking, st.erase_result.as_deref());
            vec![
                line(
                    "range",
                    "Time range",
                    Some("Visits on Copper Cloud, from every device signed in as you"),
                    vec![El::new(
                        "choice",
                        Kind::Segmented {
                            options: Span::ALL
                                .into_iter()
                                .map(|sp| Choice {
                                    label: sp.title().into(),
                                    act: Act::Span(sp),
                                    on: sp == st.span,
                                })
                                .collect(),
                            name: "Time range".into(),
                            wide: false,
                        },
                    )],
                ),
                line(
                    "site",
                    "Only this site",
                    Some(
                        "Optional; its subdomains too. Found by reading your history on this device — the cloud never looks inside it.",
                    ),
                    vec![field(FieldId::Site, "example.com", Some(170.0), false)],
                ),
                row(
                    "go",
                    vec![
                        text(
                            "status",
                            &status,
                            11.5,
                            if bad { Tone::Red } else { Tone::Muted },
                        ),
                        spacer(),
                        pill(
                            "delete",
                            "Delete history on cloud…",
                            Act::DeleteHistory,
                            PillLook::Red,
                            !st.erasing && !bad,
                        ),
                    ],
                ),
            ]
        }
        Some(false) => vec![line("unsupported", TITLE, Some(UNSUPPORTED), vec![])],
        None => vec![line(
            "asking",
            TITLE,
            Some(if st.info_asked {
                "Couldn't reach the cloud to ask whether it can delete history."
            } else {
                "Asking the cloud what it can do…"
            }),
            vec![],
        )],
    };
    section(
        "history",
        Some(TITLE),
        rows,
        st.erase_problem
            .as_deref()
            .map(|w| problem("problem", w))
            .into_iter()
            .collect(),
    )
}

/// Pair another device: one live code at a time, counted down, copyable,
/// revocable, and noticed when used.
pub fn pair(s: &Snap, st: &State) -> El {
    let busy = st.pair_working;
    let again = |key: &str, title: &str, detail: &str, filled: bool| {
        line(
            key,
            title,
            Some(detail),
            vec![pill(
                "go",
                if busy { "…" } else { "New code" },
                Act::MakeCode,
                if filled && !busy {
                    PillLook::Filled
                } else {
                    PillLook::Plain
                },
                !busy,
            )],
        )
    };
    let body = match st.pair_card(s.now) {
        PairCard::Offer => line(
            "offer",
            "One-time code",
            Some("Signs another device in as you with one paste. Works once, for 10 minutes."),
            vec![pill(
                "go",
                if busy { "Making…" } else { "Make a code" },
                Act::MakeCode,
                PillLook::Plain,
                !busy,
            )],
        ),
        PairCard::Used => again(
            "used",
            "Used — the other device is signed in",
            "Make another code for the next device.",
            false,
        ),
        PairCard::Revoked => again("revoked", "Code revoked", "Nobody can use it now.", false),
        PairCard::Expired => again(
            "expired",
            "Code expired",
            "Make a new one when the other device is ready.",
            true,
        ),
        PairCard::Live(left) => live(st, left, s.now),
    };
    section(
        "pair",
        Some("Pair another device"),
        vec![body],
        st.pair_problem
            .as_deref()
            .map(|w| problem("problem", w))
            .into_iter()
            .collect(),
    )
}

/// A live code: the whole code in three lines, the countdown, Revoke, Copy.
fn live(st: &State, left: f64, now: f64) -> El {
    let Some(m) = st.minted.as_ref() else {
        return El::new("live", Kind::Stack { gap: 0.0 });
    };
    let warn = left < model::PAIR_WARN;
    let (head, secret, tail) = model::code_parts(&m.link, &m.code);
    let mut code = Vec::new();
    if !head.is_empty() {
        code.push(mono("head", &head, 11.5, Tone::Muted));
    }
    code.push(mono("secret", &secret, 15.0, Tone::Ink).weight(Weight::Semibold));
    if !tail.is_empty() {
        code.push(mono("tail", &tail, 10.5, Tone::Muted));
    }
    block(
        "live",
        12.0,
        10.0,
        vec![
            text(
                "say",
                "Paste this into Settings › Cloud on the other device.",
                11.5,
                Tone::Muted,
            )
            .one_line(),
            El::new("code", Kind::CodeBox).with(code),
            El::new("clock", Kind::Stack { gap: 5.0 }).with(vec![
                El::new(
                    "bar",
                    Kind::Bar {
                        fraction: model::countdown_fraction(left),
                        warn,
                    },
                ),
                El::new(
                    "actions",
                    Kind::Row {
                        gap: 8.0,
                        pad: (0.0, 0.0),
                    },
                )
                .with(vec![
                    text(
                        "left",
                        &format!("Expires in {} · works once", model::countdown(left)),
                        11.5,
                        if warn { Tone::Orange } else { Tone::Muted },
                    )
                    .one_line(),
                    spacer(),
                    El::new(
                        "revoke",
                        Kind::Quiet {
                            label: "Revoke".into(),
                            act: Act::RevokeCode,
                            tone: Tone::Red,
                        },
                    ),
                    pill(
                        "copy",
                        if now < st.copied_until {
                            "Copied"
                        } else {
                            "Copy"
                        },
                        Act::CopyCode,
                        PillLook::Filled,
                        true,
                    ),
                ]),
            ]),
        ],
    )
}

/// The log: folded until opened; the newest first.
pub fn log(s: &Snap, st: &State) -> El {
    let shown = if st.log_open { "Shown" } else { "Hidden" };
    let mut kids = vec![
        El::new(
            "head",
            Kind::Press {
                act: Act::ToggleLog,
                name: "Log".into(),
                help: None,
                pad: (0.0, 0.0),
                lead: 0.0,
                value: Some(shown.into()),
            },
        )
        .with(vec![
            caption("caption", "Log"),
            El::new(
                "chevron",
                Kind::Glyph {
                    glyph: if st.log_open {
                        glyph::DOWN
                    } else {
                        glyph::RIGHT
                    },
                    size: 9.0,
                    tone: Tone::Muted,
                },
            ),
            spacer(),
        ]),
    ];
    if st.log_open {
        let mut lines = Vec::new();
        if s.log.is_empty() {
            lines.push(text("empty", "Nothing yet", 11.5, Tone::Muted));
        }
        for (i, (at, words)) in s.log.iter().enumerate() {
            lines.push(
                El::new(
                    format!("entry-{i}-{at}"),
                    Kind::Row {
                        gap: 8.0,
                        pad: (0.0, 0.0),
                    },
                )
                .with(vec![
                    mono("at", at, 10.5, Tone::Muted).one_line(),
                    text("text", words, 11.5, Tone::Ink),
                ]),
            );
        }
        kids.push(card("card", vec![block("entries", 10.0, 3.0, lines)]));
    }
    El::new("log", Kind::Stack { gap: 8.0 }).with(kids)
}
