//! The update check's Windows half (`UpdatesApp`): where Copper for
//! Windows is published, one HTTP/1.1 exchange over rustls (on a worker
//! thread, so the UI thread never waits on the network), and the line that
//! says a new version is out. The check itself — when, what is newer,
//! `updates.json` — is copper-shell-core's `updates` lane; getting the new
//! version ready and installing it is `self_update`'s.

use crate::chrome::glyph;
use crate::commands::Command;
use crate::platform::{Shell, Win};
use crate::self_update;
use copper_engine::LocalFuture;
use copper_shell_core::main_loop::spawn_blocking;
use copper_shell_core::updates::{self, Channel, MAX_ANSWER, UpdatesApp};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

/// Where the version file lives.
pub const FEED: &str =
    "https://github.com/copper-browser/copper-windows/releases/latest/download/copper-version.json";
/// The page people download from.
pub const RELEASES: &str = "https://github.com/copper-browser/copper-windows/releases/latest";

/// Copper for Windows' releases.
pub const CHANNEL: Channel = Channel {
    feed: FEED,
    releases: RELEASES,
    tag_page: "https://github.com/copper-browser/copper-windows/releases/tag/",
    system: "Windows",
    managed,
};

/// Nothing else installs Copper's updates on Windows.
fn managed() -> bool {
    false
}

struct WinUpdates;

impl UpdatesApp<Win> for WinUpdates {
    fn channel(&self) -> Channel {
        CHANNEL
    }

    fn exchange(
        &self,
        host: String,
        port: u16,
        tls: bool,
        request: Vec<u8>,
    ) -> LocalFuture<Result<Vec<u8>, String>> {
        Box::pin(async move {
            spawn_blocking(move || exchange(&host, port, tls, &request))
                .await
                .unwrap_or_else(|| Err("the update check stopped".into()))
        })
    }

    fn announce(&self, shell: &Shell, version: &str) {
        // The installed copy updates itself without a word (Settings ›
        // About tells; this runs for a new version, before it is staged, or
        // a check by hand); a portable one points at the release page.
        if !self_update::enabled() {
            shell.toast(&format!("Copper {version} is available — Settings › About"));
        } else if self_update::ready().as_deref() == Some(version) {
            shell.toast(&format!("Copper {version} is ready — Restart to Update"));
        }
        crate::settings::changed(shell, "about");
    }

    fn changed(&self, shell: &Shell) {
        crate::settings::changed(shell, "about");
        if !updates::status::<Win>().checking {
            self_update::stage(shell);
        }
    }
}

/// Start the lane and its commands.
pub fn register(shell: &Shell) {
    updates::start::<Win>(shell, Rc::new(WinUpdates));
    self_update::start(shell);
    crate::update_selftest::maybe_start(shell);
    shell.view().commands().add(
        Command::new(
            "restart-to-update",
            "Restart to Update",
            glyph::UPDATE,
            self_update::restart,
        )
        .when(|_| self_update::ready().is_some()),
    );
    shell.view().commands().add(Command::new(
        "check-updates",
        "Check for Updates",
        glyph::UPDATE,
        |s| updates::check::<Win>(s, true),
    ));
    shell.view().commands().add(
        Command::new(
            "download-update",
            "Download the New Copper",
            glyph::UPDATE,
            |_| {
                if let Err(e) = crate::sys::shell::open(&updates::release_page::<Win>()) {
                    tracing::warn!("release page: {e}");
                }
            },
        )
        .when(|_| updates::available::<Win>().is_some() && self_update::ready().is_none()),
    );
}

/// One request, one whole answer (worker thread).
fn exchange(host: &str, port: u16, tls: bool, request: &[u8]) -> Result<Vec<u8>, String> {
    let addr = (host, port);
    let tcp = TcpStream::connect(addr).map_err(|e| e.to_string())?;
    let limit = Some(Duration::from_secs(20));
    let _ = tcp.set_read_timeout(limit);
    let _ = tcp.set_write_timeout(limit);
    if !tls {
        return talk(tcp, request);
    }
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name =
        rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    talk(rustls::StreamOwned::new(conn, tcp), request)
}

fn talk(mut s: impl Read + Write, request: &[u8]) -> Result<Vec<u8>, String> {
    s.write_all(request).map_err(|e| e.to_string())?;
    s.flush().map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > MAX_ANSWER {
                    return Err("The answer was too big".into());
                }
            }
            // A server closing without TLS's goodbye after a whole answer
            // (common with `Connection: close`) is the end, not a failure.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !raw.is_empty() => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(raw)
}
