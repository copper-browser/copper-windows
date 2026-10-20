//! Intelligence's Check: one request down each lane that is set up, timed,
//! and what came back put in a word a person can act on (the gateway's or
//! Jev's failure, the network's, a refused key).

use super::*;

/// `340 ms` / `1.2 s`.
fn duration(ms: f64) -> String {
    if ms < 1000.0 {
        format!("{} ms", ms.round() as i64)
    } else {
        format!("{:.1} s", ms / 1000.0)
    }
}

/// A transport failure in a person's words.
fn network(detail: &str, who: &str, waited: u64) -> String {
    let d = detail.to_lowercase();
    if d.contains("timed out") || d.contains("timeout") {
        format!(
            "{who} didn't answer in {waited} seconds. Try again; if it keeps happening, check the address."
        )
    } else if d.contains("dns") || d.contains("lookup") || d.contains("resolve") {
        format!("{who} couldn't be found at that address — check it, or your network.")
    } else if d.contains("refused") {
        format!(
            "Nothing answered at {}'s address — check it.",
            who.to_lowercase()
        )
    } else if d.contains("certificate") || d.contains("tls") {
        format!("{who}'s address has a certificate this PC doesn't trust.")
    } else {
        format!("{who} couldn't be reached. Try again.")
    }
}

/// Jev's failure (`IntelligenceCheck.jevFailure`).
fn jev_failure(kind: &str, detail: &str) -> Verdict {
    let (text, refused) = match kind {
        "transport" => (network(detail, "Jev", 4), false),
        "http_401" | "http_403" => (
            "Jev turned the key away — paste a new one below.".into(),
            true,
        ),
        "http_402" => ("Jev says this key's account is out of credit.".into(), true),
        "http_429" => (
            "Jev is busy with this key just now — try again in a minute.".into(),
            false,
        ),
        "bad_endpoint" => ("Jev's address isn't a web address.".into(), false),
        k if k.starts_with("http_5") => (
            format!(
                "Jev is having trouble on its side ({}). Try again later.",
                &k[5..]
            ),
            false,
        ),
        k if k.starts_with("http_") => {
            (format!("Jev answered with an error ({}).", &k[5..]), false)
        }
        _ => ("Jev's answer didn't make sense. Try again.".into(), false),
    };
    Verdict {
        lane: "jev",
        ok: false,
        text,
        refused,
    }
}

/// The gateway's failure (`IntelligenceCheck.gatewayFailure`); `error` is
/// `router 401: body` for an answer, anything else for the network.
fn gateway_failure(error: &str, model: &str) -> Verdict {
    let status = error
        .strip_prefix("router ")
        .and_then(|r| r.split(':').next())
        .and_then(|s| s.parse::<u16>().ok());
    let body = error.to_lowercase();
    let (text, refused) = match status {
        Some(401 | 403) => (
            "The gateway turned the key away — paste a new one above.".into(),
            true,
        ),
        Some(400 | 404) if body.contains("model") => (
            format!("The gateway has no model called “{model}” — check Model names."),
            false,
        ),
        Some(404) => (
            "Nothing at that address answers like a gateway — check the address.".into(),
            false,
        ),
        Some(429) => (
            "The gateway is limiting this key just now — try again in a minute.".into(),
            false,
        ),
        Some(s) if s >= 500 => (
            format!("The gateway is having trouble on its side ({s}). Try again later."),
            false,
        ),
        Some(s) => (format!("The gateway answered with an error ({s})."), false),
        None if error.contains("no choices") => (
            "The gateway's answer didn't make sense — is that the right address?".into(),
            false,
        ),
        None => (network(error, "The gateway", 15), false),
    };
    Verdict {
        lane: "router",
        ok: false,
        text,
        refused,
    }
}

/// Ask Jev and the gateway one small question each and keep the verdicts.
pub(super) fn run_check(shell: &Shell) {
    if CHECK.with(|c| std::mem::replace(&mut c.borrow_mut().running, true)) {
        return;
    }
    let eff = effective(shell);
    let keys = eff.keys.clone();
    let (jev_ready, router_ready) = (eff.jev_ready(), eff.router_ready());
    let model = brain::model(&keys, None);
    let asked = whole(&keys);
    let asked_with = vec![
        ("jev", lane_key(&keys, "jev")),
        ("router", lane_key(&keys, "router")),
    ];
    redraw(shell, "intelligence");
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let version = copper_shell_core::updates::version().to_string();
        let (k, m) = (keys.clone(), model.clone());
        let answers = cloud::spawn(async move {
            let net = brain::Net::new(cloud::runtime().handle().clone(), &version);
            let jev = if jev_ready {
                let state = json!({ "word": "apple" });
                let questions = json!({ "kind": {
                    "type": "choice",
                    "instructions": "What is `word`?",
                    "criteria": { "fruit": "a fruit", "tool": "a tool" },
                }});
                Some(brain::jev_ask(&net, &k, &state, &questions, Duration::from_secs(4)).await)
            } else {
                None
            };
            let router = if router_ready {
                Some(
                    brain::router_ask(
                        &net,
                        &k,
                        &m,
                        "Reply with JSON only.",
                        "{\"ping\": true} → reply {\"pong\": true}",
                        Duration::from_secs(15),
                        20,
                    )
                    .await,
                )
            } else {
                None
            };
            (jev, router)
        })
        .await
        .ok();
        let mut verdicts = Vec::with_capacity(2);
        let (jev, router) = answers.unwrap_or((None, None));
        verdicts.push(match jev {
            Some(Ok(a)) => Verdict {
                lane: "jev",
                ok: true,
                text: format!("Jev answered in {}.", duration(a.latency_ms)),
                refused: false,
            },
            Some(Err(f)) => jev_failure(&f.kind, &f.detail),
            None => Verdict {
                lane: "jev",
                ok: false,
                text: "Jev has no key yet — paste one below.".into(),
                refused: false,
            },
        });
        verdicts.push(match router {
            Some(Ok(r)) => Verdict {
                lane: "router",
                ok: true,
                text: format!(
                    "The gateway answered as {} in {}.",
                    r.model,
                    duration(r.latency_ms)
                ),
                refused: false,
            },
            Some(Err(e)) => gateway_failure(&e, &model),
            None if keys.router_key.trim().is_empty() => Verdict {
                lane: "router",
                ok: false,
                text: "The gateway has no key yet — paste one above.".into(),
                refused: false,
            },
            None => Verdict {
                lane: "router",
                ok: false,
                text: "The gateway address isn't a web address — it should start with https://"
                    .into(),
                refused: false,
            },
        });
        CHECK.with(|c| {
            *c.borrow_mut() = Check {
                running: false,
                verdicts,
                asked: Some(asked),
                asked_with,
            }
        });
        if let Some(s) = weak.upgrade() {
            redraw(&s, "intelligence");
        }
    });
}

/// Whether `raw` can be a gateway's address.
pub(super) fn is_web_address(raw: &str) -> bool {
    url::Url::parse(raw.trim()).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty())
    })
}

/// What a key row says: where the key came from, and whether it works.
pub(super) fn key_detail(
    eff: &Effective,
    field: &str,
    plain: &str,
    ready: bool,
    refused: bool,
) -> (String, Tone) {
    if eff.sources.get(field).map(String::as_str) == Some("cloud") {
        return ("Provided by Copper Cloud".into(), Tone::Plain);
    }
    if refused {
        return (
            format!("{plain} Turned away by the last check."),
            Tone::Error,
        );
    }
    if ready {
        return (format!("{plain} Set."), Tone::Plain);
    }
    (plain.to_string(), Tone::Plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_refusals_read_as_refusals() {
        let v = gateway_failure("router 401: nope", "sonnet");
        assert!(v.refused);
        let v = gateway_failure("router 404: no such model", "big");
        assert!(v.text.contains("“big”"));
        let v = gateway_failure("error sending request: operation timed out", "x");
        assert!(v.text.contains("15 seconds"));
    }

    #[test]
    fn durations_read_like_the_mac() {
        assert_eq!(duration(212.4), "212 ms");
        assert_eq!(duration(1530.0), "1.5 s");
    }
}
