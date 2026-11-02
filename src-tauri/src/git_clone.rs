//! « Créer un espace de travail depuis un dépôt Git »: the Rust side.
//!
//! Three commands, and a fourth to cancel:
//! - `git_repo_parse` says what a pasted address is, or why it is refused;
//! - `git_repo_ownership` says whether the repository's owner is the person
//!   (or one of their organisations), as far as `gh` knows;
//! - `git_clone` clones it, or reuses a folder that already is that clone;
//! - `git_clone_cancel` stops a clone by its job id.
//!
//! Expected refusals are values, not errors, like in git.rs: the front shows
//! them as they are. An address is never trusted because the front parsed it:
//! `git_clone` parses it again.
//!
//! Nothing here keeps a credential. An address that carries one is refused, so
//! it never reaches the state or a log, and git's own messages are scrubbed
//! before they leave this module.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A clone that has not ended after this long is stopped. Generous: a large
/// repository over a slow line is legitimate.
const CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// `gh` answers from its cache or one HTTP call; past this it is stuck (on a
/// login prompt, or a dead network).
const GH_TIMEOUT: Duration = Duration::from_secs(15);

const PROGRESS_EVENT: &str = "git-clone:progress";

// ---------------------------------------------------------------------------
// Parsing an address
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoRef {
    /// What is handed to git. `owner/repo` is expanded to a GitHub address.
    pub url: String,
    /// Lower-case host name, without port.
    pub host: String,
    pub owner: String,
    /// The repository's name, without `.git`.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ParseResult {
    Ok {
        #[serde(flatten)]
        repo: RepoRef,
    },
    Refused {
        reason: String,
    },
}

fn refuse<T>(reason: &str) -> Result<T, String> {
    Err(reason.to_string())
}

/// A host name: letters, digits, dots and hyphens, not starting with either.
fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['-', '.'])
        && !host.ends_with('-')
        && !host.contains("..")
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// `host` or `host:port`, the port being digits.
fn split_host_port(authority: &str) -> Result<(String, Option<&str>), String> {
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if let Some(port) = port {
        if port.is_empty() || port.len() > 5 || !port.chars().all(|c| c.is_ascii_digit()) {
            return refuse("the port is not a number");
        }
    }
    if !valid_host(host) {
        return refuse("the host name is not valid");
    }
    Ok((host.to_ascii_lowercase(), port))
}

/// An owner or a repository name as it travels in an address.
fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.len() <= 100
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// `owner/name[.git][/]` → (owner, name).
fn split_path(path: &str) -> Result<(String, String), String> {
    let path = path.strip_suffix('/').unwrap_or(path);
    let mut parts = path.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return refuse("the address must name an owner and a repository");
    };
    let name = name.strip_suffix(".git").unwrap_or(name);
    if !valid_segment(owner) {
        return refuse("the owner is not valid");
    }
    if !valid_segment(name) {
        return refuse("the repository name is not valid");
    }
    Ok((owner.to_string(), name.to_string()))
}

/// Parses an address. Accepts `https://host/owner/repo(.git)`,
/// `ssh://[user@]host[:port]/owner/repo(.git)`, `git@host:owner/repo(.git)` and
/// `owner/repo` (GitHub). Everything else is refused.
pub fn parse_repo(input: &str) -> Result<RepoRef, String> {
    if input.is_empty() {
        return refuse("the address is empty");
    }
    if input.starts_with('-') {
        return refuse("the address cannot start with a dash");
    }
    if input.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return refuse("the address cannot contain spaces or control characters");
    }
    if input.contains("::") {
        return refuse("this kind of address is not allowed");
    }
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("file:") {
        return refuse("a local file address is not allowed");
    }

    if let Some(rest) = input
        .get(..8)
        .filter(|scheme| scheme.eq_ignore_ascii_case("https://"))
        .map(|_| &input[8..])
    {
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, ""), |(authority, path)| (authority, path));
        if authority.contains('@') {
            return refuse("an address with credentials in it is not accepted");
        }
        let (host, _) = split_host_port(authority)?;
        let (owner, name) = split_path(path)?;
        return Ok(RepoRef {
            url: format!(
                "https://{authority}/{owner}/{name}{}",
                if path.strip_suffix('/').unwrap_or(path).ends_with(".git") {
                    ".git"
                } else {
                    ""
                }
            ),
            host,
            owner,
            name,
        });
    }

    if let Some(rest) = input
        .get(..6)
        .filter(|scheme| scheme.eq_ignore_ascii_case("ssh://"))
        .map(|_| &input[6..])
    {
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, ""), |(authority, path)| (authority, path));
        let host_part = match authority.split_once('@') {
            Some((user, host_part)) => {
                if user.is_empty() || user.contains(':') || host_part.contains('@') {
                    return refuse("an address with credentials in it is not accepted");
                }
                if !user
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                {
                    return refuse("the user name is not valid");
                }
                host_part
            }
            None => authority,
        };
        let (host, _) = split_host_port(host_part)?;
        let (owner, name) = split_path(path)?;
        let tail = if path.strip_suffix('/').unwrap_or(path).ends_with(".git") {
            ".git"
        } else {
            ""
        };
        return Ok(RepoRef {
            url: format!("ssh://{authority}/{owner}/{name}{tail}"),
            host,
            owner,
            name,
        });
    }

    if input.contains("://") {
        return refuse("only https, ssh and git@host:owner/repo addresses are accepted");
    }

    // git@host:owner/repo
    if let Some((user_host, path)) = input.split_once(':') {
        let Some((user, host)) = user_host.split_once('@') else {
            return refuse("this is not a repository address");
        };
        if user.is_empty()
            || !user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return refuse("the user name is not valid");
        }
        if host.contains('@') || !valid_host(host) {
            return refuse("the host name is not valid");
        }
        let (owner, name) = split_path(path)?;
        let tail = if path.strip_suffix('/').unwrap_or(path).ends_with(".git") {
            ".git"
        } else {
            ""
        };
        return Ok(RepoRef {
            url: format!("{user}@{host}:{owner}/{name}{tail}"),
            host: host.to_ascii_lowercase(),
            owner,
            name,
        });
    }

    // owner/repo, meaning GitHub. Anything else with a separator or a drive
    // letter is a local path.
    if input.contains('\\') || input.contains('@') {
        return refuse("this is not a repository address");
    }
    let (owner, name) = split_path(input)?;
    Ok(RepoRef {
        url: format!("https://github.com/{owner}/{name}.git"),
        host: "github.com".to_string(),
        owner,
        name,
    })
}

#[tauri::command]
pub fn git_repo_parse(input: String) -> ParseResult {
    match parse_repo(&input) {
        Ok(repo) => ParseResult::Ok { repo },
        Err(reason) => ParseResult::Refused { reason },
    }
}

// ---------------------------------------------------------------------------
// Ownership
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ownership {
    pub mine: bool,
    pub reason: String,
}

fn ownership(mine: bool, reason: impl Into<String>) -> Ownership {
    Ownership {
        mine,
        reason: reason.into(),
    }
}

/// The JSON values of `gh`'s answer: one, or several when `--paginate`
/// printed one page after the other.
fn json_values(text: &str) -> Result<Vec<serde_json::Value>, String> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<serde_json::Value>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "gh answered something unexpected".to_string())
}

/// The login in `gh api user`'s answer.
pub fn parse_gh_login(text: &str) -> Result<String, String> {
    json_values(text)?
        .iter()
        .find_map(|value| value.get("login").and_then(|login| login.as_str()))
        .map(str::to_string)
        .ok_or_else(|| "gh did not say who is signed in".to_string())
}

/// The logins in `gh api user/orgs`'s answer, across pages.
pub fn parse_gh_orgs(text: &str) -> Result<Vec<String>, String> {
    let mut logins = Vec::new();
    for value in json_values(text)? {
        let Some(items) = value.as_array() else {
            return Err("gh answered something unexpected".to_string());
        };
        logins.extend(
            items
                .iter()
                .filter_map(|item| item.get("login").and_then(|login| login.as_str()))
                .map(str::to_string),
        );
    }
    Ok(logins)
}

/// The decision, from what `gh` said. `orgs` is only consulted when the owner
/// is not the person's own login, so an organisation list that could not be
/// read does not hide a personal repository.
pub fn decide_ownership(
    host: &str,
    owner: &str,
    login: &Result<String, String>,
    orgs: &dyn Fn() -> Result<Vec<String>, String>,
) -> Ownership {
    if !host.eq_ignore_ascii_case("github.com") {
        return ownership(false, "ownership is only checked on github.com");
    }
    if owner.is_empty() {
        return ownership(false, "there is no owner to check");
    }
    let login = match login {
        Ok(login) => login,
        Err(reason) => return ownership(false, reason.clone()),
    };
    if login.eq_ignore_ascii_case(owner) {
        return ownership(true, format!("{owner} is your GitHub account"));
    }
    match orgs() {
        Ok(orgs) if orgs.iter().any(|org| org.eq_ignore_ascii_case(owner)) => {
            ownership(true, format!("you belong to the {owner} organisation"))
        }
        Ok(_) => ownership(
            false,
            format!("{owner} is neither your account nor one of your organisations"),
        ),
        Err(reason) => ownership(false, reason),
    }
}

enum GhError {
    Missing,
    Failed(String),
}

/// Runs `gh <args>` read-only, with a time limit, and hands back its stdout.
fn run_gh(args: &[&str]) -> Result<String, GhError> {
    crate::path_env::ensure();
    let mut command = Command::new("gh");
    command
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => GhError::Missing,
        _ => GhError::Failed("gh could not be run".to_string()),
    })?;
    let out = read_all(child.stdout.take());
    let err = read_all(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= GH_TIMEOUT => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(GhError::Failed("gh did not answer in time".to_string()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return Err(GhError::Failed("gh could not be run".to_string())),
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if !status.success() {
        let text = String::from_utf8_lossy(&err).to_lowercase();
        let reason = if text.contains("gh auth login") || text.contains("not logged") {
            "gh is not signed in"
        } else {
            "gh could not tell"
        };
        return Err(GhError::Failed(reason.to_string()));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn read_all<R: Read + Send + 'static>(stream: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    })
}

fn gh_text(args: &[&str]) -> Result<String, String> {
    run_gh(args).map_err(|error| match error {
        GhError::Missing => "gh is not installed".to_string(),
        GhError::Failed(reason) => reason,
    })
}

#[tauri::command]
pub async fn git_repo_ownership(owner: String, host: String) -> Ownership {
    tauri::async_runtime::spawn_blocking(move || {
        if !host.eq_ignore_ascii_case("github.com") {
            return ownership(false, "ownership is only checked on github.com");
        }
        let login = gh_text(&["api", "user"]).and_then(|text| parse_gh_login(&text));
        decide_ownership(&host, &owner, &login, &|| {
            gh_text(&["api", "--paginate", "user/orgs?per_page=100"])
                .and_then(|text| parse_gh_orgs(&text))
        })
    })
    .await
    .unwrap_or_else(|_| ownership(false, "the check was interrupted"))
}

// ---------------------------------------------------------------------------
// Process helpers
// ---------------------------------------------------------------------------

/// Ends the process and everything it started.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // The clone leads its own process group (see `spawn_clone`).
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

fn git_command() -> Command {
    crate::path_env::ensure();
    let mut command = Command::new("git");
    command.env("LC_ALL", "C").env("LANGUAGE", "");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Runs `git -C <dir> <args>` and hands back its trimmed stdout, `None` on any
/// failure.
fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let output = git_command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ---------------------------------------------------------------------------
// Removing what a clone made, and comparing paths
// ---------------------------------------------------------------------------

fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn remove_link(path: &Path, meta: &std::fs::Metadata) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const DIRECTORY: u32 = 0x10;
        if meta.file_attributes() & DIRECTORY != 0 {
            return std::fs::remove_dir(path);
        }
    }
    let _ = meta;
    std::fs::remove_file(path)
}

/// Deletes `path` and what is under it. A link is removed itself and never
/// entered, so a junction inside the clone costs its own entry, not its target.
/// Read-only files (git's objects are) are made writable first.
fn remove_tree(path: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if is_link(&meta) {
        let _ = remove_link(path, &meta);
        return;
    }
    if meta.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                remove_tree(&entry.path());
            }
        }
        let _ = std::fs::remove_dir(path);
        return;
    }
    let mut permissions = meta.permissions();
    if permissions.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(path, permissions);
    }
    let _ = std::fs::remove_file(path);
}

fn comparable(path: &Path) -> String {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = real.to_string_lossy().replace('\\', "/");
    let text = text
        .strip_prefix("//?/UNC/")
        .map(|rest| format!("//{rest}"))
        .or_else(|| text.strip_prefix("//?/").map(str::to_string))
        .unwrap_or(text);
    let text = text.trim_end_matches('/').to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// Why a folder name cannot be the destination, if it cannot.
fn folder_name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() || name == "." || name == ".." {
        return Some("the folder name is not valid");
    }
    if name.starts_with('-') {
        return Some("the folder name cannot start with a dash");
    }
    if name.chars().any(|c| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) {
        return Some("the folder name holds a character that cannot be in a folder name");
    }
    if name.ends_with('.') || name.ends_with(' ') || name.starts_with(' ') {
        return Some("the folder name cannot start with a space or end with a dot or a space");
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|c: char| c.is_ascii_digit()));
    reserved.then_some("the folder name is reserved by Windows")
}

// ---------------------------------------------------------------------------
// Cloning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CloneOutcome {
    Cloned {
        path: String,
        reused: bool,
        branch: Option<String>,
    },
    Refused {
        /// `invalidUrl`, `invalidDestination`, `exists`, `busy`, `gitMissing`,
        /// `cancelled`, `timeout` or `failed`.
        kind: &'static str,
        reason: String,
    },
}

fn refused(kind: &'static str, reason: impl Into<String>) -> CloneOutcome {
    CloneOutcome::Refused {
        kind,
        reason: reason.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneProgress {
    pub job_id: String,
    /// `Receiving objects`, `Resolving deltas`…
    pub phase: String,
    pub percent: Option<u8>,
}

/// The phase and percentage of one line of `git clone --progress`'s stderr.
pub fn parse_progress(line: &str) -> Option<(String, u8)> {
    let line = line.trim();
    let line = line.strip_prefix("remote:").unwrap_or(line).trim();
    let (phase, rest) = line.split_once(':')?;
    let rest = rest.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || !rest[digits.len()..].starts_with('%') {
        return None;
    }
    let percent = digits.parse::<u16>().ok()?.min(100) as u8;
    Some((phase.trim().to_string(), percent))
}

/// Removes the credentials of any address in `text` and keeps it short.
pub fn scrub(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '/' || c == '\'' || c == '"')
            .unwrap_or(tail.len());
        let (authority, after) = tail.split_at(end);
        out.push_str(
            authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host),
        );
        rest = after;
    }
    out.push_str(rest);
    let out = out.trim();
    if out.chars().count() > 200 {
        let cut: String = out.chars().take(200).collect();
        format!("{cut}…")
    } else {
        out.to_string()
    }
}

/// The line of git's stderr worth showing.
fn diagnostic(lines: &[String]) -> String {
    let message = lines
        .iter()
        .find(|line| line.starts_with("fatal:") || line.starts_with("error:"))
        .or_else(|| lines.last())
        .map(|line| {
            line.trim_start_matches("fatal:")
                .trim_start_matches("error:")
                .trim()
        })
        .unwrap_or_default();
    if message.is_empty() {
        "git failed without saying why".to_string()
    } else {
        scrub(message)
    }
}

struct Job {
    cancelled: AtomicBool,
}

fn jobs() -> &'static Mutex<HashMap<String, Arc<Job>>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Arc<Job>>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

/// A registered job, forgotten when it ends however it ends.
struct Registered {
    id: String,
    job: Arc<Job>,
}

impl Registered {
    fn new(id: &str) -> Option<Self> {
        let mut table = jobs().lock().ok()?;
        if table.contains_key(id) {
            return None;
        }
        let job = Arc::new(Job {
            cancelled: AtomicBool::new(false),
        });
        table.insert(id.to_string(), job.clone());
        Some(Self {
            id: id.to_string(),
            job,
        })
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        if let Ok(mut table) = jobs().lock() {
            table.remove(&self.id);
        }
    }
}

/// Asks the clone running under `job_id` to stop. `false` when none is.
pub fn cancel_job(job_id: &str) -> bool {
    let Ok(table) = jobs().lock() else {
        return false;
    };
    match table.get(job_id) {
        Some(job) => {
            job.cancelled.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

#[tauri::command]
pub fn git_clone_cancel(job_id: String) -> bool {
    cancel_job(&job_id)
}

struct CloneRequest<'a> {
    url: &'a str,
    parent: &'a str,
    name: &'a str,
    job_id: &'a str,
    /// Extra `-c key=value`, after the protection. Tests only.
    extra_config: Vec<String>,
    timeout: Duration,
}

/// The current branch of the repository at `dir`, `None` when detached.
fn current_branch(dir: &Path) -> Option<String> {
    git_line(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).filter(|branch| !branch.is_empty())
}

/// Whether the folder is a git repository of its own whose origin is `wanted`.
fn is_same_clone(dir: &Path, wanted: &RepoRef) -> bool {
    if !dir.join(".git").exists() {
        return false;
    }
    let Some(top) = git_line(dir, &["rev-parse", "--show-toplevel"]) else {
        return false;
    };
    if comparable(Path::new(&top)) != comparable(dir) {
        return false;
    }
    // `config` rather than `remote get-url`: the latter applies insteadOf.
    let Some(origin) = git_line(dir, &["config", "--get", "remote.origin.url"]) else {
        return false;
    };
    let Ok(found) = parse_repo(&origin) else {
        return false;
    };
    found.host == wanted.host
        && found.owner.eq_ignore_ascii_case(&wanted.owner)
        && found.name == wanted.name
}

fn clone_into(request: &CloneRequest, progress: &dyn Fn(&str, Option<u8>)) -> CloneOutcome {
    let repo = match parse_repo(request.url) {
        Ok(repo) => repo,
        Err(reason) => return refused("invalidUrl", reason),
    };
    if let Some(problem) = folder_name_problem(request.name) {
        return refused("invalidDestination", problem);
    }
    let parent = Path::new(request.parent);
    if !parent.is_absolute() {
        return refused(
            "invalidDestination",
            "the parent folder must be an absolute path",
        );
    }
    match std::fs::metadata(parent) {
        Ok(meta) if meta.is_dir() => {}
        _ => return refused("invalidDestination", "the parent folder does not exist"),
    }
    let Some(registered) = Registered::new(request.job_id) else {
        return refused("busy", "a clone is already running under this job id");
    };
    let dest = parent.join(request.name);

    match std::fs::symlink_metadata(&dest) {
        Ok(meta) => {
            if meta.is_dir() && !is_link(&meta) && is_same_clone(&dest, &repo) {
                return CloneOutcome::Cloned {
                    path: dest.to_string_lossy().into_owned(),
                    reused: true,
                    branch: current_branch(&dest),
                };
            }
            return refused(
                "exists",
                "a different file or folder is already there; nothing was changed",
            );
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return refused("invalidDestination", "the destination cannot be checked"),
    }

    // Created here, atomically: from now on the folder is this call's, and a
    // folder somebody else makes in the meantime fails this and is left alone.
    if let Err(err) = std::fs::create_dir(&dest) {
        return if err.kind() == std::io::ErrorKind::AlreadyExists {
            refused(
                "exists",
                "a file or folder appeared there; nothing was changed",
            )
        } else {
            refused(
                "invalidDestination",
                "the destination folder cannot be created",
            )
        };
    }

    let mut command = git_command();
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .args([
            "-c",
            "protocol.ext.allow=never",
            "-c",
            "protocol.file.allow=never",
        ]);
    if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    for config in &request.extra_config {
        command.args(["-c", config]);
    }
    command
        .args(["clone", "--progress", "--", &repo.url])
        .arg(&dest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            remove_tree(&dest);
            return if err.kind() == std::io::ErrorKind::NotFound {
                refused("gitMissing", "git was not found on PATH")
            } else {
                refused("failed", "git could not be run")
            };
        }
    };

    let (sender, receiver) = mpsc::channel::<String>();
    if let Some(mut stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut pending = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        for &byte in &buffer[..read] {
                            if byte == b'\r' || byte == b'\n' {
                                if !pending.is_empty() {
                                    let line = String::from_utf8_lossy(&pending).into_owned();
                                    pending.clear();
                                    if sender.send(line).is_err() {
                                        return;
                                    }
                                }
                            } else if pending.len() < 4096 {
                                pending.push(byte);
                            }
                        }
                    }
                }
            }
            if !pending.is_empty() {
                let _ = sender.send(String::from_utf8_lossy(&pending).into_owned());
            }
        });
    }

    let mut said: Vec<String> = Vec::new();
    let mut last: Option<(String, u8)> = None;
    let mut take = |line: String, said: &mut Vec<String>| {
        let line = line.trim().to_string();
        if let Some(found) = parse_progress(&line) {
            if last.as_ref() != Some(&found) {
                progress(&found.0, Some(found.1));
                last = Some(found);
            }
        } else if !line.is_empty() {
            said.push(line);
            if said.len() > 20 {
                said.remove(0);
            }
        }
    };

    let started = Instant::now();
    let mut open = true;
    // `Ok(status)` when git ended by itself, `Err(outcome)` when we stopped it.
    let ended: Result<std::process::ExitStatus, CloneOutcome> = loop {
        if open {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => take(line, &mut said),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => open = false,
            }
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
        if registered.job.cancelled.load(Ordering::SeqCst) {
            kill_tree(&mut child);
            let _ = child.wait();
            break Err(refused("cancelled", "the clone was cancelled"));
        }
        if started.elapsed() >= request.timeout {
            kill_tree(&mut child);
            let _ = child.wait();
            break Err(refused(
                "timeout",
                "the clone took too long and was stopped",
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(_) => {
                kill_tree(&mut child);
                let _ = child.wait();
                break Err(refused("failed", "git could not be followed"));
            }
        }
    };

    match ended {
        Ok(status) => {
            // What git said last may still be in the channel.
            while let Ok(line) = receiver.recv_timeout(Duration::from_millis(300)) {
                take(line, &mut said);
            }
            if status.success() {
                CloneOutcome::Cloned {
                    path: dest.to_string_lossy().into_owned(),
                    reused: false,
                    branch: current_branch(&dest),
                }
            } else {
                remove_tree(&dest);
                refused("failed", diagnostic(&said))
            }
        }
        Err(outcome) => {
            remove_tree(&dest);
            outcome
        }
    }
}

#[tauri::command]
pub async fn git_clone(
    app: AppHandle,
    url: String,
    parent: String,
    name: String,
    job_id: String,
) -> CloneOutcome {
    tauri::async_runtime::spawn_blocking(move || {
        let request = CloneRequest {
            url: &url,
            parent: &parent,
            name: &name,
            job_id: &job_id,
            extra_config: Vec::new(),
            timeout: CLONE_TIMEOUT,
        };
        clone_into(&request, &|phase, percent| {
            let _ = app.emit(
                PROGRESS_EVENT,
                CloneProgress {
                    job_id: job_id.clone(),
                    phase: phase.to_string(),
                    percent,
                },
            );
        })
    })
    .await
    .unwrap_or_else(|_| refused("failed", "the clone was interrupted"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ok(input: &str) -> RepoRef {
        parse_repo(input).unwrap_or_else(|why| panic!("{input:?} refused: {why}"))
    }

    #[test]
    fn parse_accepts_the_four_forms() {
        let expect = |input: &str, url: &str, host: &str| {
            let repo = ok(input);
            assert_eq!(repo.url, url, "{input}");
            assert_eq!(repo.host, host, "{input}");
            assert_eq!(repo.owner, "octo", "{input}");
            assert_eq!(repo.name, "hello.world", "{input}");
        };
        expect(
            "https://github.com/octo/hello.world.git",
            "https://github.com/octo/hello.world.git",
            "github.com",
        );
        expect(
            "https://GitHub.com/octo/hello.world",
            "https://GitHub.com/octo/hello.world",
            "github.com",
        );
        expect(
            "https://gitlab.example:8443/octo/hello.world/",
            "https://gitlab.example:8443/octo/hello.world",
            "gitlab.example",
        );
        expect(
            "git@github.com:octo/hello.world.git",
            "git@github.com:octo/hello.world.git",
            "github.com",
        );
        expect(
            "ssh://git@github.com/octo/hello.world.git",
            "ssh://git@github.com/octo/hello.world.git",
            "github.com",
        );
        expect(
            "ssh://git@host.example:2222/octo/hello.world",
            "ssh://git@host.example:2222/octo/hello.world",
            "host.example",
        );
        expect(
            "octo/hello.world",
            "https://github.com/octo/hello.world.git",
            "github.com",
        );
    }

    #[test]
    fn parse_refuses_everything_else() {
        let table = [
            "",
            "ext::sh -c touch% /tmp/pwned",
            "ext::sh",
            "file:///tmp/repo",
            "FILE:///tmp/repo",
            "file:/tmp/repo",
            "/tmp/repo",
            "./repo",
            "../repo",
            "C:\\Users\\me\\repo",
            "C:/Users/me/repo",
            "\\\\server\\share\\repo",
            "~/repo",
            "repo",
            "-upload-pack=evil",
            "--help",
            "-o/x",
            "octo/hello world",
            "octo/hello\tworld",
            " octo/hello",
            "octo/hello ",
            "octo/hello\n",
            "octo/hello\0",
            "octo/hel\u{7}lo",
            "https://github.com/octo/hello\nworld",
            "http://github.com/octo/hello.git",
            "git://github.com/octo/hello.git",
            "ftp://github.com/octo/hello.git",
            "https://user:secret@github.com/octo/hello.git",
            "https://token@github.com/octo/hello.git",
            "https://@github.com/octo/hello.git",
            "ssh://user:secret@github.com/octo/hello.git",
            "ssh://a@b@github.com/octo/hello.git",
            "https://github.com/octo",
            "https://github.com/",
            "https://github.com",
            "https://github.com/octo/hello/extra",
            "https://github.com/octo/.",
            "https://github.com/octo/..",
            "https://github.com/octo/.git",
            "https://github.com/./hello",
            "https://github.com/../hello",
            "https://github.com/octo/he%6clo",
            "https://github.com/octo/hello?x=1",
            "https://github.com/octo/hello#frag",
            "https://github.com/octo/he\\llo",
            "https://-evil.com/octo/hello",
            "https://git hub.com/octo/hello",
            "https://github.com:abc/octo/hello",
            "https://github.com:/octo/hello",
            "https://[::1]/octo/hello",
            "https:///octo/hello",
            "git@github.com:octo",
            "git@github.com:/octo/hello",
            "git@github.com:octo/hello/extra",
            "git@github.com:octo/..",
            "git@github.com:../hello",
            "git@-evil.com:octo/hello",
            "@github.com:octo/hello",
            "a@b@github.com:octo/hello",
            "octo/..",
            "./hello",
            "../hello",
            "octo/hello/extra",
            "/octo/hello",
            "octo//hello",
            "octo/",
            "/hello",
            "octo\\hello",
            "octo/hello@v1",
        ];
        for input in table {
            assert!(parse_repo(input).is_err(), "{input:?} should be refused");
        }
    }

    #[test]
    fn parse_never_echoes_a_credential() {
        for input in [
            "https://user:hunter2@github.com/octo/hello.git",
            "ssh://user:hunter2@github.com/octo/hello.git",
        ] {
            let reason = parse_repo(input).unwrap_err();
            assert!(!reason.contains("hunter2"), "{reason}");
            let json = serde_json::to_string(&git_repo_parse(input.to_string())).unwrap();
            assert!(!json.contains("hunter2"), "{json}");
        }
    }

    #[test]
    fn parse_result_serialises_flat() {
        let json = serde_json::to_value(git_repo_parse("octo/hello".into())).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["owner"], "octo");
        assert_eq!(json["host"], "github.com");
        let json = serde_json::to_value(git_repo_parse("/tmp/x".into())).unwrap();
        assert_eq!(json["status"], "refused");
        assert!(json["reason"].is_string());
    }

    // -- ownership ---------------------------------------------------------

    fn no_orgs() -> Result<Vec<String>, String> {
        panic!("the organisations should not have been asked")
    }

    #[test]
    fn gh_answers_are_read() {
        assert_eq!(
            parse_gh_login(r#"{"login":"simnJS","id":1,"type":"User"}"#).unwrap(),
            "simnJS"
        );
        assert!(parse_gh_login("{}").is_err());
        assert!(parse_gh_login("not json").is_err());
        assert_eq!(
            parse_gh_orgs(r#"[{"login":"acme"},{"login":"labs"}]"#).unwrap(),
            vec!["acme", "labs"]
        );
        // --paginate prints the pages one after the other.
        assert_eq!(
            parse_gh_orgs("[{\"login\":\"a\"}][{\"login\":\"b\"}]\n").unwrap(),
            vec!["a", "b"]
        );
        assert!(parse_gh_orgs("[]").unwrap().is_empty());
        assert!(parse_gh_orgs(r#"{"message":"Not Found"}"#).is_err());
    }

    #[test]
    fn ownership_decisions() {
        let me = Ok("simnJS".to_string());
        let orgs = || Ok(vec!["acme".to_string(), "Labs".to_string()]);

        let own = decide_ownership("github.com", "SIMNJS", &me, &no_orgs);
        assert!(own.mine, "{}", own.reason);
        assert!(decide_ownership("GitHub.com", "acme", &me, &orgs).mine);
        assert!(decide_ownership("github.com", "labs", &me, &orgs).mine);
        let stranger = decide_ownership("github.com", "torvalds", &me, &orgs);
        assert!(!stranger.mine);
        assert!(stranger.reason.contains("torvalds"));
        // Another host is never mine, and gh is not even asked.
        assert!(!decide_ownership("gitlab.com", "simnJS", &me, &no_orgs).mine);
        assert!(!decide_ownership("github.com.evil.example", "simnJS", &me, &no_orgs).mine);
        assert!(!decide_ownership("github.com", "", &me, &no_orgs).mine);
        // gh missing or signed out.
        let missing: Result<String, String> = Err("gh is not installed".into());
        let result = decide_ownership("github.com", "simnJS", &missing, &no_orgs);
        assert!(!result.mine);
        assert_eq!(result.reason, "gh is not installed");
        // The organisations could not be read: not mine, with the reason.
        let failing = || Err::<Vec<String>, _>("gh did not answer in time".to_string());
        let result = decide_ownership("github.com", "acme", &me, &failing);
        assert!(!result.mine);
        assert_eq!(result.reason, "gh did not answer in time");
        // …but a personal repository does not need them.
        assert!(decide_ownership("github.com", "simnJS", &me, &failing).mine);
    }

    // -- progress, scrubbing -------------------------------------------------

    #[test]
    fn progress_lines_are_read() {
        assert_eq!(
            parse_progress("Receiving objects:  45% (45/100), 1.2 MiB | 2 MiB/s"),
            Some(("Receiving objects".to_string(), 45))
        );
        assert_eq!(
            parse_progress("remote: Counting objects: 100% (5/5), done."),
            Some(("Counting objects".to_string(), 100))
        );
        assert_eq!(parse_progress("Cloning into 'x'..."), None);
        assert_eq!(parse_progress("fatal: repository not found"), None);
    }

    #[test]
    fn scrub_drops_credentials_and_length() {
        assert_eq!(
            scrub("unable to access 'https://me:pw@github.com/o/r.git/': nope"),
            "unable to access 'https://github.com/o/r.git/': nope"
        );
        assert_eq!(scrub("ssh://tok@host/x failed"), "ssh://host/x failed");
        assert_eq!(scrub("plain"), "plain");
        assert!(scrub(&"x".repeat(500)).chars().count() <= 201);
    }

    #[test]
    fn folder_names() {
        for bad in [
            "", ".", "..", "a/b", "a\\b", "-x", "a:b", "con", "NUL.txt", "com1", "a.", "a ", "a\nb",
        ] {
            assert!(folder_name_problem(bad).is_some(), "{bad:?}");
        }
        for good in ["repo", "hello.world", "my-repo_2", "console"] {
            assert!(folder_name_problem(good).is_none(), "{good:?}");
        }
    }

    // -- real clones -----------------------------------------------------------

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hangar-clone-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        PathBuf::from(
            std::fs::canonicalize(&dir)
                .expect("canonical")
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_string(),
        )
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = git_command()
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    /// A bare repository with one commit on `main`.
    fn bare_source(root: &Path) -> PathBuf {
        let work = root.join("seed");
        std::fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        std::fs::write(work.join("README.md"), "hello\n").unwrap();
        git(&work, &["add", "."]);
        git(
            &work,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.test",
                "commit",
                "-q",
                "-m",
                "first",
            ],
        );
        let bare = root.join("source.git");
        git(
            root,
            &[
                "clone",
                "-q",
                "--bare",
                work.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        bare
    }

    const FAKE: &str = "https://example.test/octo/hello.git";

    /// The test-only bypass: `insteadOf` sends the fake address to the local
    /// bare repository, and the file protocol is allowed again after the
    /// protection that `git_clone` always passes.
    fn via(bare: &Path) -> Vec<String> {
        vec![
            format!(
                "url.{}.insteadOf={FAKE}",
                bare.to_string_lossy().replace('\\', "/")
            ),
            "protocol.file.allow=always".to_string(),
        ]
    }

    fn request<'a>(
        parent: &'a str,
        name: &'a str,
        job: &'a str,
        extra: Vec<String>,
    ) -> CloneRequest<'a> {
        CloneRequest {
            url: FAKE,
            parent,
            name,
            job_id: job,
            extra_config: extra,
            timeout: Duration::from_secs(60),
        }
    }

    fn job_id(tag: &str) -> String {
        format!("{tag}-{}", uuid::Uuid::new_v4().simple())
    }

    #[test]
    fn clones_from_a_local_bare_repository_and_reports_progress() {
        let root = temp_dir("ok");
        let bare = bare_source(&root);
        let parent = root.join("projects");
        std::fs::create_dir_all(&parent).unwrap();
        let (parent_s, id) = (parent.to_string_lossy().into_owned(), job_id("ok"));
        let seen = Mutex::new(Vec::new());
        let outcome = clone_into(
            &request(&parent_s, "hello", &id, via(&bare)),
            &|phase, pct| {
                seen.lock().unwrap().push((phase.to_string(), pct));
            },
        );
        let CloneOutcome::Cloned {
            path,
            reused,
            branch,
        } = outcome
        else {
            panic!("{outcome:?}");
        };
        assert!(!reused);
        assert_eq!(branch.as_deref(), Some("main"));
        assert!(Path::new(&path).join("README.md").is_file());
        // The protection is not left in the clone's config, and the address
        // kept is the one given, not the rewritten one.
        let origin = git_line(Path::new(&path), &["config", "--get", "remote.origin.url"]);
        assert_eq!(origin.as_deref(), Some(FAKE));
        // The job is forgotten.
        assert!(!cancel_job(&id));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reuses_the_same_repository_and_touches_nothing() {
        let root = temp_dir("reuse");
        let bare = bare_source(&root);
        let parent = root.join("projects");
        std::fs::create_dir_all(&parent).unwrap();
        let parent_s = parent.to_string_lossy().into_owned();
        let first = clone_into(
            &request(&parent_s, "hello", &job_id("a"), via(&bare)),
            &|_, _| {},
        );
        assert!(matches!(first, CloneOutcome::Cloned { reused: false, .. }));
        let marker = parent.join("hello").join("local-work.txt");
        std::fs::write(&marker, "mine").unwrap();

        // Same repository spelled differently: no .git, other case on host
        // and owner. No extra config: nothing may reach the network.
        for url in [
            "https://EXAMPLE.test/OCTO/hello",
            "git@example.test:octo/hello.git",
            "octo/hello",
        ] {
            let again = clone_into(
                &CloneRequest {
                    url,
                    ..request(&parent_s, "hello", &job_id("b"), Vec::new())
                },
                &|_, _| {},
            );
            // `octo/hello` is github.com, not example.test: refused.
            if url == "octo/hello" {
                assert!(
                    matches!(again, CloneOutcome::Refused { kind: "exists", .. }),
                    "{again:?}"
                );
            } else {
                match again {
                    CloneOutcome::Cloned {
                        reused: true,
                        branch,
                        ..
                    } => {
                        assert_eq!(branch.as_deref(), Some("main"))
                    }
                    other => panic!("{url}: {other:?}"),
                }
            }
        }
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "mine");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn another_repository_or_folder_is_refused_and_left_alone() {
        let root = temp_dir("refuse");
        let bare = bare_source(&root);
        let parent = root.join("projects");
        std::fs::create_dir_all(&parent).unwrap();
        let parent_s = parent.to_string_lossy().into_owned();

        // A plain folder with a file.
        let plain = parent.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("note.txt"), "keep").unwrap();
        // A plain file.
        std::fs::write(parent.join("afile"), "keep").unwrap();
        // An empty folder is not ours either.
        std::fs::create_dir_all(parent.join("empty")).unwrap();
        // A repository of another origin.
        let other = parent.join("other");
        std::fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-q"]);
        git(
            &other,
            &[
                "remote",
                "add",
                "origin",
                "https://example.test/octo/different.git",
            ],
        );
        // A repository with no origin.
        let nothing = parent.join("nothing");
        std::fs::create_dir_all(&nothing).unwrap();
        git(&nothing, &["init", "-q"]);
        // A folder inside a repository is not a repository root.
        let inner = other.join("sub");
        std::fs::create_dir_all(&inner).unwrap();

        for (name, parent_dir) in [
            ("plain", parent_s.clone()),
            ("afile", parent_s.clone()),
            ("empty", parent_s.clone()),
            ("other", parent_s.clone()),
            ("nothing", parent_s.clone()),
            ("sub", other.to_string_lossy().into_owned()),
        ] {
            let outcome = clone_into(
                &request(&parent_dir, name, &job_id("r"), via(&bare)),
                &|_, _| {},
            );
            assert!(
                matches!(outcome, CloneOutcome::Refused { kind: "exists", .. }),
                "{name}: {outcome:?}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(plain.join("note.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            std::fs::read_to_string(parent.join("afile")).unwrap(),
            "keep"
        );
        assert!(parent.join("empty").is_dir());
        assert!(other.join(".git").is_dir() && nothing.join(".git").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bad_requests_are_refused_before_anything_is_made() {
        let root = temp_dir("bad");
        let parent_s = root.to_string_lossy().into_owned();
        let check = |url: &str, parent: &str, name: &str, kind: &str| {
            let outcome = clone_into(
                &CloneRequest {
                    url,
                    ..request(parent, name, &job_id("x"), Vec::new())
                },
                &|_, _| {},
            );
            match outcome {
                CloneOutcome::Refused { kind: found, .. } => {
                    assert_eq!(found, kind, "{url} {name}")
                }
                other => panic!("{other:?}"),
            }
        };
        check("file:///tmp/x", &parent_s, "x", "invalidUrl");
        check("ext::sh -c id", &parent_s, "x", "invalidUrl");
        check("-evil", &parent_s, "x", "invalidUrl");
        check(FAKE, &parent_s, "..", "invalidDestination");
        check(FAKE, &parent_s, "a/b", "invalidDestination");
        check(FAKE, "relative/dir", "x", "invalidDestination");
        check(
            FAKE,
            &root.join("missing").to_string_lossy(),
            "x",
            "invalidDestination",
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);

        // A job id in use is refused.
        let id = job_id("busy");
        let _held = Registered::new(&id).unwrap();
        let outcome = clone_into(&request(&parent_s, "x", &id, Vec::new()), &|_, _| {});
        assert!(matches!(
            outcome,
            CloneOutcome::Refused { kind: "busy", .. }
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn protection_blocks_file_and_ext_without_the_test_override() {
        let root = temp_dir("protect");
        let bare = bare_source(&root);
        let parent_s = root.join("projects").to_string_lossy().into_owned();
        std::fs::create_dir_all(&parent_s).unwrap();
        // Only the insteadOf, without re-allowing the file protocol.
        let extra = vec![format!(
            "url.{}.insteadOf={FAKE}",
            bare.to_string_lossy().replace('\\', "/")
        )];
        let outcome = clone_into(
            &request(&parent_s, "hello", &job_id("p"), extra),
            &|_, _| {},
        );
        match outcome {
            CloneOutcome::Refused {
                kind: "failed",
                reason,
            } => {
                assert!(reason.to_lowercase().contains("not allowed"), "{reason}")
            }
            other => panic!("{other:?}"),
        }
        assert!(!Path::new(&parent_s).join("hello").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_clone_removes_only_what_it_made() {
        let root = temp_dir("fail");
        let parent = root.join("projects");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(parent.join("neighbour.txt"), "keep").unwrap();
        std::fs::create_dir_all(parent.join("neighbour")).unwrap();
        let parent_s = parent.to_string_lossy().into_owned();
        // The address leads to a repository that does not exist.
        let missing = root.join("nope.git");
        let outcome = clone_into(
            &request(&parent_s, "hello", &job_id("f"), via(&missing)),
            &|_, _| {},
        );
        match outcome {
            CloneOutcome::Refused {
                kind: "failed",
                reason,
            } => {
                assert!(!reason.is_empty() && reason.chars().count() <= 201);
            }
            other => panic!("{other:?}"),
        }
        assert!(!parent.join("hello").exists());
        assert!(parent.join("neighbour.txt").is_file());
        assert!(parent.join("neighbour").is_dir());
        assert!(parent.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removing_a_tree_never_follows_a_link() {
        let root = temp_dir("link");
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precious.txt"), "keep").unwrap();
        let made = root.join("made");
        std::fs::create_dir_all(made.join("inner")).unwrap();
        std::fs::write(made.join("inner").join("file.txt"), "x").unwrap();
        let readonly = made.join("ro.txt");
        std::fs::write(&readonly, "x").unwrap();
        let mut permissions = std::fs::metadata(&readonly).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&readonly, permissions).unwrap();

        let link = made.join("link");
        let linked = {
            #[cfg(windows)]
            {
                // A junction, which needs no privilege.
                Command::new("cmd")
                    .args(["/C", "mklink", "/J"])
                    .arg(&link)
                    .arg(&outside)
                    .stdout(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&outside, &link).is_ok()
            }
        };
        assert!(linked, "could not make a link for the test");
        remove_tree(&made);
        assert!(!made.exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("precious.txt")).unwrap(),
            "keep"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cancel_stops_the_clone_and_removes_the_folder() {
        let root = temp_dir("cancel");
        let parent = root.join("projects");
        std::fs::create_dir_all(&parent).unwrap();
        let parent_s = parent.to_string_lossy().into_owned();
        // A remote that never answers: git runs `sleep` through its shell.
        let hang = vec![
            "protocol.ext.allow=always".to_string(),
            format!("url.ext::sleep 60.insteadOf={FAKE}"),
        ];
        let id = job_id("cancel");
        let canceller = {
            let id = id.clone();
            std::thread::spawn(move || {
                // Let git start and hang, then cancel.
                std::thread::sleep(Duration::from_millis(700));
                assert!(cancel_job(&id), "the job should be running");
            })
        };
        let started = Instant::now();
        let outcome = clone_into(&request(&parent_s, "hello", &id, hang), &|_, _| {});
        canceller.join().unwrap();
        assert!(
            matches!(
                outcome,
                CloneOutcome::Refused {
                    kind: "cancelled",
                    ..
                }
            ),
            "{outcome:?}"
        );
        assert!(started.elapsed() >= Duration::from_millis(600));
        assert!(started.elapsed() < Duration::from_secs(30));
        assert!(!parent.join("hello").exists());
        assert!(parent.is_dir());
        // Cancelling what is not running says so.
        assert!(!cancel_job(&id));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_timeout_stops_the_clone_and_removes_the_folder() {
        let root = temp_dir("timeout");
        let parent = root.join("projects");
        std::fs::create_dir_all(&parent).unwrap();
        let parent_s = parent.to_string_lossy().into_owned();
        let hang = vec![
            "protocol.ext.allow=always".to_string(),
            format!("url.ext::sleep 60.insteadOf={FAKE}"),
        ];
        let outcome = clone_into(
            &CloneRequest {
                timeout: Duration::from_millis(600),
                ..request(&parent_s, "hello", &job_id("t"), hang)
            },
            &|_, _| {},
        );
        assert!(
            matches!(
                outcome,
                CloneOutcome::Refused {
                    kind: "timeout",
                    ..
                }
            ),
            "{outcome:?}"
        );
        assert!(!parent.join("hello").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
