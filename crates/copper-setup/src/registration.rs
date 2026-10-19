//! Everything Copper writes to the registry, as data.
//!
//! The whole per-user footprint is composed here, platform-free, so it can
//! be read and tested in one place; `sys::reg` only applies it. All keys are
//! under `HKEY_CURRENT_USER` — no admin, no UAC.
//!
//! What it adds up to:
//! * an entry in Settings › Apps (`…\Uninstall\Copper`),
//! * `Win+R copper` (`App Paths`),
//! * a browser Windows knows about, offered in Settings › Default apps
//!   (`Clients\StartMenuInternet\Copper` + `Capabilities` +
//!   `RegisteredApplications`, and two ProgIDs, `CopperHTML` for files and
//!   `CopperURL` for links),
//! * "Open with › Copper" for the file types it reads (`OpenWithProgids`).
//!
//! `mailto:` is deliberately not claimed: Copper is not a mail client, and
//! offering it as one in Default apps would be a lie.

/// The product name everywhere Windows shows one.
pub const APP_NAME: &str = "Copper";
/// The one line that says what Copper is.
pub const DESCRIPTION: &str = "Copper is a small, fast, quiet browser";
/// Publisher shown in Settings › Apps.
pub const PUBLISHER: &str = "Copper";
/// The AppUserModelID: groups the taskbar button, jump list and toasts with
/// the Start menu shortcut. The browser must set the same one on itself.
pub const APP_USER_MODEL_ID: &str = "Copper.Browser";
/// Version written to the Uninstall entry.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// ProgID for documents (`.html`, `.pdf`, …).
pub const PROGID_HTML: &str = "CopperHTML";
/// ProgID for links (`http`, `https`).
pub const PROGID_URL: &str = "CopperURL";

/// `HKCU\` + this: the entry in Settings › Apps.
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Copper";
/// `HKCU\` + this: lets `copper` be typed in Run and `start copper`.
pub const APP_PATHS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths\copper.exe";
/// `HKCU\` + this: the browser as the Start menu and Default apps see it.
pub const CLIENT_KEY: &str = r"Software\Clients\StartMenuInternet\Copper";
/// `HKCU\` + this: the list Default apps reads registered apps from.
pub const REGISTERED_APPS_KEY: &str = r"Software\RegisteredApplications";
/// What `RegisteredApplications\Copper` points at (relative to HKCU).
pub const CAPABILITIES_KEY: &str = r"Software\Clients\StartMenuInternet\Copper\Capabilities";

/// File types Copper opens, offered in Default apps and Open with.
pub const FILE_TYPES: &[&str] = &[".htm", ".html", ".pdf", ".svg", ".webp", ".xhtml"];
/// URL schemes Copper opens.
pub const URL_SCHEMES: &[&str] = &["http", "https"];

/// A registry value, in the types Copper needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `REG_SZ`.
    Str(String),
    /// `REG_DWORD`.
    Dword(u32),
}

/// One value to set: `HKCU\{key}` → `name` (`None` = the key's default).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub key: String,
    pub name: Option<String>,
    pub value: Value,
}

/// One thing to take away on uninstall.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removal {
    /// `HKCU\{0}` and everything under it.
    Tree(String),
    /// Just the value `name` in `HKCU\{key}`, leaving the key (it is shared
    /// with other apps, like `.html\OpenWithProgids`).
    Value { key: String, name: String },
}

/// Where Copper is installed, as the strings the registry stores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    /// `…\Programs\Copper`, no trailing backslash.
    pub dir: String,
    /// `…\Programs\Copper\copper.exe`.
    pub exe: String,
    /// Size on disk, for Settings › Apps, in KiB.
    pub size_kib: u32,
    /// `YYYYMMDD`, for Settings › Apps' "Installed" date.
    pub date: String,
}

impl Install {
    /// The exe, quoted for a command line.
    pub fn quoted_exe(&self) -> String {
        format!("\"{}\"", self.exe)
    }

    /// The exe's first icon, the way `DefaultIcon` and friends name it.
    pub fn icon(&self) -> String {
        format!("{},0", self.exe)
    }

    /// What Windows runs to open a file or link: everything after
    /// `--single-argument` is one target, so a URL with spaces or a leading
    /// dash can never become flags (see [`crate::cli::SINGLE_ARGUMENT`]).
    pub fn open_command(&self) -> String {
        format!("{} {} %1", self.quoted_exe(), crate::cli::SINGLE_ARGUMENT)
    }

    /// Settings › Apps › Uninstall.
    pub fn uninstall_command(&self) -> String {
        format!("{} --uninstall", self.quoted_exe())
    }

    /// Unattended uninstall (keeps browsing data).
    pub fn quiet_uninstall_command(&self) -> String {
        format!("{} --uninstall --silent", self.quoted_exe())
    }
}

fn s(v: impl Into<String>) -> Value {
    Value::Str(v.into())
}

/// Collects writes key by key, so the list reads like a .reg file.
struct Writes(Vec<Write>);

impl Writes {
    fn set(&mut self, key: &str, name: Option<&str>, value: Value) -> &mut Self {
        self.0.push(Write {
            key: key.to_owned(),
            name: name.map(str::to_owned),
            value,
        });
        self
    }
}

/// Every value Copper's registration sets, in order (parents first).
pub fn writes(install: &Install) -> Vec<Write> {
    let mut w = Writes(Vec::new());
    let icon = install.icon();
    let open = install.open_command();

    // Settings › Apps.
    w.set(UNINSTALL_KEY, Some("DisplayName"), s(APP_NAME))
        .set(UNINSTALL_KEY, Some("DisplayIcon"), s(&icon))
        .set(UNINSTALL_KEY, Some("DisplayVersion"), s(VERSION))
        .set(UNINSTALL_KEY, Some("Publisher"), s(PUBLISHER))
        .set(UNINSTALL_KEY, Some("InstallLocation"), s(&install.dir))
        .set(UNINSTALL_KEY, Some("InstallDate"), s(&install.date))
        .set(
            UNINSTALL_KEY,
            Some("UninstallString"),
            s(install.uninstall_command()),
        )
        .set(
            UNINSTALL_KEY,
            Some("QuietUninstallString"),
            s(install.quiet_uninstall_command()),
        )
        .set(
            UNINSTALL_KEY,
            Some("EstimatedSize"),
            Value::Dword(install.size_kib),
        )
        .set(UNINSTALL_KEY, Some("NoModify"), Value::Dword(1))
        .set(UNINSTALL_KEY, Some("NoRepair"), Value::Dword(1));

    // Run box / `start copper`.
    w.set(APP_PATHS_KEY, None, s(&install.exe))
        .set(APP_PATHS_KEY, Some("Path"), s(&install.dir));

    // ProgIDs: what a .html file or an https link runs when Copper is chosen.
    for (progid, friendly) in [
        (PROGID_HTML, "Copper HTML Document"),
        (PROGID_URL, "Copper URL"),
    ] {
        let root = format!(r"Software\Classes\{progid}");
        w.set(&root, None, s(friendly))
            .set(&root, Some("FriendlyTypeName"), s(friendly));
        if progid == PROGID_URL {
            w.set(&root, Some("URL Protocol"), s(""));
        }
        let app = format!(r"{root}\Application");
        w.set(&app, Some("AppUserModelID"), s(APP_USER_MODEL_ID))
            .set(&app, Some("ApplicationName"), s(APP_NAME))
            .set(&app, Some("ApplicationDescription"), s(DESCRIPTION))
            .set(&app, Some("ApplicationIcon"), s(&icon))
            .set(&app, Some("ApplicationCompany"), s(PUBLISHER));
        w.set(&format!(r"{root}\DefaultIcon"), None, s(&icon));
        w.set(&format!(r"{root}\shell\open\command"), None, s(&open));
    }

    // The browser itself, as Start menu and Default apps know browsers.
    w.set(CLIENT_KEY, None, s(APP_NAME));
    w.set(&format!(r"{CLIENT_KEY}\DefaultIcon"), None, s(&icon));
    w.set(
        &format!(r"{CLIENT_KEY}\shell\open\command"),
        None,
        s(install.quoted_exe()),
    );
    w.set(CAPABILITIES_KEY, Some("ApplicationName"), s(APP_NAME))
        .set(
            CAPABILITIES_KEY,
            Some("ApplicationDescription"),
            s(DESCRIPTION),
        )
        .set(CAPABILITIES_KEY, Some("ApplicationIcon"), s(&icon));
    let files = format!(r"{CAPABILITIES_KEY}\FileAssociations");
    for ext in FILE_TYPES {
        w.set(&files, Some(ext), s(PROGID_HTML));
    }
    let urls = format!(r"{CAPABILITIES_KEY}\URLAssociations");
    for scheme in URL_SCHEMES {
        w.set(&urls, Some(scheme), s(PROGID_URL));
    }
    w.set(
        &format!(r"{CAPABILITIES_KEY}\StartMenu"),
        Some("StartMenuInternet"),
        s(APP_NAME),
    );
    w.set(REGISTERED_APPS_KEY, Some(APP_NAME), s(CAPABILITIES_KEY));

    // Open with › Copper.
    for ext in FILE_TYPES {
        w.set(&open_with_key(ext), Some(PROGID_HTML), s(""));
    }
    w.0
}

fn open_with_key(ext: &str) -> String {
    format!(r"Software\Classes\{ext}\OpenWithProgids")
}

/// Everything [`writes`] adds, as removals: whole keys Copper owns, single
/// values in keys it shares.
pub fn removals() -> Vec<Removal> {
    let mut out = vec![
        Removal::Tree(UNINSTALL_KEY.to_owned()),
        Removal::Tree(APP_PATHS_KEY.to_owned()),
        Removal::Tree(CLIENT_KEY.to_owned()),
        Removal::Tree(format!(r"Software\Classes\{PROGID_HTML}")),
        Removal::Tree(format!(r"Software\Classes\{PROGID_URL}")),
        Removal::Value {
            key: REGISTERED_APPS_KEY.to_owned(),
            name: APP_NAME.to_owned(),
        },
    ];
    out.extend(FILE_TYPES.iter().map(|ext| Removal::Value {
        key: open_with_key(ext),
        name: PROGID_HTML.to_owned(),
    }));
    out
}

/// The `ms-settings:` page where the user makes Copper the default browser.
/// Windows 10/11 don't let an app set itself as default; this page, opened
/// on Copper's own entry, is the one sanctioned way.
pub const DEFAULT_APPS_URI: &str = "ms-settings:defaultapps?registeredAppUser=Copper";

#[cfg(test)]
mod tests {
    use super::*;

    fn install() -> Install {
        Install {
            dir: r"C:\Users\Ada Lovelace\AppData\Local\Programs\Copper".into(),
            exe: r"C:\Users\Ada Lovelace\AppData\Local\Programs\Copper\copper.exe".into(),
            size_kib: 9_216,
            date: "20261019".into(),
        }
    }

    fn get<'a>(w: &'a [Write], key: &str, name: Option<&str>) -> &'a Value {
        &w.iter()
            .find(|x| x.key == key && x.name.as_deref() == name)
            .unwrap_or_else(|| panic!("missing {key} / {name:?}"))
            .value
    }

    fn str_of<'a>(w: &'a [Write], key: &str, name: Option<&str>) -> &'a str {
        match get(w, key, name) {
            Value::Str(s) => s,
            other => panic!("{key} / {name:?} is {other:?}"),
        }
    }

    #[test]
    fn commands_quote_the_exe() {
        let i = install();
        let exe = r#""C:\Users\Ada Lovelace\AppData\Local\Programs\Copper\copper.exe""#;
        assert_eq!(i.open_command(), format!("{exe} --single-argument %1"));
        assert_eq!(i.uninstall_command(), format!("{exe} --uninstall"));
        assert_eq!(
            i.quiet_uninstall_command(),
            format!("{exe} --uninstall --silent")
        );
        assert_eq!(
            i.icon(),
            r"C:\Users\Ada Lovelace\AppData\Local\Programs\Copper\copper.exe,0"
        );
    }

    #[test]
    fn uninstall_entry_is_complete() {
        let i = install();
        let w = writes(&i);
        let k = UNINSTALL_KEY;
        assert_eq!(str_of(&w, k, Some("DisplayName")), "Copper");
        assert_eq!(str_of(&w, k, Some("DisplayIcon")), i.icon());
        assert_eq!(
            str_of(&w, k, Some("DisplayVersion")),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(str_of(&w, k, Some("Publisher")), "Copper");
        assert_eq!(str_of(&w, k, Some("InstallLocation")), i.dir);
        assert_eq!(
            str_of(&w, k, Some("UninstallString")),
            i.uninstall_command()
        );
        assert_eq!(
            str_of(&w, k, Some("QuietUninstallString")),
            i.quiet_uninstall_command()
        );
        assert_eq!(get(&w, k, Some("EstimatedSize")), &Value::Dword(9_216));
        assert_eq!(get(&w, k, Some("NoModify")), &Value::Dword(1));
        assert_eq!(get(&w, k, Some("NoRepair")), &Value::Dword(1));
    }

    #[test]
    fn browser_registration_points_at_capabilities() {
        let i = install();
        let w = writes(&i);
        assert_eq!(
            str_of(&w, REGISTERED_APPS_KEY, Some("Copper")),
            r"Software\Clients\StartMenuInternet\Copper\Capabilities"
        );
        assert_eq!(
            str_of(&w, CAPABILITIES_KEY, Some("ApplicationName")),
            "Copper"
        );
        assert_eq!(
            str_of(&w, CAPABILITIES_KEY, Some("ApplicationIcon")),
            i.icon()
        );
        assert_eq!(
            str_of(
                &w,
                &format!(r"{CAPABILITIES_KEY}\StartMenu"),
                Some("StartMenuInternet")
            ),
            "Copper"
        );
        for ext in FILE_TYPES {
            assert_eq!(
                str_of(
                    &w,
                    &format!(r"{CAPABILITIES_KEY}\FileAssociations"),
                    Some(ext)
                ),
                "CopperHTML"
            );
        }
        for scheme in ["http", "https"] {
            assert_eq!(
                str_of(
                    &w,
                    &format!(r"{CAPABILITIES_KEY}\URLAssociations"),
                    Some(scheme)
                ),
                "CopperURL"
            );
        }
        assert!(
            !w.iter().any(|x| x.name.as_deref() == Some("mailto")),
            "Copper is not a mail client"
        );
    }

    #[test]
    fn progids_open_with_single_argument() {
        let i = install();
        let w = writes(&i);
        for progid in ["CopperHTML", "CopperURL"] {
            let cmd = format!(r"Software\Classes\{progid}\shell\open\command");
            assert_eq!(str_of(&w, &cmd, None), i.open_command());
            assert_eq!(
                str_of(
                    &w,
                    &format!(r"Software\Classes\{progid}\Application"),
                    Some("AppUserModelID")
                ),
                "Copper.Browser"
            );
        }
        assert_eq!(
            str_of(&w, r"Software\Classes\CopperURL", Some("URL Protocol")),
            ""
        );
    }

    #[test]
    fn app_paths() {
        let i = install();
        let w = writes(&i);
        assert_eq!(str_of(&w, APP_PATHS_KEY, None), i.exe);
        assert_eq!(str_of(&w, APP_PATHS_KEY, Some("Path")), i.dir);
    }

    #[test]
    fn every_write_is_undone() {
        // Each written key is either inside a removed tree, or its value is
        // removed one by one.
        let removals = removals();
        for write in writes(&install()) {
            let covered = removals.iter().any(|r| match r {
                Removal::Tree(t) => {
                    write.key.eq_ignore_ascii_case(t)
                        || write
                            .key
                            .to_ascii_lowercase()
                            .starts_with(&format!("{}\\", t.to_ascii_lowercase()))
                }
                Removal::Value { key, name } => {
                    write.key == *key && write.name.as_deref() == Some(name.as_str())
                }
            });
            assert!(covered, "not removed on uninstall: {write:?}");
        }
    }

    #[test]
    fn shared_keys_are_never_deleted_whole() {
        for r in removals() {
            if let Removal::Tree(t) = r {
                assert!(!t.contains("OpenWithProgids"), "{t}");
                assert!(!t.eq_ignore_ascii_case(REGISTERED_APPS_KEY), "{t}");
                assert!(t.contains("Copper") || t.contains("copper"), "{t}");
            }
        }
    }
}
