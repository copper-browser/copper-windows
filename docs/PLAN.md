# Copper for Windows — build plan

Felipe's decision (2026-10-19): **option B** from
`reports/2026-10-19-copper-windows-options/OPTIONS.md` — a native Rust shell on
Win32 + DirectComposition + Direct2D/DirectWrite, with WebView2 (Evergreen) in
visual hosting mode. The macOS app is the golden standard for look, feel and
behaviour. Copper for Linux is the reference for the Rust structure.

## Goals

1. Looks, feels and works like Copper on the Mac (design tokens, motion, the
   one field, tab column, Spaces, ⌘K palette → Ctrl+K, Canvas, Copper Cloud,
   agents/MCP), adapted to Windows conventions only where the platform
   demands it (caption buttons, Alt+F4, Snap layouts, Ctrl instead of ⌘).
2. Very fast: window on screen < 200 ms cold, first page painted < 600 ms,
   idle RAM for the shell well under 100 MB, 60/120 fps animation.
3. One binary: `copper.exe` is the browser, the `copper` CLI, the MCP stdio
   bridge and the installer/uninstaller. The download is the same exe named
   `CopperSetup.exe`; run under that name (or with `--install`) it installs
   itself per-user and launches Copper.
4. Share code: every toolkit-free crate lives once in
   `copper-browser/copper-linux` (`shared/copper-linux` submodule here).
   Canvas and Copper Cloud must behave 1:1 with Mac and Linux, through the
   same shared code, page bundle and wire protocol.

## Repositories

| Repo | Role |
|---|---|
| `copper-browser/copper-linux` | The shared code place: `copper-core`, `copper-engine` (traits), `copper-pagejs`, `copper-shield`, `copper-creds`, `copper-import`, `copper-cloud`, `copper-mcp`, and the new `copper-shell-core` (toolkit-free shell logic extracted from `copper-app`, generic over the engine). Windows support lands there behind `cfg(windows)` with Linux behaviour unchanged. |
| `copper-browser/copper-windows` (this) | Windows-only crates: `copper-win-rt` (UI-thread runtime), `copper-engine-webview2`, `copper-win-ui` (Win32/DComp/D2D toolkit), `copper-setup` (installer library), `copper-win` (the app, bin `copper`). |

## Crates in this repo

| Crate | What |
|---|---|
| `copper-win-rt` | Message-loop driven local executor, `sleep`, `post` from other threads, message hooks, `oneshot`. Shared seam; keep it small. |
| `copper-engine-webview2` | Implements the `copper-engine` traits on WebView2 (`webview2-com` 0.39). Visual hosting via `ICoreWebView2CompositionController`; input forwarding API for the toolkit; one environment; profiles; lazy controllers + warm spare; message shim; CDP for isolated worlds/trusted input/screenshots; Shield via a generated MV3 declarativeNetRequest extension + Tracking Prevention Strict. |
| `copper-win-ui` | Toolkit: custom-frame top-level window (Mica, Snap, per-monitor DPI v2), DirectComposition visual tree, Direct2D/DirectWrite drawing, layout, input/focus, text field with IME, spring animation, scroll, menus, tooltips, UI Automation, design tokens from the Mac `Design.swift`. A `WebSlot` node hosts an engine page's visual and forwards input. |
| `copper-setup` | Installer/uninstaller/registration as a library called by `copper.exe`: per-user install to `%LOCALAPPDATA%\Programs\Copper`, Start menu, default-browser registration (HKCU StartMenuInternet + Capabilities + RegisteredApplications), Uninstall entry, WebView2 runtime check + Evergreen bootstrapper, silent mode, in-place upgrade. Its own small branded window. |
| `copper-win` | The app: wires shell-core + engine + toolkit + views; CLI/MCP/installer entry points in one `main`. |

## Data locations (Windows)

`%LOCALAPPDATA%\Copper\` is the root: `Data\` holds what `copper-core` keeps
in the Linux data dir (same file names and formats as Mac/Linux), `Cache\` the
cache, `WebView2\` the WebView2 user data folder (one environment, profiles
inside). Logs in `Data\logs\`. Nothing roams.

## Waves

| Wave | Work | Owner model |
|---|---|---|
| 1 | Shared crates Windows-correct (core/cloud/mcp/engine paths; creds → Credential Manager/DPAPI; import → Chrome/Edge/Brave DPAPI + AES-GCM). Shell-core foundation (crate, `MainLoop` trait, generic engine plumbing, engine trait additions) in copper-linux. WebView2 engine crate. Toolkit core. Installer library. CI/release workflows. | Opus high/xhigh; CI on Luna |
| 2 | Shell-core module moves (canvas, cloud, agents, forms, palette/spaces/features/session). Windows app: window + tab column + one field + palette + Spaces + panels + settings + downloads + find + permissions + Canvas + Cloud + agents/MCP + passwords/import + updater. | Opus xhigh (UX), high (wiring) |
| 3 | Validate on Felipe's Windows PC (hyperv-manager `ws-farce`), fix waves, visual parity pass against the Mac, perf pass, release `v0.1.0` (CopperSetup-x64.exe, -arm64.exe, portable zip, SHA256SUMS, `copper-version.json` `platform:"windows"`). | Opus |

## Rules for every worker

- Never copy shared code into this repo; change it in `shared/copper-linux`
  (your own copper-linux worktree) behind `cfg(windows)`, Linux unchanged.
- Cross-build from Linux: `XWIN_ACCEPT_LICENSE=1 cargo xwin build --target
  x86_64-pc-windows-msvc` (also `aarch64-pc-windows-msvc`). `cargo xwin
  clippy ... -- -D warnings` must be clean. `cargo fmt --all`.
- Every shell command bounded with `timeout`; nothing interactive
  (`GIT_TERMINAL_PROMPT=0`, `ssh -o BatchMode=yes -o ConnectTimeout=10`).
- No git mutations in worker runs; the orchestrator commits and ships.
- Native PostgreSQL only (copper-cloud live test), never Docker.
