use super::super::el::{Act, El, FieldId, Kind, PillLook};
use super::super::model::{Mode, Outcome, Span};
use super::super::state::{Minted, Snap, State};
use super::page;
use copper_cloud::sso::SsoInfo;
use copper_cloud::sync::{DeviceTabs, SyncState};
use copper_cloud::{Domain, Link, parse_link_code};
use copper_core::cloud::OpenTab;
use copper_shell_core::cloud::sso::{JoinOffer, SsoPhase, SsoTarget};

const FP: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

fn link() -> Link {
    parse_link_code(&format!(
        "copper-cloud://cloud.example.com:8443/#k=key1&fp={FP}"
    ))
    .unwrap()
}

fn unlinked() -> Snap {
    Snap {
        on: true,
        now: 1_000_000.0,
        ..Snap::default()
    }
}

fn signed_in() -> Snap {
    Snap {
        link: Some(link()),
        signed_in: true,
        account: Some(("Ann".into(), "ann@example.com".into())),
        reachable: true,
        device_name: "PC".into(),
        ..unlinked()
    }
}

fn keys(els: &[El]) -> Vec<&str> {
    els.iter().map(|e| e.key.as_str()).collect()
}

fn words(els: &[El]) -> Vec<String> {
    els.iter().flat_map(El::words).collect()
}

fn has(els: &[El], w: &str) -> bool {
    words(els).iter().any(|x| x == w)
}

/// The pill labelled `label` anywhere in `els`.
fn pill<'a>(els: &'a [El], label: &str) -> Option<(&'a Act, PillLook, bool)> {
    let mut found = None;
    for e in els {
        e.walk(&mut |e| {
            if let Kind::Pill {
                label: l,
                act,
                look,
                enabled,
            } = &e.kind
                && l == label
            {
                found = Some((act, *look, *enabled));
            }
        });
    }
    found
}

fn fields(els: &[El]) -> Vec<FieldId> {
    let mut out = vec![];
    for e in els {
        e.walk(&mut |e| {
            if let Kind::Field { id, .. } = &e.kind {
                out.push(*id);
            }
        });
    }
    out
}

#[test]
fn off_says_so() {
    let p = page(&Snap::default(), &State::default());
    assert!(has(
        &p,
        "Copper Cloud is off this run — cloud.json couldn't be read"
    ));
}

#[test]
fn unlinked_is_steps_and_connect_only() {
    let p = page(&unlinked(), &State::default());
    assert_eq!(keys(&p), ["steps", "connect"]);
    assert!(has(&p, "1 · Connect to an instance"));
    assert_eq!(fields(&p), [FieldId::Code]);
    // Nothing typed: the button is there and can't be pressed.
    assert_eq!(
        pill(&p, "Connect"),
        Some((&Act::Submit, PillLook::Plain, false))
    );
    assert!(has(&p, "Enter it by hand"));
}

#[test]
fn a_bad_code_says_why_and_keeps_the_button_off() {
    let st = State {
        code: "cp_abcdefghijklmnopqrstu".into(),
        ..State::default()
    };
    let p = page(&unlinked(), &st);
    assert!(has(
        &p,
        "That's the pairing code without its address — copy the whole code, from copper-cloud:// to the end"
    ));
    assert_eq!(pill(&p, "Connect").map(|p| p.2), Some(false));
}

#[test]
fn a_link_code_reads_with_its_pin() {
    let st = State {
        code: format!("copper-cloud://cloud.example.com:8443/#k=key1&fp={FP}"),
        ..State::default()
    };
    let p = page(&unlinked(), &st);
    assert!(has(
        &p,
        "Link code — connects to the instance; you sign in next"
    ));
    assert!(has(&p, "Pinned to this certificate"));
    assert!(has(&p, "Compare with `copper-cloud doctor` on the server"));
    assert_eq!(
        pill(&p, "Connect"),
        Some((&Act::Submit, PillLook::Filled, true))
    );
    // Connecting: the button says so and can't be pressed twice.
    let busy = State {
        connecting: true,
        ..st
    };
    assert_eq!(
        pill(&page(&unlinked(), &busy), "Connecting…").map(|p| p.2),
        Some(false)
    );
}

#[test]
fn an_untrusted_certificate_is_compared_before_trusting() {
    let code = "copper-cloud://cloud.example.com/#p=cp_abcdefghijklmnopqrstu".to_string();
    let st = State {
        code: code.clone(),
        unknown: Some((code, FP.into())),
        ..State::default()
    };
    let p = page(&unlinked(), &st);
    assert!(words(&p).iter().any(|w| w.starts_with(
        "This device doesn't trust the instance's certificate, and the pairing code has no fingerprint"
    )));
    assert!(pill(&p, "Trust and pair").is_some());
    assert!(has(&p, crate::cloud::model::UNTRUSTED_LINE));
    // Another code typed: the question goes with the old one.
    let other = State {
        code: "copper-cloud://other.example.com/#p=cp_abcdefghijklmnopqrstu".into(),
        ..st
    };
    assert!(pill(&page(&unlinked(), &other), "Pair this device").is_some());
}

#[test]
fn a_join_link_offers_its_provider_without_asking() {
    let st = State {
        code: "copper-cloud://copper.example.com/#sso=google".into(),
        ..State::default()
    };
    let p = page(&unlinked(), &st);
    assert!(has(
        &p,
        "Join link — links this device and signs you in with Google"
    ));
    assert_eq!(
        pill(&p, "Continue with Google"),
        Some((&Act::Submit, PillLook::Filled, true))
    );
    assert!(has(&p, "Trusted by its public certificate"));
}

#[test]
fn a_running_sign_in_waits_with_cancel() {
    let st = State {
        code: "copper-cloud://copper.example.com/#sso=google".into(),
        ..State::default()
    };
    let mut s = unlinked();
    let sso = copper_cloud::parse_sso_link(&st.code).unwrap();
    s.sso.target = Some(SsoTarget::Join(sso));
    s.sso.phase = SsoPhase::Waiting("Google".into());
    let p = page(&s, &st);
    assert!(has(&p, "Finish signing in in your browser…"));
    assert_eq!(
        pill(&p, "Cancel"),
        Some((&Act::SsoCancel, PillLook::Plain, true))
    );
    // A refusal shows under the card, in the shared words.
    s.sso.phase = SsoPhase::Failed {
        code: "sso_timeout".into(),
        message: "Nothing came back from the browser in 5 minutes — try again".into(),
    };
    let p = page(&s, &st);
    assert!(has(
        &p,
        "Nothing came back from the browser in 5 minutes — try again"
    ));
    assert!(pill(&p, "Continue with Google").is_some());
}

#[test]
fn manual_entry_takes_three_fields() {
    let st = State {
        manual: true,
        host: "cloud.example.com".into(),
        key: "key".into(),
        fingerprint: "nothex".into(),
        ..State::default()
    };
    let p = page(&unlinked(), &st);
    assert_eq!(
        fields(&p),
        [FieldId::Host, FieldId::Key, FieldId::Fingerprint]
    );
    assert!(has(&p, "Paste a code instead"));
    assert!(has(
        &p,
        "Check the address, the key and the fingerprint (64 hex characters, or leave it empty for a public certificate)"
    ));
}

#[test]
fn linked_folds_connect_and_asks_for_the_account() {
    let s = Snap {
        signed_in: false,
        account: None,
        ..signed_in()
    };
    let p = page(&s, &State::default());
    assert_eq!(keys(&p), ["steps", "connection", "signin"]);
    assert!(has(&p, "Connected to"));
    assert!(has(&p, "Change…"));
    assert_eq!(fields(&p), [FieldId::Email, FieldId::Password]);
    assert_eq!(pill(&p, "Sign in").map(|p| p.2), Some(false));
    let create = State {
        mode: Mode::Create,
        email: "a@b.c".into(),
        password: "short".into(),
        name: "Ann".into(),
        ..State::default()
    };
    let p = page(&s, &create);
    assert_eq!(
        fields(&p),
        [FieldId::Email, FieldId::Password, FieldId::Name]
    );
    assert!(has(&p, "At least 10 characters"));
    assert_eq!(pill(&p, "Create account").map(|p| p.2), Some(false));
    // No Google button without the instance saying it offers one.
    assert!(pill(&p, "Continue with Google").is_none());
}

fn sso_info(password_signin: bool) -> SsoInfo {
    serde_json::from_value(serde_json::json!({
        "enabled": true,
        "password_signin": password_signin,
        "providers": [{
            "id": "google", "kind": "google", "name": "Google",
            "client_id": "x", "authorization_endpoint": "https://accounts.google.com/o/oauth2/v2/auth",
            "domains": ["exowatt.com"]
        }]
    }))
    .unwrap()
}

#[test]
fn an_instance_with_single_sign_on_offers_it_first() {
    let s = Snap {
        signed_in: false,
        account: None,
        sso_info: Some(sso_info(true)),
        ..signed_in()
    };
    let p = page(&s, &State::default());
    assert!(has(&p, "Sign in with your Google account"));
    assert!(has(
        &p,
        "Use your @exowatt.com account — Copper opens your browser."
    ));
    assert_eq!(
        pill(&p, "Continue with Google"),
        Some((&Act::SsoContinue, PillLook::Filled, true))
    );
    assert_eq!(fields(&p), [FieldId::Email, FieldId::Password]);
    // Passwords for admins only: behind a line.
    let admins = Snap {
        sso_info: Some(sso_info(false)),
        ..s
    };
    let p = page(&admins, &State::default());
    assert!(fields(&p).is_empty());
    assert!(has(&p, "Sign in with a password instead"));
    let opened = State {
        use_password: true,
        ..State::default()
    };
    assert_eq!(fields(&page(&admins, &opened)).len(), 2);
}

#[test]
fn signed_in_sync_off_waits_for_turn_on() {
    let p = page(&signed_in(), &State::default());
    assert_eq!(
        keys(&p),
        [
            "steps", "browser", "canvas", "history", "account", "pair", "instance", "log"
        ]
    );
    assert!(has(&p, "3 · Browser sync"));
    assert!(has(&p, "Personal canvas"));
    assert_eq!(pill(&p, "Turn on sync").map(|p| p.2), Some(false));
    assert!(has(
        &p,
        "Pick at least one. Nothing leaves this device until you turn sync on."
    ));
    let chosen = State {
        chosen: Some([Domain::Bookmarks, Domain::History].into_iter().collect()),
        ..State::default()
    };
    let p = page(&signed_in(), &chosen);
    assert_eq!(
        pill(&p, "Turn on sync"),
        Some((&Act::TurnOn, PillLook::Filled, true))
    );
    assert!(has(
        &p,
        "2 chosen — the first sync merges this device with what the cloud has."
    ));
}

#[test]
fn sync_on_shows_status_and_hides_empty_devices() {
    let mut s = signed_in();
    s.sync_on = true;
    s.enabled = Domain::ALL.into_iter().collect();
    let p = page(&s, &State::default());
    assert_eq!(
        keys(&p),
        [
            "steps", "status", "browser", "canvas", "history", "account", "pair", "instance", "log"
        ]
    );
    assert!(has(&p, "Sync on"));
    assert!(has(&p, "Not synced yet"));
    assert!(has(&p, "Browser sync"));
    s.state = SyncState::Syncing;
    assert_eq!(
        pill(&page(&s, &State::default()), "Sync now").map(|p| p.2),
        Some(false)
    );
    s.state = SyncState::Offline;
    s.last_sync = Some(s.now - 120.0);
    let p = page(&s, &State::default());
    assert!(has(&p, "Can't reach the cloud"));
    assert!(has(&p, "Will retry · Last synced 2 minutes ago"));
    assert_eq!(pill(&p, "Sync now").map(|p| p.2), Some(true));
}

#[test]
fn other_devices_fold_and_cap_their_tabs() {
    let mut s = signed_in();
    s.sync_on = true;
    let tabs: Vec<OpenTab> = (0..80)
        .map(|i| OpenTab {
            url: format!("https://site{i}.example.com/"),
            title: format!("Site {i}"),
            space: None,
            active: None,
            extra: Default::default(),
        })
        .collect();
    s.devices = vec![DeviceTabs {
        domain: "tabs:MAC".into(),
        device_id: Some("MAC".into()),
        name: "Mac".into(),
        updated: Some(s.now - 30.0),
        version: 1,
        tabs,
    }];
    let p = page(&s, &State::default());
    assert!(keys(&p).contains(&"devices"));
    assert!(has(&p, "80 tabs · Updated just now"));
    let open = State {
        open_devices: ["tabs:MAC".to_string()].into_iter().collect(),
        ..State::default()
    };
    let p = page(&s, &open);
    let mut rows = 0;
    for e in &p {
        e.walk(&mut |e| {
            if matches!(
                e.kind,
                Kind::Press {
                    act: Act::OpenTab(_),
                    ..
                }
            ) {
                rows += 1;
            }
        });
    }
    assert_eq!(rows, 60);
}

#[test]
fn pairing_card_states() {
    let s = signed_in();
    let p = page(&s, &State::default());
    assert_eq!(pill(&p, "Make a code").map(|p| p.0), Some(&Act::MakeCode));
    let minted = Minted {
        id: "1".into(),
        code: "cp_secret".into(),
        link: format!("copper-cloud://cloud.example.com:8443/#p=cp_secret&fp={FP}"),
        expires: s.now + 30.0,
    };
    let live = State {
        minted: Some(minted.clone()),
        ..State::default()
    };
    let p = page(&s, &live);
    assert!(has(&p, "Expires in 0:30 · works once"));
    assert!(has(&p, "Copy"));
    assert!(has(&p, "Revoke"));
    let copied = State {
        copied_until: s.now + 1.0,
        ..live.clone()
    };
    assert!(has(&page(&s, &copied), "Copied"));
    let used = State {
        outcome: Some(Outcome::Used),
        ..live.clone()
    };
    assert!(has(
        &page(&s, &used),
        "Used — the other device is signed in"
    ));
    let revoked = State {
        outcome: Some(Outcome::Revoked),
        ..live.clone()
    };
    assert!(has(&page(&s, &revoked), "Code revoked"));
    let expired = State {
        minted: Some(Minted {
            expires: s.now - 1.0,
            ..minted
        }),
        ..State::default()
    };
    assert!(has(&page(&s, &expired), "Code expired"));
}

#[test]
fn history_delete_states() {
    let mut s = signed_in();
    let asking = page(&s, &State::default());
    assert!(has(&asking, "Asking the cloud what it can do…"));
    let asked = State {
        info_asked: true,
        ..State::default()
    };
    assert!(has(
        &page(&s, &asked),
        "Couldn't reach the cloud to ask whether it can delete history."
    ));
    s.deletes_history = Some(false);
    assert!(has(
        &page(&s, &State::default()),
        "This cloud can't delete history — it needs copper-cloud 0.8.0 or later."
    ));
    s.deletes_history = Some(true);
    let p = page(&s, &State::default());
    assert!(has(&p, "Last 24 hours"));
    assert_eq!(
        pill(&p, "Delete history on cloud…"),
        Some((&Act::DeleteHistory, PillLook::Red, true))
    );
    let bad = State {
        site: "not a site".into(),
        span: Span::Week,
        ..State::default()
    };
    let p = page(&s, &bad);
    assert!(has(&p, "That isn't a site — try example.com"));
    assert_eq!(
        pill(&p, "Delete history on cloud…").map(|p| p.2),
        Some(false)
    );
}

#[test]
fn details_and_log_unfold() {
    let s = Snap {
        log: vec![("12:00:01".into(), "Pushed bookmarks".into())],
        ..signed_in()
    };
    let p = page(&s, &State::default());
    assert!(!has(&p, "Pushed bookmarks"));
    assert!(pill(&p, "Disconnect").is_none());
    let open = State {
        details: true,
        log_open: true,
        ..State::default()
    };
    let p = page(&s, &open);
    assert!(has(&p, "Pushed bookmarks"));
    assert!(has(&p, "Pinned to this certificate (SHA-256):"));
    assert_eq!(
        pill(&p, "Disconnect"),
        Some((&Act::Disconnect, PillLook::Red, true))
    );
    let empty = Snap {
        log: vec![],
        ..signed_in()
    };
    assert!(has(&page(&empty, &open), "Nothing yet"));
}

#[test]
fn a_link_for_another_instance_waits_on_the_page() {
    let mut s = signed_in();
    let code = copper_cloud::parse_join_code("copper-cloud://other.example.com/#sso=google", None)
        .unwrap();
    s.sso.offer = Some(JoinOffer {
        id: 1,
        text: "copper-cloud://other.example.com/#sso=google".into(),
        code,
    });
    let p = page(&s, &State::default());
    assert_eq!(keys(&p)[1], "offer");
    assert!(has(&p, "This link is for other.example.com"));
    assert_eq!(pill(&p, "Dismiss").map(|p| p.0), Some(&Act::DismissOffer));
}
