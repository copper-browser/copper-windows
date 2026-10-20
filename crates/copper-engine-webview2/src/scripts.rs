//! Document-created scripts: the message-handler shims, user scripts, user
//! style sheets and the ordered registration chains that carry them.
//!
//! WebView2 runs `AddScriptToExecuteOnDocumentCreated` scripts (page world)
//! and CDP `Page.addScriptToEvaluateOnNewDocument {worldName}` scripts
//! (isolated worlds) in registration order, in every frame, before any page
//! script. Each user script is its own registration, so it keeps global
//! scope and one script throwing does not stop the next. A world's chain
//! starts with its shim (`window.webkit.messageHandlers`), so the handler
//! names a document sees are fixed when it is created; changing the chain
//! re-registers everything after the first changed entry ([`Chain::plan`]).
//!
//! Everything here is platform-free and unit-tested; `page::scripts` runs
//! the plans against WebView2.

use copper_engine::{
    ConsoleLevel, ConsoleMessage, Frames, InjectionTime, JsonValue, StyleLevel, UserScript,
    UserStyleSheet,
};

use crate::util::{fnv1a64, js_string};

/// Page-world shim (`window.webkit.messageHandlers` → `chrome.webview`).
const PAGE_SHIM: &str = include_str!("js/shim.js");
/// Isolated-world shim (`window.webkit.messageHandlers` → CDP binding).
const WORLD_SHIM: &str = include_str!("js/world-shim.js");
/// `PageOptions::capture_console`'s script (page world, document start).
pub(crate) const CONSOLE_JS: &str = include_str!("js/console.js");
/// Script-dispatched input (`set_trusted_input(false)`): the same
/// `input.js` WebKitGTK uses, so both engines' script path behave alike.
pub(crate) const INPUT_JS: &str =
    include_str!("../../../shared/copper-linux/crates/copper-engine-webkitgtk/src/input.js");
/// The isolated world [`INPUT_JS`] runs in (its pointer state lives there).
pub(crate) const INPUT_WORLD: &str = "copper-input";

/// The internal handler `CONSOLE_JS` posts to; never reported as a
/// `ScriptMessage` and never registrable by callers.
pub(crate) const CONSOLE_HANDLER: &str = "__copperConsole";

/// Key of a web message the page-world shim reads as its new handler list.
pub(crate) const NAMES_KEY: &str = "__copperNames";

/// The page-world shim for handler `names` (sorted, deduplicated by caller).
pub(crate) fn page_shim(names: &[String]) -> String {
    PAGE_SHIM.replace("__COPPER_NAMES__", &names_json(names))
}

/// The shim for isolated world `world`, posting through its CDP binding.
pub(crate) fn world_shim(world: &str, names: &[String]) -> String {
    WORLD_SHIM
        .replace("__COPPER_BINDING__", &js_string(&binding_name(world)))
        .replace("__COPPER_NAMES__", &names_json(names))
}

/// JS that swaps the live handler list of isolated world `world`'s shim.
pub(crate) fn world_names_update(names: &[String]) -> String {
    format!(
        "globalThis.__copperSetNames && globalThis.__copperSetNames({})",
        names_json(names)
    )
}

/// The web message that swaps the live handler list of the page-world shim.
pub(crate) fn page_names_update(names: &[String]) -> String {
    serde_json::json!({ NAMES_KEY: names }).to_string()
}

fn names_json(names: &[String]) -> String {
    serde_json::to_string(names).expect("strings serialise")
}

/// The CDP binding an isolated world's shim posts through. One per world, so
/// `Runtime.bindingCalled.name` says which world a message came from (no
/// `Runtime.enable` needed to map execution contexts, which pages can detect).
pub(crate) fn binding_name(world: &str) -> String {
    format!("__copperPost{:016x}", fnv1a64(world.as_bytes()))
}

/// A decoded `window.webkit.messageHandlers.<name>.postMessage(body)`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Message {
    pub name: String,
    pub body: JsonValue,
}

/// Decode a page-world web message (`WebMessageAsJson`). `None` for messages
/// that are not the shim's (pages may use `chrome.webview` themselves).
pub(crate) fn decode_web_message(json: &str) -> Option<Message> {
    let v: JsonValue = serde_json::from_str(json).ok()?;
    decode_value(v)
}

/// Decode an isolated-world binding payload (the shim sends a JSON string).
pub(crate) fn decode_binding_payload(payload: &str) -> Option<Message> {
    decode_web_message(payload)
}

fn decode_value(v: JsonValue) -> Option<Message> {
    let JsonValue::Object(mut map) = v else {
        return None;
    };
    let name = match map.remove("__copper")? {
        JsonValue::String(s) if !s.is_empty() => s,
        _ => return None,
    };
    let body = map.remove("body").unwrap_or(JsonValue::Null);
    Some(Message { name, body })
}

/// A `__copperConsole` message body (`{level, message}`) as a console event.
pub(crate) fn console_message(body: &JsonValue) -> Option<ConsoleMessage> {
    let level = match body.get("level")?.as_str()? {
        "info" => ConsoleLevel::Info,
        "warn" => ConsoleLevel::Warn,
        "error" => ConsoleLevel::Error,
        "debug" => ConsoleLevel::Debug,
        _ => ConsoleLevel::Log,
    };
    let message = body.get("message")?.as_str()?.to_owned();
    Some(ConsoleMessage { level, message })
}

/// A WebKit match pattern (`https://*.example.com/*`, `*://host/path*`,
/// `<all_urls>`) as a JavaScript `RegExp` source matched against
/// `location.href`. `None` for a malformed pattern (it then matches nothing,
/// as WebKit ignores it).
pub(crate) fn match_pattern_regex(pattern: &str) -> Option<String> {
    let p = pattern.trim();
    if p == "<all_urls>" || p == "*" {
        return Some("^(https?|file|ftp|wss?)://.*".into());
    }
    let (scheme, rest) = p.split_once("://")?;
    let scheme_re = match scheme {
        "*" => "https?".to_owned(),
        s if !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')) =>
        {
            regex_literal(&s.to_ascii_lowercase())
        }
        _ => return None,
    };
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None if scheme == "file" => ("", rest),
        None => return None,
    };
    let host_re = if host == "*" {
        "[^/]*".to_owned()
    } else if let Some(base) = host.strip_prefix("*.") {
        if base.is_empty() || base.contains('*') {
            return None;
        }
        format!(
            "([^/]*\\.)?{}(:[0-9]+)?",
            regex_literal(&base.to_ascii_lowercase())
        )
    } else {
        if host.contains('*') {
            return None;
        }
        format!("{}(:[0-9]+)?", regex_literal(&host.to_ascii_lowercase()))
    };
    let path_re: String = path
        .split('*')
        .map(regex_literal)
        .collect::<Vec<_>>()
        .join(".*");
    Some(format!("^{scheme_re}://{host_re}{path_re}$"))
}

fn regex_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\^$.|?*+()[]{}/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// JS expression testing `location.href` against allow/block lists, or
/// `None` when both are empty (every URL).
fn url_condition(allow: &[String], block: &[String]) -> Option<String> {
    if allow.is_empty() && block.is_empty() {
        return None;
    }
    let list = |ps: &[String]| -> String {
        let res: Vec<String> = ps
            .iter()
            .filter_map(|p| match_pattern_regex(p))
            .map(|r| format!("new RegExp({}, 'i')", js_string(&r)))
            .collect();
        format!("[{}]", res.join(", "))
    };
    let mut cond = String::from("((u) => ");
    if !allow.is_empty() {
        cond.push_str(&format!("{}.some((r) => r.test(u))", list(allow)));
    } else {
        cond.push_str("true");
    }
    if !block.is_empty() {
        cond.push_str(&format!(" && !{}.some((r) => r.test(u))", list(block)));
    }
    cond.push_str(")(String(location.href))");
    Some(cond)
}

/// A user script as the source of one document-created registration.
///
/// A document-start, all-frames script for every URL runs exactly as given
/// (global scope, like a WebKit user script). Otherwise the source runs as
/// the body of a function called with `this === globalThis` once its
/// conditions hold, so its top-level `var`s and functions are local: such
/// scripts export by assigning to `window`/`globalThis` (every Copper script
/// does). Document end is `DOMContentLoaded` (or at once if it already
/// passed), WebKit's `WKUserScriptInjectionTimeAtDocumentEnd`.
pub(crate) fn user_script_source(script: &UserScript) -> String {
    let top_only = script.frames == Frames::TopOnly;
    let at_end = script.injection_time == InjectionTime::DocumentEnd;
    let cond = url_condition(&script.allow_list, &script.block_list);
    if !top_only && !at_end && cond.is_none() {
        return script.source.clone();
    }
    let mut checks = Vec::new();
    if top_only {
        checks.push("window === window.top".to_owned());
    }
    if let Some(c) = cond {
        checks.push(c);
    }
    let guard = if checks.is_empty() {
        String::new()
    } else {
        format!("if (!({})) return;\n", checks.join(" && "))
    };
    let call = "(function () {\n".to_owned() + &script.source + "\n}).call(globalThis);";
    let body = if at_end {
        format!(
            "const run = () => {{ {call} }};\n\
             if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', run, {{ once: true }});\n\
             else run();"
        )
    } else {
        call
    };
    format!("(() => {{\n{guard}{body}\n}})();\n")
}

/// Where a document keeps the user style sheets it has adopted, by id:
/// `document[Symbol.for(SHEETS_KEY)]`, a `Map` (one per world). On the
/// document, not the window: Chromium hands the initial `about:blank`'s
/// window on to the first real document, which needs its own sheets.
const SHEETS_KEY: &str = "copper.userSheets";

/// User style sheet `id` as a script: run when a document is created (the
/// registration), and in a document already open when the sheet is added
/// (WebKit applies user sheets to live documents too).
///
/// The sheet is a constructed `CSSStyleSheet` in `document.adoptedStyleSheets`
/// (exempt from the page's CSP, invisible to the DOM, after every sheet the
/// page has). Chromium has no user-origin sheets outside extensions, so
/// [`StyleLevel::User`] also cascades as an author sheet; to keep its
/// `!important` rules ahead of the page's like a user sheet's, the sheet is
/// re-appended last whenever the page replaces `adoptedStyleSheets` (checked
/// at `DOMContentLoaded` and `load`). The document's sheets are kept by id
/// so [`style_sheet_removal`] can take one off a live document, and so a
/// sheet is never adopted twice.
pub(crate) fn style_sheet_source(sheet: &UserStyleSheet, id: u64) -> String {
    let top_only = sheet.frames == Frames::TopOnly;
    let mut checks = Vec::new();
    if top_only {
        checks.push("window === window.top".to_owned());
    }
    if let Some(c) = url_condition(&sheet.allow_list, &sheet.block_list) {
        checks.push(c);
    }
    let guard = if checks.is_empty() {
        String::new()
    } else {
        format!("if (!({})) return;\n", checks.join(" && "))
    };
    // Both levels cascade as author sheets (see above); a user-level sheet
    // is re-appended last so it stays ahead of the page's own sheets.
    let keep_last = sheet.level == StyleLevel::User;
    format!(
        "(() => {{\n{guard}\
         const key = Symbol.for({key});\n\
         const sheets = document[key] || (document[key] = new Map());\n\
         if (sheets.has({id})) return;\n\
         const css = {css};\n\
         let sheet;\n\
         try {{ sheet = new CSSStyleSheet(); sheet.replaceSync(css); }} catch (_) {{ return; }}\n\
         sheets.set({id}, sheet);\n\
         const attach = () => {{\n\
           try {{\n\
             if (sheets.get({id}) !== sheet) return;\n\
             const list = document.adoptedStyleSheets;\n\
             const at = list.indexOf(sheet);\n\
             if (at >= 0 && (at === list.length - 1 || !{keep_last})) return;\n\
             document.adoptedStyleSheets = [...list.filter((s) => s !== sheet), sheet];\n\
           }} catch (_) {{}}\n\
         }};\n\
         attach();\n\
         document.addEventListener('DOMContentLoaded', attach, {{ once: true }});\n\
         addEventListener('load', attach, {{ once: true }});\n\
         }})();\n",
        key = js_string(SHEETS_KEY),
        css = js_string(&sheet.source),
    )
}

/// Take user style sheet `id` off the document the script runs in (it stays
/// registered for no new document: its registration is gone already).
pub(crate) fn style_sheet_removal(id: u64) -> String {
    format!(
        "(() => {{\n\
         const sheets = document[Symbol.for({key})];\n\
         const sheet = sheets && sheets.get({id});\n\
         if (!sheet) return;\n\
         sheets.delete({id});\n\
         try {{ document.adoptedStyleSheets = document.adoptedStyleSheets.filter((s) => s !== sheet); }} catch (_) {{}}\n\
         }})();\n",
        key = js_string(SHEETS_KEY),
    )
}

/// The page-world chain: shim, engine scripts, user scripts and sheets (in
/// the order they were added).
pub(crate) fn page_chain(
    handler_names: &[String],
    entries: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut chain = vec![page_shim(handler_names)];
    chain.extend(entries);
    chain
}

/// An isolated world's chain: its shim, then its scripts and sheets.
pub(crate) fn world_chain(
    world: &str,
    handler_names: &[String],
    entries: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut chain = vec![world_shim(world, handler_names)];
    chain.extend(entries);
    chain
}

/// One registered document-created script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Registered {
    pub source: String,
    /// The engine's id for it (`AddScriptToExecuteOnDocumentCreated`'s id or
    /// CDP's `identifier`); `None` if registering failed.
    pub id: Option<String>,
}

/// The registered scripts of one world, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Chain {
    pub registered: Vec<Registered>,
}

/// What to do to turn a chain into the desired one: unregister `remove`
/// (ids, last first), then register `add` in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub keep: usize,
    pub remove: Vec<String>,
    pub add: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.remove.is_empty() && self.add.is_empty()
    }
}

impl Chain {
    /// Keep the longest common prefix; everything after it is re-registered
    /// so the order stays the desired order.
    pub fn plan(&self, desired: &[String]) -> Plan {
        let keep = self
            .registered
            .iter()
            .zip(desired)
            .take_while(|(r, d)| r.id.is_some() && r.source == **d)
            .count();
        Plan {
            keep,
            remove: self.registered[keep..]
                .iter()
                .rev()
                .filter_map(|r| r.id.clone())
                .collect(),
            add: desired[keep..].to_vec(),
        }
    }

    /// Record that `plan` ran: the first `keep` stay, `added` follow.
    pub fn applied(&mut self, keep: usize, added: Vec<Registered>) {
        self.registered.truncate(keep);
        self.registered.extend(added);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_bodies() {
        let m = console_message(&serde_json::json!({"level": "warn", "message": "hi"})).unwrap();
        assert_eq!((m.level, m.message.as_str()), (ConsoleLevel::Warn, "hi"));
        assert_eq!(
            console_message(&serde_json::json!({"level": "x", "message": "m"}))
                .unwrap()
                .level,
            ConsoleLevel::Log
        );
        assert!(console_message(&serde_json::json!({"level": "log"})).is_none());
    }

    #[test]
    fn shims_embed_names() {
        let names = vec!["a".to_string(), "b\"c".to_string()];
        let s = page_shim(&names);
        assert!(s.contains(r#"new Set(["a","b\"c"])"#), "{s}");
        assert!(!s.contains("__COPPER_NAMES__"));
        let w = world_shim("CopperCanvasFrames", &names);
        assert!(w.contains(&js_string(&binding_name("CopperCanvasFrames"))));
        assert!(!w.contains("__COPPER_BINDING__"));
        assert_ne!(binding_name("a"), binding_name("b"));
        assert_eq!(binding_name("a"), binding_name("a"));
        assert_eq!(
            page_names_update(&names),
            r#"{"__copperNames":["a","b\"c"]}"#
        );
    }

    #[test]
    fn messages_decode() {
        let m = decode_web_message(r#"{"__copper":"jev","body":{"x":1}}"#).unwrap();
        assert_eq!(m.name, "jev");
        assert_eq!(m.body, serde_json::json!({"x": 1}));
        let m = decode_web_message(r#"{"__copper":"n"}"#).unwrap();
        assert_eq!(m.body, JsonValue::Null);
        assert!(decode_web_message(r#"{"__copper":""}"#).is_none());
        assert!(decode_web_message(r#"{"__copper":3}"#).is_none());
        assert!(decode_web_message(r#""hello""#).is_none());
        assert!(decode_web_message(r#"{"other":1}"#).is_none());
        assert!(decode_web_message("not json").is_none());
        let m = decode_binding_payload(r#"{"__copper":"w","body":[1,2]}"#).unwrap();
        assert_eq!((m.name.as_str(), m.body), ("w", serde_json::json!([1, 2])));
    }

    fn matches(pattern: &str, url: &str) -> bool {
        // Mirror of the JS test (`new RegExp(src, 'i')`), close enough for
        // these patterns: lowercase both sides.
        let src = match_pattern_regex(pattern).unwrap();
        simple_regex_match(&src.to_ascii_lowercase(), &url.to_ascii_lowercase())
    }

    /// A tiny backtracking matcher for the regex subset `match_pattern_regex`
    /// emits: literals, `\x`, `.`, `[^/]`, `[0-9]`, `*`, `+`, `?`, groups with
    /// `|`, anchors.
    fn simple_regex_match(re: &str, s: &str) -> bool {
        #[derive(Debug, Clone)]
        enum Node {
            Lit(char),
            Any,
            NotSlash,
            Digit,
            Group(Vec<Vec<(Node, char)>>),
        }
        fn parse_seq(chars: &[char], i: &mut usize) -> Vec<Vec<(Node, char)>> {
            let mut alts = vec![Vec::new()];
            while *i < chars.len() {
                let c = chars[*i];
                let node = match c {
                    ')' => break,
                    '|' => {
                        *i += 1;
                        alts.push(Vec::new());
                        continue;
                    }
                    '^' | '$' => {
                        *i += 1;
                        continue;
                    }
                    '\\' => {
                        *i += 2;
                        Node::Lit(chars[*i - 1])
                    }
                    '.' => {
                        *i += 1;
                        Node::Any
                    }
                    '[' => {
                        let end = chars[*i..].iter().position(|&c| c == ']').unwrap() + *i;
                        let class: String = chars[*i + 1..end].iter().collect();
                        *i = end + 1;
                        match class.as_str() {
                            "^/" => Node::NotSlash,
                            "0-9" => Node::Digit,
                            "a-z" => Node::NotSlash,
                            "a-z0-9+.-" => Node::NotSlash,
                            other => panic!("class {other}"),
                        }
                    }
                    '(' => {
                        *i += 1;
                        let g = parse_seq(chars, i);
                        *i += 1;
                        Node::Group(g)
                    }
                    c => {
                        *i += 1;
                        Node::Lit(c)
                    }
                };
                let q = match chars.get(*i) {
                    Some(&q @ ('*' | '+' | '?')) => {
                        *i += 1;
                        q
                    }
                    _ => ' ',
                };
                alts.last_mut().unwrap().push((node, q));
            }
            alts
        }
        fn m_one(n: &Node, s: &[char], k: &mut dyn FnMut(&[char]) -> bool) -> bool {
            match n {
                Node::Lit(c) => s.first() == Some(c) && k(&s[1..]),
                Node::Any => !s.is_empty() && k(&s[1..]),
                Node::NotSlash => s.first().is_some_and(|c| *c != '/') && k(&s[1..]),
                Node::Digit => s.first().is_some_and(char::is_ascii_digit) && k(&s[1..]),
                Node::Group(alts) => alts.iter().any(|a| m_seq(a, s, k)),
            }
        }
        fn m_seq(seq: &[(Node, char)], s: &[char], k: &mut dyn FnMut(&[char]) -> bool) -> bool {
            let Some(((n, q), rest)) = seq.split_first() else {
                return k(s);
            };
            match q {
                ' ' => m_one(n, s, &mut |t| m_seq(rest, t, k)),
                '?' => m_one(n, s, &mut |t| m_seq(rest, t, k)) || m_seq(rest, s, k),
                '*' | '+' => {
                    fn star(
                        n: &Node,
                        rest: &[(Node, char)],
                        s: &[char],
                        min: usize,
                        k: &mut dyn FnMut(&[char]) -> bool,
                    ) -> bool {
                        (min == 0 && m_seq(rest, s, k))
                            || m_one(n, s, &mut |t| {
                                t.len() < s.len() && star(n, rest, t, min.saturating_sub(1), k)
                            })
                    }
                    star(n, rest, s, usize::from(*q == '+'), k)
                }
                _ => unreachable!(),
            }
        }
        let chars: Vec<char> = re.chars().collect();
        let mut i = 0;
        let alts = parse_seq(&chars, &mut i);
        let s: Vec<char> = s.chars().collect();
        alts.iter().any(|a| m_seq(a, &s, &mut |t| t.is_empty()))
    }

    #[test]
    fn match_patterns() {
        assert!(matches("https://*.example.com/*", "https://example.com/"));
        assert!(matches(
            "https://*.example.com/*",
            "https://a.b.example.com/x?y"
        ));
        assert!(matches(
            "https://*.example.com/*",
            "https://example.com:8443/x"
        ));
        assert!(!matches(
            "https://*.example.com/*",
            "https://badexample.com/"
        ));
        assert!(!matches("https://*.example.com/*", "http://example.com/"));
        assert!(matches("*://host/path*", "http://host/path/more"));
        assert!(matches("*://host/path*", "https://host/path"));
        assert!(!matches("*://host/path*", "https://host/other"));
        assert!(matches("*://*/*", "https://anything.org/"));
        assert!(matches("<all_urls>", "https://x.org/"));
        assert!(!matches("<all_urls>", "about:blank"));
        assert!(matches("file:///home/*", "file:///home/me/a.html"));
        assert!(match_pattern_regex("nonsense").is_none());
        assert!(match_pattern_regex("https://a*b.com/").is_none());
        assert!(match_pattern_regex("https://*./x").is_none());
    }

    #[test]
    fn user_script_wrapping() {
        let plain = UserScript::new("var x = 1;").at(InjectionTime::DocumentStart);
        assert_eq!(user_script_source(&plain), "var x = 1;");
        let end = UserScript::new("go()");
        let s = user_script_source(&end);
        assert!(s.contains("DOMContentLoaded"));
        assert!(s.contains("(function () {\ngo()\n}).call(globalThis);"));
        assert!(!s.contains("window.top"));
        let top = UserScript::new("go()")
            .at(InjectionTime::DocumentStart)
            .frames(Frames::TopOnly);
        let s = user_script_source(&top);
        assert!(s.contains("if (!(window === window.top)) return;"));
        assert!(!s.contains("DOMContentLoaded"));
        let mut listed = UserScript::new("go()").at(InjectionTime::DocumentStart);
        listed.allow_list = vec!["https://*.a.com/*".into()];
        listed.block_list = vec!["https://b.a.com/*".into()];
        let s = user_script_source(&listed);
        assert!(s.contains(".some((r) => r.test(u)) && !["), "{s}");
        assert!(s.contains("String(location.href)"));
    }

    #[test]
    fn style_sheets() {
        let sheet = UserStyleSheet::new("a { color: red }\n`${x}`");
        let s = style_sheet_source(&sheet, 7);
        assert!(
            s.contains(r#"const css = "a { color: red }\n`${x}`";"#),
            "{s}"
        );
        assert!(s.contains("adoptedStyleSheets"));
        assert!(!s.contains("window.top"));
        let top = UserStyleSheet::new("b{}").frames(Frames::TopOnly);
        assert!(style_sheet_source(&top, 8).contains("window === window.top"));
        assert!(s.contains("sheets.has(7)") && s.contains("sheets.set(7, sheet)"));
        assert!(s.contains(r#"Symbol.for("copper.userSheets")"#), "{s}");
        let off = style_sheet_removal(7);
        assert!(off.contains("sheets.get(7)") && off.contains("sheets.delete(7)"));
        assert!(off.contains(r#"Symbol.for("copper.userSheets")"#));
    }

    fn chain_of(sources: &[&str]) -> Chain {
        Chain {
            registered: sources
                .iter()
                .enumerate()
                .map(|(i, s)| Registered {
                    source: s.to_string(),
                    id: Some(format!("id{i}")),
                })
                .collect(),
        }
    }

    fn owned(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn chain_plans() {
        let c = chain_of(&["shim", "a", "b"]);
        // Appending keeps everything.
        let p = c.plan(&owned(&["shim", "a", "b", "c"]));
        assert_eq!(p.keep, 3);
        assert!(p.remove.is_empty());
        assert_eq!(p.add, owned(&["c"]));
        // Removing the middle re-registers what follows it.
        let p = c.plan(&owned(&["shim", "b"]));
        assert_eq!(p.keep, 1);
        assert_eq!(p.remove, owned(&["id2", "id1"]));
        assert_eq!(p.add, owned(&["b"]));
        // A new shim (handler names changed) redoes the chain.
        let p = c.plan(&owned(&["shim2", "a", "b"]));
        assert_eq!(p.keep, 0);
        assert_eq!(p.remove.len(), 3);
        assert_eq!(p.add.len(), 3);
        // Same chain: nothing to do.
        assert!(c.plan(&owned(&["shim", "a", "b"])).is_empty());
        // A failed registration is retried.
        let mut f = chain_of(&["shim", "a"]);
        f.registered[1].id = None;
        let p = f.plan(&owned(&["shim", "a"]));
        assert_eq!(p.keep, 1);
        assert!(p.remove.is_empty());
        assert_eq!(p.add, owned(&["a"]));
        let mut c2 = c.clone();
        c2.applied(
            1,
            vec![Registered {
                source: "z".into(),
                id: Some("n".into()),
            }],
        );
        let ids: Vec<String> = c2.registered.iter().filter_map(|r| r.id.clone()).collect();
        assert_eq!(ids, owned(&["id0", "n"]));
    }
}
