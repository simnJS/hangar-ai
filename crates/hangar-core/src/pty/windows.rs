//! The folder a pane's shell starts in, on Windows.
//!
//! `CreateProcessW` refuses a current directory longer than 258 UTF-16 units
//! (`MAX_PATH`, less the backslash it may append and the NUL), whatever
//! `LongPathsEnabled` says: the pane then shows "Failed to start shell: …
//! The directory name is invalid. (os error 267)" and has no shell at all.
//! A workspace deep in a monorepo, a worktree under it, a `node_modules`
//! folder opened in a pane: such a folder is opened as the person's other
//! folders are, so a pane must start there too.
//!
//! The same folder under its 8.3 short name (`C:\bench\long\SEGMEN~1\…`) is
//! usually well under the limit, and is the folder itself: the shell starts
//! in it and everything it runs sees it. Only a folder too long for the call
//! is given its short form; one that has none short enough (8.3 names off on
//! its volume) is passed as it is, so the error the pane shows stays the
//! system's own.
//!
//! The choice is plain string work and runs everywhere, so its tests do; only
//! the lookup of the short name is Windows'.

use std::path::{Path, PathBuf};

/// The longest current directory `CreateProcessW` starts a process in, in
/// UTF-16 units (measured on Windows 11 26100: 258 starts, 259 is refused).
pub(super) const LONGEST_CWD: usize = 258;

/// Where to start a shell for `cwd`: `cwd` itself when the system takes it,
/// else its short form when that one fits, else `cwd` again.
#[cfg(windows)]
pub(super) fn launch_cwd(cwd: &Path) -> PathBuf {
    choose(cwd, short_form)
}

/// `launch_cwd` with the short-name lookup given, so it can be tested
/// anywhere. `short` is asked only for a folder too long to start in.
#[cfg_attr(not(windows), allow(dead_code))]
fn choose(cwd: &Path, short: impl FnOnce(&Path) -> Option<PathBuf>) -> PathBuf {
    if fits(cwd) {
        return cwd.to_path_buf();
    }
    match short(cwd) {
        Some(short) if fits(&short) => short,
        _ => cwd.to_path_buf(),
    }
}

/// Whether `path` is short enough to start a process in.
#[cfg_attr(not(windows), allow(dead_code))]
fn fits(path: &Path) -> bool {
    utf16_len(path) <= LONGEST_CWD
}

/// `path`'s length as the system counts it. Lossy is exact here: what a
/// lone surrogate becomes is one unit, as the surrogate was.
#[cfg_attr(not(windows), allow(dead_code))]
fn utf16_len(path: &Path) -> usize {
    path.as_os_str().to_string_lossy().encode_utf16().count()
}

/// `path` in the `\\?\` form, which `GetShortPathNameW` takes at any length:
/// `C:\a` as `\\?\C:\a`, `\\server\share\a` as `\\?\UNC\server\share\a`, one
/// already in it as it is. `None` for a path that is not absolute, which has
/// no such form.
#[cfg_attr(not(windows), allow(dead_code))]
fn verbatim(path: &str) -> Option<String> {
    if path.starts_with(r"\\?\") {
        return Some(path.to_string());
    }
    if let Some(unc) = path.strip_prefix(r"\\") {
        return (!unc.is_empty()).then(|| format!(r"\\?\UNC\{unc}"));
    }
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive.then(|| format!(r"\\?\{}", path.replace('/', r"\")))
}

/// `verbatim` undone: the form a current directory is given in.
#[cfg_attr(not(windows), allow(dead_code))]
fn unverbatim(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(local) = path.strip_prefix(r"\\?\") {
        local.to_string()
    } else {
        path.to_string()
    }
}

/// `path` under its 8.3 short name, `None` when the system has none for it
/// (the folder is gone, or cannot be read).
#[cfg(windows)]
fn short_form(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    // kernel32 is linked by every Windows program; declared here rather than
    // through windows-sys, whose `Win32_Storage_FileSystem` set is not
    // compiled for this crate.
    #[link(name = "kernel32")]
    extern "system" {
        fn GetShortPathNameW(long: *const u16, short: *mut u16, len: u32) -> u32;
    }

    let long = verbatim(&path.to_string_lossy())?;
    let wide: Vec<u16> = std::ffi::OsStr::new(&long)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut buffer = vec![0u16; 512];
    loop {
        let len = u32::try_from(buffer.len()).ok()?;
        // SAFETY: `wide` ends in a NUL, and `buffer` has the `len` units the
        // call is told it may write.
        let written = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), len) };
        match written as usize {
            0 => return None,
            // Too small: `written` is the size it needs, NUL included.
            needed if needed > buffer.len() => buffer.resize(needed, 0),
            done => {
                buffer.truncate(done);
                let short = std::ffi::OsString::from_wide(&buffer);
                return Some(PathBuf::from(unverbatim(&short.to_string_lossy())));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute folder of exactly `len` units.
    fn folder_of(len: usize) -> PathBuf {
        let base = r"C:\work\";
        PathBuf::from(format!("{base}{}", "x".repeat(len - base.len())))
    }

    #[test]
    fn a_folder_the_system_takes_is_passed_as_it_is() {
        let cwd = folder_of(LONGEST_CWD);
        let chosen = choose(&cwd, |_| panic!("no short name is needed"));
        assert_eq!(chosen, cwd);
        let short = PathBuf::from(r"C:\Users\me\code");
        assert_eq!(choose(&short, |_| panic!("no short name is needed")), short);
    }

    #[test]
    fn a_folder_too_long_starts_in_its_short_form() {
        let cwd = folder_of(LONGEST_CWD + 1);
        let short = PathBuf::from(r"C:\work\XXXXXX~1");
        assert_eq!(choose(&cwd, |_| Some(short.clone())), short);
    }

    #[test]
    fn without_a_short_form_that_fits_the_folder_stays_as_it_was() {
        let cwd = folder_of(300);
        assert_eq!(choose(&cwd, |_| None), cwd);
        // 8.3 names off on the volume: the "short" form is as long.
        assert_eq!(choose(&cwd, |long| Some(long.to_path_buf())), cwd);
        assert_eq!(choose(&cwd, |_| Some(folder_of(LONGEST_CWD + 1))), cwd);
    }

    #[test]
    fn the_length_is_counted_in_utf16_units() {
        // "é" is two bytes in UTF-8 and one unit in UTF-16; "𝄞" four and two.
        let base = r"C:\w\";
        let accented = PathBuf::from(format!("{base}{}", "é".repeat(LONGEST_CWD - base.len())));
        assert!(fits(&accented));
        let astral = PathBuf::from(format!(
            "{base}{}",
            "𝄞".repeat((LONGEST_CWD - base.len()) / 2 + 1)
        ));
        assert!(!fits(&astral));
    }

    #[test]
    fn the_verbatim_form_and_back() {
        assert_eq!(verbatim(r"C:\a\b").as_deref(), Some(r"\\?\C:\a\b"));
        assert_eq!(verbatim("c:/a/b").as_deref(), Some(r"\\?\c:\a\b"));
        assert_eq!(
            verbatim(r"\\server\share\a").as_deref(),
            Some(r"\\?\UNC\server\share\a")
        );
        assert_eq!(verbatim(r"\\?\C:\a").as_deref(), Some(r"\\?\C:\a"));
        assert_eq!(verbatim(r"a\b"), None);
        assert_eq!(verbatim(r"C:a"), None);
        assert_eq!(verbatim(r"\\"), None);

        assert_eq!(unverbatim(r"\\?\C:\A\B~1"), r"C:\A\B~1");
        assert_eq!(
            unverbatim(r"\\?\UNC\server\share\A~1"),
            r"\\server\share\A~1"
        );
        assert_eq!(unverbatim(r"C:\a"), r"C:\a");
    }

    /// The real thing: a folder deeper than the limit, which a process
    /// cannot be started in as it is, can through what `launch_cwd` gives.
    #[cfg(windows)]
    #[test]
    fn a_shell_starts_in_a_folder_longer_than_max_path() {
        let root = tempfile::tempdir().unwrap();
        let mut deep = root.path().to_path_buf();
        while utf16_len(&deep) <= LONGEST_CWD + 20 {
            deep.push("a-folder-name-well-past-eight-dot-three");
        }
        std::fs::create_dir_all(&deep).unwrap();

        let cmd = |dir: &Path| {
            std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "exit"])
                .current_dir(dir)
                .status()
        };
        assert!(
            cmd(&deep).is_err(),
            "the system took {} units",
            utf16_len(&deep)
        );

        let chosen = launch_cwd(&deep);
        if chosen == deep {
            // 8.3 names are off on the temp folder's volume: nothing to do.
            eprintln!("no short names on this volume; skipped");
            return;
        }
        assert!(fits(&chosen), "{}", chosen.display());
        assert!(chosen.is_dir());
        assert!(cmd(&chosen).unwrap().success());
    }
}
