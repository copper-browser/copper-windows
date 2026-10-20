//! `--context-menu-check`: app-rendered context menus
//! (`WebView2Page::set_context_menu_handler`). A link (CDP right click), an
//! image, a text selection and a text field (real right clicks), a child
//! frame (CDP), the Menu key and Shift+F10 must each reach the handler with
//! the right target and WebView2's menu; a reply that runs `selectAll` must select the field; a
//! dropped reply must leave the page working; with no handler, nothing
//! reaches it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use copper_engine::{InputEvent, JsonValue, Modifiers, MouseButton, PageView};
use copper_engine_webview2::{ContextKind, ContextMenuItem, ContextMenuReply, ContextMenuRequest};

use crate::checks::{act, centre, js, js_json, pass, until, until_js};
use crate::os;
use crate::run::{Ctx, wait_load};

const MENU_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8">
<title>context menus</title><style>
body { font: 20px "Segoe UI", sans-serif; margin: 40px; }
#img { display: block; margin: 20px 0; width: 272px; height: 92px; }
#field { font-size: 20px; width: 320px; }
</style></head><body>
<p id="txt">Some selectable words here.</p>
<p><a id="link" href="https://example.com/linked">a link text</a></p>
<img id="img" alt="logo" src="https://www.google.com/images/branding/googlelogo/1x/googlelogo_color_272x92dp.png">
<input id="field" value="hello field">
<p><iframe id="frame" style="width: 320px; height: 60px; border: 1px solid #888"
 srcdoc="<p style='font: 20px sans-serif'>inside the frame</p>"></iframe></p>
</body></html>"#;

/// How long a menu request may take to arrive.
const ARRIVE: Duration = Duration::from_secs(4);

type Seen = Rc<RefCell<Vec<(ContextMenuRequest, Option<ContextMenuReply>)>>>;

pub async fn context_menu(ctx: &mut Ctx) -> bool {
    let log = ctx.log.clone();
    ctx.page.load_html(MENU_HTML, None);
    if let Err(e) = wait_load(&log, &mut ctx.events).await {
        return pass(&log, false, &format!("context menu: page failed: {e}"));
    }
    let seen: Seen = Rc::default();
    let sink = seen.clone();
    ctx.page
        .set_context_menu_handler(Some(Box::new(move |req, reply| {
            sink.borrow_mut().push((req, Some(reply)));
        })));
    // The image comes from the network.
    let _ = until_js(
        ctx,
        "(() => { const i = document.getElementById('img'); return i.complete && i.naturalWidth > 0; })()",
        &JsonValue::Bool(true),
        Duration::from_secs(10),
    )
    .await;
    let dpr = js(ctx, "devicePixelRatio")
        .await
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let physical = |(x, y): (f64, f64)| ((x * dpr).round() as i32, (y * dpr).round() as i32);
    let hwnd = ctx.window.hwnd;
    let mut ok = true;

    // 1. A link, by CDP (trusted) input; the reply dropped.
    let Some(link) = centre(ctx, "link").await else {
        return pass(&log, false, "context menu: no link");
    };
    let right = InputEvent::Click {
        x: link.0,
        y: link.1,
        button: MouseButton::Right,
        count: 1,
        modifiers: Modifiers::NONE,
    };
    let sent = act(ctx, right).await;
    let req = next_request(&seen, 1).await;
    log.at(&format!(
        "link (CDP right click {sent:?}): {}",
        describe(req.as_ref())
    ));
    ok &= pass(
        &log,
        req.as_ref().is_some_and(|r| {
            r.kind == ContextKind::Page
                && r.link_url.as_deref() == Some("https://example.com/linked")
                && r.link_text.as_deref() == Some("a link text")
                && has(&r.items, "copyLinkLocation")
                && !r.in_frame
                && r.frame_url.is_none()
        }),
        "context menu: link request (kind, link_url, link_text, copyLinkLocation)",
    );
    seen.borrow_mut()[0].1.take(); // dropped: dismissed
    let alive = js(ctx, "6 * 7").await;
    ok &= pass(
        &log,
        alive == Ok(JsonValue::from(42)),
        "context menu: dropped reply leaves the page working",
    );

    // Real input from here: the window must be in front.
    let front = os::wait_foreground(hwnd, Duration::from_secs(15)).await;
    ok &= pass(&log, front, "context menu: smoke window is foreground");

    // 2. The image, by a real right click; answered `run(None)`.
    let img = centre(ctx, "img").await.map(physical).unwrap_or((0, 0));
    os::right_click(hwnd, img);
    let req = next_request(&seen, 2).await;
    log.at(&format!("image at {img:?}: {}", describe(req.as_ref())));
    ok &= pass(
        &log,
        req.as_ref().is_some_and(|r| {
            r.kind == ContextKind::Image
                && r.source_url
                    .as_deref()
                    .is_some_and(|s| s.ends_with("googlelogo_color_272x92dp.png"))
                && has(&r.items, "copyImage")
                && has(&r.items, "saveImageAs")
                && (r.point.0 - img.0).abs() <= 1
                && (r.point.1 - img.1).abs() <= 1
        }),
        "context menu: image request (kind, source_url, copyImage/saveImageAs, point = click)",
    );
    if let Some(reply) = seen.borrow_mut()[1].1.take() {
        reply.run(None);
    }

    // 3. A selection.
    let sel = js_json(
        ctx,
        "(() => { const t = document.getElementById('txt').firstChild; const r = document.createRange(); \
         r.setStart(t, 5); r.setEnd(t, 15); const s = getSelection(); s.removeAllRanges(); s.addRange(r); \
         const b = r.getBoundingClientRect(); return JSON.stringify([b.left + b.width / 2, b.top + b.height / 2]); })()",
    )
    .await
    .ok()
    .and_then(|v| Some((v[0].as_f64()?, v[1].as_f64()?)))
    .map(physical)
    .unwrap_or((0, 0));
    os::right_click(hwnd, sel);
    let req = next_request(&seen, 3).await;
    log.at(&format!("selection: {}", describe(req.as_ref())));
    ok &= pass(
        &log,
        req.as_ref().is_some_and(|r| {
            r.kind == ContextKind::SelectedText && r.selection.as_deref() == Some("selectable")
        }),
        "context menu: selection request (kind, selection)",
    );
    seen.borrow_mut()[2].1.take();

    // 4. The field: editable, and `selectAll` run from the reply.
    let field = centre(ctx, "field").await.map(physical).unwrap_or((0, 0));
    let _ = js(ctx, "document.getElementById('field').focus(); document.getElementById('field').setSelectionRange(0, 0); 0").await;
    os::right_click(hwnd, field);
    let req = next_request(&seen, 4).await;
    log.at(&format!("field: {}", describe(req.as_ref())));
    let select_all = req
        .as_ref()
        .and_then(|r| find(&r.items, "selectAll"))
        .map(|i| i.command);
    ok &= pass(
        &log,
        req.as_ref()
            .is_some_and(|r| r.editable && has(&r.items, "paste") && select_all.is_some()),
        "context menu: field request (editable, paste, selectAll)",
    );
    if let Some(reply) = seen.borrow_mut()[3].1.take() {
        reply.run(select_all);
    }
    let all = until_js(
        ctx,
        "(() => { const f = document.getElementById('field'); return f.selectionStart === 0 && f.selectionEnd === f.value.length; })()",
        &JsonValue::Bool(true),
        Duration::from_secs(3),
    )
    .await;
    ok &= pass(
        &log,
        all,
        "context menu: run(Some(selectAll)) selects the field",
    );

    // 5. The keyboard: Shift+F10, then the Menu key, in the focused field.
    let _ = js(ctx, "document.getElementById('field').focus(); 0").await;
    os::shift_f10();
    let req = next_request(&seen, 5).await;
    log.at(&format!("Shift+F10: {}", describe(req.as_ref())));
    ok &= pass(
        &log,
        req.is_some_and(|r| r.editable),
        "context menu: Shift+F10 arrives",
    );
    seen.borrow_mut()[4].1.take();
    os::menu_key();
    let req = next_request(&seen, 6).await;
    log.at(&format!("Menu key: {}", describe(req.as_ref())));
    ok &= pass(
        &log,
        req.is_some_and(|r| r.editable),
        "context menu: Menu key arrives",
    );
    seen.borrow_mut()
        .iter_mut()
        .for_each(|(_, r)| drop(r.take()));

    // 6. Inside a child frame.
    if let Some(at) = centre(ctx, "frame").await {
        let right = InputEvent::Click {
            x: at.0,
            y: at.1,
            button: MouseButton::Right,
            count: 1,
            modifiers: Modifiers::NONE,
        };
        let _ = act(ctx, right).await;
    }
    let req = next_request(&seen, 7).await;
    log.at(&format!("frame: {}", describe(req.as_ref())));
    ok &= pass(
        &log,
        req.is_some_and(|r| r.in_frame && r.frame_url.as_deref() == Some("about:srcdoc")),
        "context menu: frame request (in_frame, frame_url)",
    );
    seen.borrow_mut()
        .iter_mut()
        .for_each(|(_, r)| drop(r.take()));

    // 7. No handler: WebView2's own menu, nothing for the app.
    ctx.page.set_context_menu_handler(None);
    os::right_click(hwnd, img);
    let extra = until(Duration::from_millis(1500), || seen.borrow().len() > 7).await;
    os::escape();
    ok &= pass(
        &log,
        !extra,
        "context menu: no handler, no request (WebView2's menu)",
    );
    if let Some((first, _)) = seen.borrow().first() {
        log.at(&format!("the link's menu: {}", names(&first.items)));
    }
    ok
}

/// The `n`th request (1-based), once it has arrived.
async fn next_request(seen: &Seen, n: usize) -> Option<ContextMenuRequest> {
    until(ARRIVE, || seen.borrow().len() >= n).await;
    seen.borrow().get(n - 1).map(|(r, _)| r.clone())
}

fn find<'a>(items: &'a [ContextMenuItem], name: &str) -> Option<&'a ContextMenuItem> {
    items.iter().find_map(|i| {
        if i.name == name {
            Some(i)
        } else {
            find(&i.children, name)
        }
    })
}

fn has(items: &[ContextMenuItem], name: &str) -> bool {
    find(items, name).is_some()
}

/// `name(label)` of each item, submenus in brackets.
fn names(items: &[ContextMenuItem]) -> String {
    items
        .iter()
        .map(|i| {
            let sub = if i.children.is_empty() {
                String::new()
            } else {
                format!(" [{}]", names(&i.children))
            };
            format!("{}#{}{}", i.name, i.command, sub)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn describe(req: Option<&ContextMenuRequest>) -> String {
    match req {
        None => "no request".into(),
        Some(r) => format!(
            "{:?} link={:?} text={:?} src={:?} sel={:?} editable={} in_frame={} frame_url={:?} point={:?} page={:?} items={}",
            r.kind,
            r.link_url,
            r.link_text,
            r.source_url,
            r.selection,
            r.editable,
            r.in_frame,
            r.frame_url,
            r.point,
            r.page_url,
            names(&r.items)
        ),
    }
}
