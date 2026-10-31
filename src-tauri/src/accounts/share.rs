//! What every profile takes from the main account, so that only the login
//! differs between accounts (see the module doc of accounts.rs for why each
//! part is shared the way it is).

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Serialize;
use serde_json::{Map, Value};

use super::link;
use super::store::write_atomic;
use super::ClaudeHome;

/// Folders a profile reaches through a link to the main account's own:
/// whatever Claude Code writes there from any account lands in one place.
pub const SHARED_DIRS: &[&str] = &[
    // Conversations, so `claude --resume <id>` finds them on any account,
    // and what a resumed conversation brings with it.
    "projects",
    "file-history",
    "todos",
    "plans",
    // The person's tools.
    "skills",
    "commands",
    "agents",
    "output-styles",
    "rules",
    "plugins",
];

/// Files copied from the main account before each launch. Claude Code
/// refuses to write a settings file through a link, and a hard link would be
/// broken by its atomic rewrites, so these are mirrored one way instead.
pub const SHARED_FILES: &[&str] = &["settings.json", "CLAUDE.md", "keybindings.json"];

/// Keys of `.claude.json` that are setup rather than login: the user-scope
/// MCP servers.
pub const SHARED_KEYS: &[&str] = &["mcpServers"];

/// Keys of each folder's entry under `projects` in `.claude.json` that are
/// setup: the trust given to the folder — without which an agent started
/// there waits on Claude Code's trust dialog — its local-scope MCP servers
/// and the choices made about its `.mcp.json` servers.
pub const SHARED_PROJECT_KEYS: &[&str] = &[
    "hasTrustDialogAccepted",
    "mcpServers",
    "enabledMcpjsonServers",
    "disabledMcpjsonServers",
    "disabledMcpServers",
    "enableAllProjectMcpServers",
];

/// Something a profile could not take from the main account. `reason` is an
/// error's kind, never a path nor a file's content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Issue {
    pub item: String,
    pub reason: String,
}

impl Issue {
    fn new(item: &str, reason: impl ToString) -> Self {
        Issue {
            item: item.to_string(),
            reason: reason.to_string(),
        }
    }
}

/// Brings `profile` level with `main`: its links, its copies, the setup keys
/// of its `.claude.json`. Its own login (`.credentials.json`, and
/// `oauthAccount` and the rest of `.claude.json`) is never touched, and
/// neither is anything of `main`, which is only read.
pub fn prepare(main: &ClaudeHome, profile: &ClaudeHome) -> Vec<Issue> {
    let mut issues = Vec::new();
    if let Err(err) = fs::create_dir_all(&profile.dir) {
        issues.push(Issue::new("profile", err));
        return issues;
    }
    // A Hangar started on a profile's folder has it as its main account:
    // linking it to itself would make a loop of its conversations.
    if link::same_place(&main.dir, &profile.dir) {
        return issues;
    }
    for name in SHARED_DIRS {
        if let Err(reason) = share_dir(&main.dir.join(name), &profile.dir.join(name)) {
            issues.push(Issue::new(name, reason));
        }
    }
    for name in SHARED_FILES {
        if let Err(err) = mirror_file(&main.dir.join(name), &profile.dir.join(name)) {
            issues.push(Issue::new(name, err));
        }
    }
    if let Err(reason) = sync_config(&main.config, &profile.config) {
        issues.push(Issue::new(".claude.json", reason));
    }
    issues
}

/// `here` made a link to `source` when the main account has that folder. A
/// link that leads elsewhere, or nowhere, is replaced; a folder of the
/// profile's own is kept when it holds anything — it is never deleted to
/// make room — and said.
fn share_dir(source: &Path, here: &Path) -> Result<(), String> {
    let wanted = source.is_dir();
    if link::is_link(here) {
        if wanted && link::same_place(here, source) {
            return Ok(());
        }
        link::remove_link(here).map_err(|e| e.to_string())?;
    } else if link::exists(here) {
        if !wanted {
            return Ok(());
        }
        if !is_empty_dir(here) {
            return Err("the profile has a folder of its own here; it is kept".into());
        }
        fs::remove_dir(here).map_err(|e| e.to_string())?;
    }
    if wanted {
        link::link_dir(source, here).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

/// `here` made a copy of `source`, or removed when the main account has no
/// such file. Written only when it differs, and never through a link.
fn mirror_file(source: &Path, here: &Path) -> io::Result<()> {
    let wanted = match fs::read(source) {
        Ok(body) => Some(body),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    if link::is_link(here) {
        link::remove_link(here)?;
    }
    match wanted {
        Some(body) => {
            if fs::read(here).ok().as_deref() != Some(body.as_slice()) {
                write_atomic(here, &body).map_err(io::Error::other)?;
            }
        }
        None => {
            if here.is_file() {
                fs::remove_file(here)?;
            }
        }
    }
    Ok(())
}

/// A file's size and modification time: what tells whether it moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// `Ok(None)` for a file that is not there; `Err` for one that is there and
/// is not a JSON object.
pub fn read_object(path: &Path) -> Result<Option<Map<String, Value>>, ()> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    match serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        _ => Err(()),
    }
}

/// The main and the profile config, as they stood when last left level.
type Level = (Option<Stamp>, Option<Stamp>);

/// The two files as they were at the last sync that left them level, by
/// profile config: unchanged since, there is nothing to read again.
fn synced() -> &'static Mutex<HashMap<PathBuf, Level>> {
    static SYNCED: std::sync::OnceLock<Mutex<HashMap<PathBuf, Level>>> = std::sync::OnceLock::new();
    SYNCED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Copies the setup keys of the main `.claude.json` into the profile's.
///
/// Claude Code rewrites its `.claude.json` under a lock of its own, which
/// Hangar does not take: the profile's file is read, merged, written to a
/// temporary file, and renamed over it only if it has not moved meanwhile —
/// otherwise it is read again. A login landing at that moment is therefore
/// never written over.
fn sync_config(main: &Path, profile: &Path) -> Result<(), String> {
    let stamps = (stamp(main), stamp(profile));
    if synced()
        .lock()
        .ok()
        .and_then(|map| map.get(profile).copied())
        == Some(stamps)
    {
        return Ok(());
    }
    let main_config = match read_object(main) {
        Ok(Some(config)) => config,
        // Nothing to share yet.
        Ok(None) => return Ok(()),
        Err(()) => return Err("the main account's configuration cannot be read".into()),
    };
    for _ in 0..3 {
        let before = stamp(profile);
        let current = match read_object(profile) {
            Ok(found) => found.unwrap_or_default(),
            Err(()) => {
                return Err("the profile's configuration cannot be read; it is left alone".into())
            }
        };
        let merged = merge_setup(&main_config, current.clone());
        if merged != current {
            let body =
                serde_json::to_string_pretty(&Value::Object(merged)).map_err(|e| e.to_string())?;
            if stamp(profile) != before {
                continue;
            }
            write_atomic(profile, body.as_bytes())?;
        }
        if let Ok(mut map) = synced().lock() {
            map.insert(profile.to_path_buf(), (stamp(main), stamp(profile)));
        }
        return Ok(());
    }
    Err("the profile's configuration kept changing; try again".into())
}

/// `profile` with the main account's setup keys: copied when the main
/// account has them, removed when it does not. A folder only the profile
/// knows keeps its entry as it is.
pub fn merge_setup(
    main: &Map<String, Value>,
    mut profile: Map<String, Value>,
) -> Map<String, Value> {
    for key in SHARED_KEYS {
        match main.get(*key) {
            Some(value) => {
                profile.insert(key.to_string(), value.clone());
            }
            None => {
                profile.remove(*key);
            }
        }
    }
    let Some(main_projects) = main.get("projects").and_then(Value::as_object) else {
        return profile;
    };
    let projects = profile
        .entry("projects")
        .or_insert_with(|| Value::Object(Map::new()));
    if !projects.is_object() {
        *projects = Value::Object(Map::new());
    }
    let Some(projects) = projects.as_object_mut() else {
        return profile;
    };
    for (folder, entry) in main_projects {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let setup: Map<String, Value> = SHARED_PROJECT_KEYS
            .iter()
            .filter_map(|key| {
                entry
                    .get(*key)
                    .map(|value| (key.to_string(), value.clone()))
            })
            .collect();
        match projects.get_mut(folder) {
            Some(Value::Object(mine)) => {
                for key in SHARED_PROJECT_KEYS {
                    match setup.get(*key) {
                        Some(value) => {
                            mine.insert(key.to_string(), value.clone());
                        }
                        None => {
                            mine.remove(*key);
                        }
                    }
                }
            }
            _ if setup.is_empty() => {}
            _ => {
                projects.insert(folder.clone(), Value::Object(setup));
            }
        }
    }
    profile
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::testing::{read_json, Scratch};
    use serde_json::json;

    fn profile_home(scratch: &Scratch, id: &str) -> ClaudeHome {
        ClaudeHome::profile(scratch.root.join("accounts").join(id))
    }

    /// A main account with conversations, tools, settings and servers.
    fn furnished(scratch: &Scratch) -> ClaudeHome {
        let main = scratch.main_home();
        scratch.file("home/.claude/projects/C--repo/conv-1.jsonl", "{\"a\":1}\n");
        scratch.file(
            "home/.claude/skills/review/SKILL.md",
            "---\nname: review\n---\n",
        );
        scratch.file("home/.claude/commands/ship.md", "Ship it.");
        scratch.file("home/.claude/agents/helper.md", "Helper.");
        scratch.file("home/.claude/plugins/installed_plugins.json", "{}");
        scratch.file("home/.claude/settings.json", "{\"model\":\"opus\"}");
        scratch.file("home/.claude/CLAUDE.md", "Be brief.");
        scratch.file(
            "home/.claude/.credentials.json",
            "{\"claudeAiOauth\":{\"accessToken\":\"main-secret\"}}",
        );
        scratch.file(
            "home/.claude.json",
            &json!({
                "oauthAccount": { "emailAddress": "main@example.com" },
                "userID": "main-user",
                "mcpServers": { "tracker": { "command": "tracker-mcp" } },
                "projects": {
                    "C:/repo": {
                        "hasTrustDialogAccepted": true,
                        "mcpServers": { "local": { "command": "local-mcp" } },
                        "lastCost": 12.5
                    },
                    "C:/plain": { "lastCost": 1 }
                }
            })
            .to_string(),
        );
        main
    }

    #[test]
    fn a_profile_shares_the_folders_and_files_of_the_main_account() {
        let scratch = Scratch::new("share-furnish");
        let main = furnished(&scratch);
        let profile = profile_home(&scratch, "work");
        let issues = prepare(&main, &profile);
        assert!(issues.is_empty(), "{issues:?}");

        for name in ["projects", "skills", "commands", "agents", "plugins"] {
            let here = profile.dir.join(name);
            assert!(link::is_link(&here), "{name} is not linked");
            assert!(link::same_place(&here, &main.dir.join(name)));
        }
        // Folders the main account does not have are not invented, there or here.
        for name in ["todos", "plans", "rules", "output-styles", "file-history"] {
            assert!(!link::exists(&profile.dir.join(name)));
            assert!(!link::exists(&main.dir.join(name)));
        }
        assert!(profile.dir.join("skills/review/SKILL.md").is_file());
        assert_eq!(
            fs::read_to_string(profile.dir.join("settings.json")).unwrap(),
            "{\"model\":\"opus\"}"
        );
        assert_eq!(
            fs::read_to_string(profile.dir.join("CLAUDE.md")).unwrap(),
            "Be brief."
        );
        assert!(!link::is_link(&profile.dir.join("settings.json")));
        // The login is the profile's own: nothing of the main one comes over.
        assert!(!link::exists(&profile.dir.join(".credentials.json")));
        let config = read_json(&profile.config);
        assert_eq!(
            config,
            json!({
                "mcpServers": { "tracker": { "command": "tracker-mcp" } },
                "projects": {
                    "C:/repo": {
                        "hasTrustDialogAccepted": true,
                        "mcpServers": { "local": { "command": "local-mcp" } }
                    }
                }
            })
        );
    }

    #[test]
    fn a_conversation_written_through_one_profile_is_seen_from_another_and_from_main() {
        let scratch = Scratch::new("share-conversation");
        let main = furnished(&scratch);
        let work = profile_home(&scratch, "work");
        let perso = profile_home(&scratch, "perso");
        assert!(prepare(&main, &work).is_empty());
        assert!(prepare(&main, &perso).is_empty());

        // Claude Code on `work` starts a conversation in a new folder.
        let written = work.dir.join("projects/C--elsewhere/conv-2.jsonl");
        fs::create_dir_all(written.parent().unwrap()).unwrap();
        fs::write(&written, "{\"turn\":1}\n").unwrap();

        // `claude --resume conv-2` on `perso` reads the same file…
        let seen = perso.dir.join("projects/C--elsewhere/conv-2.jsonl");
        assert_eq!(fs::read_to_string(seen).unwrap(), "{\"turn\":1}\n");
        // …and so does the main account, and the conversation it already had.
        assert!(main
            .dir
            .join("projects/C--elsewhere/conv-2.jsonl")
            .is_file());
        assert!(perso.dir.join("projects/C--repo/conv-1.jsonl").is_file());
    }

    #[test]
    fn preparing_again_follows_the_main_account_and_keeps_the_login() {
        let scratch = Scratch::new("share-again");
        let main = furnished(&scratch);
        let profile = profile_home(&scratch, "work");
        assert!(prepare(&main, &profile).is_empty());

        // The profile logs in: Claude Code writes its own login and state.
        fs::write(
            profile.dir.join(".credentials.json"),
            "{\"claudeAiOauth\":{\"accessToken\":\"work-secret\"}}",
        )
        .unwrap();
        let mut config = read_json(&profile.config);
        config["oauthAccount"] = json!({ "emailAddress": "work@example.com" });
        config["numStartups"] = json!(3);
        config["projects"]["C:/repo"]["lastCost"] = json!(2);
        fs::write(&profile.config, config.to_string()).unwrap();
        // A settings change made on the profile is the main account's to make.
        fs::write(profile.dir.join("settings.json"), "{\"model\":\"haiku\"}").unwrap();

        // The person changes their setup on the main account.
        fs::write(main.dir.join("settings.json"), "{\"model\":\"sonnet\"}").unwrap();
        fs::remove_file(main.dir.join("CLAUDE.md")).unwrap();
        fs::create_dir_all(main.dir.join("rules")).unwrap();
        let mut main_config = read_json(&main.config);
        main_config["mcpServers"] = json!({ "docs": { "url": "https://docs.example" } });
        main_config["projects"]["C:/repo"]
            .as_object_mut()
            .unwrap()
            .remove("mcpServers");
        main_config["projects"]["C:/new"] = json!({ "hasTrustDialogAccepted": true });
        fs::write(&main.config, main_config.to_string()).unwrap();

        assert!(prepare(&main, &profile).is_empty());
        assert_eq!(
            fs::read_to_string(profile.dir.join("settings.json")).unwrap(),
            "{\"model\":\"sonnet\"}"
        );
        assert!(!link::exists(&profile.dir.join("CLAUDE.md")));
        assert!(link::is_link(&profile.dir.join("rules")));
        assert_eq!(
            fs::read_to_string(profile.dir.join(".credentials.json")).unwrap(),
            "{\"claudeAiOauth\":{\"accessToken\":\"work-secret\"}}"
        );
        let config = read_json(&profile.config);
        assert_eq!(config["oauthAccount"]["emailAddress"], "work@example.com");
        assert_eq!(config["numStartups"], 3);
        assert_eq!(
            config["mcpServers"],
            json!({ "docs": { "url": "https://docs.example" } })
        );
        assert_eq!(
            config["projects"]["C:/repo"],
            json!({ "hasTrustDialogAccepted": true, "lastCost": 2 })
        );
        assert_eq!(
            config["projects"]["C:/new"],
            json!({ "hasTrustDialogAccepted": true })
        );
        // And the main account was only ever read.
        assert_eq!(read_json(&main.config), main_config);
        assert_eq!(
            fs::read_to_string(main.dir.join(".credentials.json")).unwrap(),
            "{\"claudeAiOauth\":{\"accessToken\":\"main-secret\"}}"
        );
    }

    #[test]
    fn a_folder_of_the_profiles_own_is_kept_and_said() {
        let scratch = Scratch::new("share-own");
        let main = furnished(&scratch);
        let profile = profile_home(&scratch, "work");
        scratch.file("accounts/work/projects/C--mine/conv-3.jsonl", "{}\n");
        fs::create_dir_all(profile.dir.join("skills")).unwrap();

        let issues = prepare(&main, &profile);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].item, "projects");
        assert!(!link::is_link(&profile.dir.join("projects")));
        assert!(profile.dir.join("projects/C--mine/conv-3.jsonl").is_file());
        // An empty one makes room for the link.
        assert!(link::is_link(&profile.dir.join("skills")));
    }

    #[test]
    fn a_broken_profile_config_is_left_alone() {
        let scratch = Scratch::new("share-broken");
        let main = furnished(&scratch);
        let profile = profile_home(&scratch, "work");
        scratch.file("accounts/work/.claude.json", "{ half written");
        let issues = prepare(&main, &profile);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].item, ".claude.json");
        assert_eq!(
            fs::read_to_string(&profile.config).unwrap(),
            "{ half written"
        );
    }

    #[test]
    fn a_profile_on_the_main_folder_is_left_as_it_is() {
        let scratch = Scratch::new("share-self");
        let main = furnished(&scratch);
        let same = ClaudeHome::profile(main.dir.clone());
        assert!(prepare(&main, &same).is_empty());
        assert!(!link::is_link(&main.dir.join("projects")));
        assert!(!main.dir.join(".claude.json").exists());
    }
}
