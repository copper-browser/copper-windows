//! 3 · Browser sync (Mac: `CloudSyncCard`) and On your other devices
//! (`CloudDevicesCard`).

use super::super::el::{
    Act, El, Kind, PillLook, Tone, caption, card, line, pill, row, section, spacer, text,
};
use super::super::model::{self, TABS_SHOWN};
use super::super::state::{Snap, State};
use super::glyph;
use copper_cloud::Domain;

/// The status card while sync is on, the browser's switches, the canvas
/// switch — and, until sync is on, "Turn on sync" under the last.
pub fn cards(s: &Snap, st: &State) -> Vec<El> {
    let mut out = Vec::new();
    if s.sync_on {
        out.push(section("status", Some("Sync"), status_rows(s), vec![]));
    }
    let chosen = st.chosen(&s.enabled);
    let switch = |d: Domain| {
        let on = if s.sync_on {
            s.enabled.contains(&d)
        } else {
            chosen.contains(&d)
        };
        line(
            d.as_str(),
            d.title(),
            Some(d.detail()),
            vec![El::new(
                "switch",
                Kind::Switch {
                    on,
                    act: Act::Domain(d),
                    name: d.title().into(),
                },
            )],
        )
    };
    let browser: Vec<El> = model::browser_domains().into_iter().map(switch).collect();
    out.push(section(
        "browser",
        Some(if s.sync_on {
            "Browser sync"
        } else {
            "3 · Browser sync"
        }),
        browser,
        vec![],
    ));
    let mut canvas = vec![
        caption("caption", "Canvas"),
        card("card", vec![switch(Domain::Canvas)]),
    ];
    if !s.sync_on {
        let n = chosen.len();
        let ok = n > 0 && !st.starting;
        canvas.push(card(
            "start",
            vec![row(
                "start",
                vec![
                    text("words", &model::chosen_text(n), 11.5, Tone::Muted),
                    spacer(),
                    pill(
                        "go",
                        "Turn on sync",
                        Act::TurnOn,
                        if n > 0 {
                            PillLook::Filled
                        } else {
                            PillLook::Plain
                        },
                        ok,
                    ),
                ],
            )],
        ));
    }
    out.push(El::new("canvas", Kind::Stack { gap: 8.0 }).with(canvas));
    out
}

/// How sync is doing and since when; Sync now; Pause sync.
fn status_rows(s: &Snap) -> Vec<El> {
    let detail = model::status_detail(&s.state, s.last_sync, |t| model::ago(t, s.now));
    let title = model::status_title(&s.state, s.last_sync);
    let syncing = s.state == copper_cloud::sync::SyncState::Syncing;
    vec![
        line(
            "state",
            title,
            Some(&detail),
            vec![
                El::new(
                    "dot",
                    Kind::Dot {
                        dot: Some(model::status_dot(&s.state)),
                        help: title.into(),
                    },
                ),
                pill("now", "Sync now", Act::SyncNow, PillLook::Plain, !syncing),
            ],
        ),
        line(
            "pause",
            "Pause sync",
            Some(model::PAUSE_DETAIL),
            vec![pill("off", "Turn off", Act::TurnOff, PillLook::Plain, true)],
        ),
    ]
}

/// The tabs open on the other devices, each device folded until opened; a
/// tab opens here only when clicked.
pub fn devices(s: &Snap, st: &State) -> El {
    let mut rows = Vec::new();
    for d in &s.devices {
        let open = st.open_devices.contains(&d.domain);
        let mut kids = vec![
            El::new(
                "head",
                Kind::Press {
                    act: Act::Device(d.domain.clone()),
                    name: format!("{}'s tabs", d.name),
                    help: None,
                    pad: (14.0, 10.0),
                    lead: 0.0,
                    value: Some(if open { "Shown" } else { "Hidden" }.into()),
                },
            )
            .with(vec![
                El::new(
                    "icon",
                    Kind::Glyph {
                        glyph: glyph::LAPTOP,
                        size: 12.0,
                        tone: Tone::Muted,
                    },
                ),
                El::new("words", Kind::Stack { gap: 2.0 }).with(vec![
                    text("name", &d.name, 13.0, Tone::Ink).one_line(),
                    text(
                        "line",
                        &model::device_line(d.tabs.len(), d.updated, |t| model::ago(t, s.now)),
                        11.5,
                        Tone::Muted,
                    )
                    .one_line(),
                ]),
                spacer(),
                El::new(
                    "chevron",
                    Kind::Glyph {
                        glyph: if open { glyph::DOWN } else { glyph::RIGHT },
                        size: 10.0,
                        tone: Tone::Muted,
                    },
                ),
            ]),
        ];
        if open {
            let tabs: Vec<El> = d
                .tabs
                .iter()
                .take(TABS_SHOWN)
                .enumerate()
                .map(|(i, t)| {
                    let title = if t.title.trim().is_empty() {
                        t.url.as_str()
                    } else {
                        t.title.as_str()
                    };
                    El::new(
                        format!("tab-{i}"),
                        Kind::Press {
                            act: Act::OpenTab(t.url.clone()),
                            name: title.to_string(),
                            help: Some(model::tab_help(&t.url, t.space.as_deref())),
                            pad: (14.0, 5.0),
                            lead: 26.0,
                            value: None,
                        },
                    )
                    .with(vec![
                        El::new(
                            "icon",
                            Kind::Glyph {
                                glyph: if t.active == Some(true) {
                                    glyph::ACTIVE
                                } else {
                                    glyph::GLOBE
                                },
                                size: 10.0,
                                tone: Tone::Muted,
                            },
                        ),
                        text("title", title, 12.0, Tone::Ink).one_line(),
                        spacer(),
                        text("host", &model::tab_host(&t.url), 11.0, Tone::Muted).one_line(),
                    ])
                })
                .collect();
            kids.push(
                El::new(
                    "tabs",
                    Kind::Block {
                        gap: 0.0,
                        pad: (0.0, 0.0),
                    },
                )
                .with(tabs),
            );
        }
        rows.push(El::new(d.domain.clone(), Kind::Stack { gap: 0.0 }).with(kids));
    }
    section("devices", Some("On your other devices"), rows, vec![])
}
