# Copper for Windows — the app shell

`copper.exe` (`crates/copper-win`) is the browser. It is a thin Windows
front end over **copper-shell-core** (the platform-free browser shared with
Linux and the Mac), the **WebView2 engine** (`copper-engine-webview2`), the
**toolkit** (`copper-win-ui`: tree, layout, Direct2D/DirectComposition
painting, UIA) and the **loop** (`copper-win-rt`, the only message loop).
The Mac app is the reference: same structure, same words, same behaviour;
Windows-native only where the platform insists (caption buttons top right,
Ctrl for ⌘, Mica, jump list, single instance by named mutex).

## Start-up

`main.rs` → `args::parse` → installer / uninstaller first (untouched) →
CLI front ends (`cli.rs`: `--mcp-stdio`, `cloud …`, `--cli <verb>` /
`<verb>`) → `app::run`:

1. `SetCurrentProcessExplicitAppUserModelID("Copper.Browser")`, per-monitor
   DPI v2, logging to `Data\logs\copper.log` (panics are logged too).
2. `instance::claim` — one browser per `COPPER_WORLD`. A later start finds
   the owner's message-only window, hands over its URLs (`WM_COPYDATA`) and
   exits (~35 ms on the PC).
3. WebView2 probe; a plain-words dialog if the runtime is missing.
4. `Shell::new(Win)` → `features::register` (the lanes, in order) →
   `shell.start()` (session restore, first tab shown) → window shown.
5. `copper_win_rt::run()`; afterwards `app::end` exits the process at once
   (everything was saved synchronously by `quit`; nothing may be left for
   a task after the loop — copper-win-rt drops pending tasks there).

## Module map

| Module | What it is |
|---|---|
| `platform.rs` | `Win: Platform` — engine, loop, view, favicon icon, page frame, user name, languages. |
| `app.rs` | Start-up, quit, the fast exit. |
| `view.rs` | `ShellView` for the window: every method real, all forwarded to `chrome`. |
| `chrome/` | The window. `build.rs` the node tree; `column.rs` the left column (space chips, field, pins, tab list with the sliding pill, New Tab, foot); `strip.rs` tabs-on-top; `stage.rs` the stage card and its page slots; `card.rs` the one field's card (palette and address); `sheet.rs` `ask()`; `toast.rs`; `menus.rs`; `events.rs` all toolkit events → shell; `model.rs` view-model diffing; `glyph.rs`, `images.rs`. |
| `slot.rs` | CONTRACT §2 glue: `PageHost: SlotHost` over `WebView2Page`. |
| `frame.rs` | `PageFrame` status overlay (failed / crashed / offline, with Reload). |
| `commands.rs` | The command registry (id → run, palette name, glyph, `when`). |
| `keys.rs` | The chord table; one table for chrome-focus and page-focus keys. |
| `actions.rs` | The built-in commands (tabs, navigation, window). |
| `features.rs` | Lane start order and per-page hooks. |
| `panels/` | Floating panels; `history.rs` is the worked example; `plate.rs` the chrome. |
| `settings/` | Settings panel: rail of pages, each a `Page` drawing a `form::Form`. |
| `palette/` | The card's logic (Ctrl+K / Ctrl+L / Ctrl+T), ranked by shell-core. |
| `find.rs` | Find bar (Ctrl+F, F3, Shift+F3). |
| `forms.rs`, `agents.rs`, `updates.rs`, `canvas.rs`, `cloud.rs` | The lanes' Windows halves. |
| `instance.rs`, `args.rs`, `cli.rs` | Single instance, arguments, CLI front ends. |
| `sys/` | Win32 helpers: calendar, dialogs, network, screenshots, shell (open/jump list), user, window placement/Mica. |
| `perf.rs` | `copper::perf` timings in the log (first frame, first page, key→frame, tab switch). |
| `picture.rs` | PNG encode for screenshots/snapshots. |

## Extension points (wave 2b)

Every feature plugs in with **one registration line in
`features::register`** (`crates/copper-win/src/features.rs`) calling the
feature's own `register(shell)`; the feature then adds what it needs
through the points below. Never call the shell from inside a
`Chrome::edit` closure (read first with `Chrome::read`, then edit): the
toolkit is re-entrancy-safe only that way.

| Point | How | Where it lands |
|---|---|---|
| **Command** | `shell.view().commands().add(Command::new(id, name, glyph, run).when(...))`; `Command::key(id, run)` for key-only ones. | Palette lists it; `commands::run(shell, id)` runs it. |
| **Shortcut** | Add `("id", &[c(l('X'))])` to `BINDINGS` in `keys.rs`. | Works with chrome **or** page focus (page keys come via `AcceleratorKeyPressed`, `chrome/events.rs::hook_page`). |
| **Panel** | Implement `panels::Panel` (`build` inside an edit, `refresh`/`click` with nothing borrowed); `shell.view().panels().add(shell, Rc::new(MyPanel))`. Adds `show-<id>`. | Floats on a plate; Esc / cross / dim click closes. |
| **Settings page** | Implement `settings::Page` (id, title, `form` from the shell, `act` on clicks); `settings::add(Rc::new(MyPage))`. | Rail entry in Settings (`Ctrl+,`). |
| **Lane** | Start the shell-core lane in `features::register` in the documented order: shield, curtain, updates, forms, cloud, canvas, agents. | — |
| **Per-page hook** | Inside `features::page_created` (called for every page, restored ones included). | — |
| **Toast / ask** | `shell.toast(..)`, `shell.ask(..)` (shell-core); drawn by `chrome/toast.rs` / `chrome/sheet.rs`. | — |
| **Settings key** | Settings live in shell-core `settings.json`; read via the shell, never cache in chrome. | — |

### Who owns what in wave 2b

| Worker | Files |
|---|---|
| History / bookmarks / downloads panels | `panels/<name>.rs` + one `panels.add` in `panels::register` |
| Settings pages | `settings/pages.rs` (one `Page` per section) |
| Palette | `palette/mod.rs`, `chrome/card.rs` |
| Find | `find.rs` |
| Forms / passwords | `forms.rs`, `settings/pages.rs` › Passwords |
| Agents / MCP | `agents.rs`, `cli.rs` (`--mcp-stdio`) |
| Updates | `updates.rs` |
| Canvas | `canvas.rs` (needs the engine's `register_scheme`, now on main) |
| Cloud | `cloud.rs`, `cli.rs` (`cloud …`) |
| Window chrome / look | `chrome/*` (one owner at a time — shared hot path) |
| Toolkit | `crates/copper-win-ui/**` |

## Focus and keys

The page (WebView2) and the chrome (toolkit) share one HWND. A chord in
`keys::BINDINGS` is Copper's wherever focus is; other keys go to whoever
has focus. Escape is taken from a page only while the chrome has an
overlay open (`keys::page_takes`). When the chrome focuses a node while the
page holds keyboard focus, the toolkit calls `SetFocus(hwnd)` (only if the
window is active, so it never steals activation).

## Performance budget and measurements (PC, release)

| Measure | Goal | Measured |
|---|---|---|
| Window first frame (process start) | < 200 ms | 294–325 ms warm, 407–463 ms cold |
| First page painted | < 600 ms | 640–720 ms warm (wiki); 1.9–2.6 s for heavier pages |
| Key → frame (typing in the field) | 1 frame | 1.3–25 ms (CPU 1–10 ms) |
| Tab switch → frame | 1 frame | 7–40 ms |
| Idle RAM | < 100 MB | copper 63–72 MB WS (54–58 MB private) + WebView2 browser process ~32–36 MB private |
| Second instance hand-off | — | ~33–35 ms, exit 0 |
| Quit (Ctrl+Q) | — | 21–244 ms, exit 0 |

The log's `copper::perf` target records all of these
(`COPPER_LOG=info,copper=debug`).
