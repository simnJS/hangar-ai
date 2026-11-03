//! PATH repair for a window that was not opened from a shell.
//!
//! On Windows an installer writes to the registry and every process sees the
//! same PATH. On macOS it does not work that way: a bundled app is started by
//! `launchd`, which hands it the bare system PATH — `/usr/bin:/bin:/usr/sbin:
//! /sbin` — and nothing a developer installs lives there. Homebrew, npm, cargo
//! and bun all put their binaries somewhere the shell adds on login, so the app
//! opens with every agent greyed out as "not installed" and half the shells
//! missing, on a machine where all of them are one `which` away.
//!
//! The user's login shell is the authority on what their PATH is, so it gets
//! asked once and its answer goes in front. Well-known install directories are
//! appended as a floor, for the machine whose shell cannot be probed at all —
//! an unsupported shell, or an rc file that hangs.
//!
//! Doing this to our own process is what makes it reach everything downstream:
//! shell detection, agent detection, and the panes themselves, which inherit
//! the environment we were fixed with.

#[cfg(unix)]
use std::collections::HashSet;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Runs the repair once per process. Every entry point that reads PATH calls
/// this first, so the order the frontend happens to issue its commands in
/// cannot decide whether detection works.
pub fn ensure() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(apply);
}

#[cfg(not(unix))]
fn apply() {}

#[cfg(unix)]
fn apply() {
    let mut merged: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    // The login shell leads: it is the PATH the user actually works in, and the
    // one their agents were installed into.
    if let Some(answer) = login_shell_path() {
        push_all(std::env::split_paths(&answer), &mut merged, &mut seen);
    }
    if let Some(current) = std::env::var_os("PATH") {
        push_all(std::env::split_paths(&current), &mut merged, &mut seen);
    }
    push_all(well_known(), &mut merged, &mut seen);

    if let Ok(joined) = std::env::join_paths(&merged) {
        std::env::set_var("PATH", joined);
    }
}

/// Keeps directories that exist, in the order first seen. A PATH entry that is
/// not there is noise every later lookup would walk over.
#[cfg(unix)]
fn push_all(
    candidates: impl IntoIterator<Item = PathBuf>,
    merged: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    for path in candidates {
        if path.as_os_str().is_empty() || !path.is_dir() {
            continue;
        }
        if seen.insert(path.clone()) {
            merged.push(path);
        }
    }
}

#[cfg(unix)]
fn well_known() -> Vec<PathBuf> {
    // Homebrew moved to /opt/homebrew on Apple Silicon and kept /usr/local on
    // Intel, so both are worth having.
    let mut paths: Vec<PathBuf> = ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"]
        .iter()
        .map(PathBuf::from)
        .collect();

    if let Some(home) = dirs::home_dir() {
        for relative in [
            ".local/bin",
            ".cargo/bin",
            ".bun/bin",
            ".deno/bin",
            ".volta/bin",
            ".npm-global/bin",
            "go/bin",
        ] {
            paths.push(home.join(relative));
        }
    }

    paths
}

/// Wraps the answer so whatever an rc file prints on the way — a banner, an
/// instant prompt, a shell integration escape sequence — can be cut away.
#[cfg(unix)]
const MARKER: &str = "__HANGAR_PATH__";

/// How long the shell gets before we take the well-known directories instead.
/// An interactive rc that blocks would otherwise hold the first detection call
/// for as long as it felt like.
#[cfg(unix)]
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);

/// The one-liner that prints the PATH, in the dialect of the shell running it.
///
/// `fish` keeps `$PATH` as a list rather than a colon-joined string, so it
/// needs its own spelling. A shell we do not know how to ask — nushell, elvish
/// — returns `None` and the caller falls back rather than running a line that
/// would mean something else there.
///
/// The braces around `PATH` are not decoration. A marker made of word
/// characters runs straight into the name, and `$PATH__HANGAR_PATH__` is a
/// perfectly valid reference to a variable nobody ever set — which expands to
/// nothing, quietly, on every machine.
#[cfg(unix)]
fn script_for(shell: &Path) -> Option<String> {
    match shell.file_name()?.to_str()? {
        "fish" => Some(format!(
            r#"printf '%s' "{MARKER}"(string join : $PATH)"{MARKER}""#
        )),
        "sh" | "bash" | "zsh" | "ksh" | "ksh93" | "mksh" | "dash" | "ash" => {
            Some(format!(r#"printf '%s' "{MARKER}${{PATH}}{MARKER}""#))
        }
        _ => None,
    }
}

/// Asks the login shell for its PATH, or `None` if it could not be asked.
///
/// `-l -i` because the two halves of a shell's configuration hide in different
/// files: Homebrew's `shellenv` is conventionally in `~/.zprofile`, which only
/// a login shell reads, while version managers like nvm and rbenv initialise
/// from `~/.zshrc`, which only an interactive one does. Asking for both is what
/// makes the answer match the terminal the user would have typed in.
#[cfg(unix)]
fn login_shell_path() -> Option<std::ffi::OsString> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::{mpsc, Arc, Mutex};

    let shell = PathBuf::from(std::env::var_os("SHELL")?);
    let script = script_for(&shell)?;

    // The child is parked where the timeout below can reach it. Reading its
    // output happens on a cloned handle, so the shell can be killed while the
    // worker is still blocked on a read that will never finish.
    let child = Arc::new(Mutex::new(None::<std::process::Child>));
    let worker_child = child.clone();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let spawned = Command::new(&shell)
            .args(["-l", "-i", "-c", &script])
            // A shell that reads from stdin on startup gets EOF rather than
            // waiting on a terminal that is not there.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        let Ok(mut spawned) = spawned else {
            let _ = tx.send(None);
            return;
        };
        let Some(mut stdout) = spawned.stdout.take() else {
            let _ = tx.send(None);
            return;
        };
        *worker_child.lock().unwrap() = Some(spawned);

        let mut output = String::new();
        let read = stdout.read_to_string(&mut output).is_ok();

        // Taken back out before waiting, so a slow exit does not hold the lock
        // the timeout path needs to kill it.
        if let Some(mut spawned) = worker_child.lock().unwrap().take() {
            let _ = spawned.wait();
        }
        let _ = tx.send(read.then_some(output));
    });

    let answer = rx.recv_timeout(PROBE_TIMEOUT).ok().flatten();
    if answer.is_none() {
        if let Some(spawned) = child.lock().unwrap().as_mut() {
            let _ = spawned.kill();
        }
    }

    let output = answer?;
    let value = between_markers(&output)?;
    Some(std::ffi::OsString::from(value))
}

/// The text between the two markers, or `None` when the shell printed neither.
#[cfg(unix)]
fn between_markers(output: &str) -> Option<&str> {
    let (_, rest) = output.split_once(MARKER)?;
    let (value, _) = rest.split_once(MARKER)?;
    (!value.trim().is_empty()).then(|| value.trim())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn reads_the_path_out_of_a_noisy_answer() {
        let output =
            format!("\u{1b}]633;A\u{7}welcome!\n{MARKER}/opt/homebrew/bin:/usr/bin{MARKER}");
        assert_eq!(between_markers(&output), Some("/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn ignores_an_answer_that_never_arrived() {
        assert_eq!(between_markers("command not found"), None);
        assert_eq!(between_markers(&format!("{MARKER}   {MARKER}")), None);
    }

    /// Runs the script the way the probe does, against a PATH we set, and
    /// checks the answer comes back whole. A shell quietly expanding the
    /// wrong variable name reads exactly like a machine with nothing
    /// installed, so this is worth proving rather than eyeballing.
    #[test]
    fn the_posix_script_prints_the_path_it_was_given() {
        let shell = Path::new("/bin/sh");
        let script = script_for(shell).expect("/bin/sh is a shell we know");
        let output = std::process::Command::new(shell)
            .args(["-c", &script])
            .env("PATH", "/somewhere/bin:/else/bin")
            .output()
            .expect("/bin/sh runs");

        let text = String::from_utf8_lossy(&output.stdout);
        assert_eq!(between_markers(&text), Some("/somewhere/bin:/else/bin"));
    }

    #[test]
    fn only_speaks_to_shells_it_knows() {
        assert!(script_for(Path::new("/bin/zsh")).is_some());
        assert!(script_for(Path::new("/opt/homebrew/bin/fish")).is_some());
        assert!(script_for(Path::new("/opt/homebrew/bin/nu")).is_none());
    }
}
