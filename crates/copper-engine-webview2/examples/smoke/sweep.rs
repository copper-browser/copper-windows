//! `--sweep LIST`: a crash sweep over real sites in one page.
//!
//! `LIST` is a file with one URL per line (`#` comments), or URLs separated
//! by commas. Each site is loaded in the same page — so leaving it destroys
//! its frames, as browsing does — then scrolled down and back with trusted
//! wheel input, its first visible text field clicked and typed into with
//! trusted keys, and snapshotted (`--sweep-out DIR`, default `%TEMP%`).
//! Every step is logged as it happens: if the process dies, the log's last
//! line names the site and the step. A load that fails or times out is
//! reported and the sweep goes on; only a crash ends it early.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use copper_engine::{InputEvent, JsonValue, PageView, SnapshotRegion, World};

use crate::run::{Ctx, within};

/// How long one site may take to load before the sweep moves on.
const SITE_LOAD_LIMIT: Duration = Duration::from_secs(30);
/// Paint and script time after a load, before the input starts.
const SITE_SETTLE: Duration = Duration::from_millis(1500);
/// Time for whatever typing opens (suggestions, validation) to show.
const AFTER_TYPING: Duration = Duration::from_millis(1500);
/// The text typed into each site's field.
const TYPED: &str = "copper sweep";

/// The centre (CSS px) of the first visible, enabled text field of the
/// page, scrolled into view; `null` if there is none.
const FIRST_FIELD: &str = r#"(() => {
  const sel = 'input:not([type]),input[type=text],input[type=search],input[type=email],' +
    'input[type=url],input[type=tel],textarea,[contenteditable=""],[contenteditable=true],[role=combobox],[role=searchbox]';
  for (const el of document.querySelectorAll(sel)) {
    if (el.disabled || el.readOnly) continue;
    const st = getComputedStyle(el);
    if (st.visibility === 'hidden' || st.display === 'none') continue;
    el.scrollIntoView({ block: 'center', inline: 'center' });
    const r = el.getBoundingClientRect();
    if (r.width < 8 || r.height < 8) continue;
    if (r.bottom < 0 || r.right < 0 || r.top > innerHeight || r.left > innerWidth) continue;
    return JSON.stringify({ x: r.left + Math.min(r.width / 2, 120), y: r.top + r.height / 2,
      tag: el.tagName.toLowerCase(), name: el.name || el.id || el.getAttribute('aria-label') || '' });
  }
  return null;
})()"#;

/// What the focused element holds after typing (proof the keys arrived).
const FOCUSED_VALUE: &str = r#"(() => { const a = document.activeElement;
  if (!a) return null; const v = ('value' in a) ? a.value : a.textContent;
  return String(v || '').slice(0, 60); })()"#;

/// The sites of `spec` (a file, or a comma-separated list).
pub fn sites(spec: &str) -> Result<Vec<String>, String> {
    let text = if Path::new(spec).is_file() {
        std::fs::read_to_string(spec).map_err(|e| format!("--sweep {spec}: {e}"))?
    } else {
        spec.replace(',', "\n")
    };
    let list: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    if list.is_empty() {
        return Err(format!("--sweep {spec}: no sites"));
    }
    Ok(list)
}

/// Run the sweep; `true` if every site still rendered (its snapshot was
/// written) after loading, scrolling and typing.
pub async fn sweep(ctx: &mut Ctx, sites: &[String], out: &Path) -> bool {
    let log = ctx.log.clone();
    let _ = std::fs::create_dir_all(out);
    let mut ok = true;
    let mut rows = Vec::new();
    for (i, url) in sites.iter().enumerate() {
        let n = i + 1;
        let started = Instant::now();
        log.at(&format!("sweep {n}/{}: {url}: load", sites.len()));
        // The last site's late loads (redirects, reloads) are not this one's.
        while ctx.events.try_next().is_some() {}
        ctx.page.load(url);
        let loaded = within(
            crate::run::wait_load(&log, &mut ctx.events),
            SITE_LOAD_LIMIT,
        )
        .await
        .unwrap_or_else(|| Err("no load in time".to_owned()));
        let load_ms = started.elapsed().as_millis();
        let load = match &loaded {
            Ok(_) => format!("loaded {load_ms} ms"),
            Err(e) => format!("load: {e} ({load_ms} ms)"),
        };
        copper_win_rt::sleep(SITE_SETTLE).await;
        log.at(&format!("sweep {n}: {url}: scroll"));
        let scrolled = scroll(ctx).await;
        log.at(&format!("sweep {n}: {url}: type"));
        let typed = type_into_field(ctx).await;
        copper_win_rt::sleep(AFTER_TYPING).await;
        let shot = out.join(format!("sweep-{n:02}-{}.png", host_of(url)));
        let snap = match ctx.page.snapshot(SnapshotRegion::Visible).await {
            Ok(img) => std::fs::write(&shot, &img.png)
                .map(|()| shot.display().to_string())
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        // A page that renders after all that passes; typing is reported
        // (sites may refuse it: sign-in walls, CAPTCHAs).
        let row_ok = snap.is_ok();
        ok &= row_ok;
        let row = format!(
            "SWEEP {} | {url} | {load} | final {} | scroll {} | {} | shot {}",
            if row_ok { "PASS" } else { "FAIL" },
            ctx.page.url().unwrap_or_default(),
            if scrolled { "ok" } else { "failed" },
            typed.describe(),
            snap.unwrap_or_else(|e| format!("failed: {e}")),
        );
        log.at(&row);
        rows.push(row);
    }
    log.at("sweep summary:");
    for row in &rows {
        log.line(row);
    }
    ok
}

/// Wheel down twice and back up, at the viewport's centre.
async fn scroll(ctx: &Ctx) -> bool {
    let mut ok = true;
    for dy in [600.0, 600.0, -1200.0] {
        if let Err(e) = ctx.page.input(InputEvent::Scroll { dx: 0.0, dy }).await {
            ctx.log.at(&format!("  scroll: {e}"));
            ok = false;
        }
        copper_win_rt::sleep(Duration::from_millis(250)).await;
    }
    ok
}

/// What happened when the sweep typed into a site.
enum Typed {
    /// The field holds the text afterwards.
    Took(String),
    /// The site has no visible text field (or a sign-in wall, a CAPTCHA…).
    NoField,
    /// Typed, but the field does not show it (a site may rewrite it).
    Lost(String),
}

impl Typed {
    fn describe(&self) -> String {
        match self {
            Typed::Took(field) => format!("typed into {field}"),
            Typed::NoField => "no text field".into(),
            Typed::Lost(why) => format!("typing lost: {why}"),
        }
    }
}

async fn type_into_field(ctx: &Ctx) -> Typed {
    let field = match ctx.page.eval(FIRST_FIELD, &World::Page).await {
        Ok(JsonValue::String(s)) => serde_json::from_str::<JsonValue>(&s).ok(),
        _ => None,
    };
    let Some(field) = field else {
        return Typed::NoField;
    };
    let (Some(x), Some(y)) = (
        field.get("x").and_then(JsonValue::as_f64),
        field.get("y").and_then(JsonValue::as_f64),
    ) else {
        return Typed::NoField;
    };
    let name = format!(
        "<{}> {:?} at ({x:.0}, {y:.0})",
        field.get("tag").and_then(JsonValue::as_str).unwrap_or("?"),
        field.get("name").and_then(JsonValue::as_str).unwrap_or("")
    );
    if let Err(e) = ctx.page.input(InputEvent::click(x, y)).await {
        return Typed::Lost(format!("{name}: click: {e}"));
    }
    copper_win_rt::sleep(Duration::from_millis(300)).await;
    if let Err(e) = ctx.page.input(InputEvent::Type(TYPED.into())).await {
        return Typed::Lost(format!("{name}: type: {e}"));
    }
    match ctx.page.eval(FOCUSED_VALUE, &World::Page).await {
        Ok(JsonValue::String(v)) if v.contains(TYPED) => Typed::Took(name),
        Ok(v) => Typed::Lost(format!("{name}: focused element holds {v}")),
        Err(e) => Typed::Lost(format!("{name}: {e}")),
    }
}

/// A file-name-safe host for `url`.
fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| "page".into())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `--sweep-out`'s default.
pub fn default_out() -> PathBuf {
    std::env::temp_dir().join("copper-sweep")
}
