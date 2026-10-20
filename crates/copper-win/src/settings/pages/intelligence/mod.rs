//! Intelligence (`SettingsFork.swift` 10–335, `IntelligenceCheck.swift`):
//! the model behind the agent pane and Ask on page, and Jev's fast lane.
//!
//! Keys live in `intelligence.json` — the private file the agents' MCP
//! host already reads ([`copper_mcp::intelligence::read_files`]) — never in
//! `settings.json`. While Copper Cloud provides the gateway key it decides
//! the lane and the key/address rows fade; this PC's own choices stay in
//! the file for after sign-out ([`copper_mcp::intelligence::merge`]).
//!
//! The Claude account lane is not on Windows yet: the page says so and the
//! lane stays on the API key. Check asks Jev and the gateway one small
//! question each, for real, on the cloud lane's network runtime — never on
//! the UI thread.

use super::redraw;
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::{Form, Pill, Tone};
use copper_core::intelligence::IntelligenceKeys;
use copper_mcp::intelligence::{self as brain, Effective, Keys, Lane, Tier};
use copper_shell_core::cloud;
use serde_json::json;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

mod check;
use check::{is_web_address, key_detail, run_check};

/// Intelligence: model access and Jev.
pub(super) struct Intelligence;

/// One lane's verdict from the last check.
#[derive(Clone)]
struct Verdict {
    /// `jev` or `router`.
    lane: &'static str,
    ok: bool,
    text: String,
    /// The key itself was turned away (not the network, not the model).
    refused: bool,
}

/// The last check, kept for as long as the setup it asked with holds.
#[derive(Default)]
struct Check {
    running: bool,
    verdicts: Vec<Verdict>,
    /// [`whole`] of the setup the verdicts were asked with.
    asked: Option<u64>,
    /// [`lane_key`] of each lane that was asked.
    asked_with: Vec<(&'static str, u64)>,
}

thread_local! {
    static CHECK: RefCell<Check> = RefCell::default();
}

const TIERS: [Tier; 3] = [Tier::Haiku, Tier::Sonnet, Tier::Opus];

/// `Haiku`.
fn tier_title(t: Tier) -> &'static str {
    match t {
        Tier::Haiku => "Haiku",
        Tier::Sonnet => "Sonnet",
        Tier::Opus => "Opus",
    }
}

/// `quick`, as the Model row says it.
fn tier_blurb(t: Tier) -> &'static str {
    match t {
        Tier::Haiku => "quick",
        Tier::Sonnet => "the balance",
        Tier::Opus => "thinks hardest",
    }
}

/// `intelligence.json` as it is on disk (absent reads as empty).
fn local(shell: &Shell) -> IntelligenceKeys {
    shell
        .store()
        .load::<IntelligenceKeys>()
        .map(|l| l.value)
        .unwrap_or_default()
}

/// Change `intelligence.json`, keeping every field this page doesn't know.
fn edit(shell: &Shell, f: impl FnOnce(&mut IntelligenceKeys)) {
    let mut k = local(shell);
    f(&mut k);
    if let Err(e) = shell.store().save(&k) {
        tracing::warn!("intelligence.json: {e}");
    }
}

/// The keys in force: this PC's merged with Copper Cloud's, and where each
/// came from. The Claude lane can't be signed in on Windows.
fn effective(shell: &Shell) -> Effective {
    Effective::new(&brain::read_files(shell.store()), false)
}

/// One lane's key and address, hashed so no second copy of a key is kept.
fn lane_key(keys: &Keys, lane: &str) -> u64 {
    let mut h = DefaultHasher::new();
    lane.hash(&mut h);
    match lane {
        "jev" => (&keys.jev_key, &keys.jev_endpoint).hash(&mut h),
        _ => (&keys.router_key, &keys.router_url).hash(&mut h),
    }
    h.finish()
}

/// Everything a verdict depends on: both lanes' keys and the model asked.
fn whole(keys: &Keys) -> u64 {
    let mut h = DefaultHasher::new();
    lane_key(keys, "jev").hash(&mut h);
    lane_key(keys, "router").hash(&mut h);
    brain::model(keys, None).hash(&mut h);
    h.finish()
}

/// The last check's verdict while it still describes this setup.
fn verdict_line(keys: &Keys) -> Option<String> {
    CHECK.with(|c| {
        let c = c.borrow();
        (!c.verdicts.is_empty() && c.asked == Some(whole(keys))).then(|| {
            c.verdicts
                .iter()
                .map(|v| v.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    })
}

/// Whether the last check saw this lane's key, as it is now, turned away.
fn refused(keys: &Keys, lane: &str) -> bool {
    CHECK.with(|c| {
        let c = c.borrow();
        let now = lane_key(keys, lane);
        c.verdicts.iter().any(|v| v.lane == lane && v.refused)
            && c.asked_with.iter().any(|(l, k)| *l == lane && *k == now)
    })
}

impl Page for Intelligence {
    fn id(&self) -> &'static str {
        "intelligence"
    }
    fn title(&self) -> &'static str {
        "Intelligence"
    }
    fn blurb(&self) -> &'static str {
        "The model behind the agent pane and Ask on page, and Jev's fast lane."
    }
    fn glyph(&self) -> char {
        glyph::INTELLIGENCE
    }

    fn draw(&self, shell: &Shell) -> Form {
        let file = local(shell);
        let mine = Keys::from_file(&file);
        let eff = effective(shell);
        let cloud_lane = eff.sources.get("lane").map(String::as_str) == Some("cloud");
        let running = CHECK.with(|c| c.borrow().running);
        let mut f = Form::default();

        let sec = f.section("Model access");
        if mine.lane == Lane::Claude && !cloud_lane {
            sec.button(
                "Use",
                Some("This PC is set to a Claude account, which can't sign in on Windows yet — use an API key for a model gateway"),
                "lane-key",
                "Use an API key",
                true,
            )
            .tone(Tone::Error);
        } else {
            sec.value(
                "Use",
                Some(if cloud_lane {
                    "Set by Copper Cloud, which provides the model key while you're signed in to it."
                } else {
                    "An API key for a model gateway. Signing in with a Claude account is not yet on Windows."
                }),
                "API key",
            );
        }
        sec.anchor("intelligence.lane")
            .keywords(&["claude account", "lane"]);

        let (detail, tone) = key_detail(
            &eff,
            "routerKey",
            "From your gateway — anything that speaks /v1/chat/completions.",
            eff.router_ready(),
            refused(&eff.keys, "router"),
        );
        sec.secret(
            "API key",
            Some(&detail),
            "router-key",
            if cloud_lane {
                ""
            } else {
                mine.router_key.as_str()
            },
            if cloud_lane { "••••" } else { "sk-…" },
        )
        .tone(tone)
        .disabled(cloud_lane)
        .mono()
        .on_submit("router-key-done", Some(200.0))
        .anchor("intelligence.key");

        let url_from_cloud = eff.sources.get("routerURL").map(String::as_str) == Some("cloud");
        let (gw_detail, gw_tone) = if url_from_cloud {
            ("Provided by Copper Cloud".to_string(), Tone::Plain)
        } else if !is_web_address(&eff.keys.router_url) {
            (
                "That isn't a web address — it should start with https://".to_string(),
                Tone::Error,
            )
        } else {
            ("Where the gateway lives".to_string(), Tone::Plain)
        };
        sec.field(
            "Gateway address",
            Some(&gw_detail),
            "router-url",
            if cloud_lane {
                ""
            } else {
                file.router_url.as_deref().unwrap_or(&mine.router_url)
            },
            if url_from_cloud || cloud_lane {
                eff.keys.router_url.as_str()
            } else {
                "https://…"
            },
        )
        .tone(gw_tone)
        .disabled(cloud_lane)
        .mono()
        .on_submit("router-url-done", Some(220.0))
        .anchor("intelligence.gateway");

        let tier = eff.keys.tier;
        let name = brain::model(&eff.keys, None);
        sec.choice(
            "Model",
            Some(&format!(
                "{} — {}. Asks for “{name}”. You can also change it from the agent pane.",
                tier_title(tier),
                tier_blurb(tier)
            )),
            "tier",
            &[("haiku", "Haiku"), ("sonnet", "Sonnet"), ("opus", "Opus")],
            tier.name(),
        )
        .anchor("intelligence.tier");
        sec.note(
            "Model names",
            Some("What Haiku, Sonnet and Opus are called on your gateway. Leave one empty for the plain name."),
        )
        .anchor("intelligence.names");
        for t in TIERS {
            let typed = file
                .router_models
                .as_ref()
                .and_then(|m| m.get(t.name()))
                .cloned()
                .unwrap_or_default();
            sec.field(
                tier_title(t),
                None,
                &format!("name-{}", t.name()),
                &typed,
                t.name(),
            )
            .mono()
            .on_submit(&format!("name-{}-done", t.name()), Some(150.0))
            .keywords(&["model name", "gateway model"]);
        }
        let line = verdict_line(&eff.keys);
        let all_ok = CHECK.with(|c| c.borrow().verdicts.iter().all(|v| v.ok));
        sec.pills(
            "Check",
            Some(line.as_deref().unwrap_or(
                "Asks Jev and the model one small question each, so you know they answer.",
            )),
            vec![Pill {
                action: "test".into(),
                label: match (running, &line) {
                    (true, _) => "Asking…",
                    (false, None) => "Test",
                    (false, Some(_)) => "Test again",
                }
                .into(),
                filled: false,
                enabled: !running,
            }],
        )
        .tone(match &line {
            Some(_) if all_ok => Tone::Good,
            Some(_) => Tone::Error,
            None => Tone::Plain,
        })
        .anchor("intelligence.check");

        let mut footnote = "Keys stay on this PC, in your own Windows profile folder.".to_string();
        if eff.sources.values().any(|s| s == "cloud") {
            footnote += " Keys from Copper Cloud go when you sign out of it. While it provides a gateway key, that key is used; a Jev key typed here still wins over the cloud's.";
        }
        let jev_cloud = eff.sources.get("jevKey").map(String::as_str) == Some("cloud");
        let (detail, tone) = key_detail(
            &eff,
            "jevKey",
            "Picks between choices and says how sure it is, in about a fifth of a second. Agents use it for quick decisions.",
            eff.jev_ready(),
            refused(&eff.keys, "jev"),
        );
        f.section("Jev — the fast lane")
            .secret(
                "Jev",
                Some(&detail),
                "jev-key",
                &mine.jev_key,
                if jev_cloud {
                    "•••• · type to override"
                } else {
                    "ts-…"
                },
            )
            .tone(tone)
            .mono()
            .on_submit("jev-key-done", Some(200.0))
            .anchor("intelligence.jev")
            .footnote(&footnote);
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        let action = action.strip_suffix("-done").unwrap_or(action);
        let text = value.trim().to_string();
        let empty = |s: &str| (!s.is_empty()).then(|| s.to_string());
        match action {
            "lane-key" => edit(shell, |k| k.lane = Some("key".into())),
            "router-key" => edit(shell, |k| k.router_key = empty(&text)),
            "router-url" => edit(shell, |k| k.router_url = empty(&text)),
            "jev-key" => edit(shell, |k| k.jev_key = empty(&text)),
            "tier" => {
                if let Some(t) = Tier::parse(value) {
                    edit(shell, |k| k.tier = Some(t.name().into()));
                }
            }
            "test" => run_check(shell),
            a if a.starts_with("name-") => {
                let tier = &a[5..];
                edit(shell, |k| {
                    let map = k.router_models.get_or_insert_with(Default::default);
                    match empty(&text) {
                        Some(n) => {
                            map.insert(tier.to_string(), n);
                        }
                        None => {
                            map.remove(tier);
                        }
                    }
                });
            }
            _ => {}
        }
    }
}
