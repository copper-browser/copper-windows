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

#[cfg(test)]
mod tests {
    use super::*;

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
