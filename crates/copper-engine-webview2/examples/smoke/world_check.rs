//! `--world-check`: a message handler in an isolated world hears every
//! document, not only the one there when it was added. V8 puts a CDP
//! binding into contexts made later only while the Runtime domain is on
//! (the engine never turns it on), so each new document's world must get
//! its binding from the engine. Both a script at document start (before
//! the engine can have bound the new document: the shim holds it) and one
//! at document end must arrive, on a same-site reload, a same-process
//! navigation and a cross-site one.

use std::future::poll_fn;
use std::time::{Duration, Instant};

use copper_engine::{Frames, InjectionTime, LoadState, PageEvent, PageView, UserScript, World};

use crate::checks::pass;
use crate::run::{Ctx, within};

const WORLD: &str = "smoke-world";
const HANDLER: &str = "smokeProbe";
const URLS: [&str; 4] = [
    "https://example.com/?w1",
    "https://example.com/?w2",
    "https://httpbin.org/html",
    "https://example.com/?w3",
];
/// How long one document may take to load and speak.
const LIMIT: Duration = Duration::from_secs(30);

fn script(at: &str, injection_time: InjectionTime) -> UserScript {
    UserScript {
        source: format!(
            "webkit.messageHandlers.{HANDLER}.postMessage({{ at: '{at}', href: location.href }});"
        ),
        injection_time,
        frames: Frames::TopOnly,
        world: World::isolated(WORLD),
        allow_list: Vec::new(),
        block_list: Vec::new(),
    }
}

pub async fn world_messages(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    ctx.page
        .add_message_handler(HANDLER, &World::isolated(WORLD));
    ctx.page
        .add_user_script(&script("start", InjectionTime::DocumentStart));
    ctx.page
        .add_user_script(&script("end", InjectionTime::DocumentEnd));
    // Let the scripts reach the webview (a load does not wait for them).
    copper_win_rt::sleep(Duration::from_millis(1500)).await;
    let mut ok = true;
    for url in URLS {
        let t = Instant::now();
        ctx.page.load(url);
        let mut heard: Vec<String> = Vec::new();
        let mut loaded = false;
        let events = &mut ctx.events;
        let watch = async {
            loop {
                match poll_fn(|cx| events.poll_recv(cx)).await {
                    Some(PageEvent::ScriptMessage { name, body }) if name == HANDLER => {
                        let at = body.get("at").and_then(|a| a.as_str()).unwrap_or("?");
                        let href = body.get("href").and_then(|a| a.as_str()).unwrap_or("?");
                        log.at(&format!(
                            "  world message {at} {href} after {:.0} ms",
                            t.elapsed().as_secs_f64() * 1000.0
                        ));
                        if href == url || href.starts_with(url) {
                            heard.push(at.to_owned());
                        }
                    }
                    Some(PageEvent::Load(LoadState::Finished { .. })) => loaded = true,
                    Some(PageEvent::Load(LoadState::Failed { error, .. })) => {
                        log.at(&format!("  load failed: {error}"));
                        return;
                    }
                    Some(_) => {}
                    None => return,
                }
                if loaded && heard.iter().any(|a| a == "start") && heard.iter().any(|a| a == "end")
                {
                    return;
                }
            }
        };
        let _ = within(watch, LIMIT).await;
        let both = heard.iter().any(|a| a == "start") && heard.iter().any(|a| a == "end");
        ok &= pass(
            &log,
            loaded && both,
            &format!("isolated-world messages from {url} (heard {heard:?})"),
        );
    }
    ok
}
