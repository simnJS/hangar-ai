//! `command.run`: a kit's tab running a command its kit declares, on tools
//! already on the machine (`gh run view`, `docker ps`), without a network, a
//! token or a shell of its own (docs/kits.md).
//!
//! A kit is code written by others, and a command runs with the person's
//! logins, so what it can do is narrow on purpose:
//!
//! - **No shell, ever.** An argv, handed to the program as it is. The program
//!   is a name found on the PATH, not in the project's folder, and one that
//!   turns out to be a batch file on Windows is refused: cmd.exe would parse
//!   its arguments, which brings a shell back.
//! - **Values fit their rule, whole.** A value is checked against the pattern
//!   the kit declared for it, anchored whatever its author wrote, and one that
//!   starts with `-` is refused whatever the pattern says: a parameter never
//!   becomes an option.
//! - **Bound in time, in output and in number.** A timeout that kills the
//!   tree, 1 MB of each stream, four at once per kit.
//!
//! The window checks the tab holds `command:<name>` (src/lib/kitApi.ts). The
//! manifest is read again at each call, and a GitHub kit's commands have to be
//! the very ones the window says were accepted, so an update cannot slip a new
//! argv under a grant that was given for another.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::manifest::{self, ArgPart, CommandArg, Manifest};
use super::setup::sha256_hex;
use super::store::{Loaded, Source};
use super::{off_thread, TabError};

/// The most kept of each of stdout and stderr, in bytes.
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
/// The most commands of one kit running at once.
pub const MAX_RUNNING: usize = 4;

/// How often a running command is looked at.
const POLL: Duration = Duration::from_millis(20);
/// How long its output is waited for once it ended. A pipe a process it left
/// behind still holds would otherwise keep the call open as long as that lives.
const DRAIN: Duration = Duration::from_millis(500);
/// What Windows tries when `PATHEXT` is not set.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What a command answered. A code that is not 0 is an answer, not a failure.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// Null when it did not end by itself: at its timeout, or by a signal.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    /// Whether stdout or stderr went past `MAX_OUTPUT_BYTES`.
    pub truncated: bool,
}

/// A command as the person is shown it: its argv for this OS, as written.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    /// The program, then the literal items and the placeholders, verbatim.
    pub argv: Vec<String>,
    /// The rule of each placeholder of that argv.
    pub args: BTreeMap<String, CommandArg>,
    pub timeout_seconds: u64,
}

/// The kit's commands that have an argv for `os`, by name.
pub fn for_os(manifest: &Manifest, os: &str) -> Vec<CommandInfo> {
    manifest
        .commands
        .iter()
        .filter_map(|(name, command)| {
            let argv = command.run.for_os(os)?;
            let args = argv
                .iter()
                .skip(1)
                .flat_map(|item| manifest::arg_parts(item))
                .filter_map(|part| match part {
                    ArgPart::Value(name) => {
                        Some((name.to_string(), command.args.get(name)?.clone()))
                    }
                    ArgPart::Text(_) => None,
                })
                .collect();
            Some(CommandInfo {
                name: name.clone(),
                description: command.description.clone(),
                argv: argv.to_vec(),
                args,
                timeout_seconds: command.timeout_seconds,
            })
        })
        .collect()
}

/// What decides what can run: the field order is the canonical form's.
#[derive(Serialize)]
struct Canonical<'a> {
    name: &'a str,
    run: &'a [String],
    args: BTreeMap<&'a str, &'a str>,
}

/// What accepting a kit's commands stores on a workspace:
/// `commands:<first 16 hex of the SHA-256 of the canonical list>`, for this OS.
/// `None` when it has none here, which stores nothing. A reworded description
/// or another timeout changes nothing that can run, and asks nothing.
///
/// A command that runs a file of the kit, `{kitDir}/tools/x.js`, runs what that
/// file holds: the content of every file the argvs name in `root` is hashed with
/// the list (code.rs), `[list, [[path, sha256], …]]`, so an update that changes
/// the script is asked about as one that changes the argv. A kit whose commands
/// name none of its files keeps the hash of the list alone.
pub fn grant(manifest: &Manifest, root: &Path, os: &str) -> Option<String> {
    let commands = for_os(manifest, os);
    if commands.is_empty() {
        return None;
    }
    let canonical: Vec<Canonical> = commands
        .iter()
        .map(|command| Canonical {
            name: &command.name,
            run: &command.argv,
            args: command
                .args
                .iter()
                .map(|(name, arg)| (name.as_str(), arg.pattern.as_str()))
                .collect(),
        })
        .collect();
    let code = super::code::files(
        root,
        commands
            .iter()
            .flat_map(|command| command.argv.iter().skip(1).map(String::as_str)),
        &super::code::KIT_DIR,
        &[],
    );
    let text = if code.is_empty() {
        serde_json::to_string(&canonical).ok()?
    } else {
        serde_json::to_string(&(&canonical, &code)).ok()?
    };
    Some(format!("commands:{}", &sha256_hex(&text)[..16]))
}

fn bad_args(what: impl Into<String>) -> TabError {
    TabError::new(TabError::BAD_ARGS, what)
}

/// One value against the rule of its placeholder.
fn check_value(name: &str, value: &str, arg: &CommandArg) -> Result<(), TabError> {
    if value.starts_with('-') {
        return Err(bad_args(format!(
            "{name} must not start with -: a value never becomes an option"
        )));
    }
    if value.contains('\0') {
        return Err(bad_args(format!("{name} must not hold a NUL character")));
    }
    let rule = manifest::whole_value_rule(&arg.pattern)
        .map_err(|why| TabError::new(TabError::FAILED, format!("{name}'s rule {why}")))?;
    if rule.is_match(value) {
        Ok(())
    } else {
        Err(bad_args(format!(
            "{name} does not match its rule {}",
            arg.pattern
        )))
    }
}

/// The arguments the program gets: every value given checked, and each
/// placeholder of the argv filled in once, so a value holding `{other}` stays
/// that text. `{kitDir}` is the kit's folder, in the item it is written in, so a
/// folder with spaces stays one argument.
fn expand(
    argv: &[String],
    declared: &BTreeMap<String, CommandArg>,
    given: &HashMap<String, String>,
    kit_dir: &str,
) -> Result<Vec<String>, TabError> {
    for (name, value) in given {
        let arg = declared
            .get(name)
            .ok_or_else(|| bad_args(format!("{name} is not an argument of this command")))?;
        check_value(name, value, arg)?;
    }
    argv.iter()
        .skip(1)
        .map(|item| {
            let mut out = String::with_capacity(item.len());
            for part in manifest::arg_parts(item) {
                match part {
                    ArgPart::Text(text) => out.push_str(text),
                    ArgPart::Value(manifest::KIT_DIR) => out.push_str(kit_dir),
                    ArgPart::Value(name) => out.push_str(
                        given
                            .get(name)
                            .ok_or_else(|| bad_args(format!("{name} is missing")))?,
                    ),
                }
            }
            // `{a}-{b}` with an empty `a`: no value starts with `-`, and the
            // item does all the same.
            if out.starts_with('-') && !item.starts_with('-') {
                return Err(bad_args(format!(
                    "{item} must not start with - once filled in: a value never becomes an option"
                )));
            }
            Ok(out)
        })
        .collect()
}

/// Where programs are looked for.
#[derive(Debug, Clone)]
pub struct Search {
    path: OsString,
    /// `PATHEXT`, where programs go by extensions and batch files exist.
    extensions: Option<String>,
}

impl Search {
    /// The PATH Hangar computes for everything it starts (path_env.rs).
    pub fn of_this_process() -> Self {
        crate::path_env::ensure();
        Search {
            path: std::env::var_os("PATH").unwrap_or_default(),
            extensions: cfg!(windows).then(|| {
                std::env::var("PATHEXT")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_PATHEXT.to_string())
            }),
        }
    }

    /// The first file of that name on the PATH. A relative or empty entry is
    /// passed over: it means the folder a command starts in, a project's, where
    /// anyone can leave a `gh`.
    fn find(&self, program: &str) -> Option<PathBuf> {
        let names = names_of(program, self.extensions.as_deref());
        std::env::split_paths(&self.path)
            .filter(|dir| dir.is_absolute())
            .find_map(|dir| {
                names
                    .iter()
                    .map(|name| dir.join(name))
                    .find(|candidate| runnable(candidate))
            })
    }

    /// The program to start. Absent is an ordinary state for a tab, `gh` not
    /// being installed, and nothing is logged.
    fn program(&self, name: &str) -> Result<PathBuf, TabError> {
        let found = self.find(name).ok_or_else(|| {
            TabError::new(
                TabError::NOT_FOUND,
                format!("{name} is not installed, or not on the PATH"),
            )
        })?;
        if self.extensions.is_some()
            && (is_batch(&found) || dunce::canonicalize(&found).is_ok_and(|real| is_batch(&real)))
        {
            return Err(TabError::new(
                TabError::FAILED,
                format!(
                    "{name} is {}, a batch file: cmd.exe would parse its arguments, so Hangar does not run it",
                    found.display()
                ),
            ));
        }
        Ok(found)
    }
}

/// What `program` may be called in a folder of the PATH. With extensions, as on
/// Windows, a name that has one may be the file itself; a bare name never is:
/// the `npm` beside `npm.cmd` is a script for sh, which Windows cannot start.
fn names_of(program: &str, extensions: Option<&str>) -> Vec<String> {
    let Some(extensions) = extensions else {
        return vec![program.to_string()];
    };
    let mut names = Vec::new();
    if Path::new(program).extension().is_some() {
        names.push(program.to_string());
    }
    names.extend(
        extensions
            .split(';')
            .map(str::trim)
            .filter(|extension| !extension.is_empty())
            .map(|extension| format!("{program}{extension}")),
    );
    names
}

#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Not followed: an app alias of the Store (`winget.exe`) is a link that cannot
/// be, and starts all the same.
#[cfg(windows)]
fn runnable(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| !meta.is_dir())
}

/// Whether Windows would hand this file to cmd.exe, which ignores the dots and
/// spaces a name ends in.
fn is_batch(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy().to_ascii_lowercase();
    let name = name.trim_end_matches(['.', ' ']);
    name.ends_with(".bat") || name.ends_with(".cmd")
}

fn running() -> MutexGuard<'static, HashMap<String, usize>> {
    static RUNNING: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    RUNNING
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One of a kit's places to run a command, given back however the call ends.
struct Slot(String);

impl Slot {
    fn take(kit: &str) -> Option<Slot> {
        let mut running = running();
        let count = running.entry(kit.to_string()).or_insert(0);
        if *count >= MAX_RUNNING {
            return None;
        }
        *count += 1;
        Some(Slot(kit.to_string()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut running = running();
        if let Some(count) = running.get_mut(&self.0) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                running.remove(&self.0);
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Kept {
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
}

/// A stream being read on a thread of its own. It is read to its end past the
/// cap: a program writing to a full pipe would otherwise stop right there, until
/// its timeout.
pub(super) struct Capture {
    kept: Arc<Mutex<Kept>>,
    ended: mpsc::Receiver<()>,
}

impl Capture {
    pub(super) fn start(mut stream: impl Read + Send + 'static) -> Self {
        let kept = Arc::new(Mutex::new(Kept::default()));
        let (done, ended) = mpsc::channel();
        let shared = kept.clone();
        std::thread::spawn(move || {
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        let mut kept = shared
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let room = MAX_OUTPUT_BYTES - kept.bytes.len();
                        kept.bytes.extend_from_slice(&chunk[..read.min(room)]);
                        kept.truncated |= read > room;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = done.send(());
        });
        Capture { kept, ended }
    }

    /// What came, once the stream ended or `until` is reached. What is still
    /// holding the pipe then is not waited for.
    pub(super) fn finish(self, until: Instant) -> Kept {
        let _ = self
            .ended
            .recv_timeout(until.saturating_duration_since(Instant::now()));
        let mut kept = self
            .kept
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *kept)
    }
}

/// As text. The cap may have cut a character in two, and half of one is not
/// something the program wrote.
pub(super) fn text_of(kept: Kept) -> (String, bool) {
    let Kept {
        mut bytes,
        truncated,
    } = kept;
    if truncated {
        if let Err(err) = std::str::from_utf8(&bytes) {
            if err.error_len().is_none() {
                bytes.truncate(err.valid_up_to());
            }
        }
    }
    (String::from_utf8_lossy(&bytes).into_owned(), truncated)
}

/// A process group of its own on unix, which is exactly its tree. No console
/// window on Windows: a tab polling would flash one every second.
#[cfg(unix)]
pub(super) fn prepare(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
pub(super) fn prepare(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Ends the command and everything below it. The process first, so it starts
/// nothing more. Holding its handle keeps its id from being given to another
/// process meanwhile; the process table finds what its group does not hold.
pub(super) fn kill_tree(child: &mut Child) {
    let root = child.id();
    let _ = child.kill();
    #[cfg(unix)]
    if let Ok(group) = libc::pid_t::try_from(root) {
        if group > 1 {
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }
    }
    if let Some(table) = crate::procs::table() {
        let below = crate::procs::descendants(&table, root, crate::procs::started);
        crate::procs::terminate(&below);
    }
    let _ = child.wait();
}

fn execute(
    program: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<Output, TabError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    prepare(&mut command);
    let mut child = command.spawn().map_err(|err| {
        let name = program.display();
        if err.kind() == std::io::ErrorKind::NotFound {
            TabError::new(TabError::NOT_FOUND, format!("{name} could not be started"))
        } else {
            TabError::new(TabError::FAILED, format!("{name}: {err}"))
        }
    })?;
    let stdout = Capture::start(child.stdout.take().expect("piped above"));
    let stderr = Capture::start(child.stderr.take().expect("piped above"));

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                kill_tree(&mut child);
                timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(err) => {
                kill_tree(&mut child);
                return Err(TabError::new(TabError::FAILED, err.to_string()));
            }
        }
    };

    let until = Instant::now() + DRAIN;
    let (stdout, stdout_cut) = text_of(stdout.finish(until));
    let (stderr, stderr_cut) = text_of(stderr.finish(until));
    Ok(Output {
        exit_code: status.and_then(|status| status.code()),
        stdout,
        stderr,
        timed_out,
        truncated: stdout_cut || stderr_cut,
    })
}

/// One call, as the window passes it on.
#[derive(Debug, Clone, Default)]
pub struct Call {
    pub name: String,
    pub args: HashMap<String, String>,
    /// The workspace folder of the tab.
    pub cwd: String,
    /// The `commands:<hash>` the window holds for this tab; null for a linked
    /// folder, which is not asked.
    pub accepted: Option<String>,
}

pub fn run(kit: &Loaded, call: &Call, os: &str, search: &Search) -> Result<Output, TabError> {
    if matches!(kit.entry.source, Source::Git(_)) && call.accepted != kit.commands_permission(os) {
        return Err(TabError::new(
            TabError::DENIED,
            "this kit's commands are not the ones accepted on this workspace",
        ));
    }
    let command = kit.manifest.commands.get(&call.name).ok_or_else(|| {
        TabError::new(
            TabError::DENIED,
            format!("this kit declares no command {:?}", call.name),
        )
    })?;
    let argv = command.run.for_os(os).ok_or_else(|| {
        TabError::new(
            TabError::NOT_FOUND,
            format!("the command {} has no form for this system", call.name),
        )
    })?;
    let args = expand(argv, &command.args, &call.args, &kit.root.to_string_lossy())?;
    let cwd = Path::new(&call.cwd);
    if !cwd.is_dir() {
        return Err(TabError::new(
            TabError::UNAVAILABLE,
            format!("the workspace folder {} is not there", call.cwd),
        ));
    }
    let _slot = Slot::take(&kit.entry.id).ok_or_else(|| {
        TabError::new(
            TabError::BUSY,
            format!("at most {MAX_RUNNING} commands of a kit run at once; try again later"),
        )
    })?;
    let program = search.program(&argv[0])?;
    execute(
        &program,
        &args,
        cwd,
        Duration::from_secs(command.timeout_seconds),
    )
}

/// `command.run`. The window checked the tab holds `command:<name>`.
pub async fn kit_command_run(
    kit: String,
    name: String,
    args: HashMap<String, String>,
    cwd: String,
    accepted: Option<String>,
) -> Result<Output, TabError> {
    off_thread(move || {
        let unavailable = |why: String| TabError::new(TabError::UNAVAILABLE, why);
        let store = super::open().map_err(unavailable)?;
        let loaded = store
            .entry(&kit)
            .and_then(|entry| store.loaded(&entry))
            .map_err(unavailable)?;
        let call = Call {
            name,
            args,
            cwd,
            accepted,
        };
        run(
            &loaded,
            &call,
            manifest::current_os(),
            &Search::of_this_process(),
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::source::GitSource;
    use crate::kits::store::Store;
    use crate::kits::testing::Folder;
    use serde_json::{json, Value};
    use std::io::Write;

    const TEST_PROGRAM: &str = "kits::commands::tests::test_program";
    const MARK: &str = "<<<said>>>";

    /// The program the tests below run: this very test binary, a real process
    /// with a real argv, on every OS. Given a mode after `--`, it does what the
    /// mode says; in the suite's own run, nothing.
    #[test]
    fn test_program() {
        let args: Vec<String> = std::env::args().collect();
        let Some(split) = args.iter().position(|arg| arg == "--") else {
            return;
        };
        let Some((mode, rest)) = args[split + 1..].split_first() else {
            return;
        };
        let say = |what: String| {
            println!("{MARK}{what}{MARK}");
            std::io::stdout().flush().unwrap();
        };
        let keep_going = || std::thread::sleep(Duration::from_secs(60));
        match mode.as_str() {
            "echo" => say(serde_json::to_string(rest).unwrap()),
            "cwd" => say(std::env::current_dir().unwrap().display().to_string()),
            "stdin" => {
                let mut all = Vec::new();
                std::io::stdin().read_to_end(&mut all).unwrap();
                say(format!("{} bytes", all.len()));
            }
            "exit" => {
                eprint!("leaving");
                std::process::exit(rest[0].parse().unwrap());
            }
            "sleep" => keep_going(),
            "flood" => {
                let bytes = |count: &String| vec![b'a'; count.parse().unwrap()];
                std::io::stdout().write_all(&bytes(&rest[0])).unwrap();
                std::io::stderr().write_all(&bytes(&rest[1])).unwrap();
            }
            // A child that inherits the pipes, as a daemon or a stray process
            // does, then either stays or leaves it behind.
            "parent" | "leave-child" => {
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST_PROGRAM, "--nocapture", "--", "sleep"])
                    .spawn()
                    .unwrap();
                say(child.id().to_string());
                if mode == "leave-child" {
                    std::process::exit(0);
                }
                let _ = child.wait();
            }
            _ => {}
        }
    }

    /// `gh` for the tests: where this binary is, as the PATH.
    fn search() -> Search {
        let exe = std::env::current_exe().unwrap();
        Search {
            path: exe.parent().unwrap().as_os_str().to_owned(),
            extensions: cfg!(windows).then(|| DEFAULT_PATHEXT.to_string()),
        }
    }

    /// The test program's argv, its mode and what follows being `rest`.
    fn program(rest: &[&str]) -> Value {
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        let mut argv: Vec<String> = vec![
            name,
            "--exact".into(),
            TEST_PROGRAM.into(),
            "--nocapture".into(),
            "--".into(),
        ];
        argv.extend(rest.iter().map(|item| item.to_string()));
        json!(argv)
    }

    struct Kit {
        loaded: Loaded,
        /// The workspace folder of the calls.
        cwd: Folder,
        _folder: Folder,
        _home: Folder,
    }

    impl Kit {
        fn new(id: &str, commands: Value) -> Kit {
            Kit::in_folder(id, &format!("cmd-{id}"), commands)
        }

        /// The same, in a folder of the kit's named after `folder`.
        fn in_folder(id: &str, folder: &str, commands: Value) -> Kit {
            let home = Folder::new(&format!("cmd-home-{id}"));
            let folder = Folder::new(folder);
            let store = Store::at(home.root.join("kits"));
            folder.file(
                manifest::FILE,
                &json!({
                    "schema": 1, "id": id, "name": id, "version": "1.0.0", "description": "d",
                    "commands": commands
                })
                .to_string(),
            );
            let loaded = store
                .add_folder(&folder.root.to_string_lossy(), 1)
                .and_then(|entry| store.loaded(&entry))
                .unwrap();
            Kit {
                loaded,
                cwd: Folder::new(&format!("cmd-ws-{id}")),
                _folder: folder,
                _home: home,
            }
        }

        fn call(&self, name: &str, args: &[(&str, &str)]) -> Call {
            Call {
                name: name.to_string(),
                args: args
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect(),
                cwd: self.cwd.root.to_string_lossy().into_owned(),
                accepted: None,
            }
        }

        fn run(&self, name: &str, args: &[(&str, &str)]) -> Result<Output, TabError> {
            run(
                &self.loaded,
                &self.call(name, args),
                manifest::current_os(),
                &search(),
            )
        }
    }

    /// What the test program said, between its marks.
    fn said(output: &Output) -> &str {
        output
            .stdout
            .split(MARK)
            .nth(1)
            .unwrap_or_else(|| panic!("it said nothing: {output:?}"))
    }

    fn command(run: Value, args: Value) -> Value {
        json!({ "description": "d", "run": run, "args": args })
    }

    fn echo(rest: &[&str], args: Value) -> Value {
        let mut all = vec!["echo"];
        all.extend_from_slice(rest);
        json!({ "echo": command(program(&all), args) })
    }

    #[test]
    fn shell_metacharacters_reach_the_program_as_plain_text() {
        let kit = Kit::new(
            "cmd-meta",
            echo(
                &["--value={value}", "{value}", "tail"],
                json!({ "value": { "pattern": "(?s).*" } }),
            ),
        );
        for value in [
            "plain",
            "a b",
            "a;b",
            "a&b",
            "a&&calc",
            "a|b",
            "$(whoami)",
            "`whoami`",
            "%PATH%",
            "!PATH!",
            "$HOME",
            "\"quoted\"",
            "it's",
            ">out.txt",
            "<in.txt",
            "*",
            "?",
            "~",
            "^",
            "C:\\dir\\",
            "trailing\\\"",
            "{other}",
            "{value}",
            "é",
            "two\nlines",
            "",
        ] {
            let output = kit.run("echo", &[("value", value)]).unwrap();
            assert_eq!(output.exit_code, Some(0), "{value:?}: {output:?}");
            let argv: Vec<String> = serde_json::from_str(said(&output)).unwrap();
            assert_eq!(
                argv,
                [&format!("--value={value}"), value, "tail"],
                "{value:?}"
            );
        }
        assert_eq!(
            std::fs::read_dir(&kit.cwd.root).unwrap().count(),
            0,
            "nothing was redirected into the folder"
        );
    }

    #[test]
    fn a_value_must_match_its_rule_whole() {
        let kit = Kit::new(
            "cmd-values",
            echo(
                &["--repo={repo}", "{run}"],
                json!({
                    "repo": { "pattern": "[a-z]+/[a-z]+" },
                    "run": { "pattern": "[0-9]+" }
                }),
            ),
        );
        let output = kit.run("echo", &[("repo", "o/r"), ("run", "42")]).unwrap();
        let argv: Vec<String> = serde_json::from_str(said(&output)).unwrap();
        assert_eq!(argv, ["--repo=o/r", "42"]);

        for args in [
            // No match.
            vec![("repo", "o/r"), ("run", "abc")],
            // A partial match, against a rule its author left unanchored.
            vec![("repo", "o/r"), ("run", "42; rm -rf /")],
            vec![("repo", "o/r"), ("run", "x42")],
            vec![("repo", "o/r"), ("run", "42\n")],
            // Missing.
            vec![("repo", "o/r")],
            vec![],
            // Not an argument of this command.
            vec![("repo", "o/r"), ("run", "42"), ("extra", "1")],
        ] {
            let err = kit.run("echo", &args).unwrap_err();
            assert_eq!(err.code, TabError::BAD_ARGS, "{args:?}: {}", err.message);
        }
    }

    #[test]
    fn a_value_starting_with_a_dash_is_refused_even_when_the_rule_allows_it() {
        let kit = Kit::new(
            "cmd-dash",
            json!({
                "echo": command(program(&["echo", "{value}"]), json!({ "value": { "pattern": ".*" } })),
                "glued": command(program(&["echo", "{a}-{b}"]), json!({
                    "a": { "pattern": ".*" },
                    "b": { "pattern": ".*" }
                }))
            }),
        );
        for value in ["-x", "--output=/etc/passwd", "-"] {
            let err = kit.run("echo", &[("value", value)]).unwrap_err();
            assert_eq!(err.code, TabError::BAD_ARGS, "{value}");
            assert!(err.message.contains("option"), "{}", err.message);
        }
        // A dash further on is only text.
        assert!(kit.run("echo", &[("value", "a-b")]).is_ok());
        // No value starts with one here, and the item would.
        let err = kit.run("glued", &[("a", ""), ("b", "x")]).unwrap_err();
        assert_eq!(err.code, TabError::BAD_ARGS, "{}", err.message);
        assert!(kit.run("glued", &[("a", "a"), ("b", "x")]).is_ok());
        // The kit's own option stays one.
        let own = Kit::new(
            "cmd-dash-own",
            echo(
                &["--json", "--repo={r}"],
                json!({ "r": { "pattern": "x" } }),
            ),
        );
        let argv: Vec<String> =
            serde_json::from_str(said(&own.run("echo", &[("r", "x")]).unwrap())).unwrap();
        assert_eq!(argv, ["--json", "--repo=x"]);
    }

    #[test]
    fn kit_dir_is_the_kits_folder_and_stays_one_argument() {
        let kit = Kit::in_folder(
            "cmd-kit-dir",
            "a kit folder with spaces",
            echo(
                &[
                    "{kitDir}/tools/audit.js",
                    "--root={kitDir}",
                    "{kitDir}",
                    "{id}",
                ],
                json!({ "id": { "pattern": "[0-9]+" } }),
            ),
        );
        let dir = kit.loaded.root.to_string_lossy().into_owned();
        assert!(dir.contains("a kit folder with spaces"), "{dir}");
        let output = kit.run("echo", &[("id", "42")]).unwrap();
        let argv: Vec<String> = serde_json::from_str(said(&output)).unwrap();
        assert_eq!(
            argv,
            [
                format!("{dir}/tools/audit.js"),
                format!("--root={dir}"),
                dir.clone(),
                "42".to_string()
            ]
        );
    }

    #[test]
    fn kit_dir_is_not_a_tabs_to_give() {
        let kit = Kit::new(
            "cmd-kit-dir-given",
            echo(
                &["{kitDir}", "{value}"],
                json!({ "value": { "pattern": "(?s).*" } }),
            ),
        );
        let err = kit
            .run("echo", &[("value", "x"), ("kitDir", "/elsewhere")])
            .unwrap_err();
        assert_eq!(err.code, TabError::BAD_ARGS, "{}", err.message);
        // A value holding it stays that text: a value is filled in once.
        let output = kit.run("echo", &[("value", "{kitDir}")]).unwrap();
        let argv: Vec<String> = serde_json::from_str(said(&output)).unwrap();
        assert_eq!(argv[1], "{kitDir}");
    }

    #[test]
    fn kit_dir_is_shown_as_written_with_no_rule_and_hashed_as_written() {
        let commands = json!({ "audit": {
            "description": "d",
            "run": ["node", "{kitDir}/tools/audit.js", "{id}"],
            "args": { "id": { "pattern": "[0-9]+" } }
        } });
        let found = for_os(&manifest_of(commands.clone()), "linux");
        assert_eq!(found[0].argv, ["node", "{kitDir}/tools/audit.js", "{id}"]);
        assert_eq!(found[0].args.keys().collect::<Vec<_>>(), ["id"]);
        // The folder a kit lives in asks nothing: the hash is of what it wrote.
        let canonical = r#"[{"name":"audit","run":["node","{kitDir}/tools/audit.js","{id}"],"args":{"id":"[0-9]+"}}]"#;
        assert_eq!(
            hash(commands, "linux").unwrap(),
            format!("commands:{}", &sha256_hex(canonical)[..16])
        );
    }

    #[test]
    fn what_a_kit_does_not_declare_cannot_run() {
        let kit = Kit::new("cmd-undeclared", echo(&[], json!({})));
        assert_eq!(kit.run("deploy", &[]).unwrap_err().code, TabError::DENIED);
        let other_os = Kit::new(
            "cmd-other-os",
            json!({ "t": command(json!({ "freebsd": ["x"], "macos": ["x"] }), json!({})) }),
        );
        let err = run(
            &other_os.loaded,
            &other_os.call("t", &[]),
            "linux",
            &search(),
        )
        .unwrap_err();
        assert_eq!(err.code, TabError::NOT_FOUND, "{}", err.message);
    }

    #[test]
    fn an_absent_program_is_not_found() {
        let kit = Kit::new(
            "cmd-absent",
            json!({ "t": command(json!(["hangar-no-such-program-anywhere"]), json!({})) }),
        );
        let err = kit.run("t", &[]).unwrap_err();
        assert_eq!(err.code, TabError::NOT_FOUND, "{}", err.message);
        assert!(err.message.contains("hangar-no-such-program-anywhere"));
    }

    #[test]
    fn a_program_is_never_found_in_the_folder_a_command_starts_in() {
        // Whatever a project keeps there, a relative or empty PATH entry included.
        let here = std::env::current_dir().unwrap();
        assert!(here.join("Cargo.toml").is_file());
        for path in [".", "", "./"] {
            let search = Search {
                path: OsString::from(path),
                extensions: cfg!(windows).then(|| DEFAULT_PATHEXT.to_string()),
            };
            assert_eq!(search.find("Cargo.toml"), None, "{path:?}");
        }
    }

    #[test]
    fn extensions_are_tried_as_windows_does() {
        assert_eq!(names_of("gh", None), ["gh"]);
        assert_eq!(names_of("gh.exe", None), ["gh.exe"]);
        assert_eq!(
            names_of("gh", Some(".COM;.EXE; ;.CMD;")),
            ["gh.COM", "gh.EXE", "gh.CMD"],
            "a bare name is never the file itself"
        );
        assert_eq!(
            names_of("gh.exe", Some(".EXE")),
            ["gh.exe", "gh.exe.EXE"],
            "a name with an extension may be"
        );
    }

    #[test]
    fn a_batch_file_is_known_however_its_name_is_written() {
        for name in [
            "x.bat", "x.cmd", "x.BAT", "x.Cmd", "x.cmd.", "x.cmd ", "x.bat. .",
        ] {
            assert!(is_batch(Path::new(name)), "{name}");
        }
        for name in ["x.exe", "cmd", "bat", "x.cmd.exe", "xcmd", "x.bat.txt"] {
            assert!(!is_batch(Path::new(name)), "{name}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_batch_file_on_the_path_is_refused_and_not_run() {
        for script in ["tool.cmd", "tool.bat", "tool.CMD"] {
            let dir = Folder::new("cmd-batch");
            let ran = dir.root.join("ran.txt");
            dir.file(script, &format!("@echo ran> \"{}\"\r\n", ran.display()));
            let kit = Kit::new(
                "cmd-batch-kit",
                json!({
                    "bare": command(json!(["tool"]), json!({})),
                    "named": command(json!([script]), json!({}))
                }),
            );
            let on_dir = Search {
                path: dir.root.as_os_str().to_owned(),
                extensions: Some(DEFAULT_PATHEXT.to_string()),
            };
            for name in ["bare", "named"] {
                let err = run(&kit.loaded, &kit.call(name, &[]), "windows", &on_dir).unwrap_err();
                assert_eq!(err.code, TabError::FAILED, "{script}: {}", err.message);
                assert!(err.message.contains("cmd.exe"), "{}", err.message);
                assert!(err.message.contains("batch file"), "{}", err.message);
            }
            assert!(!ran.exists(), "{script} was run");
        }
    }

    #[cfg(windows)]
    #[test]
    fn an_exe_beside_a_batch_file_is_the_one_found() {
        let dir = Folder::new("cmd-exe-first");
        dir.file("tool.cmd", "@echo no\r\n").file("tool.exe", "");
        let on_dir = Search {
            path: dir.root.as_os_str().to_owned(),
            extensions: Some(DEFAULT_PATHEXT.to_string()),
        };
        assert_eq!(on_dir.find("tool"), Some(dir.root.join("tool.EXE")));
    }

    #[test]
    fn a_command_runs_in_the_workspace_folder_with_nothing_to_read() {
        let kit = Kit::new(
            "cmd-cwd",
            json!({
                "cwd": command(program(&["cwd"]), json!({})),
                "stdin": command(program(&["stdin"]), json!({})),
                "exit": command(program(&["exit", "7"]), json!({}))
            }),
        );
        let output = kit.run("cwd", &[]).unwrap();
        assert_eq!(
            dunce::canonicalize(said(&output)).unwrap(),
            dunce::canonicalize(&kit.cwd.root).unwrap()
        );
        // It would wait for its input until the timeout if it had any to read.
        assert_eq!(said(&kit.run("stdin", &[]).unwrap()), "0 bytes");

        let left = kit.run("exit", &[]).unwrap();
        assert_eq!(left.exit_code, Some(7), "a code is an answer: {left:?}");
        assert_eq!(left.stderr, "leaving");
        assert!(!left.timed_out && !left.truncated);

        let mut gone = kit.call("cwd", &[]);
        gone.cwd = kit.cwd.root.join("gone").to_string_lossy().into_owned();
        let err = run(&kit.loaded, &gone, manifest::current_os(), &search()).unwrap_err();
        assert_eq!(err.code, TabError::UNAVAILABLE, "{}", err.message);
    }

    fn alive(pid: u32) -> bool {
        crate::procs::table()
            .unwrap()
            .iter()
            .any(|proc| proc.pid == pid)
    }

    fn gone_soon(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(pid) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
    }

    #[test]
    fn the_timeout_kills_the_command_and_its_tree() {
        let kit = Kit::new(
            "cmd-timeout",
            json!({ "tree": {
                "description": "d",
                "run": program(&["parent"]),
                "timeoutSeconds": 5
            } }),
        );
        let started = Instant::now();
        let output = kit.run("tree", &[]).unwrap();
        let took = started.elapsed();
        assert!(output.timed_out, "{output:?}");
        assert_eq!(output.exit_code, None);
        assert!(took >= Duration::from_secs(5), "{took:?}");
        assert!(
            took < Duration::from_secs(5) + Duration::from_secs(10),
            "{took:?}"
        );
        // What it said before is kept.
        let below: u32 = said(&output).parse().unwrap();
        assert!(gone_soon(below), "the process below it is still running");
        // The place is given back.
        assert!(running().get("cmd-timeout").is_none());
    }

    #[test]
    fn a_process_left_behind_does_not_hold_the_call_up() {
        let kit = Kit::new(
            "cmd-left",
            json!({ "left": command(program(&["leave-child"]), json!({})) }),
        );
        let started = Instant::now();
        let output = kit.run("left", &[]).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(25),
            "{:?}",
            started.elapsed()
        );
        assert!(!output.timed_out, "{output:?}");
        assert_eq!(output.exit_code, Some(0));
        crate::procs::terminate(&[said(&output).parse().unwrap()]);
    }

    #[test]
    fn each_stream_is_capped_at_one_megabyte() {
        let cap = MAX_OUTPUT_BYTES.to_string();
        let over = (MAX_OUTPUT_BYTES + 1).to_string();
        let much = (5 * MAX_OUTPUT_BYTES).to_string();
        let kit = Kit::new(
            "cmd-cap",
            json!({
                "stderr-at-cap": command(program(&["flood", "0", &cap]), json!({})),
                "stderr-over": command(program(&["flood", "0", &over]), json!({})),
                "stdout-much": command(program(&["flood", &much, "3"]), json!({}))
            }),
        );
        // The test harness writes on stdout too, so the exact edge is on stderr.
        let at_cap = kit.run("stderr-at-cap", &[]).unwrap();
        assert_eq!(at_cap.stderr.len(), MAX_OUTPUT_BYTES);
        assert!(!at_cap.truncated);
        let over = kit.run("stderr-over", &[]).unwrap();
        assert_eq!(over.stderr.len(), MAX_OUTPUT_BYTES);
        assert!(over.truncated);
        // Read to its end past the cap: it ended by itself, and the other
        // stream is whole.
        let much = kit.run("stdout-much", &[]).unwrap();
        assert_eq!(much.stdout.len(), MAX_OUTPUT_BYTES);
        assert_eq!(much.stderr, "aaa");
        assert!(much.truncated);
        assert_eq!(much.exit_code, Some(0));
        assert!(!much.timed_out);
    }

    #[test]
    fn a_character_cut_by_the_cap_is_dropped() {
        let mut bytes = vec![b'a'; 3];
        bytes.extend_from_slice(&"é".as_bytes()[..1]);
        assert_eq!(
            text_of(Kept {
                bytes: bytes.clone(),
                truncated: true
            }),
            ("aaa".to_string(), true)
        );
        // Not cut, it is what the program wrote, as far as it can be read.
        assert_eq!(
            text_of(Kept {
                bytes,
                truncated: false
            }),
            ("aaa\u{fffd}".to_string(), false)
        );
    }

    #[test]
    fn a_fifth_command_of_a_kit_is_busy_and_another_kits_is_not() {
        let kit = Arc::new(Kit::new(
            "cmd-busy",
            json!({
                "sleep": { "description": "d", "run": program(&["sleep"]), "timeoutSeconds": 3 },
                "echo": command(program(&["echo"]), json!({}))
            }),
        ));
        let other = Kit::new("cmd-busy-other", echo(&[], json!({})));
        let held: Vec<_> = (0..MAX_RUNNING)
            .map(|_| {
                let kit = kit.clone();
                std::thread::spawn(move || kit.run("sleep", &[]))
            })
            .collect();
        let deadline = Instant::now() + Duration::from_secs(2);
        while running().get("cmd-busy").copied() != Some(MAX_RUNNING) {
            assert!(Instant::now() < deadline, "the four never started");
            std::thread::sleep(Duration::from_millis(5));
        }
        let err = kit.run("echo", &[]).unwrap_err();
        assert_eq!(err.code, TabError::BUSY, "{}", err.message);
        assert!(other.run("echo", &[]).is_ok(), "another kit's are its own");
        for call in held {
            assert!(call.join().unwrap().unwrap().timed_out);
        }
        assert!(kit.run("echo", &[]).is_ok(), "the places were given back");
    }

    #[test]
    fn calls_one_after_the_other_never_get_in_each_others_way() {
        let kit = Kit::new("cmd-poll", echo(&[], json!({})));
        for _ in 0..(MAX_RUNNING * 2) {
            assert!(kit.run("echo", &[]).is_ok());
        }
    }

    fn manifest_of(commands: Value) -> Manifest {
        let folder = Folder::new("cmd-hash");
        folder
            .parse(&json!({
                "schema": 1, "id": "hash", "name": "h", "version": "1.0.0", "description": "d",
                "commands": commands
            }))
            .unwrap()
    }

    fn hash(commands: Value, os: &str) -> Option<String> {
        grant(&manifest_of(commands), Path::new("/nowhere"), os)
    }

    #[test]
    fn the_hash_is_stable_and_changes_with_what_can_run() {
        let base = || {
            json!({
                "view": {
                    "description": "A run.",
                    "run": ["gh", "run", "view", "{run}"],
                    "args": { "run": { "pattern": "[0-9]+" } }
                },
                "list": { "description": "Runs.", "run": ["gh", "run", "list"] }
            })
        };
        let first = hash(base(), "linux").unwrap();
        assert!(first.starts_with("commands:"), "{first}");
        assert_eq!(first.len(), "commands:".len() + 16, "{first}");
        assert!(first["commands:".len()..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));

        // Read again, its keys in another order: the same list.
        assert_eq!(hash(base(), "linux").unwrap(), first);
        let reordered = json!({
            "list": { "run": ["gh", "run", "list"], "description": "Runs." },
            "view": {
                "args": { "run": { "pattern": "[0-9]+" } },
                "run": ["gh", "run", "view", "{run}"],
                "description": "A run."
            }
        });
        assert_eq!(hash(reordered, "linux").unwrap(), first);

        // Nothing that can run changes.
        let mut reworded = base();
        reworded["view"]["description"] = json!("Another sentence.");
        reworded["view"]["args"]["run"]["description"] = json!("Its number.");
        reworded["view"]["timeoutSeconds"] = json!(60);
        assert_eq!(hash(reworded, "linux").unwrap(), first);

        let mut argv = base();
        argv["view"]["run"] = json!(["gh", "run", "view", "{run}", "--log"]);
        assert_ne!(hash(argv, "linux").unwrap(), first, "another argv");
        let mut item = base();
        item["list"]["run"] = json!(["gh", "run", "lists"]);
        assert_ne!(hash(item, "linux").unwrap(), first, "another item");
        let mut rule = base();
        rule["view"]["args"]["run"]["pattern"] = json!(".*");
        assert_ne!(hash(rule, "linux").unwrap(), first, "a wider rule");
        let mut renamed = base();
        renamed["lists"] = renamed["list"].clone();
        renamed.as_object_mut().unwrap().remove("list");
        assert_ne!(hash(renamed, "linux").unwrap(), first, "another name");
        let mut added = base();
        added["more"] = json!({ "description": "d", "run": ["gh"] });
        assert_ne!(hash(added, "linux").unwrap(), first, "one more");
    }

    #[test]
    fn the_hash_is_for_this_os() {
        let commands = json!({
            "tail": {
                "description": "d",
                "run": { "windows": ["tailer.exe"], "linux": ["tail"] }
            }
        });
        let on_linux = hash(commands.clone(), "linux").unwrap();
        assert_ne!(hash(commands.clone(), "windows").unwrap(), on_linux);
        assert_eq!(hash(commands, "macos"), None, "nothing to accept there");
        assert_eq!(hash(json!({}), "linux"), None);
    }

    #[test]
    fn the_hash_is_the_documented_one() {
        let canonical = r#"[{"name":"view","run":["gh","{run}"],"args":{"run":"[0-9]+"}}]"#;
        let commands = json!({ "view": {
            "description": "d",
            "run": ["gh", "{run}"],
            "args": { "run": { "pattern": "[0-9]+" } }
        } });
        assert_eq!(
            hash(commands, "linux").unwrap(),
            format!("commands:{}", &sha256_hex(canonical)[..16])
        );
    }

    #[test]
    fn the_list_shows_each_command_for_this_os_verbatim() {
        let found = for_os(
            &manifest_of(json!({
                "view": {
                    "description": "A run.",
                    "run": {
                        "windows": ["gh.exe", "run", "view", "{run}", "--repo={repo}", "{{.Names}}"],
                        "linux": ["gh", "{run}"]
                    },
                    "args": {
                        "run": { "pattern": "[0-9]+", "description": "Its number" },
                        "repo": { "pattern": "[a-z/]+" }
                    },
                    "timeoutSeconds": 45
                },
                "mac-only": { "description": "d", "run": { "macos": ["x"] } }
            })),
            "windows",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "view");
        assert_eq!(found[0].description, "A run.");
        assert_eq!(found[0].timeout_seconds, 45);
        assert_eq!(
            found[0].argv,
            [
                "gh.exe",
                "run",
                "view",
                "{run}",
                "--repo={repo}",
                "{{.Names}}"
            ]
        );
        assert_eq!(found[0].args["run"].pattern, "[0-9]+");
        assert_eq!(found[0].args["repo"].pattern, "[a-z/]+");
        let on_linux = for_os(
            &manifest_of(json!({
                "view": {
                    "description": "d",
                    "run": { "windows": ["gh", "{repo}"], "linux": ["gh", "{run}"] },
                    "args": { "run": { "pattern": "a" }, "repo": { "pattern": "b" } }
                }
            })),
            "linux",
        );
        assert_eq!(
            on_linux[0].args.keys().collect::<Vec<_>>(),
            ["run"],
            "only the placeholders of this very argv"
        );
    }

    /// The same kit, installed from GitHub.
    fn from_github(kit: &Kit) -> Loaded {
        let mut loaded = kit.loaded.clone();
        loaded.entry.source = Source::Git(GitSource {
            url: "https://github.com/o/r".to_string(),
            git_ref: None,
            subdir: None,
        });
        loaded
    }

    #[test]
    fn a_github_kit_runs_only_the_commands_that_were_accepted() {
        let kit = Kit::new("cmd-consent", echo(&[], json!({})));
        let os = manifest::current_os();
        let github = from_github(&kit);
        let current = github.commands_permission(os);
        assert!(current.is_some());
        let call = |accepted: Option<&str>| Call {
            accepted: accepted.map(str::to_string),
            ..kit.call("echo", &[])
        };

        assert!(run(&github, &call(current.as_deref()), os, &search()).is_ok());
        for accepted in [None, Some("commands:0123456789abcdef")] {
            let err = run(&github, &call(accepted), os, &search()).unwrap_err();
            assert_eq!(err.code, TabError::DENIED, "{accepted:?}: {}", err.message);
        }
        // A linked folder is the person's own, read live: it is not asked.
        for accepted in [None, Some("commands:0123456789abcdef")] {
            assert!(run(&kit.loaded, &call(accepted), os, &search()).is_ok());
        }
    }
}
