# Copper for Windows

A small, fast, quiet web browser: Copper, the browser from the Mac, built
natively for Windows in Rust on Win32, DirectComposition and Direct2D, with
Microsoft Edge WebView2 as the engine.

Status: in development. See [docs/PLAN.md](docs/PLAN.md).

The shared, toolkit-free Copper crates live in
[copper-linux](https://github.com/copper-browser/copper-linux) (submodule at
`shared/copper-linux`).

```sh
git clone --recurse-submodules https://github.com/copper-browser/copper-windows
XWIN_ACCEPT_LICENSE=1 cargo xwin build --release --target x86_64-pc-windows-msvc -p copper-win
```
