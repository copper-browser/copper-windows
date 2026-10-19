//! Links copper.exe's Windows resources: the app icon, the application
//! manifest and the version block.
//!
//! The `.rc` and manifest are templates in `assets/`; the crate version is
//! filled in here (one source of truth: `Cargo.toml`) and the results are
//! written to `OUT_DIR` and compiled by `embed-resource` — `llvm-rc` when
//! cross-compiling from Linux/macOS with cargo-xwin, `rc.exe` on Windows.
//! Non-Windows targets get nothing.

use std::path::{Path, PathBuf};

fn main() {
    let assets = PathBuf::from(env("CARGO_MANIFEST_DIR")).join("assets");
    for file in ["copper.ico", "copper.exe.manifest", "copper.rc.in"] {
        println!("cargo:rerun-if-changed=assets/{file}");
    }
    println!("cargo:rerun-if-changed=build.rs");

    if env("CARGO_CFG_TARGET_OS") != "windows" {
        return;
    }

    let version = env("CARGO_PKG_VERSION");
    let part = |name: &str| -> u16 {
        env(name)
            .parse()
            .unwrap_or_else(|_| panic!("{name} isn't a 16-bit number"))
    };
    let (major, minor, patch) = (
        part("CARGO_PKG_VERSION_MAJOR"),
        part("CARGO_PKG_VERSION_MINOR"),
        part("CARGO_PKG_VERSION_PATCH"),
    );
    let out = PathBuf::from(env("OUT_DIR"));

    // Manifests want a four-part version.
    let manifest = out.join("copper.exe.manifest");
    let text = read(&assets.join("copper.exe.manifest"))
        .replace("@VERSION@", &format!("{major}.{minor}.{patch}.0"));
    write(&manifest, &text);

    let rc = out.join("copper.rc");
    let text = read(&assets.join("copper.rc.in"))
        .replace("@ICON@", &rc_path(&assets.join("copper.ico")))
        .replace("@MANIFEST@", &rc_path(&manifest))
        .replace("@VERSION@", &version)
        .replace("@V1@", &major.to_string())
        .replace("@V2@", &minor.to_string())
        .replace("@V3@", &patch.to_string());
    write(&rc, &text);

    if let Err(e) = embed_resource::compile(&rc, embed_resource::NONE).manifest_required() {
        panic!("couldn't compile copper.exe's resources: {e}");
    }

    // The manifest is linked as a resource above; stop the MSVC linker from
    // generating a second, default one alongside it.
    if env("CARGO_CFG_TARGET_ENV") == "msvc" {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("cargo didn't set {name}"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// A path as an `.rc` string literal's contents: forward slashes (which
/// rc.exe and llvm-rc both accept) so Windows backslashes aren't escapes.
fn rc_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
