use super::*;

const FP: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

fn link() -> Link {
    copper_cloud::parse_link_code(&format!(
        "copper-cloud://cloud.example.com:8443/#k=key1&fp={FP}"
    ))
    .expect("link")
}

#[test]
fn steps_and_marks() {
    assert_eq!(step(false, false), 1);
    assert_eq!(step(true, false), 2);
    assert_eq!(step(true, true), 3);
    assert_eq!(mark(1, 1, false), Mark::Here);
    assert_eq!(mark(2, 1, false), Mark::Ahead);
    assert_eq!(mark(1, 3, false), Mark::Done);
    assert_eq!(mark(3, 3, false), Mark::Here);
    assert_eq!(mark(3, 3, true), Mark::Done);
    assert_eq!(third_step(false, &SyncState::Syncing), "Sync");
    assert_eq!(third_step(true, &SyncState::Idle), "Sync on");
    assert_eq!(third_step(true, &SyncState::Offline), "Sync on");
    assert_eq!(third_step(true, &SyncState::Syncing), "Syncing…");
}

#[test]
fn codes_read_locally() {
    assert_eq!(read_code("  ", None), Read::Empty);
    let l = read_code(
        &format!("copper-cloud://h.example:8443/#k=abc&fp={FP}"),
        None,
    );
    assert!(matches!(l, Read::Code(JoinCode::Link(_))));
    let p = read_code(
        "copper-cloud://h.example:8443/#p=cp_abcdefghijklmnopqrstu",
        None,
    );
    assert!(matches!(p, Read::Code(JoinCode::Pairing(_))));
    let s = read_code("copper-cloud://h.example/#sso=google", None);
    assert!(matches!(s, Read::Code(JoinCode::Sso(_))));
    // A deep link reads as the code it carries.
    let deep = "copper://cloud/join?code=copper-cloud%3A%2F%2Fh.example%2F%23sso%3Dgoogle";
    assert!(matches!(
        read_code(deep, None),
        Read::Code(JoinCode::Sso(_))
    ));
    // Bare pairing: only against the instance linked already.
    let bare = "cp_abcdefghijklmnopqrstu";
    assert_eq!(read_code(bare, None), Read::Bad(BARE_PAIRING));
    assert!(matches!(
        read_code(bare, Some(&link())),
        Read::Code(JoinCode::Pairing(_))
    ));
    assert_eq!(read_code("hello there", None), Read::Bad(NOT_A_CODE));
    assert_eq!(
        read_code("https://example.com", None),
        Read::Bad(NOT_A_CODE)
    );
}

#[test]
fn manual_fields() {
    assert_eq!(read_manual("", "k", ""), Read::Empty);
    assert_eq!(read_manual("h.example:443", "", ""), Read::Empty);
    assert!(matches!(
        read_manual("h.example:443", "key", ""),
        Read::Code(JoinCode::Link(_))
    ));
    assert!(matches!(
        read_manual("h.example:443", "key", FP),
        Read::Code(JoinCode::Link(l)) if l.fingerprint.as_deref() == Some(FP)
    ));
    // 63 hex characters is not a fingerprint.
    assert_eq!(
        read_manual("h.example", "key", &FP[1..]),
        Read::Bad(BAD_MANUAL)
    );
    // A key with a space in it is not a key.
    assert_eq!(read_manual("h.example", "a key", ""), Read::Bad(BAD_MANUAL));
}

#[test]
fn kinds_and_buttons() {
    let l = read_code(&format!("copper-cloud://h.example/#k=abc&fp={FP}"), None);
    let code = l.code().unwrap();
    assert_eq!(
        kind(code, "Google", None, 0).1,
        "Link code — connects to the instance; you sign in next"
    );
    assert_eq!(
        connect_button(Some(code), false, false, "Google"),
        "Connect"
    );
    assert_eq!(
        connect_button(Some(code), true, false, "Google"),
        "Connecting…"
    );
    assert_eq!(
        connect_button(Some(code), false, true, "Google"),
        "Trust and connect"
    );
    let p = read_code("copper-cloud://h.example/#p=cp_abcdefghijklmnopqrstu", None);
    let p = p.code().unwrap();
    let (_, t, d) = kind(p, "Google", None, 0);
    assert_eq!(t, "Pairing code — links this device and signs you in");
    assert_eq!(
        d.unwrap(),
        "Good once, for 10 minutes. Sync turns on for everything — switch any of it off after."
    );
    // Only "everything" when everything was in fact chosen.
    assert!(!kind(p, "Google", None, 2).2.unwrap().contains("everything"));
    assert_eq!(
        connect_button(Some(p), false, false, ""),
        "Pair this device"
    );
    assert_eq!(connect_button(Some(p), true, false, ""), "Pairing…");
    assert_eq!(connect_button(Some(p), false, true, ""), "Trust and pair");
    let s = read_code("copper-cloud://h.example/#sso=google", None);
    let s = s.code().unwrap();
    let (_, t, d) = kind(s, "Google", Some("Use your @exowatt.com account"), 0);
    assert_eq!(
        t,
        "Join link — links this device and signs you in with Google"
    );
    assert!(
        d.unwrap()
            .starts_with("Use your @exowatt.com account. Copper opens your browser")
    );
    assert_eq!(
        connect_button(Some(s), false, false, "Google"),
        "Continue with Google"
    );
    assert_eq!(
        connect_button(Some(s), false, true, "Google"),
        "Trust and continue with Google"
    );
    assert!(untrusted_text(p).contains("the pairing code has no fingerprint"));
    assert!(untrusted_text(s).contains("the join link has no fingerprint"));
}

#[test]
fn trust_words() {
    let https = Endpoint {
        https: true,
        host: "h.example".into(),
        port: None,
    };
    let http = Endpoint {
        https: false,
        host: "127.0.0.1".into(),
        port: Some(8080),
    };
    assert_eq!(trust_line(&https, true), "Pinned to this certificate");
    assert_eq!(
        trust_line(&https, false),
        "Trusted by its public certificate"
    );
    assert_eq!(
        trust_line(&http, false),
        "Plain HTTP — this device only, for development"
    );
    assert_eq!(trust_capsule(&https, None).1, "Public certificate");
    assert_eq!(trust_capsule(&http, None).1, "Plain HTTP");
    assert_eq!(trust_capsule(&https, Some(FP)).1, "cd:cd:cd:cd…");
    assert!(trust_help(&https, Some(FP)).starts_with("Pinned certificate — SHA-256 cd:cd"));
}

#[test]
fn fingerprints_in_two_lines() {
    let lines = pair_lines(FP);
    let parts: Vec<&str> = lines.split('\n').collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].split(':').filter(|p| !p.is_empty()).count(), 16);
    assert!(parts[0].ends_with(':'));
    assert_eq!(parts[1].split(':').count(), 16);
    assert_eq!(pair_lines("ab:cd"), "ab:cd");
    assert_eq!(pair_lines(&pairs(FP)), lines);
}

#[test]
fn account_rules() {
    assert!(!account_ready(Mode::SignIn, "", "pw", ""));
    assert!(account_ready(Mode::SignIn, "a@b.c", "pw", ""));
    assert!(!account_ready(Mode::Create, "a@b.c", "short", "Ann"));
    assert!(!account_ready(Mode::Create, "a@b.c", "long enough pw", " "));
    assert!(account_ready(
        Mode::Create,
        "a@b.c",
        "long enough pw",
        "Ann"
    ));
    assert_eq!(account_button(Mode::Create, false), "Create account");
    assert_eq!(account_button(Mode::SignIn, true), "…");
    assert_eq!(
        account_note(Mode::SignIn),
        "Same email and password as on your other devices."
    );
}

#[test]
fn status_line() {
    let ago = |_| "2 minutes ago".to_string();
    assert_eq!(status_title(&SyncState::Idle, None), "Sync on");
    assert_eq!(status_title(&SyncState::Idle, Some(1.0)), "Up to date");
    assert_eq!(status_title(&SyncState::Syncing, Some(1.0)), "Syncing…");
    assert_eq!(
        status_title(&SyncState::Offline, None),
        "Can't reach the cloud"
    );
    assert_eq!(
        status_title(&SyncState::Error("x".into()), None),
        "Couldn't sync"
    );
    assert_eq!(status_detail(&SyncState::Idle, None, ago), "Not synced yet");
    assert_eq!(
        status_detail(&SyncState::Idle, Some(1.0), ago),
        "Last synced 2 minutes ago"
    );
    assert_eq!(status_detail(&SyncState::Offline, None, ago), "Will retry");
    assert_eq!(
        status_detail(&SyncState::Offline, Some(1.0), ago),
        "Will retry · Last synced 2 minutes ago"
    );
    assert_eq!(
        status_detail(&SyncState::Error("Server said no".into()), Some(1.0), ago),
        "Server said no · Last synced 2 minutes ago"
    );
    assert_eq!(status_dot(&SyncState::Offline), Dot::Orange);
    assert_eq!(browser_domains().len(), 5);
    assert!(!browser_domains().contains(&Domain::Canvas));
    assert!(chosen_text(0).starts_with("Pick at least one."));
    assert_eq!(
        chosen_text(3),
        "3 chosen — the first sync merges this device with what the cloud has."
    );
}

#[test]
fn devices_and_log() {
    let ago = |_| "just now".to_string();
    assert_eq!(device_line(1, None, ago), "1 tab");
    assert_eq!(device_line(3, Some(1.0), ago), "3 tabs · Updated just now");
    assert_eq!(tab_host("https://www.example.com/a?b"), "www.example.com");
    assert_eq!(tab_host("not a url"), "");
    assert_eq!(
        tab_help("https://x.com/", Some("Work")),
        "https://x.com/ — in Work"
    );
    assert_eq!(tab_help("https://x.com/", None), "https://x.com/");
    let log: Vec<(f64, String)> = (0..200).map(|i| (i as f64, format!("e{i}"))).collect();
    let rows = log_rows(&log);
    assert_eq!(rows.len(), LOG_SHOWN);
    assert_eq!(rows[0].1, "e199");
    assert!(log_rows(&[]).is_empty());
}

#[test]
fn pairing_card() {
    assert_eq!(pair_card(None, 0.0), PairCard::Offer);
    assert_eq!(pair_card(Some((100.0, None)), 40.0), PairCard::Live(60.0));
    assert_eq!(pair_card(Some((100.0, None)), 100.0), PairCard::Expired);
    assert_eq!(
        pair_card(Some((100.0, Some(Outcome::Used))), 200.0),
        PairCard::Used
    );
    assert_eq!(
        pair_card(Some((100.0, Some(Outcome::Revoked))), 0.0),
        PairCard::Revoked
    );
    assert_eq!(countdown(600.0), "10:00");
    assert_eq!(countdown(59.2), "1:00");
    assert_eq!(countdown(5.0), "0:05");
    assert_eq!(countdown(-3.0), "0:00");
    assert_eq!(countdown_fraction(300.0), 0.5);
    assert_eq!(countdown_fraction(900.0), 1.0);
    let link = format!("copper-cloud://h:8443/#p=cp_secret&fp={FP}");
    let (head, secret, tail) = code_parts(&link, "cp_secret");
    assert_eq!(head, "copper-cloud://h:8443/#p=");
    assert_eq!(secret, "cp_secret");
    assert_eq!(tail, format!("&fp={FP}"));
    assert_eq!(
        code_parts("x", "y"),
        (String::new(), "x".into(), String::new())
    );
}

#[test]
fn ago_words() {
    assert_eq!(ago(100.0, 120.0), "just now");
    assert_eq!(ago(200.0, 100.0), "just now");
    assert_eq!(ago(0.0, 60.0), "1 minute ago");
    assert_eq!(ago(0.0, 600.0), "10 minutes ago");
    assert_eq!(ago(0.0, 7_200.0), "2 hours ago");
    assert_eq!(ago(0.0, 86_400.0 * 3.0), "3 days ago");
    // The same words as the shared lane's, which reads the clock itself.
    let now = copper_core::date::now_unix();
    for back in [10.0, 300.0, 10_000.0, 400_000.0] {
        assert_eq!(
            ago(now - back, now),
            copper_shell_core::cloud::ago(now - back)
        );
    }
}
