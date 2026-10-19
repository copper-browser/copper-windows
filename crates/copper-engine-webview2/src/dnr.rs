//! WebKit content-blocker JSON → Chromium Manifest V3 extension
//! (declarativeNetRequest rules + hide-element style sheets).
//!
//! WebView2 has no content-blocker API. Blocking through
//! `AddWebResourceRequestedFilter` costs a UI-thread hop per request and any
//! such filter can wedge service workers (WebView2Feedback #5764, Gmail), so
//! Copper compiles each list into an unpacked MV3 extension instead: network
//! rules run inside the browser process with no host round trip.
//!
//! # Mapping
//!
//! | WebKit | DNR |
//! |---|---|
//! | `url-filter` | `regexFilter`; the common `^https?://([^/]+\.)?host\.tld` shape becomes `requestDomains` (cheaper, and it adds the domain boundary the list means) |
//! | `url-filter-is-case-sensitive` | `isUrlFilterCaseSensitive` (WebKit's default, `false`, is set explicitly) |
//! | `load-type` first/third-party | `domainType` |
//! | `resource-type` | `resourceTypes` (absent: every type but `main_frame`, DNR's default) |
//! | `if-domain` / `unless-domain` | `initiatorDomains` / `excludedInitiatorDomains` |
//! | `block` | `block` |
//! | `make-https` | `upgradeScheme` |
//! | `block-cookies` | `modifyHeaders` removing `Cookie` / `Set-Cookie` |
//! | `ignore-previous-rules` | `allow` one priority above every earlier rule; with `if-domain` and a match-all filter, `allowAllRequests` on those top-level sites |
//! | `css-display-none` | a content-script style sheet (all frames, document start) |
//!
//! Rules DNR cannot express (`if-top-url`, `unless-top-url`, `if-frame-url`,
//! `redirect`, `notify`, …) are skipped and reported in [`Compiled::skipped`]
//! rather than approximated in a way that could block too much.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Value, json};

/// The extension's display name starts with this, so stale Copper filters can
/// be told apart from any other extension a profile has.
pub const EXTENSION_NAME_PREFIX: &str = "Copper filter: ";

/// Every DNR resource type except `main_frame` (DNR's own default).
const SUBRESOURCE_TYPES: [&str; 11] = [
    "sub_frame",
    "stylesheet",
    "script",
    "image",
    "font",
    "object",
    "xmlhttprequest",
    "ping",
    "media",
    "websocket",
    "other",
];

/// Why a list could not be compiled at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompileError {
    #[error("rule list is not valid JSON: {0}")]
    Json(String),
    #[error("rule {index}: {reason}")]
    Rule { index: usize, reason: String },
}

/// A compiled list: the files of one MV3 extension.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    /// `rules.json`: the static DNR ruleset.
    pub rules: Vec<Value>,
    /// Hide-element style sheets, each with the sites it applies to.
    pub styles: Vec<HideSheet>,
    /// Rules left out, with the reason (for logs).
    pub skipped: Vec<String>,
}

/// One content-script style sheet: `selectors { display: none !important }`
/// on the pages `matches` covers, except `exclude`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HideSheet {
    pub css: String,
    pub matches: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Deserialize)]
struct RawRule {
    trigger: RawTrigger,
    action: RawAction,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawTrigger {
    url_filter: String,
    #[serde(default)]
    url_filter_is_case_sensitive: bool,
    #[serde(default)]
    load_type: Option<Vec<String>>,
    #[serde(default)]
    resource_type: Option<Vec<String>>,
    #[serde(default)]
    if_domain: Option<Vec<String>>,
    #[serde(default)]
    unless_domain: Option<Vec<String>>,
    #[serde(default)]
    if_top_url: Option<Value>,
    #[serde(default)]
    unless_top_url: Option<Value>,
    #[serde(default)]
    if_frame_url: Option<Value>,
    #[serde(default)]
    unless_frame_url: Option<Value>,
}

#[derive(Deserialize)]
struct RawAction {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    selector: Option<String>,
}

/// A WebKit domain list entry: `*example.com` covers subdomains, a bare
/// `example.com` only that host.
struct DomainEntry {
    host: String,
    subdomains: bool,
}

fn domain_entries(list: &[String]) -> Vec<DomainEntry> {
    list.iter()
        .filter_map(|d| {
            let (subdomains, host) = match d.strip_prefix('*') {
                Some(rest) => (true, rest),
                None => (false, d.as_str()),
            };
            let host = host.trim_matches('.').to_ascii_lowercase();
            (!host.is_empty()).then_some(DomainEntry { host, subdomains })
        })
        .collect()
}

/// Content-script match patterns for a domain list.
fn match_patterns(entries: &[DomainEntry]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for e in entries {
        out.insert(format!("*://{}/*", e.host));
        if e.subdomains {
            out.insert(format!("*://*.{}/*", e.host));
        }
    }
    out.into_iter().collect()
}

/// `^https?://([^/]+\.)?host\.tld` (WebKit escapes the slashes or not) →
/// `host.tld`. Anything else is not the plain domain shape.
fn domain_shape(filter: &str) -> Option<String> {
    let f = filter.replace("\\/", "/");
    let rest = f.strip_prefix("^https?://([^/]+\\.)?")?;
    let mut host = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'.') => {
                chars.next();
                host.push('.');
            }
            c if c.is_ascii_alphanumeric() || c == '-' => host.push(c.to_ascii_lowercase()),
            _ => return None,
        }
    }
    let ok = host.contains('.') && !host.starts_with('.') && !host.ends_with('.');
    ok.then_some(host)
}

/// A host as a literal inside an RE2 pattern.
fn regex_escape(host: &str) -> String {
    let mut out = String::with_capacity(host.len() + 4);
    for c in host.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn is_match_all(filter: &str) -> bool {
    matches!(filter, ".*" | "^.*" | "^.*$" | ".*$" | "")
}

fn resource_types(types: &[String], skipped: &mut Vec<String>) -> Vec<&'static str> {
    let mut out = BTreeSet::new();
    for t in types {
        match t.as_str() {
            "document" => {
                out.insert("main_frame");
                out.insert("sub_frame");
            }
            "image" => {
                out.insert("image");
            }
            "style-sheet" => {
                out.insert("stylesheet");
            }
            "script" => {
                out.insert("script");
            }
            "font" => {
                out.insert("font");
            }
            "raw" => {
                out.insert("xmlhttprequest");
                out.insert("websocket");
                out.insert("other");
            }
            "fetch" => {
                out.insert("xmlhttprequest");
            }
            "websocket" => {
                out.insert("websocket");
            }
            "svg-document" => {
                out.insert("sub_frame");
                out.insert("image");
            }
            "media" => {
                out.insert("media");
            }
            "ping" => {
                out.insert("ping");
            }
            "other" => {
                out.insert("other");
                out.insert("object");
            }
            other => skipped.push(format!("resource-type {other:?} has no DNR equivalent")),
        }
    }
    out.into_iter().collect()
}

/// Compile a WebKit content-blocker list. Malformed JSON (or a rule without
/// a trigger/action) is an error; rules DNR cannot express are skipped.
pub fn compile(rules_json: &str) -> Result<Compiled, CompileError> {
    let raw: Vec<Value> =
        serde_json::from_str(rules_json).map_err(|e| CompileError::Json(e.to_string()))?;
    let mut out = Compiled {
        rules: Vec::new(),
        styles: Vec::new(),
        skipped: Vec::new(),
    };
    // Every `ignore-previous-rules` starts a new generation; a rule's
    // priority grows with its generation so later rules beat earlier ones,
    // and the exception itself sits between the two.
    let mut generation: u32 = 0;
    // Sites exempted by `ignore-previous-rules` + `if-domain`: earlier hide
    // sheets must not apply there either.
    let mut css_exempt: Vec<(usize, Vec<String>)> = Vec::new();
    for (index, value) in raw.into_iter().enumerate() {
        let rule: RawRule = serde_json::from_value(value).map_err(|e| CompileError::Rule {
            index,
            reason: e.to_string(),
        })?;
        let t = &rule.trigger;
        let unsupported = [
            ("if-top-url", t.if_top_url.is_some()),
            ("unless-top-url", t.unless_top_url.is_some()),
            ("if-frame-url", t.if_frame_url.is_some()),
            ("unless-frame-url", t.unless_frame_url.is_some()),
        ];
        if let Some((name, _)) = unsupported.iter().find(|(_, on)| *on) {
            out.skipped.push(format!(
                "rule {index}: trigger {name} has no DNR equivalent"
            ));
            continue;
        }
        if t.if_domain.is_some() && t.unless_domain.is_some() {
            out.skipped.push(format!(
                "rule {index}: if-domain and unless-domain together are invalid"
            ));
            continue;
        }
        let if_domains = t.if_domain.as_deref().map(domain_entries);
        let unless_domains = t.unless_domain.as_deref().map(domain_entries);

        if rule.action.kind == "css-display-none" {
            let Some(selector) = rule.action.selector.as_deref().map(str::trim) else {
                return Err(CompileError::Rule {
                    index,
                    reason: "css-display-none without a selector".into(),
                });
            };
            if !is_match_all(&t.url_filter) {
                out.skipped.push(format!(
                    "rule {index}: css-display-none limited by url-filter {:?} is applied on every URL of its sites",
                    t.url_filter
                ));
            }
            let matches = match &if_domains {
                Some(d) => match_patterns(d),
                None => vec!["<all_urls>".into()],
            };
            let exclude = unless_domains
                .as_deref()
                .map(match_patterns)
                .unwrap_or_default();
            out.styles.push(HideSheet {
                css: format!("{selector} {{ display: none !important; }}\n"),
                matches,
                exclude,
            });
            continue;
        }

        let mut condition = serde_json::Map::new();
        let domain = domain_shape(&t.url_filter);
        match &domain {
            Some(host) => {
                condition.insert("requestDomains".into(), json!([host]));
            }
            None if is_match_all(&t.url_filter) => {}
            None => {
                condition.insert("regexFilter".into(), json!(t.url_filter));
                condition.insert(
                    "isUrlFilterCaseSensitive".into(),
                    json!(t.url_filter_is_case_sensitive),
                );
            }
        }
        if let Some(load) = &t.load_type {
            match (
                load.iter().any(|l| l == "first-party"),
                load.iter().any(|l| l == "third-party"),
            ) {
                (true, false) => {
                    condition.insert("domainType".into(), json!("firstParty"));
                }
                (false, true) => {
                    condition.insert("domainType".into(), json!("thirdParty"));
                }
                _ => {}
            }
        }
        let types = match &t.resource_type {
            Some(types) => resource_types(types, &mut out.skipped),
            None => SUBRESOURCE_TYPES.to_vec(),
        };
        if types.is_empty() {
            out.skipped
                .push(format!("rule {index}: no resource type DNR can match"));
            continue;
        }
        if let Some(d) = &if_domains {
            let hosts: BTreeSet<&str> = d.iter().map(|e| e.host.as_str()).collect();
            condition.insert("initiatorDomains".into(), json!(hosts));
        }
        if let Some(d) = &unless_domains {
            let hosts: BTreeSet<&str> = d.iter().map(|e| e.host.as_str()).collect();
            condition.insert("excludedInitiatorDomains".into(), json!(hosts));
        }

        let base = 2 * generation + 1;
        let (action, priority) = match rule.action.kind.as_str() {
            "block" => (json!({"type": "block"}), base),
            "make-https" => (json!({"type": "upgradeScheme"}), base),
            "block-cookies" => (
                json!({
                    "type": "modifyHeaders",
                    "requestHeaders": [{"header": "cookie", "operation": "remove"}],
                    "responseHeaders": [{"header": "set-cookie", "operation": "remove"}],
                }),
                base,
            ),
            "ignore-previous-rules" => {
                generation += 1;
                // Whole sites exempted (Shield's per-site pause): allow every
                // request made in tabs whose top document is on those sites.
                // Exact hosts match by regex (`requestDomains` would take
                // their subdomains too); `*host` entries by domain.
                if let (Some(d), true) = (&if_domains, is_match_all(&t.url_filter)) {
                    css_exempt.push((out.styles.len(), match_patterns(d)));
                    let exact: BTreeSet<String> = d
                        .iter()
                        .filter(|e| !e.subdomains)
                        .map(|e| regex_escape(&e.host))
                        .collect();
                    let wide: BTreeSet<&str> = d
                        .iter()
                        .filter(|e| e.subdomains)
                        .map(|e| e.host.as_str())
                        .collect();
                    let mut conditions = Vec::new();
                    if !exact.is_empty() {
                        let alts: Vec<String> = exact.into_iter().collect();
                        conditions.push(json!({
                            "regexFilter": format!("^[a-z][a-z0-9+.-]*://({})(:[0-9]+)?(/|$)", alts.join("|")),
                            "resourceTypes": ["main_frame"],
                        }));
                    }
                    if !wide.is_empty() {
                        conditions.push(json!({
                            "requestDomains": wide,
                            "resourceTypes": ["main_frame"],
                        }));
                    }
                    for condition in conditions {
                        let id = out.rules.len() + 1;
                        out.rules.push(json!({
                            "id": id,
                            "priority": base + 1,
                            "action": {"type": "allowAllRequests"},
                            "condition": condition,
                        }));
                    }
                    continue;
                }
                (json!({"type": "allow"}), base + 1)
            }
            other => {
                out.skipped.push(format!(
                    "rule {index}: action {other:?} has no DNR equivalent"
                ));
                continue;
            }
        };
        let mut types = types;
        if rule.action.kind == "ignore-previous-rules" && t.resource_type.is_none() {
            types.insert(0, "main_frame");
        }
        condition.insert("resourceTypes".into(), json!(types));
        let id = out.rules.len() + 1;
        out.rules.push(json!({
            "id": id,
            "priority": priority,
            "action": action,
            "condition": Value::Object(condition),
        }));
    }
    // Sheets that came before a site exemption do not apply on those sites.
    for (before, patterns) in css_exempt {
        for sheet in out.styles.iter_mut().take(before) {
            for p in &patterns {
                if !sheet.exclude.contains(p) {
                    sheet.exclude.push(p.clone());
                }
            }
        }
    }
    Ok(out)
}

impl Compiled {
    /// Whether any rule rewrites headers (needs host permissions).
    fn modifies_headers(&self) -> bool {
        self.rules
            .iter()
            .any(|r| r["action"]["type"] == "modifyHeaders")
    }

    /// `manifest.json` for the extension of filter `id`. `version_hash`
    /// changes with the rules so a reload is never mistaken for the same build.
    pub fn manifest(&self, id: &str, version_hash: u64) -> Value {
        let content_scripts: Vec<Value> = self
            .styles
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut cs = json!({
                    "matches": s.matches,
                    "css": [format!("hide-{i}.css")],
                    "all_frames": true,
                    "match_about_blank": true,
                    "run_at": "document_start",
                });
                if !s.exclude.is_empty() {
                    cs["exclude_matches"] = json!(s.exclude);
                }
                cs
            })
            .collect();
        // Chromium wants 1-4 dot-separated integers, each below 65536.
        let v = version_hash;
        let version = format!(
            "1.{}.{}.{}",
            (v >> 32) & 0xffff,
            (v >> 16) & 0xffff,
            v & 0xffff
        );
        let mut m = json!({
            "manifest_version": 3,
            "name": format!("{EXTENSION_NAME_PREFIX}{id}"),
            "description": "Content blocking rules compiled by Copper.",
            "version": version,
            "permissions": ["declarativeNetRequest"],
            "declarative_net_request": {
                "rule_resources": [{"id": "rules", "enabled": true, "path": "rules.json"}]
            },
        });
        if !content_scripts.is_empty() {
            m["content_scripts"] = Value::Array(content_scripts);
        }
        if self.modifies_headers() {
            m["host_permissions"] = json!(["<all_urls>"]);
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN: &str = include_str!(
        "../../../shared/copper-linux/crates/copper-shield/tests/golden/mac-rules.json"
    );

    #[test]
    fn golden_mac_list() {
        let c = compile(GOLDEN).unwrap();
        assert!(c.skipped.is_empty(), "{:?}", c.skipped);
        assert_eq!(c.rules.len(), 44);
        assert_eq!(c.styles.len(), 1);
        let first = &c.rules[0];
        assert_eq!(first["id"], 1);
        assert_eq!(first["priority"], 1);
        assert_eq!(first["action"]["type"], "block");
        assert_eq!(
            first["condition"]["requestDomains"],
            json!(["doubleclick.net"])
        );
        assert_eq!(first["condition"]["domainType"], "thirdParty");
        assert!(first["condition"].get("regexFilter").is_none());
        let types = first["condition"]["resourceTypes"].as_array().unwrap();
        assert!(!types.contains(&json!("main_frame")));
        // Every UNWANTED domain is there, as a domain rule.
        for d in copper_shield::UNWANTED {
            assert!(
                c.rules
                    .iter()
                    .any(|r| r["condition"]["requestDomains"] == json!([d])),
                "{d}"
            );
        }
        let sheet = &c.styles[0];
        assert_eq!(sheet.matches, vec!["<all_urls>".to_string()]);
        assert!(sheet.css.contains(".adsbygoogle"));
        assert!(sheet.css.ends_with("{ display: none !important; }\n"));
        // Ids are unique and start at 1.
        let ids: BTreeSet<u64> = c.rules.iter().map(|r| r["id"].as_u64().unwrap()).collect();
        assert_eq!(ids.len(), c.rules.len());
        assert_eq!(ids.first(), Some(&1));
    }

    #[test]
    fn shield_lists_compile() {
        let standard = copper_shield::List::standard();
        assert_eq!(compile(&standard.json).unwrap(), compile(GOLDEN).unwrap());
        let exempt = copper_shield::List::exempting(["news.example.com", "Shop.Example.org."]);
        let c = compile(&exempt.json).unwrap();
        assert_eq!(c.rules.len(), 45);
        let last = c.rules.last().unwrap();
        assert_eq!(last["action"]["type"], "allowAllRequests");
        assert_eq!(last["priority"], 2);
        assert_eq!(
            last["condition"]["regexFilter"],
            r"^[a-z][a-z0-9+.-]*://(news\.example\.com|shop\.example\.org)(:[0-9]+)?(/|$)"
        );
        assert_eq!(last["condition"]["resourceTypes"], json!(["main_frame"]));
        // The hide sheet stays off on the exempted sites.
        let ex = &c.styles[0].exclude;
        assert!(ex.contains(&"*://news.example.com/*".to_string()), "{ex:?}");
        assert!(ex.contains(&"*://shop.example.org/*".to_string()), "{ex:?}");
    }

    #[test]
    fn generations_and_actions() {
        let json = r#"[
          {"trigger":{"url-filter":"ads[0-9]+\\.js","resource-type":["script"],"url-filter-is-case-sensitive":true},"action":{"type":"block"}},
          {"trigger":{"url-filter":".*","if-domain":["*good.com"]},"action":{"type":"ignore-previous-rules"}},
          {"trigger":{"url-filter":"^http://","unless-domain":["plain.org"]},"action":{"type":"make-https"}},
          {"trigger":{"url-filter":"track","load-type":["first-party"]},"action":{"type":"block-cookies"}},
          {"trigger":{"url-filter":"x","if-top-url":["a"]},"action":{"type":"block"}},
          {"trigger":{"url-filter":"x"},"action":{"type":"notify","notification":"n"}},
          {"trigger":{"url-filter":"tracker"},"action":{"type":"ignore-previous-rules"}},
          {"trigger":{"url-filter":".*","if-domain":["*shop.com","exact.net"]},"action":{"type":"css-display-none","selector":".promo"}}
        ]"#;
        let c = compile(json).unwrap();
        assert_eq!(c.skipped.len(), 2, "{:?}", c.skipped);
        let r = &c.rules;
        assert_eq!(r[0]["condition"]["regexFilter"], "ads[0-9]+\\.js");
        assert_eq!(r[0]["condition"]["isUrlFilterCaseSensitive"], true);
        assert_eq!(r[0]["condition"]["resourceTypes"], json!(["script"]));
        assert_eq!(r[0]["priority"], 1);
        assert_eq!(r[1]["action"]["type"], "allowAllRequests");
        assert_eq!(r[1]["priority"], 2);
        assert_eq!(r[1]["condition"]["requestDomains"], json!(["good.com"]));
        assert_eq!(r[2]["action"]["type"], "upgradeScheme");
        assert_eq!(r[2]["priority"], 3);
        assert_eq!(
            r[2]["condition"]["excludedInitiatorDomains"],
            json!(["plain.org"])
        );
        assert_eq!(r[3]["action"]["type"], "modifyHeaders");
        assert_eq!(r[3]["condition"]["domainType"], "firstParty");
        assert_eq!(r[4]["action"]["type"], "allow");
        assert_eq!(r[4]["priority"], 4);
        let types = r[4]["condition"]["resourceTypes"].as_array().unwrap();
        assert_eq!(types[0], "main_frame");
        let sheet = &c.styles[0];
        assert_eq!(
            sheet.matches,
            vec![
                "*://*.shop.com/*".to_string(),
                "*://exact.net/*".to_string(),
                "*://shop.com/*".to_string()
            ]
        );
        let m = c.manifest("t", 0x1234_5678_9abc);
        assert_eq!(m["host_permissions"], json!(["<all_urls>"]));
        assert_eq!(m["name"], "Copper filter: t");
        assert_eq!(m["version"], "1.4660.22136.39612");
        assert_eq!(m["content_scripts"][0]["css"], json!(["hide-0.css"]));
    }

    #[test]
    fn domain_shapes() {
        assert_eq!(
            domain_shape(r"^https?:\/\/([^\/]+\.)?ads\.example\.com").as_deref(),
            Some("ads.example.com")
        );
        assert_eq!(
            domain_shape(r"^https?://([^/]+\.)?x-y\.io").as_deref(),
            Some("x-y.io")
        );
        assert_eq!(domain_shape(r"^https?://([^/]+\.)?example\.com/ads"), None);
        assert_eq!(domain_shape(r"^https?://([^/]+\.)?localhost"), None);
        assert_eq!(domain_shape("example.com"), None);
    }

    #[test]
    fn malformed_lists_fail() {
        assert!(matches!(compile("{"), Err(CompileError::Json(_))));
        assert!(matches!(compile("{}"), Err(CompileError::Json(_))));
        assert!(matches!(
            compile(r#"[{"trigger":{},"action":{"type":"block"}}]"#),
            Err(CompileError::Rule { index: 0, .. })
        ));
        assert!(matches!(
            compile(r#"[{"trigger":{"url-filter":".*"},"action":{"type":"css-display-none"}}]"#),
            Err(CompileError::Rule { index: 0, .. })
        ));
        assert_eq!(compile("[]").unwrap().rules.len(), 0);
    }
}
