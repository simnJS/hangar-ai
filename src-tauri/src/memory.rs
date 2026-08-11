//! Global agent memory: the facts agents keep between sessions.
//!
//! The board lives next to the code it describes, one file per workspace.
//! Memory is the opposite: a single store for the whole app, because what an
//! agent learns in one project has to still be there when another agent opens a
//! different one. That is why it sits in the app data directory beside
//! `state.json` instead of in `.hangar/`, and why nothing here is keyed by the
//! workspace — `workspace` is recorded as the origin of a fact, not as a
//! partition of the store.
//!
//! Everything below the store is a plain function over `Memory`, so the rules
//! that actually matter (upsert by title, the limits, the ranking) are testable
//! without a Tauri app or an HTTP server in the way.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// Hard ceilings, enforced with an explicit error rather than by truncating.
///
/// An agent that silently loses the tail of what it wrote would keep a fact
/// that reads as complete and is not, which is worse than no fact at all. The
/// entry ceiling is there because nothing else stops a loop from writing until
/// the file is unusable.
const MAX_ENTRIES: usize = 1000;
const MAX_CONTENT_CHARS: usize = 20_000;
const MAX_TITLE_CHARS: usize = 200;
const MAX_TAGS: usize = 10;

/// Search paging. The default is small on purpose: these results are read by a
/// model, and a hundred hits would cost more context than the memory saves.
const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 100;

/// Excerpt window, in characters, and how much of it sits before the match.
const EXCERPT_CHARS: usize = 160;
const EXCERPT_LEAD: usize = 60;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Workspace the fact was written from. Kept so an agent can ask for what
    /// is known about the project it is in, without the store being split.
    #[serde(default)]
    pub workspace: String,
    /// Agent that wrote it, e.g. "claude-1". Free-form, like `Task::assignee`.
    #[serde(default)]
    pub agent: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Memory {
    #[serde(default)]
    pub entries: Vec<MemoryEntry>,
}

#[derive(Debug, Default, Deserialize)]
pub struct NewMemory {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub agent: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct MemoryPatch {
    pub title: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// A search result carries no `content`.
///
/// The caller is almost always a model deciding which fact to open: sending
/// twenty full entries to answer one question is exactly the context cost the
/// memory is supposed to avoid. `excerpt` shows why the entry matched, and
/// `memory_read` fetches the rest when it is worth it.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub workspace: String,
    pub agent: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    pub results: Vec<SearchHit>,
    /// Matches before `limit` was applied, so a caller knows it is seeing a page.
    pub total: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// Free text. Split on whitespace; every term has to match.
    pub q: String,
    /// Exact tag, case-insensitive.
    pub tag: Option<String>,
    /// Substring of the origin workspace path, case-insensitive.
    pub workspace: Option<String>,
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// The upsert key: two titles that differ only by case or by edge whitespace
/// are the same fact, so re-learning something updates it instead of leaving
/// two half-answers side by side.
fn title_key(title: &str) -> String {
    title.trim().to_lowercase()
}

fn clean_title(title: &str) -> Result<String, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("title must not be empty".into());
    }
    let length = trimmed.chars().count();
    if length > MAX_TITLE_CHARS {
        return Err(format!(
            "title is {length} characters, the limit is {MAX_TITLE_CHARS}"
        ));
    }
    Ok(trimmed.to_string())
}

/// Rejects an empty body on updates too, not just on creation: an entry with a
/// title and nothing under it teaches nothing, and `memory_delete` is the way
/// to get rid of a fact.
fn clean_content(content: &str) -> Result<String, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("content must not be empty".into());
    }
    let length = trimmed.chars().count();
    if length > MAX_CONTENT_CHARS {
        return Err(format!(
            "content is {length} characters, the limit is {MAX_CONTENT_CHARS}; split it into several entries"
        ));
    }
    Ok(trimmed.to_string())
}

/// Drops blanks and case-insensitive duplicates, then counts what is left: the
/// ceiling has to be about distinct tags, otherwise "rust, Rust, RUST" would be
/// three of the ten an agent is allowed.
fn clean_tags(tags: Vec<String>) -> Result<Vec<String>, String> {
    let mut kept: Vec<String> = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        if kept
            .iter()
            .any(|seen| seen.to_lowercase() == trimmed.to_lowercase())
        {
            continue;
        }
        kept.push(trimmed.to_string());
    }
    if kept.len() > MAX_TAGS {
        return Err(format!("{} tags, the limit is {MAX_TAGS}", kept.len()));
    }
    Ok(kept)
}

// ---------------------------------------------------------------------------
// Operations (pure, over a `Memory`)
// ---------------------------------------------------------------------------

pub fn find<'a>(memory: &'a Memory, id: &str) -> Option<&'a MemoryEntry> {
    memory.entries.iter().find(|entry| entry.id == id)
}

/// Writes a fact, replacing the one that already carries the same title.
///
/// Returns the entry and whether it is new. Dedup by title is what keeps the
/// store readable over time: agents re-learn the same things constantly, and
/// without this the tenth run leaves ten near-identical notes for a human to
/// sort out. The id and `created_at` survive the update so anything holding a
/// reference to the fact keeps pointing at it.
pub fn upsert(memory: &mut Memory, input: NewMemory) -> Result<(MemoryEntry, bool), String> {
    let title = clean_title(&input.title)?;
    let content = clean_content(&input.content)?;
    let tags = clean_tags(input.tags)?;
    let key = title_key(&title);
    let stamp = now_ms();

    if let Some(existing) = memory
        .entries
        .iter_mut()
        .find(|entry| title_key(&entry.title) == key)
    {
        existing.title = title;
        existing.content = content;
        existing.tags = tags;
        // Only overwrite the origin when the writer gave one: a correction made
        // from the UI carries neither, and must not erase who first learned it.
        if !input.workspace.is_empty() {
            existing.workspace = input.workspace;
        }
        if !input.agent.is_empty() {
            existing.agent = input.agent;
        }
        existing.updated_at = stamp;
        return Ok((existing.clone(), false));
    }

    if memory.entries.len() >= MAX_ENTRIES {
        return Err(format!(
            "memory holds {MAX_ENTRIES} entries, which is the limit; delete something before writing more"
        ));
    }

    let entry = MemoryEntry {
        id: new_id(),
        title,
        content,
        tags,
        workspace: input.workspace,
        agent: input.agent,
        created_at: stamp,
        updated_at: stamp,
    };
    memory.entries.push(entry.clone());
    Ok((entry, true))
}

/// Edits an entry in place. Renaming onto another entry's title is refused
/// rather than merged: `upsert` picks the first title match, so two entries
/// sharing one title would make every later write land on an arbitrary one.
pub fn patch(memory: &mut Memory, id: &str, patch: MemoryPatch) -> Result<MemoryEntry, String> {
    let title = patch.title.as_deref().map(clean_title).transpose()?;
    let content = patch.content.as_deref().map(clean_content).transpose()?;
    let tags = patch.tags.map(clean_tags).transpose()?;

    if let Some(title) = &title {
        let key = title_key(title);
        if memory
            .entries
            .iter()
            .any(|entry| entry.id != id && title_key(&entry.title) == key)
        {
            return Err(format!("another entry is already titled '{title}'"));
        }
    }

    let entry = memory
        .entries
        .iter_mut()
        .find(|entry| entry.id == id)
        .ok_or("memory entry not found")?;

    if let Some(v) = title {
        entry.title = v;
    }
    if let Some(v) = content {
        entry.content = v;
    }
    if let Some(v) = tags {
        entry.tags = v;
    }
    entry.updated_at = now_ms();
    Ok(entry.clone())
}

pub fn delete(memory: &mut Memory, id: &str) -> Result<(), String> {
    let before = memory.entries.len();
    memory.entries.retain(|entry| entry.id != id);
    if memory.entries.len() == before {
        return Err("memory entry not found".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Case-insensitive "carries this tag", the one rule for every filter.
fn has_tag(entry: &MemoryEntry, tag: &str) -> bool {
    tag.is_empty()
        || entry
            .tags
            .iter()
            .any(|candidate| candidate.to_lowercase() == tag)
}

/// Case-insensitive "written from a workspace containing this text".
fn from_workspace(entry: &MemoryEntry, workspace: &str) -> bool {
    workspace.is_empty() || entry.workspace.to_lowercase().contains(workspace)
}

/// An entry as the index shows it: everything but the content, plus how much
/// content there is. The caller is a model deciding what to read — sending the
/// text it is trying not to pay for would defeat the listing.
#[derive(Debug, Clone, Serialize)]
pub struct ListedEntry {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub workspace: String,
    pub agent: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// Characters of content behind `memory_read`.
    pub chars: usize,
}

/// The compact index of the store, optionally narrowed by tag or origin.
///
/// Empty filters mean "no filter": the callers pass whatever arrived on the
/// URL, and a model that fills every property of a schema would otherwise ask
/// for the entries tagged with nothing and be told the memory is empty.
pub fn list(memory: &Memory, tag: Option<&str>, workspace: Option<&str>) -> Vec<ListedEntry> {
    let tag = tag.map(str::trim).unwrap_or_default().to_lowercase();
    let workspace = workspace.map(str::trim).unwrap_or_default().to_lowercase();

    memory
        .entries
        .iter()
        .filter(|entry| has_tag(entry, &tag) && from_workspace(entry, &workspace))
        .map(|entry| ListedEntry {
            id: entry.id.clone(),
            title: entry.title.clone(),
            tags: entry.tags.clone(),
            workspace: entry.workspace.clone(),
            agent: entry.agent.clone(),
            created_at: entry.created_at,
            updated_at: entry.updated_at,
            chars: entry.content.chars().count(),
        })
        .collect()
}

/// Non-overlapping occurrences of `needle` in `haystack`. Both are expected to
/// be lowercased already by the caller, which does it once per entry instead of
/// once per term.
fn occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

/// Slices by character, clamped at both ends.
///
/// Byte offsets are not usable here: they come from a lowercased copy of the
/// content, and a handful of code points change length when lowercased, so an
/// offset may not even land on a character boundary of the original. Counting
/// characters can be off by one on those same code points, which costs a letter
/// at the edge of an excerpt and never panics.
fn char_window(text: &str, start: usize, len: usize) -> String {
    text.chars().skip(start).take(len).collect()
}

/// A one-line window of the content around the first term that matched.
///
/// Falls back to the head of the content when the match was in the title or in
/// a tag: the point is to show enough for the caller to decide whether to open
/// the entry, and the opening lines do that just as well.
fn excerpt(content: &str, terms: &[String]) -> String {
    let lowered = content.to_lowercase();
    let hit = terms.iter().filter_map(|term| lowered.find(term)).min();

    let match_at = hit.map_or(0, |byte| lowered[..byte].chars().count());
    let start = match_at.saturating_sub(if hit.is_some() { EXCERPT_LEAD } else { 0 });
    let total = content.chars().count();

    // Whitespace is collapsed so a multi-line note stays one readable line in a
    // list of results.
    let window = char_window(content, start, EXCERPT_CHARS)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(&window);
    if start + EXCERPT_CHARS < total {
        out.push('…');
    }
    out
}

/// Full-text search over titles, contents and tags.
///
/// Every term has to appear somewhere in an entry (AND), because the callers are
/// models that add words to narrow a search and would otherwise get a longer
/// list for a more precise question. The weights say where a word carries the
/// most meaning: a title is what the fact is about, a tag is how it was filed,
/// the content merely mentions it.
pub fn search(memory: &Memory, query: &SearchQuery) -> SearchResults {
    let terms: Vec<String> = query.q.split_whitespace().map(str::to_lowercase).collect();
    let tag = query.tag.as_ref().map(|t| t.trim().to_lowercase());
    let workspace = query.workspace.as_ref().map(|w| w.trim().to_lowercase());
    let limit = query
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .min(MAX_SEARCH_LIMIT);

    let mut scored: Vec<(usize, &MemoryEntry)> = Vec::new();

    for entry in &memory.entries {
        if let Some(tag) = &tag {
            if !has_tag(entry, tag) {
                continue;
            }
        }
        if let Some(workspace) = &workspace {
            if !from_workspace(entry, workspace) {
                continue;
            }
        }

        let title = entry.title.to_lowercase();
        let content = entry.content.to_lowercase();
        let tags = entry.tags.join(" ").to_lowercase();

        let mut score = 0usize;
        let mut matched_every_term = true;
        for term in &terms {
            let in_title = occurrences(&title, term);
            let in_tags = occurrences(&tags, term);
            let in_content = occurrences(&content, term);
            if in_title + in_tags + in_content == 0 {
                matched_every_term = false;
                break;
            }
            score += 3 * in_title + 2 * in_tags + in_content;
        }
        if !matched_every_term {
            continue;
        }
        scored.push((score, entry));
    }

    // Best match first, then the freshest — an old fact and a new one that score
    // the same are usually the same fact restated, and the new one is the one
    // that still holds.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.updated_at.cmp(&a.1.updated_at)));

    let total = scored.len();
    let results = scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| SearchHit {
            id: entry.id.clone(),
            title: entry.title.clone(),
            tags: entry.tags.clone(),
            workspace: entry.workspace.clone(),
            agent: entry.agent.clone(),
            created_at: entry.created_at,
            updated_at: entry.updated_at,
            excerpt: excerpt(&entry.content, &terms),
        })
        .collect();

    SearchResults { results, total }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn memory_path(dir: &Path) -> PathBuf {
    dir.join("memory.json")
}

/// Moves a memory file that will not parse out of the way.
///
/// This is the one failure a retry cannot fix, so it is also the one where the
/// read does have to hand back an empty memory — and an empty memory means the
/// next write persists that emptiness over the file. A board can afford that —
/// it describes work still in front of the agent — but this file is the only
/// copy of everything the agents have learned. Renaming costs nothing and
/// leaves the JSON there to be repaired by hand. `quarantine()` is the same
/// move store.rs makes for `state.json`, copy-on-refused-rename included.
fn rescue_unreadable(path: &Path) {
    let _ = crate::store::quarantine(path);
}

/// Reads the store, keeping "there is nothing yet" apart from "it would not
/// open".
///
/// Collapsing the two into an empty `Memory` is what loses the file: on Windows
/// a transient lock — an antivirus or an indexer holding a handle, the same
/// scenario `quarantine()` in store.rs works around — makes the read fail on a
/// file that is perfectly intact, and the write that follows the mutation would
/// then persist that emptiness over every fact the agents have. Only
/// `NotFound` is a genuinely empty memory (first launch); every other I/O error
/// is propagated, and `MemoryStore::update` stops on it before writing
/// anything.
///
/// Invalid JSON stays the third case and keeps its own answer: the file was
/// read, its content is worthless as it stands, so `rescue_unreadable` moves it
/// aside and the caller starts from an empty store.
fn read_memory(dir: &Path) -> Result<Memory, StoreError> {
    let path = memory_path(dir);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Memory::default()),
        Err(err) => {
            return Err(StoreError::Storage(format!(
                "the memory at {} could not be read ({err}); nothing was modified",
                path.display()
            )))
        }
    };
    match serde_json::from_str(&raw) {
        Ok(memory) => Ok(memory),
        Err(_) => {
            rescue_unreadable(&path);
            Ok(Memory::default())
        }
    }
}

fn write_memory(dir: &Path, memory: &Memory) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = memory_path(dir);
    let body = serde_json::to_string_pretty(memory).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    // Write-then-rename, so a crash mid-save cannot truncate the real file.
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Why a store operation failed.
///
/// The two cases deserve different answers and the caller cannot tell them
/// apart from a string: a refused mutation is the caller's to fix, while a file
/// that would not open is the machine's problem and, above all, means the write
/// never happened. server.rs turns the first into a 400 and the second into a
/// 500; the Tauri commands flatten both back to the message, which is all the
/// frontend can show.
#[derive(Debug)]
pub enum StoreError {
    /// `memory.json` could not be read or written. Whatever is on disk is
    /// exactly what was there before the call.
    Storage(String),
    /// The mutation itself was rejected — a validation error, an unknown id.
    Refused(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Storage(message) | StoreError::Refused(message) => f.write_str(message),
        }
    }
}

/// Serializes every mutation of the single memory file.
///
/// The store is global, so the contention is real: agents from several
/// workspaces write to the same file at the same time, and without one writer a
/// read-modify-write pair would drop whichever fact lost the race.
#[derive(Default)]
pub struct MemoryStore {
    guard: Mutex<()>,
}

impl MemoryStore {
    /// Fails instead of answering "empty" when the file is there and will not
    /// open: every caller renders what it gets, and an empty list is
    /// indistinguishable from a memory that has been lost.
    pub fn read(&self, dir: &Path) -> Result<Memory, StoreError> {
        let _guard = self.guard.lock().unwrap();
        read_memory(dir)
    }

    /// Applies `mutate` under the global lock and persists the result.
    ///
    /// The read comes first and its failure returns before `mutate` ever runs:
    /// a read that could not see the file must never become a write that
    /// replaces it.
    pub fn update<T>(
        &self,
        dir: &Path,
        mutate: impl FnOnce(&mut Memory) -> Result<T, String>,
    ) -> Result<T, StoreError> {
        let _guard = self.guard.lock().unwrap();
        let mut memory = read_memory(dir)?;
        let outcome = mutate(&mut memory).map_err(StoreError::Refused)?;
        write_memory(dir, &memory).map_err(StoreError::Storage)?;
        Ok(outcome)
    }
}

/// The one store the UI and the HTTP API share, so a card edited in the window
/// and a fact written by an agent contend for the same lock.
pub fn store() -> &'static MemoryStore {
    static STORE: OnceLock<MemoryStore> = OnceLock::new();
    STORE.get_or_init(MemoryStore::default)
}

/// Directory holding `memory.json` — the same one as `state.json`, because the
/// memory belongs to the install, not to any workspace.
pub fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Tells the UI to reload. The payload is empty on purpose: memory is global,
/// so unlike `board:changed` there is no workspace for a listener to filter on.
pub fn notify_changed(app: &AppHandle) {
    let _ = app.emit("memory:changed", "");
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn memory_load(app: AppHandle) -> Result<Memory, String> {
    let root = dir(&app)?;
    // Reported rather than swallowed: MemoryView shows the message and stays on
    // an empty list, which is honest, where a silent empty load would look like
    // a memory that never had anything in it.
    store().read(&root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn memory_create(app: AppHandle, mut input: NewMemory) -> Result<MemoryEntry, String> {
    let root = dir(&app)?;
    // The same identity the board resolves to: a fact learned in a worktree
    // belongs to the project, and the `workspace` search filter runs over this
    // very string. See `shared_workspace` in board.rs.
    if !input.workspace.is_empty() {
        input.workspace = crate::git::shared_root(&input.workspace).unwrap_or(input.workspace);
    }
    let (entry, _created) = store()
        .update(&root, |memory| upsert(memory, input))
        .map_err(|e| e.to_string())?;
    notify_changed(&app);
    Ok(entry)
}

#[tauri::command]
pub fn memory_update(
    app: AppHandle,
    id: String,
    patch: MemoryPatch,
) -> Result<MemoryEntry, String> {
    let root = dir(&app)?;
    let entry = store()
        .update(&root, |memory| self::patch(memory, &id, patch))
        .map_err(|e| e.to_string())?;
    notify_changed(&app);
    Ok(entry)
}

#[tauri::command]
pub fn memory_delete(app: AppHandle, id: String) -> Result<(), String> {
    let root = dir(&app)?;
    store()
        .update(&root, |memory| delete(memory, &id))
        .map_err(|e| e.to_string())?;
    notify_changed(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hangar-memory-{tag}-{}", new_id()));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn fact(title: &str, content: &str) -> NewMemory {
        NewMemory {
            title: title.into(),
            content: content.into(),
            ..NewMemory::default()
        }
    }

    fn tagged(title: &str, content: &str, tags: &[&str]) -> NewMemory {
        NewMemory {
            title: title.into(),
            content: content.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..NewMemory::default()
        }
    }

    fn query(q: &str) -> SearchQuery {
        SearchQuery {
            q: q.into(),
            ..SearchQuery::default()
        }
    }

    #[test]
    fn writing_the_same_title_again_updates_the_entry_in_place() {
        let mut memory = Memory::default();
        let (first, created) = upsert(
            &mut memory,
            NewMemory {
                title: "Build steps".into(),
                content: "run cargo check".into(),
                workspace: "C:/repo".into(),
                agent: "claude-1".into(),
                ..NewMemory::default()
            },
        )
        .expect("create");
        assert!(created);

        // Back-dated rather than slept on: the assertion is about which stamp is
        // preserved, and a millisecond clock cannot be relied on to tick.
        memory.entries[0].created_at = 1;
        memory.entries[0].updated_at = 1;

        let (second, created) =
            upsert(&mut memory, fact("  bUiLd StEpS  ", "run cargo test too")).expect("update");

        assert!(!created, "a known title must not create a second entry");
        assert_eq!(memory.entries.len(), 1);
        assert_eq!(second.id, first.id, "the id has to survive the update");
        assert_eq!(second.created_at, 1, "created_at has to survive too");
        assert!(second.updated_at > 1, "updated_at has to move forward");
        assert_eq!(second.content, "run cargo test too");
        assert_eq!(second.title, "bUiLd StEpS", "the new spelling wins");
        assert_eq!(
            second.workspace, "C:/repo",
            "origin is kept when not resent"
        );
        assert_eq!(second.agent, "claude-1");
    }

    #[test]
    fn the_entry_ceiling_is_refused_with_an_explicit_message() {
        let mut memory = Memory::default();
        for nth in 0..MAX_ENTRIES {
            upsert(&mut memory, fact(&format!("fact {nth}"), "body")).expect("fill");
        }

        let err = upsert(&mut memory, fact("one too many", "body")).expect_err("refused");

        assert!(err.contains(&MAX_ENTRIES.to_string()), "got: {err}");
        assert_eq!(memory.entries.len(), MAX_ENTRIES);
        // The ceiling must not block corrections to what is already stored.
        assert!(upsert(&mut memory, fact("fact 0", "revised")).is_ok());
    }

    #[test]
    fn empty_or_oversized_fields_are_refused() {
        let mut memory = Memory::default();

        let err = upsert(&mut memory, fact("   ", "body")).expect_err("empty title");
        assert!(err.contains("title"), "got: {err}");

        let long_title = "t".repeat(MAX_TITLE_CHARS + 1);
        let err = upsert(&mut memory, fact(&long_title, "body")).expect_err("long title");
        assert!(err.contains(&MAX_TITLE_CHARS.to_string()), "got: {err}");

        let err = upsert(&mut memory, fact("empty", "  \n ")).expect_err("empty content");
        assert!(err.contains("content"), "got: {err}");

        let long_content = "c".repeat(MAX_CONTENT_CHARS + 1);
        let err = upsert(&mut memory, fact("long", &long_content)).expect_err("long content");
        assert!(err.contains(&MAX_CONTENT_CHARS.to_string()), "got: {err}");

        let many: Vec<&str> = vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"];
        let err = upsert(&mut memory, tagged("tags", "body", &many)).expect_err("too many tags");
        assert!(err.contains(&MAX_TAGS.to_string()), "got: {err}");

        assert!(memory.entries.is_empty(), "nothing must have been stored");
    }

    #[test]
    fn duplicate_tags_are_folded_before_the_ceiling_is_checked() {
        let mut memory = Memory::default();
        let (entry, _) = upsert(
            &mut memory,
            tagged("tags", "body", &["rust", "Rust", " RUST ", "", "cargo"]),
        )
        .expect("stored");

        assert_eq!(entry.tags, vec!["rust".to_string(), "cargo".to_string()]);
    }

    #[test]
    fn every_term_has_to_match_and_the_title_outranks_the_tags_and_the_content() {
        let mut memory = Memory::default();
        upsert(
            &mut memory,
            fact("Cargo build fails on Windows", "see below"),
        )
        .unwrap();
        upsert(
            &mut memory,
            fact(
                "Release notes",
                "cargo build fails on windows every single time",
            ),
        )
        .unwrap();
        upsert(
            &mut memory,
            tagged("Toolchain", "nothing to see", &["cargo", "build"]),
        )
        .unwrap();
        upsert(&mut memory, fact("Cargo only", "no other word here")).unwrap();

        let found = search(&memory, &query("cargo BUILD"));

        let titles: Vec<&str> = found.results.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Cargo build fails on Windows", "Toolchain", "Release notes"],
            "title (3) then tags (2) then content (1)"
        );
        assert_eq!(found.total, 3, "'Cargo only' misses the second term");
    }

    #[test]
    fn equal_scores_are_broken_by_the_most_recent_update() {
        let mut memory = Memory::default();
        upsert(&mut memory, fact("older", "deploy")).unwrap();
        upsert(&mut memory, fact("newer", "deploy")).unwrap();
        memory.entries[0].updated_at = 100;
        memory.entries[1].updated_at = 200;

        let found = search(&memory, &query("deploy"));

        assert_eq!(found.results[0].title, "newer");
        assert_eq!(found.results[1].title, "older");
    }

    #[test]
    fn the_excerpt_frames_the_first_match_and_never_carries_the_whole_content() {
        let mut memory = Memory::default();
        let content = format!("{} needle {}", "x".repeat(300), "y".repeat(300));
        upsert(&mut memory, fact("Haystack", &content)).unwrap();
        upsert(&mut memory, fact("Needle in the title", "short body")).unwrap();

        let found = search(&memory, &query("needle"));
        let framed = found
            .results
            .iter()
            .find(|hit| hit.title == "Haystack")
            .expect("hit");

        assert!(framed.excerpt.contains("needle"));
        assert!(framed.excerpt.starts_with('…'), "got: {}", framed.excerpt);
        assert!(framed.excerpt.ends_with('…'), "got: {}", framed.excerpt);
        assert!(
            framed.excerpt.chars().count() <= EXCERPT_CHARS + 2,
            "excerpt is {} characters",
            framed.excerpt.chars().count()
        );

        // Matched on the title only: the head of the content is the fallback.
        let head = found
            .results
            .iter()
            .find(|hit| hit.title == "Needle in the title")
            .expect("hit");
        assert_eq!(head.excerpt, "short body");
    }

    #[test]
    fn filters_narrow_by_tag_and_by_origin_workspace() {
        let mut memory = Memory::default();
        upsert(
            &mut memory,
            NewMemory {
                title: "Windows quirk".into(),
                content: "paths are backslashed".into(),
                tags: vec!["Windows".into()],
                workspace: "C:/Users/simnJS/Documents/Hangar".into(),
                ..NewMemory::default()
            },
        )
        .unwrap();
        upsert(
            &mut memory,
            NewMemory {
                title: "Linux quirk".into(),
                content: "paths are fine".into(),
                tags: vec!["linux".into()],
                workspace: "/home/simnjs/other".into(),
                ..NewMemory::default()
            },
        )
        .unwrap();

        let by_tag = search(
            &memory,
            &SearchQuery {
                q: "paths".into(),
                tag: Some("WINDOWS".into()),
                ..SearchQuery::default()
            },
        );
        assert_eq!(by_tag.total, 1);
        assert_eq!(by_tag.results[0].title, "Windows quirk");

        let by_workspace = search(
            &memory,
            &SearchQuery {
                q: "paths".into(),
                workspace: Some("HANGAR".into()),
                ..SearchQuery::default()
            },
        );
        assert_eq!(by_workspace.total, 1);
        assert_eq!(by_workspace.results[0].title, "Windows quirk");
    }

    /// The index the proxy serves to models: filtered like the search, with
    /// the content replaced by its size.
    #[test]
    fn the_listing_filters_like_the_search_and_carries_no_content() {
        let mut memory = Memory::default();
        upsert(&mut memory, tagged("Rusty", "abcde", &["Rust"])).unwrap();
        upsert(
            &mut memory,
            NewMemory {
                title: "Other".into(),
                content: "body".into(),
                workspace: "C:/Users/x/Hangar".into(),
                ..NewMemory::default()
            },
        )
        .unwrap();

        let all = list(&memory, None, None);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].chars, 5, "content travels as its size only");

        // Case-insensitive tag, like the search route.
        let by_tag = list(&memory, Some("rust"), None);
        assert_eq!(by_tag.len(), 1);
        assert_eq!(by_tag[0].title, "Rusty");

        // Substring on the origin, like the search route.
        let by_workspace = list(&memory, Some(""), Some("hangar"));
        assert_eq!(by_workspace.len(), 1);
        assert_eq!(by_workspace[0].title, "Other");

        // Empty filters are no filters: a model that fills every property of
        // the schema still gets the whole index.
        assert_eq!(list(&memory, Some(" "), Some("")).len(), 2);
    }

    #[test]
    fn the_result_count_survives_the_limit() {
        let mut memory = Memory::default();
        for nth in 0..5 {
            upsert(&mut memory, fact(&format!("fact {nth}"), "shared word")).unwrap();
        }

        let found = search(
            &memory,
            &SearchQuery {
                q: "shared".into(),
                limit: Some(2),
                ..SearchQuery::default()
            },
        );

        assert_eq!(found.results.len(), 2);
        assert_eq!(found.total, 5, "total is what matched, not what was sent");
    }

    #[test]
    fn renaming_onto_another_title_is_refused_so_the_upsert_key_stays_unique() {
        let mut memory = Memory::default();
        upsert(&mut memory, fact("first", "body")).unwrap();
        let (second, _) = upsert(&mut memory, fact("second", "body")).unwrap();

        let err = patch(
            &mut memory,
            &second.id,
            MemoryPatch {
                title: Some("FIRST".into()),
                ..MemoryPatch::default()
            },
        )
        .expect_err("refused");

        assert!(err.contains("already titled"), "got: {err}");
        assert_eq!(memory.entries[1].title, "second");
    }

    #[test]
    fn deleting_an_unknown_entry_is_an_error() {
        let mut memory = Memory::default();
        upsert(&mut memory, fact("kept", "body")).unwrap();

        assert!(delete(&mut memory, "no-such-id").is_err());
        assert!(patch(&mut memory, "no-such-id", MemoryPatch::default()).is_err());
        assert_eq!(memory.entries.len(), 1);
    }

    #[test]
    fn an_entry_written_through_the_store_is_read_back_from_the_file() {
        let dir = temp_dir("persist");
        let store = MemoryStore::default();

        store
            .update(&dir, |memory| upsert(memory, fact("Persisted", "body")))
            .expect("write");

        let reread = store.read(&dir).expect("read back");
        assert_eq!(reread.entries.len(), 1);
        assert_eq!(reread.entries[0].title, "Persisted");
        assert!(memory_path(&dir).is_file());
        assert!(
            !dir.join("memory.json.tmp").exists(),
            "the temporary file has to be renamed away, not left behind"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_refused_write_leaves_the_file_untouched() {
        let dir = temp_dir("refused");
        let store = MemoryStore::default();
        store
            .update(&dir, |memory| upsert(memory, fact("Kept", "body")))
            .expect("write");

        store
            .update(&dir, |memory| upsert(memory, fact("", "body")))
            .expect_err("refused");

        assert_eq!(store.read(&dir).expect("read back").entries.len(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unreadable_file_is_moved_aside_instead_of_being_overwritten() {
        let dir = temp_dir("corrupt");
        let store = MemoryStore::default();
        fs::write(memory_path(&dir), "{ this is not json").unwrap();

        assert!(store
            .read(&dir)
            .expect("a file that will not parse is not a read failure")
            .entries
            .is_empty());

        assert!(!memory_path(&dir).exists(), "the file has to be moved away");
        let rescued: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt."))
            .collect();
        assert_eq!(rescued.len(), 1, "exactly one rescue copy");

        fs::remove_dir_all(&dir).ok();
    }

    /// The test that guards the data loss.
    ///
    /// A memory file that exists but will not open — on Windows a lock held for
    /// a moment by an antivirus or an indexer, the scenario store.rs already
    /// documents — used to read as an empty memory, and the write that followed
    /// persisted that emptiness over everything the agents had learned. A
    /// directory named `memory.json` reproduces the failure portably: opening
    /// it errors on Windows and on Unix alike, with a kind that is never
    /// `NotFound`.
    #[test]
    fn a_memory_that_exists_but_will_not_open_refuses_the_write_instead_of_emptying_it() {
        let dir = temp_dir("unopenable");
        let store = MemoryStore::default();
        fs::create_dir_all(memory_path(&dir)).expect("stand-in for an unreadable file");

        let err = store
            .update(&dir, |memory| upsert(memory, fact("New", "body")))
            .expect_err("the write has to be refused");

        let message = err.to_string();
        assert!(message.contains("could not be read"), "got: {message}");
        assert!(message.contains("nothing was modified"), "got: {message}");
        assert!(
            matches!(err, StoreError::Storage(_)),
            "an I/O failure is not a refused mutation"
        );

        // Nothing written and nothing renamed: quarantine is for a file that
        // parses to nothing, not for one that may simply be busy right now.
        assert!(
            memory_path(&dir).is_dir(),
            "the store must have left the path alone"
        );
        let strays: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "memory.json")
            .collect();
        assert!(
            strays.is_empty(),
            "no rescue copy and no leftover .tmp, got: {strays:?}"
        );

        // And a plain read says so too, instead of answering "empty memory".
        assert!(
            store.read(&dir).is_err(),
            "a read that failed must not look like a store with nothing in it"
        );

        fs::remove_dir_all(&dir).ok();
    }

    /// The other half of the same distinction: a first launch has no file at
    /// all, which is normal and must stay silent. Only a file that is there and
    /// will not open is an error.
    #[test]
    fn a_memory_that_was_never_written_still_reads_as_empty() {
        let dir = temp_dir("absent");
        let store = MemoryStore::default();

        let memory = store
            .read(&dir)
            .expect("an absent file is an empty memory, not a failure");
        assert!(memory.entries.is_empty());

        store
            .update(&dir, |memory| upsert(memory, fact("First", "body")))
            .expect("the first write has to go through");
        assert_eq!(store.read(&dir).expect("read back").entries.len(), 1);

        fs::remove_dir_all(&dir).ok();
    }
}
