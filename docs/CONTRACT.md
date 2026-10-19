# Wave 1 contract — seams between parallel workers

Read-only for workers. If a seam does not work for you, say so in your final
report (with the change you need) instead of editing another worker's crate.

## 0. Ground truth

- Golden standard: the Mac app, `~/src/copper-mac-ref` (Swift; `Sources/`,
  `Fork/`, design tokens in `Design.swift`, screenshot
  `.github/screenshot.png`).
- Rust reference: Copper for Linux, `~/src/copper-linux-shared` (main) — read
  `crates/copper-app/src/**` to see how the shell uses the shared crates.
- Research: `~/exowatt/reports/2026-10-19-copper-windows-options/`
  (`OPTIONS.md`, `webview2-findings.md`, `code-map.md` — the code map lists
  every Mac feature, its Linux equivalent and the WebView2 API to use).
- Toolchain: `cargo-xwin` 0.23.1 on this Linux aarch64 host;
  `XWIN_ACCEPT_LICENSE=1 cargo xwin {check,clippy,build} --target
  x86_64-pc-windows-msvc`. There is no Windows here to run binaries; the
  orchestrator runs them on a Windows 11 VM. Unit-test pure logic on Linux
  where it is platform-free (`cargo test` on the host).

## 1. `copper-win-rt` (exists; orchestrator-owned)

`init()`, `run()`, `quit(code)`, `spawn_local(fut)`, `sleep(dur)`,
`post(f)` (any thread → UI thread), `add_message_hook(|&RawMsg| -> bool)`,
`oneshot()`, `on_ui_thread()`. Everything Copper does on the UI thread goes
through this; no other message loop, no nested pumps (do not copy wry's
nested-pump waits).

## 2. Engine ↔ toolkit (visual hosting)

The engine never creates top-level windows for visible pages and never
depends on `copper-win-ui`; the toolkit never depends on the engine. The app
glues them.

`copper-engine-webview2` exposes on its page type (inherent methods, beside
the `copper_engine::PageView` impl):

```rust
/// Host this page's visual under `visual` (a DComp visual owned by the
/// toolkit's WebSlot) in top-level window `hwnd`. Creates the
/// CompositionController lazily if needed. Idempotent.
fn attach(&self, hwnd: HWND, visual: &IDCompositionVisual);
fn detach(&self);
/// Bounds in physical pixels, relative to the window client area.
fn set_bounds(&self, rect: RECT);
fn set_visible(&self, visible: bool);
fn set_rasterization_scale(&self, scale: f64);
/// Forward input that the toolkit hit-tested into the slot. Point is in
/// slot-local physical pixels. Mirrors ICoreWebView2CompositionController.
fn send_mouse_input(&self, kind: MouseEventKind, keys: VirtualKeys, mouse_data: u32, point: POINT);
fn send_pointer_input(&self, kind: PointerEventKind, info: &POINTER_INFO /* via CreateCoreWebView2PointerInfo */);
fn move_focus(&self, reason: FocusReason);
/// Current cursor for the slot (from CursorChanged); the toolkit sets it.
fn cursor(&self) -> HCURSOR;
fn on_cursor_changed(&self, f: impl Fn(HCURSOR) + 'static);
/// IRawElementProviderSimple for UIA, from Controller2::AutomationProvider.
fn automation_provider(&self) -> Option<windows::core::IUnknown>;
```

(Types in those signatures may be the engine's own small enums; keep them
thin wrappers over the WebView2 values.)

`copper-win-ui` provides a `WebSlot` node: owns an `IDCompositionVisual` in
the tree at the slot's layout rect, reports rect/scale/visibility changes and
delivers raw mouse/pointer/focus input for its area through a callback
interface the app implements (so the app forwards to the page). Keyboard and
IME go straight to WebView2's own child HWND when the page has focus; the
toolkit must hand focus back and forth cleanly (Ctrl+L / Ctrl+T / Ctrl+K /
Escape go to the shell even while the page is focused: handle them with
`AcceleratorKeyPressed` on the engine side → app callback, and with
`copper_win_rt::add_message_hook` on the shell side).

## 3. Engine ↔ shared shell

`copper-engine-webview2` implements `copper_engine::{Engine, Profile,
PageView, ContentFilter, Backstage}` as they are on copper-linux main
(`f814631`). The shell-core worker may add trait methods in copper-linux
(custom URI scheme registration, file-chooser hooks, a MainLoop trait, an
engine image type); those land in wave 2 and the engine catches up then.
Until then put anything extra the Windows engine needs as inherent methods.

`new_page` is sync but WebView2 controller creation is async: return the page
handle at once and queue operations until the controller exists. One
`CoreWebView2Environment` for the process, created at launch (browser
extensions enabled, `--disable-features` as needed), user data folder
`%LOCALAPPDATA%\Copper\WebView2`.

## 4. Data locations

Root `%LOCALAPPDATA%\Copper\`: `Data\` (copper-core store; same file names
and formats as Linux/Mac), `Cache\`, `WebView2\`, `Data\logs\`. Use
`SHGetKnownFolderPath(FOLDERID_LocalAppData)` (or the `directories` crate
when it resolves the same place). Honour `COPPER_DATA_DIR` /
`COPPER_CACHE_DIR` overrides if the Linux code already does.

## 5. Single binary

`copper.exe` (crate `copper-win`, bin `copper`) is the browser, the CLI
(`copper <subcommand>` — attach to the parent console with
`AttachConsole(ATTACH_PARENT_PROCESS)`), the MCP stdio bridge (std handles are
inherited pipes), and the installer: when the exe's file stem starts with
`CopperSetup` (case-insensitive) or it gets `--install`, it calls
`copper_setup::run_installer()`; `--uninstall` calls
`copper_setup::run_uninstaller()`. No loader DLLs next to the exe: WebView2
loading must be static/in-process (verify what `webview2-com-sys` links and
fix it if it needs `WebView2Loader.dll`). The C runtime is linked statically
(`+crt-static`).
