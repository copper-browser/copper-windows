//! Settings › Cloud's state: what the cloud says now ([`Snap`], read fresh
//! for every draw) and what the person has typed and chosen on the page
//! ([`State`], kept for the whole run so a draft, a pairing code or a
//! history delete's result is still there when Settings opens again).
//!
//! Pure data, so the page's words are tested without a window.

use super::model::{Mode, Outcome, Span};
use copper_cloud::sso::SsoInfo;
use copper_cloud::sync::{DeviceTabs, SyncState};
use copper_cloud::{Domain, Link};
use copper_shell_core::cloud::sso::SsoState;
use std::collections::BTreeSet;

/// What the cloud says right now.
#[derive(Clone, Debug, Default)]
pub struct Snap {
    /// Copper Cloud is on this run (`cloud.json` read).
    pub on: bool,
    pub link: Option<Link>,
    pub signed_in: bool,
    /// The account's display name and email.
    pub account: Option<(String, String)>,
    /// The instance answered lately.
    pub reachable: bool,
    pub device_name: String,
    /// Browser sync is on.
    pub sync_on: bool,
    /// The switches that are on (or were, before sync was turned off).
    pub enabled: BTreeSet<Domain>,
    pub state: SyncState,
    pub last_sync: Option<f64>,
    pub devices: Vec<DeviceTabs>,
    /// The engine's log as shown: its time of day and words, the newest
    /// first, at most [`super::model::LOG_SHOWN`].
    pub log: Vec<(String, String)>,
    pub sso: SsoState,
    /// What the linked instance offers for single sign-on, once asked.
    pub sso_info: Option<SsoInfo>,
    /// What the instance of the join link in the Connect field offers, if
    /// it has said (it is never asked just because a link was pasted).
    pub join_info: Option<SsoInfo>,
    /// `GET /v1/info` said whether this cloud deletes history.
    pub deletes_history: Option<bool>,
    /// Unix seconds.
    pub now: f64,
}

/// A pairing code this device made.
#[derive(Clone, Debug, PartialEq)]
pub struct Minted {
    pub id: String,
    /// The `cp_…` part.
    pub code: String,
    /// The whole code to paste (always carries the pin).
    pub link: String,
    /// Unix seconds.
    pub expires: f64,
}

/// The page's own: drafts, folds and work under way.
#[derive(Clone, Debug, Default)]
pub struct State {
    // -- 1. connect
    pub code: String,
    /// Address, key and fingerprint by hand instead of a pasted code.
    pub manual: bool,
    pub host: String,
    pub key: String,
    pub fingerprint: String,
    pub connecting: bool,
    pub problem: Option<String>,
    /// The instance showed a certificate nobody vouches for, and the code
    /// has no pin: the code it was for (as typed) and the fingerprint seen,
    /// for a person to compare and trust.
    pub unknown: Option<(String, String)>,

    // -- the instance line
    pub details: bool,

    // -- 2. account
    pub mode: Mode,
    pub email: String,
    pub password: String,
    pub name: String,
    pub working: bool,
    pub account_problem: Option<String>,
    /// "Sign in with a password instead", on an instance that keeps
    /// passwords for its admins.
    pub use_password: bool,
    /// The linked instance's single sign-on was asked about (this showing).
    pub sso_asked: bool,

    // -- 3. sync
    /// The switches before sync is on (the last saved ones to start with).
    pub chosen: Option<BTreeSet<Domain>>,
    pub starting: bool,
    /// Devices whose tabs are shown, by domain.
    pub open_devices: BTreeSet<String>,

    // -- account card
    /// The device name as typed, while it differs from the saved one.
    pub device_name: Option<String>,
    pub rename_problem: Option<String>,

    // -- pairing
    pub minted: Option<Minted>,
    pub outcome: Option<Outcome>,
    pub pair_working: bool,
    /// "Copied" until then (unix seconds).
    pub copied_until: f64,
    pub pair_problem: Option<String>,

    // -- delete history on cloud
    pub span: Span,
    pub site: String,
    pub erasing: bool,
    pub looking: bool,
    pub erase_result: Option<String>,
    pub erase_problem: Option<String>,
    /// The switch of the delete under way (Stop presses it).
    pub erase_stop: Option<copper_cloud::sync::delete::Stop>,
    /// `GET /v1/info` was asked this showing (answered or not).
    pub info_asked: bool,
    /// `GET /v1/info` is on its way.
    pub info_asking: bool,
    /// The other devices' tabs were fetched this showing.
    pub devices_asked: bool,

    // -- the log
    pub log_open: bool,
}

impl State {
    /// A fresh page: the name to create an account with is this user's.
    pub fn new(full_name: String) -> State {
        State {
            name: full_name,
            ..State::default()
        }
    }

    /// The switches "Turn on sync" would turn on: the draft, or the ones
    /// saved from before.
    pub fn chosen(&self, saved: &BTreeSet<Domain>) -> BTreeSet<Domain> {
        self.chosen.clone().unwrap_or_else(|| saved.clone())
    }

    /// The fingerprint waiting to be trusted for `code` (as typed now).
    pub fn seen_for(&self, code: &str) -> Option<&str> {
        self.unknown
            .as_ref()
            .filter(|(c, _)| c == code)
            .map(|(_, s)| s.as_str())
    }

    /// The text of a field's draft.
    pub fn draft(&self, id: super::el::FieldId) -> &str {
        use super::el::FieldId::*;
        match id {
            Code => &self.code,
            Host => &self.host,
            Key => &self.key,
            Fingerprint => &self.fingerprint,
            Email => &self.email,
            Password => &self.password,
            Name => &self.name,
            DeviceName => self.device_name.as_deref().unwrap_or(""),
            Site => &self.site,
        }
    }

    /// Keep what a field now says. `true` when it changes what the page
    /// shows (a code that now reads, a hint), so the page redraws.
    pub fn set_draft(&mut self, id: super::el::FieldId, text: &str) {
        use super::el::FieldId::*;
        let t = text.to_string();
        match id {
            Code => {
                if self.code != t {
                    self.problem = None;
                }
                self.code = t;
            }
            Host => self.host = t,
            Key => self.key = t,
            Fingerprint => self.fingerprint = t,
            Email => self.email = t,
            Password => self.password = t,
            Name => self.name = t,
            DeviceName => self.device_name = Some(t),
            Site => self.site = t,
        }
        if matches!(id, Host | Key | Fingerprint) {
            self.problem = None;
        }
        if matches!(id, Email | Password | Name) {
            self.account_problem = None;
        }
    }

    /// The pairing code's card at `now`, in [`super::model::pair_card`]'s
    /// terms.
    pub fn pair_card(&self, now: f64) -> super::model::PairCard {
        super::model::pair_card(self.minted.as_ref().map(|m| (m.expires, self.outcome)), now)
    }
}
