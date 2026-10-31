//! « Comptes Claude »: several Claude subscriptions on one machine, one
//! Claude Code profile per account, and every Claude Code pane launched on
//! the account chosen for it.
//!
//! # Profiles
//!
//! Claude Code reads its login and its configuration from the folder named
//! by `CLAUDE_CONFIG_DIR` — `~/.claude`, with `~/.claude.json` beside it,
//! when the variable is not set; under the variable, `.claude.json` lives
//! inside the folder. An account is such a folder:
//!
//! - **main** is Claude Code's own configuration, the one Hangar was started
//!   with (`CLAUDE_CONFIG_DIR` if it was set, `~/.claude` otherwise). It is
//!   always there, cannot be removed, and its files are only ever read.
//! - every other account is a profile Hangar made, `~/.hangar/accounts/<id>/`
//!   (beside `HANGAR_ENDPOINT_FILE` for a development build, like the mods of
//!   bridge.rs), listed in `~/.hangar/accounts/accounts.json` as
//!   `{ id, label, dir, addedAt }` (store.rs).
//!
//! A Claude Code pane on a profile is started with `CLAUDE_CONFIG_DIR` set to
//! its folder (pty.rs); a pane on main, and every pane that is not Claude
//! Code, gets the environment Hangar has. Nothing is ever switched without
//! the person: there is no rotation when a limit is reached, here or
//! anywhere else.
//!
//! # Same setup, own login
//!
//! A profile holds its own login — `.credentials.json` (or its own Keychain
//! entry on macOS) and its own `.claude.json`, where Claude Code keeps the
//! account it is logged in as — and takes everything else from main
//! (share.rs), before each launch:
//!
//! - **Folders** are links to main's: a junction on Windows, which needs no
//!   privilege, a symbolic link elsewhere. Claude Code writes through them
//!   as through any folder, so a conversation started on one account is
//!   filed in main's `projects/` and `claude --resume <id>` finds it from
//!   every account. Linked: `projects`, `file-history`, `todos`, `plans` (a
//!   conversation and what it brings with it), `skills`, `commands`,
//!   `agents`, `output-styles`, `rules`, `plugins` (the person's tools). A
//!   folder main does not have is not linked, nor created on main.
//! - **Files** Claude Code rewrites in place are copied, one way, from main:
//!   `settings.json`, `CLAUDE.md`, `keybindings.json`. A link would not do:
//!   Claude Code 2.1.316 refuses to write its settings through a symbolic
//!   link, and its atomic rewrites break a hard link. A change made from a
//!   pane on a profile is therefore undone at the next launch: main is where
//!   the setup is changed.
//! - **`.claude.json`** is the profile's own, but its setup keys are copied
//!   from main's: the user-scope MCP servers (`mcpServers`), and for each
//!   folder main knows, the trust given to it — an agent would otherwise
//!   wait on the trust dialog — its local-scope servers and the choices made
//!   about its `.mcp.json` servers. The login, the account and everything
//!   else Claude Code keeps there are never touched. Hangar does not take
//!   Claude Code's lock on that file: it renames its version over it only if
//!   the file has not moved since it was read, and reads it again otherwise.
//!
//! Nothing of main is modified, logged, or copied anywhere but into the
//! profile, which lives beside it in the person's home. Since every
//! conversation lands in main's `projects/`, whatever reads transcripts —
//! the session list and resume (sessions.rs), the statistics (stats.rs), the
//! context gauge and the phone's chat (context.rs) — sees every account's
//! by reading main's alone, and counts each once. A profile whose folder
//! could not be linked keeps its own; the session list and the statistics
//! read it too (`projects_dirs`).
//!
//! # macOS
//!
//! On macOS Claude Code keeps the login in the Keychain, not in
//! `.credentials.json`. Claude Code 2.1.316 names the entry
//! `Claude Code-credentials` for the default folder and
//! `Claude Code-credentials-<first 8 hex digits of sha256(folder)>` when
//! `CLAUDE_CONFIG_DIR` is set (read in its bundle), so each profile has an
//! entry of its own and logins stay apart. `CLAUDE_SECURESTORAGE_CONFIG_DIR`
//! overrides that name, so it is taken out of a profile pane's environment.
//! An older Claude Code that kept one entry for every folder would log a
//! profile in over main: `accounts_support` checks that each logged-in
//! profile has its own entry and says plainly when one does not
//! (login.rs).
//!
//! # Logging in
//!
//! `account_add` prepares the profile and answers with `claude auth login`,
//! which the person runs in a visible Claude Code pane on that account —
//! `Pane.login` in src/types.ts — and finishes in their browser. Hangar
//! never logs in, out, or switches an account by itself. `accounts:changed`
//! is emitted after every change made here, and when a profile's login
//! appears, changes or goes (`watch`).

mod link;
mod login;
mod share;
mod store;
#[cfg(test)]
mod testing;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

pub use share::Issue;
use share::{stamp, Stamp};
use store::Store;

/// The account that is Claude Code's own configuration.
pub const MAIN: &str = "main";
/// The agent an account applies to.
pub const CLAUDE_AGENT: &str = "claude";
pub const CHANGED_EVENT: &str = "accounts:changed";
/// What logs a profile in, typed in a pane whose Claude Code runs on it
/// (checked against Claude Code 2.1.316, which also has `claude` then
/// `/login`).
pub const LOGIN_COMMAND: &str = "claude auth login";
/// How often the logins are looked at for `accounts:changed`.
const WATCH_EVERY: Duration = Duration::from_secs(3);
/// Claude Code keeps tokens in the Keychain on macOS, in
/// `.credentials.json` elsewhere.
const KEYCHAIN: bool = cfg!(target_os = "macos");

/// A Claude Code configuration: the folder `CLAUDE_CONFIG_DIR` names, and
/// the `.claude.json` that goes with it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeHome {
    pub dir: PathBuf,
    pub config: PathBuf,
}

impl ClaudeHome {
    /// The main account: Claude Code's default, or the `CLAUDE_CONFIG_DIR`
    /// Hangar was started with. Hangar never sets the variable on itself,
    /// only on the panes it starts.
    pub fn main() -> Option<Self> {
        let custom = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from);
        Some(Self::main_at(&dirs::home_dir()?, custom))
    }

    fn main_at(home: &Path, custom: Option<PathBuf>) -> Self {
        let dir = custom.clone().unwrap_or_else(|| home.join(".claude"));
        let parent = custom.unwrap_or_else(|| home.to_path_buf());
        Self::with_config(dir, &parent)
    }

    /// A profile: `.claude.json` inside its folder, as under
    /// `CLAUDE_CONFIG_DIR`.
    pub fn profile(dir: PathBuf) -> Self {
        let parent = dir.clone();
        Self::with_config(dir, &parent)
    }

    /// A configuration written by old versions, `.config.json` in the folder,
    /// is the one Claude Code still prefers when it is there.
    fn with_config(dir: PathBuf, parent: &Path) -> Self {
        let legacy = dir.join(".config.json");
        let config = if legacy.is_file() {
            legacy
        } else {
            parent.join(".claude.json")
        };
        ClaudeHome { dir, config }
    }

    pub fn settings(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    pub fn credentials(&self) -> PathBuf {
        self.dir.join(".credentials.json")
    }

    pub fn projects(&self) -> PathBuf {
        self.dir.join("projects")
    }
}

/// `~/.hangar/accounts/`, or beside the endpoint file a development build
/// run next to an installed Hangar was given (endpoint.rs), so that it does
/// not adopt the installed app's profiles.
fn store_root() -> Option<PathBuf> {
    if let Some(file) = std::env::var_os("HANGAR_ENDPOINT_FILE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(file).parent()?.join("accounts"));
    }
    Some(dirs::home_dir()?.join(".hangar").join("accounts"))
}

fn homes() -> Result<(ClaudeHome, Store), String> {
    match (ClaudeHome::main(), store_root()) {
        (Some(main), Some(root)) => Ok((main, Store::at(root))),
        _ => Err("No home directory on this system.".into()),
    }
}

/// Every change to the index or to a profile's folder goes through here, so
/// two panes starting on one profile prepare it one after the other.
fn lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

/// One account, as the window lists it. The email and the plan come from the
/// account's own files on this machine and go nowhere else.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    pub is_main: bool,
    pub logged_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

/// Logins already read, by folder, with the stamps of the two files they
/// were read from: polling every few seconds reads nothing that did not
/// move.
fn read_login(home: &ClaudeHome) -> login::Login {
    type Cache = HashMap<PathBuf, (Option<Stamp>, Option<Stamp>, login::Login)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let stamps = (stamp(&home.config), stamp(&home.credentials()));
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((config, credentials, login)) = cache.get(&home.dir) {
        if (*config, *credentials) == stamps {
            return login.clone();
        }
    }
    let login = login::read_login(home, KEYCHAIN);
    cache.insert(home.dir.clone(), (stamps.0, stamps.1, login.clone()));
    login
}

fn account(id: &str, label: &str, home: &ClaudeHome) -> Account {
    let login = read_login(home);
    Account {
        id: id.to_string(),
        label: label.to_string(),
        is_main: id == MAIN,
        logged_in: login.logged_in,
        email: login.email,
        plan: login.plan,
    }
}

/// Main first, then the profiles in the order they were added. An index
/// nobody can read lists main alone rather than nothing.
fn list_with(main: &ClaudeHome, store: &Store) -> Vec<Account> {
    let index = store.read().unwrap_or_else(|err| {
        eprintln!("accounts: {err}");
        store::Index::default()
    });
    std::iter::once(account(MAIN, index.main_label(), main))
        .chain(index.accounts.iter().map(|entry| {
            account(
                &entry.id,
                &entry.label,
                &ClaudeHome::profile(store.dir_of(&entry.id)),
            )
        }))
        .collect()
}

fn list() -> Vec<Account> {
    match homes() {
        Ok((main, store)) => list_with(&main, &store),
        Err(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// What the other modules ask
// ---------------------------------------------------------------------------

/// The account a pane is launched on.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    /// The account it really runs on: main when the one asked for is gone.
    pub id: String,
    /// What `CLAUDE_CONFIG_DIR` is set to; `None` on main, which keeps the
    /// environment Hangar has.
    pub dir: Option<PathBuf>,
    /// What the profile could not take from main this time.
    pub issues: Vec<Issue>,
}

impl Launch {
    fn main() -> Self {
        Launch {
            id: MAIN.to_string(),
            dir: None,
            issues: Vec::new(),
        }
    }
}

fn chosen(account: Option<&str>) -> Option<&str> {
    account
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != MAIN)
}

/// The account a pane of `agent` starts on, its profile prepared: `None` for
/// a pane that is not Claude Code, which no account applies to. An id that
/// is no longer listed — removed since the window chose it — runs on main.
pub fn for_launch(agent: Option<&str>, account: Option<&str>) -> Option<Launch> {
    if agent != Some(CLAUDE_AGENT) {
        return None;
    }
    let Some(id) = chosen(account) else {
        return Some(Launch::main());
    };
    let Ok((main, store)) = homes() else {
        return Some(Launch::main());
    };
    Some(launch_with(&main, &store, id))
}

fn launch_with(main: &ClaudeHome, store: &Store, id: &str) -> Launch {
    let _guard = lock();
    let listed = store
        .read()
        .is_ok_and(|index| index.accounts.iter().any(|entry| entry.id == id));
    if !listed {
        return Launch::main();
    }
    let profile = ClaudeHome::profile(store.dir_of(id));
    let issues = share::prepare(main, &profile);
    Launch {
        id: id.to_string(),
        dir: Some(profile.dir),
        issues,
    }
}

/// The configuration a Claude Code pane on `account` reads, for the MCP
/// servers its workspace chose (mcp_scope.rs): main's for main, an unknown
/// id or none; the profile's otherwise, `prepared` first when the file
/// written from it is about to be used.
pub fn home_for(account: Option<&str>, prepared: bool) -> Option<ClaudeHome> {
    let main = ClaudeHome::main()?;
    let Some(id) = chosen(account) else {
        return Some(main);
    };
    let Some(root) = store_root() else {
        return Some(main);
    };
    let store = Store::at(root);
    if prepared {
        return Some(match launch_with(&main, &store, id).dir {
            Some(dir) => ClaudeHome::profile(dir),
            None => main,
        });
    }
    let listed = store
        .read()
        .is_ok_and(|index| index.accounts.iter().any(|entry| entry.id == id));
    Some(if listed {
        ClaudeHome::profile(store.dir_of(id))
    } else {
        main
    })
}

/// Every folder Claude Code files conversations in, each once: main's
/// `projects/`, which every profile's links to, and the `projects/` of a
/// profile that could not be linked and kept its own.
pub fn projects_dirs() -> Vec<PathBuf> {
    match homes() {
        Ok((main, store)) => projects_with(&main, &store),
        Err(_) => Vec::new(),
    }
}

fn projects_with(main: &ClaudeHome, store: &Store) -> Vec<PathBuf> {
    let mut dirs = vec![main.projects()];
    let profiles = store.read().map(|index| index.accounts).unwrap_or_default();
    for entry in profiles {
        let projects = ClaudeHome::profile(store.dir_of(&entry.id)).projects();
        if projects.is_dir() && !dirs.iter().any(|known| link::same_place(known, &projects)) {
            dirs.push(projects);
        }
    }
    dirs
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct LoginCommand {
    command: String,
}

#[derive(Debug, Serialize)]
pub struct Added {
    account: Account,
    login: LoginCommand,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Support {
    isolated_logins: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

/// The last list told to the windows, so that the watch does not tell a
/// change made here a second time.
fn last_told() -> &'static Mutex<Option<Vec<Account>>> {
    static LAST: Mutex<Option<Vec<Account>>> = Mutex::new(None);
    &LAST
}

fn tell(app: &AppHandle) {
    let now = list();
    if let Ok(mut last) = last_told().lock() {
        *last = Some(now);
    }
    let _ = app.emit(CHANGED_EVENT, ());
}

#[tauri::command]
pub fn accounts_list() -> Vec<Account> {
    list()
}

/// A new profile named `label`, ready for its login. An account that could
/// not share main's conversations is not made: a conversation could not
/// move to it or from it.
#[tauri::command]
pub fn account_add(app: AppHandle, label: String) -> Result<Added, String> {
    let (main, store) = homes()?;
    let added = add_with(&main, &store, &label, now_ms())?;
    tell(&app);
    Ok(added)
}

fn add_with(main: &ClaudeHome, store: &Store, label: &str, now: u64) -> Result<Added, String> {
    let _guard = lock();
    let entry = store.add(label, now)?;
    let profile = ClaudeHome::profile(store.dir_of(&entry.id));
    let issues = share::prepare(main, &profile);
    if let Some(issue) = issues
        .iter()
        .find(|issue| issue.item == "projects" || issue.item == "profile")
    {
        let _ = store.forget(&entry.id);
        delete_tree(&profile.dir);
        return Err(format!(
            "The account could not share the conversations of the main one: {}",
            issue.reason
        ));
    }
    Ok(Added {
        account: account(&entry.id, &entry.label, &profile),
        login: LoginCommand {
            command: LOGIN_COMMAND.to_string(),
        },
    })
}

#[tauri::command]
pub fn account_rename(app: AppHandle, id: String, label: String) -> Result<Account, String> {
    let (main, store) = homes()?;
    let label = {
        let _guard = lock();
        store.rename(&id, &label)?
    };
    let home = if id == MAIN {
        main
    } else {
        ClaudeHome::profile(store.dir_of(&id))
    };
    let renamed = account(&id, &label, &home);
    tell(&app);
    Ok(renamed)
}

/// Deletes a profile Hangar made — its folder, its login — once the window
/// has asked the person. Never main, nothing the links lead to, and not
/// while a pane still runs on it.
#[tauri::command]
pub fn account_remove(
    app: AppHandle,
    manager: State<'_, crate::pty::PtyManager>,
    id: String,
) -> Result<(), String> {
    let running = crate::pty::panes_on(&manager, &id);
    if running > 0 {
        return Err(format!(
            "{running} pane(s) still run on this account: switch or close them first."
        ));
    }
    let (main, store) = homes()?;
    remove_with(&main, &store, &id, now_ms())?;
    tell(&app);
    Ok(())
}

fn remove_with(main: &ClaudeHome, store: &Store, id: &str, now: u64) -> Result<(), String> {
    if id == MAIN {
        return Err("The main account cannot be removed.".into());
    }
    let _guard = lock();
    let index = store.read()?;
    if !store::valid_id(id) || !index.accounts.iter().any(|entry| entry.id == id) {
        return Err("No such account.".into());
    }
    let dir = store.dir_of(id);
    if link::same_place(&dir, &main.dir) || main.dir.starts_with(&dir) {
        return Err("This account's folder is the main one's: it is not removed.".into());
    }
    // Moved aside in one step first: a folder still in use stays whole.
    let trash = store.trash(id, now);
    if link::exists(&dir) {
        fs::rename(&dir, &trash).map_err(|err| {
            format!("The account's folder is in use ({err}): close what uses it and try again.")
        })?;
    }
    if let Err(err) = store.forget(id) {
        let _ = fs::rename(&trash, &dir);
        return Err(err);
    }
    delete_tree(&trash);
    Ok(())
}

/// A profile's folder, deleted: its links taken out first, so nothing is
/// ever removed through one. Only ever called on a folder of the store.
fn delete_tree(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if link::is_link(&path) {
                let _ = link::remove_link(&path);
            }
        }
    }
    let _ = fs::remove_dir_all(dir);
}

/// Whether logging in to one account here leaves the others logged in.
#[tauri::command]
pub fn accounts_support() -> Support {
    if !KEYCHAIN {
        return Support {
            isolated_logins: true,
            note: None,
        };
    }
    let profiles: Vec<ClaudeHome> = match homes() {
        Ok((_, store)) => store
            .read()
            .map(|index| index.accounts)
            .unwrap_or_default()
            .iter()
            .map(|entry| ClaudeHome::profile(store.dir_of(&entry.id)))
            .collect(),
        Err(_) => Vec::new(),
    };
    support_from(login::logins_isolated(&profiles, login::keychain_has))
}

fn support_from(isolated: bool) -> Support {
    Support {
        isolated_logins: isolated,
        note: (!isolated).then(|| {
            "This Claude Code keeps a single Keychain login for every account. \
             Update Claude Code (2.1.316 keeps one per account), then log in to each account again."
                .to_string()
        }),
    }
}

/// Tells the windows when a profile's login appears, changes or goes —
/// the person logged in from a pane, or out — by looking at the accounts'
/// files every few seconds. Also deletes what a removal could not.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        if let Some(root) = store_root() {
            for trash in Store::at(root).trashed() {
                delete_tree(&trash);
            }
        }
        loop {
            let now = list();
            let changed = {
                let mut last = last_told()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let changed = last.as_ref().is_some_and(|last| *last != now);
                *last = Some(now);
                changed
            };
            if changed {
                let _ = app.emit(CHANGED_EVENT, ());
            }
            std::thread::sleep(WATCH_EVERY);
        }
    });
}

/// The variables a pane gets for its account: `CLAUDE_CONFIG_DIR` for a
/// Claude Code pane on a profile, and nothing for any other — those keep
/// the environment Hangar has, its own `CLAUDE_CONFIG_DIR` included.
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` would make every profile share one
/// Keychain login on macOS, so a profile pane goes without it.
pub fn env_for(agent: Option<&str>, dir: Option<&Path>) -> Vec<(&'static str, Option<PathBuf>)> {
    match (agent, dir) {
        (Some(CLAUDE_AGENT), Some(dir)) => vec![
            ("CLAUDE_CONFIG_DIR", Some(dir.to_path_buf())),
            ("CLAUDE_SECURESTORAGE_CONFIG_DIR", None),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testing::Scratch;

    fn store(scratch: &Scratch) -> Store {
        Store::at(scratch.root.join("accounts"))
    }

    fn furnished(scratch: &Scratch) -> ClaudeHome {
        let main = scratch.main_home();
        scratch.file("home/.claude/projects/C--repo/conv-1.jsonl", "{}\n");
        scratch.file("home/.claude/skills/review/SKILL.md", "skill");
        scratch.file("home/.claude/settings.json", "{}");
        scratch.file(
            "home/.claude/.credentials.json",
            r#"{"claudeAiOauth":{"accessToken":"main","subscriptionType":"pro"}}"#,
        );
        scratch.file(
            "home/.claude.json",
            r#"{"oauthAccount":{"emailAddress":"main@example.com"}}"#,
        );
        main
    }

    #[test]
    fn main_is_claude_codes_default_or_the_folder_hangar_was_given() {
        let home = Path::new("/home/me");
        let main = ClaudeHome::main_at(home, None);
        assert_eq!(main.dir, home.join(".claude"));
        assert_eq!(main.config, home.join(".claude.json"));
        let custom = ClaudeHome::main_at(home, Some(PathBuf::from("/cfg")));
        assert_eq!(custom.dir, PathBuf::from("/cfg"));
        assert_eq!(custom.config, PathBuf::from("/cfg/.claude.json"));
        let profile = ClaudeHome::profile(PathBuf::from("/p/work"));
        assert_eq!(profile.config, PathBuf::from("/p/work/.claude.json"));
    }

    #[test]
    fn an_added_account_is_listed_after_main_and_logged_out() {
        let scratch = Scratch::new("add");
        let main = furnished(&scratch);
        let store = store(&scratch);
        let added = add_with(&main, &store, "Work", 7).unwrap();
        assert_eq!(added.login.command, "claude auth login");
        assert_eq!(added.account.id, "work");
        assert!(!added.account.logged_in);

        let listed = list_with(&main, &store);
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed[0],
            Account {
                id: "main".into(),
                label: "Main".into(),
                is_main: true,
                logged_in: !KEYCHAIN,
                email: (!KEYCHAIN).then(|| "main@example.com".to_string()),
                plan: (!KEYCHAIN).then(|| "pro".to_string()),
            }
        );
        assert_eq!(listed[1].id, "work");
        assert!(!listed[1].is_main);
        // Its folder is ready: the conversations and tools of main are there.
        let dir = store.dir_of("work");
        assert!(dir.join("projects/C--repo/conv-1.jsonl").is_file());
        assert!(dir.join("skills/review/SKILL.md").is_file());

        // The serialized shape the window reads (src/lib/accounts.ts).
        let json = serde_json::to_value(&added).unwrap();
        assert_eq!(json["login"]["command"], "claude auth login");
        assert_eq!(json["account"]["isMain"], false);
        assert_eq!(json["account"]["loggedIn"], false);
        assert!(json["account"].get("email").is_none());
    }

    #[test]
    fn removing_a_profile_leaves_main_and_the_shared_data_intact() {
        let scratch = Scratch::new("remove");
        let main = furnished(&scratch);
        let store = store(&scratch);
        add_with(&main, &store, "Work", 1).unwrap();
        add_with(&main, &store, "Perso", 2).unwrap();
        let work = store.dir_of("work");
        // A conversation written on the account, and its own login.
        scratch.file("accounts/work/projects/C--repo/conv-2.jsonl", "{}\n");
        scratch.file(
            "accounts/work/.credentials.json",
            r#"{"claudeAiOauth":{"accessToken":"w"}}"#,
        );

        assert!(remove_with(&main, &store, "main", 3).is_err());
        assert!(remove_with(&main, &store, "../home", 3).is_err());
        assert!(remove_with(&main, &store, "nobody", 3).is_err());
        remove_with(&main, &store, "work", 3).unwrap();

        assert!(!link::exists(&work));
        assert!(store.trashed().is_empty());
        let ids: Vec<String> = list_with(&main, &store).into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["main", "perso"]);
        // Everything shared is still main's, the conversation made on the
        // removed account included, and the other profile still reaches it.
        assert!(main.dir.join("projects/C--repo/conv-1.jsonl").is_file());
        assert!(main.dir.join("projects/C--repo/conv-2.jsonl").is_file());
        assert!(main.dir.join("skills/review/SKILL.md").is_file());
        assert!(main.dir.join(".credentials.json").is_file());
        assert!(main.config.is_file());
        assert!(store
            .dir_of("perso")
            .join("projects/C--repo/conv-2.jsonl")
            .is_file());
    }

    #[test]
    fn a_launch_prepares_the_profile_and_falls_back_to_main_for_a_gone_account() {
        let scratch = Scratch::new("launch");
        let main = furnished(&scratch);
        let store = store(&scratch);
        add_with(&main, &store, "Work", 1).unwrap();
        // Main's setup changed since the account was added.
        scratch.file("home/.claude/CLAUDE.md", "New rule.");

        let launch = launch_with(&main, &store, "work");
        assert_eq!(launch.id, "work");
        assert_eq!(launch.dir.as_deref(), Some(store.dir_of("work").as_path()));
        assert!(launch.issues.is_empty(), "{:?}", launch.issues);
        assert!(store.dir_of("work").join("CLAUDE.md").is_file());

        assert_eq!(launch_with(&main, &store, "gone"), Launch::main());
    }

    #[test]
    fn panes_get_claude_config_dir_only_for_claude_code_on_a_profile() {
        let dir = PathBuf::from("/accounts/work");
        assert_eq!(
            env_for(Some("claude"), Some(&dir)),
            vec![
                ("CLAUDE_CONFIG_DIR", Some(dir.clone())),
                ("CLAUDE_SECURESTORAGE_CONFIG_DIR", None),
            ]
        );
        // Main keeps Hangar's environment.
        assert!(env_for(Some("claude"), None).is_empty());
        // No account applies to the other agents, nor to a shell.
        for agent in ["codex", "gemini", "opencode", "shell"] {
            assert!(env_for(Some(agent), Some(&dir)).is_empty(), "{agent}");
        }
        assert!(env_for(None, Some(&dir)).is_empty());
        // And no launch is ever made for them.
        assert_eq!(for_launch(Some("codex"), Some("work")), None);
        assert_eq!(for_launch(Some("claude"), None), Some(Launch::main()));
        assert_eq!(
            for_launch(Some("claude"), Some(" main ")),
            Some(Launch::main())
        );
    }

    #[test]
    fn conversations_are_read_once_and_a_profile_of_its_own_too() {
        let scratch = Scratch::new("projects");
        let main = furnished(&scratch);
        let store = store(&scratch);
        add_with(&main, &store, "Work", 1).unwrap();
        // A profile whose link was replaced by a folder of its own.
        add_with(&main, &store, "Perso", 2).unwrap();
        let perso = store.dir_of("perso").join("projects");
        link::remove_link(&perso).unwrap();
        scratch.file("accounts/perso/projects/C--x/conv-9.jsonl", "{}\n");

        let dirs = projects_with(&main, &store);
        assert_eq!(dirs, vec![main.projects(), perso]);
    }

    #[test]
    fn the_support_note_is_only_there_when_logins_are_shared() {
        assert_eq!(
            support_from(true),
            Support {
                isolated_logins: true,
                note: None
            }
        );
        let shared = serde_json::to_value(support_from(false)).unwrap();
        assert_eq!(shared["isolatedLogins"], false);
        assert!(shared["note"]
            .as_str()
            .is_some_and(|note| note.contains("Keychain")));
    }
}
