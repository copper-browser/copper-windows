//! Intelligence (`SettingsFork.swift` 10–335, `IntelligenceCheck.swift`):
//! the model behind the agent pane and Ask on page, Jev's fast lane, and
//! voice (not on Windows yet).
//!
//! Keys live in `intelligence.json` — the private file the agents' MCP
//! host already reads ([`copper_mcp::intelligence::read_files`]) — never in
//! `settings.json`. While Copper Cloud provides the gateway key it decides
//! the lane and the key/address rows fade; this PC's own choices stay in
//! the file for after sign-out ([`copper_mcp::intelligence::merge`]).
//!
//! Use chooses the lane: an API key for a gateway, or a Claude account
//! (copper-shell-core's `claude`: Sign in with Claude opens claude.ai in a
//! tab, and a code claude.ai shows can be pasted here when the tab can't
//! come back). Check asks Jev and the model lane in force one small
//! question each, for real, on the cloud lane's network runtime — never on
//! the UI thread.

use super::{not_yet, redraw};
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::fold;
use crate::settings::form::{Form, Pill, Section, Tone};
use copper_core::intelligence::IntelligenceKeys;
use copper_mcp::claude::{Phase, Status};
use copper_mcp::intelligence::{self as brain, Effective, Keys, Lane, Tier};
use copper_shell_core::cloud;
use std::cell::{Cell, RefCell};

mod check;
use brain::check::{Check, is_web_address};
use check::{key_detail, run_check};

/// Intelligence: model access and Jev.
pub(super) struct Intelligence;

thread_local! {
    static CHECK: RefCell<Check> = RefCell::default();
    /// The Claude sign-in the last check was asked under.
    static SESSION: Cell<u64> = const { Cell::new(0) };
    /// What is typed in the sign-in code box (sent on Enter).
    static PASTED: RefCell<String> = const { RefCell::new(String::new()) };
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
/// came from; the Claude lane is ready while the account is signed in.
fn effective(shell: &Shell) -> Effective {
    Effective::new(
        &brain::read_files(shell.store()),
        copper_shell_core::claude::ready(shell.store()),
    )
}

/// The account's line (`IntelligencePage.accountDetail`).
fn account_detail(st: &Status) -> (String, Tone) {
    if st.signed_in {
        let mut detail = format!("Signed in as {}", st.who());
        if !st.organization.is_empty() {
            detail += &format!(" · {}", st.organization);
        }
        return (detail, Tone::Plain);
    }
    let text = match &st.phase {
        Phase::Idle => "Copper opens claude.ai in a tab; sign in there and you're back here.",
        Phase::Waiting => {
            "Finish in the tab that opened. If it doesn't come back on its own, paste the code claude.ai shows below."
        }
        Phase::Exchanging => "Finishing…",
        Phase::Failed(text) => return (text.clone(), Tone::Error),
    };
    (text.into(), Tone::Plain)
}

/// The API key lane's rows: the gateway's key and address.
fn gateway_rows(
    sec: &mut Section,
    file: &IntelligenceKeys,
    mine: &Keys,
    eff: &Effective,
    cloud_lane: bool,
) {
    let (detail, tone) = key_detail(
        eff,
        "routerKey",
        "From your gateway — anything that speaks /v1/chat/completions.",
        eff.router_ready(),
        CHECK.with(|c| c.borrow().refused(&eff.keys, "router")),
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
}

/// The Claude account lane's rows (`IntelligencePage.accountControl`):
/// Sign in with Claude, then who it is and Sign out; while the tab is out,
/// Cancel and a box for the code claude.ai shows; a failure and Try again.
fn account_rows(sec: &mut Section, st: &Status) {
    let (detail, tone) = account_detail(st);
    let line = if st.signed_in {
        sec.button(
            "Claude account",
            Some(&detail),
            "claude-signout",
            "Sign out",
            false,
        )
    } else {
        match &st.phase {
            Phase::Idle => sec.button(
                "Claude account",
                Some(&detail),
                "claude-signin",
                "Sign in with Claude",
                true,
            ),
            Phase::Waiting => sec.button(
                "Claude account",
                Some(&detail),
                "claude-cancel",
                "Cancel",
                false,
            ),
            Phase::Exchanging => sec.note("Claude account", Some(&detail)),
            Phase::Failed(_) => sec.button(
                "Claude account",
                Some(&detail),
                "claude-signin",
                "Try again",
                true,
            ),
        }
    };
    line.tone(tone).anchor("intelligence.account").keywords(&[
        "sign in with claude",
        "claude",
        "anthropic",
        "sign out",
    ]);
    if !st.signed_in && st.phase == Phase::Waiting {
        let typed = PASTED.with(|p| p.borrow().clone());
        sec.field(
            "Sign-in code",
            Some("Paste the code claude.ai shows, then press Enter."),
            "claude-code",
            &typed,
            "Paste the code",
        )
        .mono()
        .on_submit("claude-paste", Some(200.0))
        .anchor("intelligence.code")
        .keywords(&["claude code", "paste"]);
    }
}

impl Page for Intelligence {
    fn id(&self) -> &'static str {
        "intelligence"
    }
    fn title(&self) -> &'static str {
        "Intelligence"
    }
    fn blurb(&self) -> &'static str {
        "The model behind the agent pane, Jev's fast lane, and voice."
    }
    fn glyph(&self) -> char {
        glyph::INTELLIGENCE
    }

    fn draw(&self, shell: &Shell) -> Form {
        let file = local(shell);
        let mine = Keys::from_file(&file);
        let eff = effective(shell);
        let cloud_lane = eff.sources.get("lane").map(String::as_str) == Some("cloud");
        let account = copper_shell_core::claude::account(shell.store()).status();
        // A verdict is about the sign-in it was asked under.
        if SESSION.replace(account.session) != account.session {
            CHECK.with(|c| {
                if !c.borrow().running {
                    *c.borrow_mut() = Check::default();
                }
            });
        }
        let running = CHECK.with(|c| c.borrow().running);
        let claude = eff.keys.lane == Lane::Claude;
        let mut f = Form::default();

        let sec = f.section("Model access");
        let use_detail = match (cloud_lane, mine.lane == Lane::Claude || account.signed_in) {
            (false, _) => {
                "Sign in with your Claude account (Pro, Max, Team or Enterprise), or use an API key for a model gateway."
            }
            (true, true) => {
                "Set by Copper Cloud, which provides the model key. Your Claude account is back when you sign out of it."
            }
            (true, false) => {
                "Set by Copper Cloud, which provides the model key while you're signed in to it."
            }
        };
        sec.choice(
            "Use",
            Some(use_detail),
            "lane",
            &[("key", "API key"), ("claude", "Claude account")],
            if claude { "claude" } else { "key" },
        )
        .disabled(cloud_lane)
        .anchor("intelligence.lane")
        .keywords(&["claude account", "lane", "sign in with claude"]);

        if claude {
            account_rows(sec, &account);
        } else {
            gateway_rows(sec, &file, &mine, &eff, cloud_lane);
        }

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
        let line = CHECK.with(|c| c.borrow().line(&eff.keys));
        let all_ok = CHECK.with(|c| c.borrow().all_ok());
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
        // Model names are for gateways that call them something else:
        // folded under Advanced (Copper#118).
        if fold::advanced(sec, "intelligence.more", &["intelligence.names"]) {
            sec.note(
                "Model names",
                Some(if claude {
                    "What Haiku, Sonnet and Opus are called for your Claude account. Leave one empty for Copper's own."
                } else {
                    "What Haiku, Sonnet and Opus are called on your gateway. Leave one empty for the plain name."
                }),
            )
            .anchor("intelligence.names");
            let defaults = Keys::default().claude_models;
            for t in TIERS {
                let names = if claude {
                    &file.claude_models
                } else {
                    &file.router_models
                };
                let typed = names
                    .as_ref()
                    .and_then(|m| m.get(t.name()))
                    .cloned()
                    .unwrap_or_default();
                let placeholder = match defaults.get(t.name()) {
                    Some(d) if claude => d.as_str(),
                    _ => t.name(),
                };
                sec.field(
                    tier_title(t),
                    None,
                    &format!("name-{}", t.name()),
                    &typed,
                    placeholder,
                )
                .mono()
                .on_submit(&format!("name-{}-done", t.name()), Some(150.0))
                .anchor("intelligence.names")
                .keywords(&["model name", "gateway model"]);
            }
        }

        let mut footnote =
            "Keys and the Claude sign-in stay on this PC, in your own Windows profile folder."
                .to_string();
        if eff.sources.values().any(|s| s == "cloud") {
            footnote += " Keys from Copper Cloud go when you sign out of it. While it provides a gateway key, that key is used; a Jev key typed here still wins over the cloud's.";
        }
        let jev_cloud = eff.sources.get("jevKey").map(String::as_str) == Some("cloud");
        let (detail, tone) = key_detail(
            &eff,
            "jevKey",
            if eff.jev_ready() {
                "Quick decisions for agents, in about a fifth of a second."
            } else {
                "Quick decisions for agents, in about a fifth of a second. Needs a key from your TypeSafe account."
            },
            eff.jev_ready(),
            CHECK.with(|c| c.borrow().refused(&eff.keys, "jev")),
        );
        f.section("Jev")
            .secret(
                "Jev key",
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

        // Voice (`VoiceSettings.swift`, a page of its own until Copper#115).
        not_yet(
            f.section("Voice"),
            "Talk to the agent",
            "Not yet on Windows — Copper for Windows has no speech model, so it never listens",
        )
        .anchor("voice.enabled")
        .keywords(&["dictation", "speech", "microphone", "push to talk"]);
        f
    }

    fn act(&self, shell: &Shell, action: &str, value: &str) {
        let action = action.strip_suffix("-done").unwrap_or(action);
        let text = value.trim().to_string();
        let empty = |s: &str| (!s.is_empty()).then(|| s.to_string());
        match action {
            "lane" if matches!(value, "key" | "claude") => {
                edit(shell, |k| k.lane = Some(value.into()))
            }
            "claude-signin" => {
                // The tab opens behind this panel: put it away first.
                shell.view().panels().hide(shell);
                // A refusal is the account's status, drawn on this row.
                let _ = copper_shell_core::claude::sign_in(shell);
            }
            "claude-cancel" => copper_shell_core::claude::account(shell.store()).cancel(),
            "claude-signout" => copper_shell_core::claude::account(shell.store()).sign_out(),
            "claude-code" => PASTED.with(|p| *p.borrow_mut() = text),
            "claude-paste" => {
                PASTED.with(|p| p.borrow_mut().clear());
                if !text.is_empty() {
                    copper_shell_core::claude::account(shell.store()).paste(&text);
                }
            }
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
                let claude = effective(shell).keys.lane == Lane::Claude;
                edit(shell, |k| {
                    let names = if claude {
                        &mut k.claude_models
                    } else {
                        &mut k.router_models
                    };
                    let map = names.get_or_insert_with(Default::default);
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
