//! Usage statistics: tokens, active time and their API-equivalent cost, read
//! from the transcripts the agents already keep on disk.
//!
//! Claude Code writes every exchange to `~/.claude/projects/<project>/` —
//! its subagents to a `subagents` folder under the session — and stamps each
//! answer with its token usage. Codex keeps rollouts under
//! `~/.codex/sessions/` and `archived_sessions/`, plain or compressed, with
//! running token totals. Gemini CLI and OpenCode keep nothing readable of the
//! kind, so they do not appear.
//!
//! That is gigabytes for a busy machine, so nothing is read twice: every file
//! leaves a digest (parse.rs) in a cache keyed by its path, size and
//! modification time, kept in memory and written to the app data folder, so
//! that even the first look after a launch only reads what changed. A
//! transcript that only grew picks up where its last read stopped. Nothing
//! leaves the machine: the files are read, never sent.

mod aggregate;
mod parse;

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{Local, Offset, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub use aggregate::{Bucket, ModelInfo, SessionInfo, Window, ACTIVE_GAP_CAP_MS};
use parse::{Agent, Digest};

/// Bumped whenever a digest would come out differently for the same file —
/// a parser fix — so the caches written before it are dropped, not trusted.
const CACHE_VERSION: u32 = 2;
const CACHE_FILE: &str = "usage-stats-cache.json";
/// Bytes hashed at the head of a file to tell an appended one from a
/// rewritten one.
const HEAD_BYTES: usize = 4096;

// ---------------------------------------------------------------------------
// Finding the transcripts
// ---------------------------------------------------------------------------

/// Where each agent keeps its transcripts.
#[derive(Debug, Clone)]
pub struct Roots {
    pub claude: Vec<PathBuf>,
    pub codex: Vec<PathBuf>,
}

impl Roots {
    /// The agents' own defaults, and the variables each one reads to move
    /// them. Claude Code's are the main account's `projects/`, which every
    /// Claude account's profile shares, so a turn made on any account is
    /// counted once — plus the folder of a profile that could not share it
    /// (accounts.rs).
    pub fn from_env() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let under = |var: &str, fallback: &str| {
            std::env::var_os(var)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(fallback))
        };
        let mut claude = crate::accounts::projects_dirs();
        if claude.is_empty() {
            claude.push(under("CLAUDE_CONFIG_DIR", ".claude").join("projects"));
        }
        let codex = under("CODEX_HOME", ".codex");
        Self {
            claude,
            codex: vec![codex.join("sessions"), codex.join("archived_sessions")],
        }
    }
}

#[derive(Debug, Clone)]
struct Found {
    path: PathBuf,
    agent: Agent,
    /// A Claude subagent's transcript.
    sub: bool,
    compressed: bool,
    modified: u64,
    size: u64,
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(&path, depth + 1, out);
        } else {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".jsonl") || name.ends_with(".jsonl.zst") {
                out.push(path);
            }
        }
    }
}

/// What each root holds, for the page to say which agents it could read.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub agent: Agent,
    pub root: String,
    pub found: bool,
    pub files: usize,
}

fn discover(roots: &Roots) -> (Vec<Found>, Vec<Source>) {
    let mut found = Vec::new();
    let mut sources = Vec::new();
    let all = roots
        .claude
        .iter()
        .map(|root| (Agent::Claude, root))
        .chain(roots.codex.iter().map(|root| (Agent::Codex, root)));
    for (agent, root) in all {
        let mut files = Vec::new();
        walk(root, 0, &mut files);
        sources.push(Source {
            agent,
            root: root.to_string_lossy().into_owned(),
            found: root.is_dir(),
            files: files.len(),
        });
        for path in files {
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_millis() as u64);
            found.push(Found {
                sub: path
                    .components()
                    .any(|part| part.as_os_str() == "subagents"),
                compressed: path.extension().is_some_and(|ext| ext == "zst"),
                agent,
                path,
                modified,
                size: meta.len(),
            });
        }
    }
    (found, sources)
}

// ---------------------------------------------------------------------------
// Reading one file
// ---------------------------------------------------------------------------

/// A zstd stream of one or more frames, decoded as one: a rollout appended
/// to after compression is a second frame after the first.
struct Frames<R: BufRead> {
    decoder: Option<ruzstd::decoding::StreamingDecoder<R, ruzstd::decoding::FrameDecoder>>,
}

impl<R: BufRead> Frames<R> {
    fn new(source: R) -> std::io::Result<Self> {
        let decoder =
            ruzstd::decoding::StreamingDecoder::new(source).map_err(std::io::Error::other)?;
        Ok(Self {
            decoder: Some(decoder),
        })
    }
}

impl<R: BufRead> Read for Frames<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let Some(decoder) = self.decoder.as_mut() else {
                return Ok(0);
            };
            let read = decoder.read(buf)?;
            if read > 0 || buf.is_empty() {
                return Ok(read);
            }
            // This frame is done; another may follow.
            let mut source = self.decoder.take().map(|done| done.into_inner()).unwrap();
            if source.fill_buf()?.is_empty() {
                return Ok(0);
            }
            // Whatever follows and is not a frame (padding, a skippable
            // frame) ends the stream rather than the read.
            self.decoder = ruzstd::decoding::StreamingDecoder::new(source).ok();
        }
    }
}

/// What the cache keeps per file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Entry {
    modified: u64,
    size: u64,
    head: u64,
    digest: Digest,
}

fn head_hash(path: &Path) -> std::io::Result<u64> {
    let mut file = fs::File::open(path)?;
    let mut head = vec![0u8; HEAD_BYTES];
    let mut filled = 0;
    while filled < HEAD_BYTES {
        let read = file.read(&mut head[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(parse::fnv1a(&[&head[..filled]]))
}

/// The session a file belongs to when none of its lines said: its own name,
/// which for both agents carries the session id.
fn session_from_name(path: &Path) -> String {
    let stem = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = stem.trim_end_matches(".zst").trim_end_matches(".jsonl");
    stem.to_string()
}

/// Reads a file into an entry: from where `previous` stopped when the file
/// only grew, from scratch otherwise.
fn read_file(found: &Found, previous: Option<&Entry>) -> std::io::Result<Entry> {
    let head = head_hash(&found.path)?;
    let resumable = previous.filter(|entry| {
        !found.compressed
            && entry.head == head
            && found.size >= entry.size
            && entry.digest.offset <= found.size
    });
    let mut file = fs::File::open(&found.path)?;
    let mut digest = match resumable {
        Some(entry) => {
            file.seek(SeekFrom::Start(entry.digest.offset))?;
            entry.digest.clone()
        }
        None => Digest::new(found.agent, found.sub),
    };
    let consumed = if found.compressed {
        let frames = Frames::new(BufReader::with_capacity(256 * 1024, file))?;
        // A compressed rollout is finished: its last line counts even
        // without a newline.
        parse::read_into(&mut digest, frames, true)?
    } else {
        parse::read_into(&mut digest, file, false)?
    };
    digest.offset += consumed;
    if digest.session.is_empty() {
        digest.session = session_from_name(&found.path);
    }
    Ok(Entry {
        modified: found.modified,
        size: found.size,
        head,
        digest,
    })
}

// ---------------------------------------------------------------------------
// The cache
// ---------------------------------------------------------------------------

#[derive(Default, Serialize, Deserialize)]
struct DiskCache {
    version: u32,
    files: HashMap<String, Entry>,
}

#[derive(Default)]
struct Cache {
    /// Read from disk yet, or found missing.
    loaded: bool,
    files: HashMap<String, Entry>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// One scan at a time: a second caller waits for the first one's work
/// instead of redoing it.
fn scanning() -> &'static Mutex<()> {
    static SCANNING: OnceLock<Mutex<()>> = OnceLock::new();
    SCANNING.get_or_init(|| Mutex::new(()))
}

fn load_disk(path: &Path) -> HashMap<String, Entry> {
    let Ok(file) = fs::File::open(path) else {
        return HashMap::new();
    };
    match serde_json::from_reader::<_, DiskCache>(BufReader::new(file)) {
        Ok(cache) if cache.version == CACHE_VERSION => cache.files,
        // Another version's cache, or a damaged one: it is only a cache, and
        // the next scan writes a good one over it.
        _ => HashMap::new(),
    }
}

fn save_disk(path: &Path, files: &HashMap<String, Entry>) -> std::io::Result<()> {
    #[derive(Serialize)]
    struct Borrowed<'a> {
        version: u32,
        files: &'a HashMap<String, Entry>,
    }
    let tmp = path.with_extension("json.tmp");
    {
        let file = fs::File::create(&tmp)?;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer(
            &mut writer,
            &Borrowed {
                version: CACHE_VERSION,
                files,
            },
        )?;
        std::io::Write::flush(&mut writer)?;
    }
    fs::rename(&tmp, path)
}

fn ensure_loaded(cache: &mut Cache, disk: Option<&Path>) -> bool {
    if cache.loaded {
        return false;
    }
    cache.loaded = true;
    if let Some(path) = disk {
        cache.files = load_disk(path);
    }
    true
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanInfo {
    /// Transcripts on disk.
    pub files: usize,
    /// Read this time, whole or from where they stopped.
    pub parsed: usize,
    /// Taken from the cache as they were.
    pub reused: usize,
    /// Could not be read; skipped.
    pub failed: usize,
    /// Bytes read this time.
    pub bytes: u64,
    pub ms: u64,
    /// Answered from the cache alone, without looking at the disk.
    pub cached_only: bool,
}

/// Brings the cache up to date with the disk and reports what that took.
fn refresh(roots: &Roots, disk: Option<&Path>) -> (ScanInfo, Vec<Source>) {
    let _one_at_a_time = scanning().lock().unwrap_or_else(|e| e.into_inner());
    let started = Instant::now();
    let (found, sources) = discover(roots);

    // What needs reading, decided under the lock; the reading happens
    // outside it, so a cached-only answer is never held up by a scan. The
    // files that did not change stay where they are, uncopied.
    let (todo, reused, gone) = {
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        ensure_loaded(&mut cache, disk);
        let mut todo: Vec<(Found, Option<Entry>)> = Vec::new();
        let mut reused = 0;
        for file in found.iter() {
            let key = file.path.to_string_lossy().into_owned();
            match cache.files.get(&key) {
                Some(entry) if entry.modified == file.modified && entry.size == file.size => {
                    reused += 1;
                }
                previous => todo.push((file.clone(), previous.cloned())),
            }
        }
        let on_disk: std::collections::HashSet<String> = found
            .iter()
            .map(|file| file.path.to_string_lossy().into_owned())
            .collect();
        let gone: Vec<String> = cache
            .files
            .keys()
            .filter(|key| !on_disk.contains(*key))
            .cloned()
            .collect();
        (todo, reused, gone)
    };

    let bytes: u64 = todo
        .iter()
        .map(|(file, previous)| match previous {
            Some(entry) if !file.compressed && file.size >= entry.size => {
                file.size - entry.digest.offset.min(file.size)
            }
            _ => file.size,
        })
        .sum();

    // Read in parallel: on a cold start that is gigabytes, and the parse,
    // not the disk, is the slow part.
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(String, std::io::Result<Entry>)>> = Mutex::new(Vec::new());
    let workers = num_cpus::get_physical().clamp(1, 8).min(todo.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, Ordering::Relaxed);
                let Some((file, previous)) = todo.get(at) else {
                    break;
                };
                let entry = read_file(file, previous.as_ref());
                let key = file.path.to_string_lossy().into_owned();
                results
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((key, entry));
            });
        }
    });

    let parsed = todo.len();
    let mut failed = 0;
    let changed = {
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        for (key, entry) in results.into_inner().unwrap_or_else(|e| e.into_inner()) {
            match entry {
                Ok(entry) => {
                    cache.files.insert(key, entry);
                }
                // Locked by another process for a moment, say: what the last
                // read found stays until a read works.
                Err(_) => failed += 1,
            }
        }
        for key in &gone {
            cache.files.remove(key);
        }
        parsed > failed || !gone.is_empty()
    };
    if changed {
        if let Some(path) = disk {
            let cache = cache().lock().unwrap_or_else(|e| e.into_inner());
            if let Err(err) = save_disk(path, &cache.files) {
                eprintln!("usage stats: could not write the cache: {err}");
            }
        }
    }

    let info = ScanInfo {
        files: found.len(),
        parsed: parsed - failed,
        reused,
        failed,
        bytes,
        ms: started.elapsed().as_millis() as u64,
        cached_only: false,
    };
    (info, sources)
}

// ---------------------------------------------------------------------------
// The command
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    pub generated_at: i64,
    pub scan: ScanInfo,
    pub sources: Vec<Source>,
    /// The longest a silence counts for in active time, for the page to say.
    pub active_gap_cap_ms: i64,
    pub sessions: Vec<SessionInfo>,
    pub models: Vec<ModelInfo>,
    pub buckets: Vec<Bucket>,
}

/// The start of the local hour holding `at`. The offset is looked up once
/// per UTC hour: it only changes on the hour, at a daylight-saving switch,
/// and zones half an hour off UTC still get their own hour boundaries.
fn local_hours() -> impl Fn(i64) -> i64 {
    const HOUR: i64 = 3_600_000;
    let offsets: Mutex<HashMap<i64, i64>> = Mutex::new(HashMap::new());
    move |at: i64| {
        let utc_hour = at.div_euclid(HOUR);
        let mut offsets = offsets.lock().unwrap_or_else(|e| e.into_inner());
        let offset = *offsets.entry(utc_hour).or_insert_with(|| {
            Local
                .timestamp_millis_opt(utc_hour * HOUR)
                .single()
                .map_or(0, |local| {
                    local.offset().fix().local_minus_utc() as i64 * 1000
                })
        });
        (at + offset).div_euclid(HOUR) * HOUR - offset
    }
}

fn answer(scan: ScanInfo, sources: Vec<Source>, window: Window) -> UsageStats {
    let cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    let result = aggregate::aggregate(
        cache.files.values().map(|entry| &entry.digest),
        window,
        local_hours(),
    );
    UsageStats {
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as i64),
        scan,
        sources,
        active_gap_cap_ms: ACTIVE_GAP_CAP_MS,
        sessions: result.sessions,
        models: result.models,
        buckets: result.buckets,
    }
}

/// Everything the stats page shows, for the period between `from` and `to`
/// (ms since the epoch, either open).
///
/// `cached_only` answers from what the cache already holds, without looking
/// at the disk — on disk from the previous run if this one has not scanned
/// yet. That answer is instant, and the page shows it while the full one
/// reads whatever changed.
pub fn usage_stats_at(
    roots: &Roots,
    disk: Option<&Path>,
    window: Window,
    cached_only: bool,
) -> UsageStats {
    if cached_only {
        let started = Instant::now();
        let files = {
            let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
            ensure_loaded(&mut cache, disk);
            cache.files.len()
        };
        let scan = ScanInfo {
            files,
            reused: files,
            ms: started.elapsed().as_millis() as u64,
            cached_only: true,
            ..ScanInfo::default()
        };
        return answer(scan, Vec::new(), window);
    }
    let (scan, sources) = refresh(roots, disk);
    answer(scan, sources, window)
}

fn cache_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join(CACHE_FILE))
}

#[tauri::command]
pub async fn usage_stats(
    app: AppHandle,
    from: Option<i64>,
    to: Option<i64>,
    cached_only: Option<bool>,
) -> Result<UsageStats, String> {
    let disk = cache_path(&app);
    crate::git::off_thread(move || {
        Ok(usage_stats_at(
            &Roots::from_env(),
            disk.as_deref(),
            Window { from, to },
            cached_only.unwrap_or(false),
        ))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// The cache is process-wide; tests that scan take turns on it.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static TURN: Mutex<()> = Mutex::new(());
        TURN.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reset_cache() {
        *cache().lock().unwrap() = Cache::default();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hangar-stats-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn assistant(id: &str, at: &str, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{at}","sessionId":"s-1","cwd":"C:\\dev\\app","requestId":"r-{id}","message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"input_tokens":1,"output_tokens":{output},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        )
    }

    fn total_output(stats: &UsageStats) -> u64 {
        stats.buckets.iter().map(|row| row.output).sum()
    }

    struct Fixture {
        root: PathBuf,
        roots: Roots,
        disk: PathBuf,
        transcript: PathBuf,
    }

    fn fixture(tag: &str) -> Fixture {
        let root = temp_dir(tag);
        let project = root.join("claude").join("C--dev-app");
        fs::create_dir_all(&project).unwrap();
        let transcript = project.join("s-1.jsonl");
        fs::write(
            &transcript,
            format!("{}\n", assistant("m1", "2026-10-01T10:00:00Z", 100)),
        )
        .unwrap();
        Fixture {
            roots: Roots {
                claude: vec![root.join("claude")],
                codex: vec![root.join("codex")],
            },
            disk: root.join("cache.json"),
            transcript,
            root,
        }
    }

    #[test]
    fn a_second_scan_reuses_what_did_not_change() {
        let _turn = serial();
        reset_cache();
        let f = fixture("reuse");
        let first = usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);
        assert_eq!((first.scan.parsed, first.scan.reused), (1, 0));
        assert_eq!(total_output(&first), 100);
        assert!(f.disk.is_file());
        assert!(first
            .sources
            .iter()
            .any(|s| s.agent == Agent::Codex && !s.found));

        let second = usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);
        assert_eq!((second.scan.parsed, second.scan.reused), (0, 1));
        assert_eq!(total_output(&second), 100);
        fs::remove_dir_all(&f.root).ok();
    }

    #[test]
    fn an_appended_transcript_is_read_from_where_it_stopped() {
        let _turn = serial();
        reset_cache();
        let f = fixture("append");
        usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);
        let before = fs::metadata(&f.transcript).unwrap().len();

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&f.transcript)
            .unwrap();
        writeln!(file, "{}", assistant("m2", "2026-10-01T10:05:00Z", 30)).unwrap();
        drop(file);
        let after = usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);
        assert_eq!(after.scan.parsed, 1);
        // Only the new line was read.
        assert_eq!(
            after.scan.bytes,
            fs::metadata(&f.transcript).unwrap().len() - before
        );
        assert_eq!(total_output(&after), 130);
        fs::remove_dir_all(&f.root).ok();
    }

    #[test]
    fn a_rewritten_transcript_is_read_again_from_the_start() {
        let _turn = serial();
        reset_cache();
        let f = fixture("rewrite");
        usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);
        // Same session file, other content, longer: the head no longer matches.
        let text = [
            assistant("x1", "2026-10-02T10:00:00Z", 7),
            assistant("x2", "2026-10-02T10:01:00Z", 8),
        ]
        .join("\n");
        fs::write(&f.transcript, format!("{text}\n")).unwrap();
        let after = usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);
        assert_eq!(total_output(&after), 15);
        fs::remove_dir_all(&f.root).ok();
    }

    #[test]
    fn the_cache_on_disk_answers_a_fresh_process_and_a_deleted_file_leaves_it() {
        let _turn = serial();
        reset_cache();
        let f = fixture("disk");
        usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);

        // A new process: nothing in memory, the file on disk.
        reset_cache();
        let instant = usage_stats_at(&f.roots, Some(&f.disk), Window::default(), true);
        assert!(instant.scan.cached_only);
        assert_eq!(total_output(&instant), 100);

        // A cache from another version is not trusted.
        let stale = fs::read_to_string(&f.disk).unwrap().replacen(
            &format!("\"version\":{CACHE_VERSION}"),
            "\"version\":0",
            1,
        );
        fs::write(&f.disk, stale).unwrap();
        reset_cache();
        let empty = usage_stats_at(&f.roots, Some(&f.disk), Window::default(), true);
        assert!(empty.buckets.is_empty());

        fs::remove_file(&f.transcript).unwrap();
        let gone = usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);
        assert_eq!(gone.scan.files, 0);
        assert!(gone.buckets.is_empty());
        fs::remove_dir_all(&f.root).ok();
    }

    #[test]
    fn compressed_codex_rollouts_and_claude_subagents_are_read() {
        let _turn = serial();
        reset_cache();
        let f = fixture("zst");
        let subagents = f.transcript.with_extension("").join("subagents");
        fs::create_dir_all(&subagents).unwrap();
        fs::write(
            subagents.join("agent-a1.jsonl"),
            format!(
                "{}\n",
                assistant("sub-1", "2026-10-01T10:00:30Z", 5).replace(
                    r#""type":"assistant""#,
                    r#""type":"assistant","isSidechain":true"#
                )
            ),
        )
        .unwrap();

        let day = f.root.join("codex").join("2026").join("10").join("02");
        fs::create_dir_all(&day).unwrap();
        let rollout = [
            r#"{"timestamp":"2026-10-02T09:00:00Z","type":"session_meta","payload":{"id":"thread-1","cwd":"/home/me/app","source":"cli"}}"#,
            r#"{"timestamp":"2026-10-02T09:00:01Z","type":"turn_context","payload":{"model":"gpt-5.5"}}"#,
            r#"{"timestamp":"2026-10-02T09:00:10Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":60,"total_tokens":1060}}}}"#,
        ]
        .join("\n");
        let compressed = ruzstd::encoding::compress_to_vec(
            rollout.as_bytes(),
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        fs::write(
            day.join("rollout-2026-10-02T09-00-00-thread-1.jsonl.zst"),
            compressed,
        )
        .unwrap();

        let stats = usage_stats_at(&f.roots, Some(&f.disk), Window::default(), false);
        assert_eq!(stats.scan.files, 3);
        assert_eq!(stats.scan.failed, 0);
        // 100 from the session, 5 from its subagent, 60 from Codex.
        assert_eq!(total_output(&stats), 165);
        // The subagent is part of its session, not one of its own.
        assert_eq!(stats.sessions.len(), 2);
        let codex = stats
            .sessions
            .iter()
            .find(|s| s.agent == Agent::Codex)
            .unwrap();
        assert_eq!(codex.cwd, "/home/me/app");
        assert!(stats.models.iter().any(|m| m.id == "gpt-5.5" && m.priced));
        fs::remove_dir_all(&f.root).ok();
    }

    /// The real transcripts of this machine, for timings and totals:
    /// `cargo test --release stats::tests::real -- --ignored --nocapture`.
    /// Prints sums and durations only — never a line of a conversation.
    #[test]
    #[ignore]
    fn real_transcripts_cold_then_warm() {
        let _turn = serial();
        reset_cache();
        let roots = Roots::from_env();
        let dir = temp_dir("real");
        let disk = dir.join("cache.json");

        let cold = usage_stats_at(&roots, Some(&disk), Window::default(), false);
        let warm = usage_stats_at(&roots, Some(&disk), Window::default(), false);
        reset_cache();
        let launch = Instant::now();
        let from_disk = usage_stats_at(&roots, Some(&disk), Window::default(), true);
        let from_disk_ms = launch.elapsed().as_millis();

        let sum = |pick: fn(&Bucket) -> u64| cold.buckets.iter().map(pick).sum::<u64>();
        let cost: f64 = cold.buckets.iter().map(|row| row.cost).sum();
        let active: i64 = cold.buckets.iter().map(|row| row.active_ms).sum();
        let turns: u32 = cold.buckets.iter().map(|row| row.turns).sum();
        println!(
            "files {} ({} failed), {:.2} GB read",
            cold.scan.files,
            cold.scan.failed,
            cold.scan.bytes as f64 / 1e9
        );
        println!(
            "cold scan {} ms, warm scan {} ms, cache-only after a restart {} ms",
            cold.scan.ms, warm.scan.ms, from_disk_ms
        );
        println!(
            "cache on disk {:.1} MB",
            fs::metadata(&disk).map_or(0, |m| m.len()) as f64 / 1e6
        );
        println!(
            "sessions {}, turns {}, models {}, buckets {}, active {:.1} h",
            cold.sessions.len(),
            turns,
            cold.models.len(),
            cold.buckets.len(),
            active as f64 / 3.6e6
        );
        println!(
            "tokens: input {}, output {}, cache write {}, cache read {}; API-equivalent ${cost:.2}",
            sum(|r| r.input),
            sum(|r| r.output),
            sum(|r| r.cache_write),
            sum(|r| r.cache_read)
        );
        for source in &cold.sources {
            println!(
                "source {:?} found={} files={}",
                source.agent, source.found, source.files
            );
        }
        assert_eq!(from_disk.buckets.len(), cold.buckets.len());
        fs::remove_dir_all(&dir).ok();
    }
}
