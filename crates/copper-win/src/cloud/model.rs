//! Settings › Cloud's words and decisions, without a window (Mac:
//! `CloudSettings.swift`, `CloudPairing.swift`, `CloudHistoryEraser.swift`).
//!
//! Everything here is pure: which step the page is on, what a pasted code
//! is and what pressing the button will do with it, the status line, the
//! pairing countdown, the history-delete question. The Windows page
//! ([`super::page`]) only draws what these say, so the rules are tested on
//! any machine. The protocol itself — parsing codes, pinning, pairing,
//! single sign-on, sync — is copper-cloud's and copper-shell-core's; this
//! module never talks to a server.
//!
//! Words are the Mac's, with its platform nouns made portable the way the
//! Linux page does ("this device", "your other devices"): a Windows PC is
//! not a Mac.

use copper_cloud::link::{is_bare_pairing_code, manual_link, pairs};
use copper_cloud::sync::SyncState;
use copper_cloud::{Domain, Endpoint, JoinCode, Link, parse_join_code};

/// The fewest characters a new account's password may have
/// (`Cloud.minimumPassword`).
pub const MIN_PASSWORD: usize = 10;
/// How long a pairing code works (`CloudPairing.lifetime`).
pub const PAIR_LIFETIME: f64 = 600.0;
/// The log card's rows (newest first) out of the engine's 200.
pub const LOG_SHOWN: usize = 60;
/// Remote tab rows shown per device.
pub const TABS_SHOWN: usize = 60;
/// Seconds before a pairing code's expiry when its countdown turns orange.
pub const PAIR_WARN: f64 = 60.0;

// MARK: steps

/// Where the person is: 1 connect, 2 account, 3 sync.
pub fn step(linked: bool, signed_in: bool) -> u8 {
    if !linked {
        1
    } else if !signed_in {
        2
    } else {
        3
    }
}

/// The third mark's label: "Sync on" once it is — the status line says how
/// it is doing — and "Syncing…" only while a sync is under way.
pub fn third_step(on: bool, state: &SyncState) -> &'static str {
    if !on {
        "Sync"
    } else if *state == SyncState::Syncing {
        "Syncing…"
    } else {
        "Sync on"
    }
}

/// One mark of the strip: done (a check), here (outlined) or still to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Done,
    Here,
    Ahead,
}

/// Mark `n` (1–3) when the page is on `step`; the third is done once sync
/// is on.
pub fn mark(n: u8, step: u8, sync_on: bool) -> Mark {
    if n < step || (n == 3 && sync_on) {
        Mark::Done
    } else if n == step {
        Mark::Here
    } else {
        Mark::Ahead
    }
}

// MARK: 1. connect

/// What the Connect field holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    /// Nothing typed yet.
    Empty,
    /// A code that reads.
    Code(JoinCode),
    /// Something that doesn't: the hint to show, verbatim.
    Bad(&'static str),
}

impl Read {
    pub fn code(&self) -> Option<&JoinCode> {
        match self {
            Read::Code(c) => Some(c),
            _ => None,
        }
    }
}

/// The bare `cp_…` hint (`CloudConnectCard.hint`).
pub const BARE_PAIRING: &str = "That's the pairing code without its address — copy the whole code, from copper-cloud:// to the end";
/// The hint for text that is no code at all.
pub const NOT_A_CODE: &str = "That isn't a link code, a join link or a pairing code — all start copper-cloud://, then #k=, #sso= or #p=";
/// The hint under the hand-entered fields.
pub const BAD_MANUAL: &str = "Check the address, the key and the fingerprint (64 hex characters, or leave it empty for a public certificate)";

/// Read the pasted text locally — never a request. A bare `cp_…` reads only
/// against the instance this Copper is linked to already.
pub fn read_code(text: &str, linked: Option<&Link>) -> Read {
    let typed = text.trim();
    if typed.is_empty() {
        return Read::Empty;
    }
    if let Some(code) = parse_join_code(typed, linked) {
        return Read::Code(code);
    }
    let raw: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
    if is_bare_pairing_code(&raw) {
        Read::Bad(BARE_PAIRING)
    } else {
        Read::Bad(NOT_A_CODE)
    }
}

/// Read the hand-entered address, key and fingerprint: empty until both the
/// address and the key are given (`CloudConnectCard.hint`, advanced).
pub fn read_manual(address: &str, key: &str, fingerprint: &str) -> Read {
    if address.trim().is_empty() || key.trim().is_empty() {
        return Read::Empty;
    }
    let fp = Some(fingerprint.trim()).filter(|f| !f.is_empty());
    match manual_link(address, key, fp) {
        Some(link) => Read::Code(JoinCode::Link(link)),
        None => Read::Bad(BAD_MANUAL),
    }
}

/// The instance a code is for.
pub fn endpoint(code: &JoinCode) -> &Endpoint {
    match code {
        JoinCode::Link(l) => &l.endpoint,
        JoinCode::Pairing(p) => &p.endpoint,
        JoinCode::Sso(s) => &s.endpoint,
    }
}

/// The pin a code carries (lowercase hex), if any.
pub fn pin(code: &JoinCode) -> Option<&str> {
    match code {
        JoinCode::Link(l) => l.fingerprint.as_deref(),
        JoinCode::Pairing(p) => p.fingerprint.as_deref(),
        JoinCode::Sso(s) => s.fingerprint.as_deref(),
    }
}

/// Which kind of code, as the line under the field says it: a glyph name,
/// the title and the detail (`CloudConnectCard.kind`). `provider` is the
/// join link's provider name ("Google"); `domain_hint` "Use your @x.com
/// account" once the instance has said; `remembered` how many switches this
/// device had on before (0: everything turns on).
pub fn kind(
    code: &JoinCode,
    provider: &str,
    domain_hint: Option<&str>,
    remembered: usize,
) -> (KindIcon, String, Option<String>) {
    let after = if remembered == 0 || remembered == Domain::ALL.len() {
        "Sync turns on for everything — switch any of it off after."
    } else {
        "Sync turns on for what this device synced before — switch any of it off after."
    };
    match code {
        JoinCode::Link(_) => (
            KindIcon::Link,
            "Link code — connects to the instance; you sign in next".into(),
            None,
        ),
        JoinCode::Pairing(_) => (
            KindIcon::Pairing,
            "Pairing code — links this device and signs you in".into(),
            Some(format!("Good once, for 10 minutes. {after}")),
        ),
        JoinCode::Sso(_) => {
            let hint = domain_hint.map(|h| format!("{h}. ")).unwrap_or_default();
            (
                KindIcon::Join,
                format!("Join link — links this device and signs you in with {provider}"),
                Some(format!(
                    "{hint}Copper opens your browser to sign in. {after}"
                )),
            )
        }
    }
}

/// The glyph beside a code's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindIcon {
    Link,
    Pairing,
    Join,
}

/// The Instance line while its certificate waits to be compared: the Mac
/// keeps "Trusted by its public certificate" here, which the box under it
/// contradicts.
pub const UNTRUSTED_LINE: &str = "Not trusted on this device yet — compare the fingerprint below";

/// How an instance's certificate is trusted, in a line (`trust(_:)`).
pub fn trust_line(endpoint: &Endpoint, pinned: bool) -> &'static str {
    if !endpoint.https {
        "Plain HTTP — this device only, for development"
    } else if pinned {
        "Pinned to this certificate"
    } else {
        "Trusted by its public certificate"
    }
}

/// The Instance details' first line (`CloudConnection.trust`).
pub fn trust_details(endpoint: &Endpoint, pinned: bool) -> &'static str {
    if !endpoint.https {
        "Plain HTTP — this device only, for development"
    } else if pinned {
        "Pinned to this certificate (SHA-256):"
    } else {
        "Trusted by its public certificate"
    }
}

/// The trust capsule beside the host: its glyph and label
/// (`CloudTrustPill`).
pub fn trust_capsule(endpoint: &Endpoint, pin: Option<&str>) -> (TrustIcon, String) {
    if !endpoint.https {
        return (TrustIcon::Warning, "Plain HTTP".into());
    }
    match pin {
        None => (TrustIcon::Seal, "Public certificate".into()),
        Some(hex) => {
            let p = pairs(hex);
            (
                TrustIcon::Lock,
                format!("{}…", p.chars().take(11).collect::<String>()),
            )
        }
    }
}

/// The capsule's tooltip.
pub fn trust_help(endpoint: &Endpoint, pin: Option<&str>) -> String {
    if !endpoint.https {
        return "Plain HTTP on this device (development)".into();
    }
    match pin {
        None => "Trusted by this device's own certificate roots".into(),
        Some(hex) => format!("Pinned certificate — SHA-256 {}", pairs(hex)),
    }
}

/// The capsule's glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustIcon {
    Warning,
    Seal,
    Lock,
}

/// A fingerprint's 32 pairs as two lines of 16, so it never breaks inside a
/// pair (`cloudPairLines`); `hex` may be plain hex or already in pairs.
pub fn pair_lines(hex: &str) -> String {
    let p = if hex.contains(':') {
        hex.to_string()
    } else {
        pairs(hex)
    };
    let all: Vec<&str> = p.split(':').collect();
    if all.len() <= 16 {
        return p;
    }
    format!("{}:\n{}", all[..16].join(":"), all[16..].join(":"))
}

/// The Connect card's button (`buttonTitle`).
pub fn connect_button(
    code: Option<&JoinCode>,
    connecting: bool,
    trusting: bool,
    provider: &str,
) -> String {
    match code {
        Some(JoinCode::Pairing(_)) => {
            if connecting {
                "Pairing…".into()
            } else if trusting {
                "Trust and pair".into()
            } else {
                "Pair this device".into()
            }
        }
        Some(JoinCode::Sso(_)) => {
            if trusting {
                format!("Trust and continue with {provider}")
            } else {
                format!("Continue with {provider}")
            }
        }
        _ if connecting => "Connecting…".into(),
        _ if trusting => "Trust and connect".into(),
        _ => "Connect".into(),
    }
}

/// The certificate question's first line (`unknownSeen`'s card).
pub fn untrusted_text(code: &JoinCode) -> String {
    let what = match code {
        JoinCode::Link(_) => "link code",
        JoinCode::Pairing(_) => "pairing code",
        JoinCode::Sso(_) => "join link",
    };
    format!(
        "This device doesn't trust the instance's certificate, and the {what} has no fingerprint to pin it by. Its certificate's fingerprint is:"
    )
}

/// The other-instance notice's words (`CloudOfferNotice`).
pub fn offer_notice(code_host: &str, linked_host: &str) -> (String, String) {
    (
        format!("This link is for {code_host}"),
        format!(
            "This device is connected to {linked_host}. To use the link instead, disconnect below, then open it again."
        ),
    )
}

// MARK: 2. account

/// Sign in, or make the account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    SignIn,
    Create,
}

/// Whether the account form's button may be pressed (`ready`): an email and
/// a password, and — creating — a long enough password and a name.
pub fn account_ready(mode: Mode, email: &str, password: &str, name: &str) -> bool {
    !email.trim().is_empty()
        && !password.is_empty()
        && (mode == Mode::SignIn
            || (password.chars().count() >= MIN_PASSWORD && !name.trim().is_empty()))
}

/// The account form's note beside its button.
pub fn account_note(mode: Mode) -> &'static str {
    match mode {
        Mode::Create => "The first account on an instance is its admin.",
        Mode::SignIn => "Same email and password as on your other devices.",
    }
}

/// The account form's button.
pub fn account_button(mode: Mode, working: bool) -> &'static str {
    match (working, mode) {
        (true, _) => "…",
        (false, Mode::Create) => "Create account",
        (false, Mode::SignIn) => "Sign in",
    }
}

// MARK: 3. sync

/// The status dot's colour (`dot`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Green,
    Yellow,
    Orange,
    Red,
}

/// The status line's title: never "Up to date" before a sync has finished,
/// "Syncing…" only while one is under way (`statusTitle`).
pub fn status_title(state: &SyncState, last_sync: Option<f64>) -> &'static str {
    match state {
        SyncState::Idle if last_sync.is_none() => "Sync on",
        SyncState::Idle => "Up to date",
        SyncState::Syncing => "Syncing…",
        SyncState::Offline => "Can't reach the cloud",
        SyncState::Error(_) => "Couldn't sync",
    }
}

/// The status line's detail (`statusDetail`); `ago` turns a time into
/// "2 minutes ago".
pub fn status_detail(
    state: &SyncState,
    last_sync: Option<f64>,
    ago: impl Fn(f64) -> String,
) -> String {
    let last = last_sync.map(|t| format!("Last synced {}", ago(t)));
    match state {
        SyncState::Idle | SyncState::Syncing => last.unwrap_or_else(|| "Not synced yet".into()),
        SyncState::Offline => join_dot(&[Some("Will retry".to_string()), last]),
        SyncState::Error(message) => join_dot(&[Some(message.clone()), last]),
    }
}

fn join_dot(parts: &[Option<String>]) -> String {
    parts
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The status dot.
pub fn status_dot(state: &SyncState) -> Dot {
    match state {
        SyncState::Idle => Dot::Green,
        SyncState::Syncing => Dot::Yellow,
        SyncState::Offline => Dot::Orange,
        SyncState::Error(_) => Dot::Red,
    }
}

/// The browser's switches, in the Mac's order (every domain but the canvas).
pub fn browser_domains() -> Vec<Domain> {
    Domain::ALL
        .into_iter()
        .filter(|d| *d != Domain::Canvas)
        .collect()
}

/// What "Turn on sync" sits beside (`start`).
pub fn chosen_text(chosen: usize) -> String {
    if chosen == 0 {
        "Pick at least one. Nothing leaves this device until you turn sync on.".into()
    } else {
        format!("{chosen} chosen — the first sync merges this device with what the cloud has.")
    }
}

/// Pause sync's detail.
pub const PAUSE_DETAIL: &str =
    "Pauses browser sync and the Personal canvas. Shared canvases stay live.";

/// A device's line: "3 tabs · Updated 2 minutes ago".
pub fn device_line(tabs: usize, updated: Option<f64>, ago: impl Fn(f64) -> String) -> String {
    format!(
        "{tabs} tab{}{}",
        if tabs == 1 { "" } else { "s" },
        updated
            .map(|u| format!(" · Updated {}", ago(u)))
            .unwrap_or_default()
    )
}

/// The host part of a remote tab's address, for the row's right side.
pub fn tab_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

/// A remote tab's tooltip: its address, and the space it is in.
pub fn tab_help(url: &str, space: Option<&str>) -> String {
    match space {
        Some(s) if !s.is_empty() => format!("{url} — in {s}"),
        _ => url.to_string(),
    }
}

/// The log's rows: the newest [`LOG_SHOWN`] first.
pub fn log_rows(log: &[(f64, String)]) -> Vec<(f64, String)> {
    log.iter().rev().take(LOG_SHOWN).cloned().collect()
}

// MARK: pairing

/// What became of a code this device made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Used,
    Revoked,
}

/// The pair card's state at `now` (`content(now:)`).
#[derive(Clone, Debug, PartialEq)]
pub enum PairCard {
    /// No code yet.
    Offer,
    /// A live code: seconds left.
    Live(f64),
    Used,
    Revoked,
    Expired,
}

/// The pair card at `now`, for a code that expires at `expires` (unix
/// seconds) and has `outcome` so far.
pub fn pair_card(code: Option<(f64, Option<Outcome>)>, now: f64) -> PairCard {
    match code {
        None => PairCard::Offer,
        Some((_, Some(Outcome::Used))) => PairCard::Used,
        Some((_, Some(Outcome::Revoked))) => PairCard::Revoked,
        Some((expires, None)) if expires - now <= 0.0 => PairCard::Expired,
        Some((expires, None)) => PairCard::Live(expires - now),
    }
}

/// `m:ss` for a countdown (`CloudPairing.left`), rounding up so it never
/// shows 0:00 while the code still works.
pub fn countdown(left: f64) -> String {
    let secs = left.max(0.0).ceil() as i64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The countdown bar's fraction (0–1).
pub fn countdown_fraction(left: f64) -> f32 {
    (left / PAIR_LIFETIME).clamp(0.0, 1.0) as f32
}

/// A minted code shown in three parts (`CloudCodeBox`): the address, the
/// `cp_…` secret on its own line, the fingerprint. The whole when the
/// secret is not in it.
pub fn code_parts(link: &str, secret: &str) -> (String, String, String) {
    match link.find(secret).filter(|_| !secret.is_empty()) {
        Some(at) => (
            link[..at].to_string(),
            secret.to_string(),
            link[at + secret.len()..].to_string(),
        ),
        None => (String::new(), link.to_string(), String::new()),
    }
}

// MARK: delete history on cloud

// The card's words and choices are shell-core's, lifted from here.
pub use copper_shell_core::cloud::eraser::{
    DELETE_MESSAGE, Span, delete_question, delete_status, finished, selector, typed_site,
};

// MARK: words

/// "just now" for the first 45 seconds (and a time a skewed clock puts a
/// little ahead), then "2 minutes ago", "3 hours ago", "4 days ago": the
/// shared lane's `ago` words, against a given `now` so a whole page is drawn
/// at one instant (and is tested without the clock).
pub fn ago(then: f64, now: f64) -> String {
    let secs = now - then;
    if secs < 45.0 {
        return "just now".into();
    }
    let (n, unit) = if secs < 3_600.0 {
        ((secs / 60.0).round().max(1.0), "minute")
    } else if secs < 86_400.0 {
        ((secs / 3_600.0).round(), "hour")
    } else {
        ((secs / 86_400.0).round(), "day")
    };
    let n = n as i64;
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests;
