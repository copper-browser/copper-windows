//! Settings › Cloud as [`El`]s (Mac: `CloudPage`): three steps, in order,
//! each only when the one before is done — connect to an instance, sign
//! in, choose what syncs — then, signed in, the devices, deleting history
//! on the cloud, the account, pairing another device, the instance and the
//! log. A finished step folds to one line. Nothing here acts: every press
//! is an [`super::el::Act`] the page carries out.

mod account;
pub mod connect;
mod more;
mod sync;

#[cfg(test)]
mod tests;

use super::el::{Act, El, Kind, PillLook, Tone, block, card, pill, spacer, text};
use super::model::{self, Mark};
use super::state::{Snap, State};
use copper_shell_core::cloud::sso::same_instance;
use copper_win_ui::Weight;

/// The glyphs the page draws (Segoe Fluent Icons).
pub mod glyph {
    pub const LINK: char = '\u{E71B}';
    pub const KEY: char = '\u{E8D7}';
    pub const SHIELD: char = '\u{EA18}';
    pub const PASTE: char = '\u{E77F}';
    pub const RIGHT: char = '\u{E76C}';
    pub const DOWN: char = '\u{E70D}';
    pub const LAPTOP: char = '\u{E7F8}';
    pub const GLOBE: char = '\u{E774}';
    pub const ACTIVE: char = '\u{E91F}';
    pub const WARNING: char = '\u{E7BA}';
    pub const LOCK: char = '\u{E72E}';
    pub const SEAL: char = '\u{E930}';
}

/// The whole page, top to bottom (18 apart).
pub fn page(s: &Snap, st: &State) -> Vec<El> {
    if !s.on {
        return vec![card(
            "off",
            vec![block(
                "words",
                10.0,
                0.0,
                vec![text(
                    "text",
                    "Copper Cloud is off this run — cloud.json couldn't be read",
                    12.5,
                    Tone::Muted,
                )],
            )],
        )];
    }
    let mut out = vec![steps(s)];
    if let Some(notice) = offer_notice(s) {
        out.push(notice);
    }
    match (&s.link, s.signed_in) {
        (None, _) => out.push(connect::card(s, st)),
        (Some(_), false) => {
            out.push(more::connection(s, st, "Change…", Some("Connected to")));
            out.push(account::form(s, st));
        }
        (Some(_), true) => {
            out.extend(sync::cards(s, st));
            if s.sync_on && !s.devices.is_empty() {
                out.push(sync::devices(s, st));
            }
            out.push(more::history(s, st));
            out.push(account::card(s, st));
            out.push(more::pair(s, st));
            out.push(El::new("instance", Kind::Stack { gap: 8.0 }).with(vec![
                super::el::caption("caption", "Instance"),
                more::connection(s, st, "Details", None),
            ]));
            out.push(more::log(s, st));
        }
    }
    out
}

/// Where you are: the intro and the three marks.
fn steps(s: &Snap) -> El {
    let step = model::step(s.link.is_some(), s.signed_in);
    let third = model::third_step(s.sync_on, &s.state);
    let mut marks = Vec::new();
    for (n, title) in [(1u8, "Connect"), (2, "Account"), (3, third)] {
        if n > 1 {
            marks.push(El::new(
                format!("bar-{n}"),
                Kind::StepBar { done: step >= n },
            ));
        }
        let mark = model::mark(n, step, s.sync_on);
        let here = mark == Mark::Here;
        marks.push(
            El::new(
                format!("step-{n}"),
                Kind::Row {
                    gap: 6.0,
                    pad: (0.0, 0.0),
                },
            )
            .with(vec![
                El::new("mark", Kind::StepMark { n, mark }),
                text(
                    "title",
                    title,
                    12.0,
                    if mark == Mark::Ahead {
                        Tone::Muted
                    } else {
                        Tone::Ink
                    },
                )
                .weight(if here {
                    Weight::Medium
                } else {
                    Weight::Regular
                })
                .one_line(),
            ]),
        );
    }
    El::new("steps", Kind::Stack { gap: 10.0 }).with(vec![
        text(
            "intro",
            "Browser sync — spaces, settings, bookmarks, history — and shared canvases, through a Copper Cloud instance you or your team runs.",
            12.0,
            Tone::Muted,
        ),
        El::new(
            "marks",
            Kind::Row {
                gap: 6.0,
                pad: (0.0, 0.0),
            },
        )
        .with(marks),
    ])
}

/// A `copper://cloud/join` link for another instance than the linked one:
/// nothing changes until it is dismissed or the instance disconnected.
fn offer_notice(s: &Snap) -> Option<El> {
    let offer = s.sso.offer.as_ref()?;
    let linked = s.link.as_ref()?;
    if same_instance(&linked.endpoint, offer.endpoint()) {
        return None;
    }
    let (title, detail) = model::offer_notice(&offer.host(), &linked.endpoint.host_text());
    Some(card(
        "offer",
        vec![super::el::row(
            "notice",
            vec![
                El::new(
                    "glyph",
                    Kind::Glyph {
                        glyph: glyph::LINK,
                        size: 10.5,
                        tone: Tone::Ink,
                    },
                ),
                El::new("words", Kind::Stack { gap: 2.0 }).with(vec![
                    text("title", &title, 12.0, Tone::Ink).weight(Weight::Medium),
                    text("detail", &detail, 11.5, Tone::Muted),
                ]),
                spacer(),
                pill(
                    "dismiss",
                    "Dismiss",
                    Act::DismissOffer,
                    PillLook::Plain,
                    true,
                ),
            ],
        )],
    ))
}

/// While a single sign-on is under way: what Copper waits for, and Cancel
/// (`CloudSSOWaiting`), in Connect and in the account step alike.
fn sso_waiting(s: &Snap) -> El {
    let words = s.sso.phase.waiting_text().unwrap_or("");
    let mut kids = vec![text("words", words, 11.5, Tone::Muted), spacer()];
    if s.sso.phase.cancellable() {
        kids.push(pill(
            "cancel",
            "Cancel",
            Act::SsoCancel,
            PillLook::Plain,
            true,
        ));
    }
    super::el::row("sso-waiting", kids)
}
