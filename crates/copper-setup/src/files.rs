//! Names and command lines for replacing and removing copper.exe.
//!
//! Windows will not overwrite or delete an exe that is running, but it will
//! rename one. So an upgrade moves the old `copper.exe` aside to
//! `copper.exe.<n>.old`, puts the new one in its place, and the next run
//! sweeps the leftovers. An uninstall that runs from the installed exe can't
//! delete itself either; it leaves a short-lived `cmd.exe` behind that waits
//! for it to exit and then removes the folder.
//!
//! Platform-free so the exact strings are tested on Linux.

/// The installed exe's file name.
pub const EXE_NAME: &str = "copper.exe";
/// Where a new exe is written before it is renamed into place.
pub const STAGED_NAME: &str = "copper.exe.new";

/// The `n`th name to move a running exe aside to.
pub fn aside_name(n: u32) -> String {
    format!("{EXE_NAME}.{n}.old")
}

/// Is `name` something an earlier install or upgrade left behind that is
/// safe to delete: a moved-aside exe or a half-written new one?
pub fn is_leftover(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == STAGED_NAME {
        return true;
    }
    lower
        .strip_prefix("copper.exe.")
        .and_then(|rest| rest.strip_suffix(".old"))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// How long the self-delete waits for this process to exit, in seconds.
pub const SELF_DELETE_DELAY_S: u32 = 3;

/// Arguments for `cmd.exe` that wait a moment, then delete `exe` and remove
/// `dir` (if it is empty by then, or with everything left in it).
///
/// Passed verbatim (raw, not re-quoted) after `cmd.exe`: `/d` skips AutoRun,
/// `/c` runs one line. With more than two quotes on the line, cmd strips the
/// first and the last, so the whole command is wrapped in one extra pair.
/// `ping` is the delay because `timeout` refuses to run without a console
/// input. Paths are quoted; a path holding a `"` cannot exist on Windows.
pub fn self_delete_args(exe: &str, dir: &str) -> String {
    // ping -n N waits N-1 seconds.
    let pings = SELF_DELETE_DELAY_S + 1;
    format!(
        "/d /c \"ping -n {pings} 127.0.0.1 >nul & del /f /q \"{exe}\" & rmdir /s /q \"{dir}\"\""
    )
}

/// Is `path` inside the folder `dir`, the way NTFS compares names
/// (case-insensitively; `/` and `\` alike; a trailing separator on `dir`
/// or not)? The folder itself is not inside itself, and
/// `…\Copper2\copper.exe` is not inside `…\Copper`.
///
/// Decides which running Copper an upgrade must close (only one started
/// from the install folder holds the files being replaced) and whether the
/// uninstaller is the installed exe.
pub fn is_inside(path: &str, dir: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").to_lowercase();
    let (p, d) = (norm(path), norm(dir));
    let d = d.trim_end_matches('\\');
    !d.is_empty() && p.len() > d.len() + 1 && p.starts_with(d) && p.as_bytes()[d.len()] == b'\\'
}

/// What to tell the user when putting Copper's files in place failed, in
/// words they can act on. The full error, codes and all, goes to the log.
pub fn failure_sentence(error: &anyhow::Error) -> &'static str {
    // Windows error codes (also what `raw_os_error` holds for them).
    const ACCESS_DENIED: i32 = 5;
    const SHARING_VIOLATION: i32 = 32;
    const LOCK_VIOLATION: i32 = 33;
    const HANDLE_DISK_FULL: i32 = 39;
    const DISK_FULL: i32 = 112;
    let code = error
        .chain()
        .find_map(|e| e.downcast_ref::<std::io::Error>())
        .and_then(std::io::Error::raw_os_error);
    match code {
        Some(SHARING_VIOLATION | LOCK_VIOLATION) => {
            "Another program is using Copper's files. Close it, then try again."
        }
        Some(ACCESS_DENIED) => {
            "Windows wouldn't let setup change Copper's files. Close anything using Copper, then try again."
        }
        Some(DISK_FULL | HANDLE_DISK_FULL) => {
            "There isn't enough space on this disk. Free some up, then try again."
        }
        _ => "Something went wrong. The log has the details.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_the_install_folder() {
        let dir = r"C:\Users\Ada\AppData\Local\Programs\Copper";
        for p in [
            r"C:\Users\Ada\AppData\Local\Programs\Copper\copper.exe",
            r"c:\users\ada\appdata\local\programs\copper\COPPER.EXE",
            r"C:\Users\Ada\AppData\Local\Programs\Copper\copper.exe.0.old",
            "C:/Users/Ada/AppData/Local/Programs/Copper/copper.exe",
        ] {
            assert!(is_inside(p, dir), "{p}");
            assert!(is_inside(p, &format!("{dir}\\")), "{p} (trailing \\)");
        }
        for p in [
            r"C:\Users\Ada\AppData\Local\Programs\Copper",
            r"C:\Users\Ada\AppData\Local\Programs\Copper\",
            r"C:\Users\Ada\AppData\Local\Programs\Copper2\copper.exe",
            r"C:\Users\Public\app-shell\copper-appshell.exe",
            r"D:\Copper\copper.exe",
        ] {
            assert!(!is_inside(p, dir), "{p}");
        }
        assert!(!is_inside(r"C:\x.exe", ""));
    }

    #[test]
    fn failures_in_plain_words() {
        use anyhow::Context;
        let io = |code| -> anyhow::Error {
            Err::<(), _>(std::io::Error::from_raw_os_error(code))
                .context("couldn't move the old Copper aside")
                .unwrap_err()
        };
        assert!(failure_sentence(&io(32)).starts_with("Another program"));
        assert!(failure_sentence(&io(33)).starts_with("Another program"));
        assert!(failure_sentence(&io(5)).starts_with("Windows wouldn't"));
        assert!(failure_sentence(&io(112)).starts_with("There isn't enough space"));
        assert!(failure_sentence(&io(2)).starts_with("Something went wrong"));
        assert!(failure_sentence(&anyhow::anyhow!("no io here")).starts_with("Something went"));
        // No codes or OS wording ever reach the window.
        for code in [2, 5, 32, 112] {
            assert!(!failure_sentence(&io(code)).contains("error"));
        }
    }

    #[test]
    fn aside_names_are_leftovers() {
        for n in [0, 1, 7, 4_000_000_000] {
            assert!(is_leftover(&aside_name(n)), "{}", aside_name(n));
        }
        assert!(is_leftover("copper.exe.new"));
        assert!(is_leftover("COPPER.EXE.3.OLD"));
    }

    #[test]
    fn real_files_are_not_leftovers() {
        for name in [
            "copper.exe",
            "copper.exe.old",
            "copper.exe..old",
            "copper.exe.x.old",
            "copper.exe.1.old.exe",
            "copper.ico",
            "notes.old",
            "copper.exe.new.bak",
        ] {
            assert!(!is_leftover(name), "{name}");
        }
    }

    #[test]
    fn self_delete_quotes_paths_with_spaces() {
        let exe = r"C:\Users\Ada Lovelace\AppData\Local\Programs\Copper\copper.exe";
        let dir = r"C:\Users\Ada Lovelace\AppData\Local\Programs\Copper";
        assert_eq!(
            self_delete_args(exe, dir),
            "/d /c \"ping -n 4 127.0.0.1 >nul & del /f /q \"C:\\Users\\Ada Lovelace\\AppData\\Local\\Programs\\Copper\\copper.exe\" & rmdir /s /q \"C:\\Users\\Ada Lovelace\\AppData\\Local\\Programs\\Copper\"\""
        );
    }
}
