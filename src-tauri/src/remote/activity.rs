//! What happened in the projects, kept on disk so a week away can be read
//! back after any number of restarts.
//!
//! Append-only JSON lines, one file per day (UTC) in `<app data>/activity/`,
//! named `YYYY-MM-DD.jsonl`; the window writes them (lib/activityLog.ts) and
//! the phone's "Since your last visit" reads them (remote_history). A day
//! older than 30 is deleted the next time something is written.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::DateTime;
use serde::{Deserialize, Serialize};

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Today and the 29 days before it.
const KEEP_DAYS: u64 = 30;
/// What a read returns when it is not told otherwise, and the most it ever does.
pub const DEFAULT_LIMIT: usize = 500;
pub const MAX_LIMIT: usize = 2000;
/// A line is a sentence, not a log: a runaway text is cut.
const MAX_TEXT: usize = 500;
const MAX_WORKSPACE: usize = 200;
const MAX_KIND: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Milliseconds since the epoch.
    pub at: u64,
    pub workspace: String,
    pub kind: String,
    pub text: String,
}

fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

impl Entry {
    fn trimmed(&self) -> Self {
        Self {
            at: self.at,
            workspace: clip(&self.workspace, MAX_WORKSPACE),
            kind: clip(&self.kind, MAX_KIND),
            text: clip(&self.text, MAX_TEXT),
        }
    }
}

/// `2026-10-28`, the file an instant belongs to.
fn day_of(at: u64) -> String {
    DateTime::from_timestamp_millis(at as i64)
        .map(|time| time.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

fn file_day(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let day = name.strip_suffix(".jsonl")?;
    // Shaped like a day, or not ours to touch.
    let bytes = day.as_bytes();
    let shaped = bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
    shaped.then(|| day.to_string())
}

pub struct ActivityLog {
    dir: PathBuf,
    /// Appends, prunes and reads one at a time: a line is never cut by another.
    lock: Mutex<()>,
}

impl ActivityLog {
    pub fn open(dir: PathBuf) -> Self {
        Self {
            dir,
            lock: Mutex::new(()),
        }
    }

    /// Every entry goes to the file of its own day; those older than the 30
    /// days kept are not written at all. `now` decides which days are old.
    pub fn append(&self, entries: &[Entry], now: u64) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        let oldest = day_of(now.saturating_sub((KEEP_DAYS - 1) * DAY_MS));
        let mut days: Vec<(String, String)> = Vec::new();
        for entry in entries {
            let day = day_of(entry.at);
            if day < oldest {
                continue;
            }
            let line = serde_json::to_string(&entry.trimmed()).map_err(|err| err.to_string())?;
            match days.iter_mut().find(|(name, _)| *name == day) {
                Some((_, lines)) => {
                    lines.push('\n');
                    lines.push_str(&line);
                }
                None => days.push((day, line)),
            }
        }
        if !days.is_empty() {
            fs::create_dir_all(&self.dir)
                .map_err(|err| format!("could not create the activity folder: {err}"))?;
        }
        for (day, lines) in days {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join(format!("{day}.jsonl")))
                .map_err(|err| format!("could not open the activity log: {err}"))?;
            file.write_all(format!("{lines}\n").as_bytes())
                .map_err(|err| format!("could not write the activity log: {err}"))?;
        }
        self.prune(&oldest);
        Ok(())
    }

    /// Deleting is best effort: a file that stays is tried again next time.
    fn prune(&self, oldest: &str) {
        for (day, path) in self.days() {
            if day.as_str() < oldest {
                let _ = fs::remove_file(path);
            }
        }
    }

    /// The day files, newest first.
    fn days(&self) -> Vec<(String, PathBuf)> {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut days: Vec<(String, PathBuf)> = read
            .flatten()
            .filter_map(|item| {
                let path = item.path();
                file_day(&path).map(|day| (day, path))
            })
            .collect();
        days.sort_by(|a, b| b.0.cmp(&a.0));
        days
    }

    /// What happened after `since` (ms), newest first, at most `limit`
    /// entries: the newest ones, when there are more.
    pub fn read(&self, since: u64, limit: usize) -> Vec<Entry> {
        let _guard = self.lock.lock().unwrap();
        let limit = limit.clamp(1, MAX_LIMIT);
        let mut out: Vec<Entry> = Vec::new();
        for (day, path) in self.days() {
            // A day that ended before `since` and every one before it.
            let Ok(start) = chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d") else {
                continue;
            };
            let end = start
                .and_hms_opt(0, 0, 0)
                .map(|time| time.and_utc().timestamp_millis() as u64 + DAY_MS)
                .unwrap_or(u64::MAX);
            if end <= since.saturating_add(1) {
                break;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let mut today: Vec<Entry> = text
                .lines()
                .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
                .filter(|entry| entry.at > since)
                .collect();
            // Written in the order they came; the newest last.
            today.reverse();
            today.sort_by_key(|entry| std::cmp::Reverse(entry.at));
            out.extend(today);
            if out.len() >= limit {
                break;
            }
        }
        out.truncate(limit);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-10-28T12:00:00Z
    const NOON: u64 = 1_793_188_800_000;

    fn log(tag: &str) -> (ActivityLog, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("hangar-activity-{tag}-{}", uuid::Uuid::new_v4()));
        (ActivityLog::open(dir.clone()), dir)
    }

    fn entry(at: u64, text: &str) -> Entry {
        Entry {
            at,
            workspace: "Shop".into(),
            kind: "delivered".into(),
            text: text.into(),
        }
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .map(|read| {
                read.flatten()
                    .map(|item| item.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn the_day_of_an_instant_is_its_utc_date() {
        assert_eq!(day_of(NOON), "2026-10-28");
        assert_eq!(day_of(NOON + 12 * 3_600_000 - 1), "2026-10-28");
        assert_eq!(day_of(NOON + 12 * 3_600_000), "2026-10-29");
    }

    #[test]
    fn what_is_appended_is_read_back_newest_first() {
        let (log, dir) = log("append");
        log.append(
            &[entry(NOON, "one"), entry(NOON + 1000, "two")],
            NOON + 2000,
        )
        .unwrap();
        log.append(&[entry(NOON + 500, "between")], NOON + 2000)
            .unwrap();
        let read = log.read(0, DEFAULT_LIMIT);
        assert_eq!(
            read.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["two", "between", "one"]
        );
        assert_eq!(files(&dir), ["2026-10-28.jsonl"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_written_yet_reads_empty() {
        let (log, _dir) = log("empty");
        assert!(log.read(0, DEFAULT_LIMIT).is_empty());
    }

    #[test]
    fn a_read_since_a_time_leaves_out_what_came_before() {
        let (log, dir) = log("since");
        log.append(
            &[
                entry(NOON, "old"),
                entry(NOON + 1000, "edge"),
                entry(NOON + 2000, "new"),
            ],
            NOON + 3000,
        )
        .unwrap();
        let read = log.read(NOON + 1000, DEFAULT_LIMIT);
        assert_eq!(
            read.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["new"]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn each_day_has_its_own_file() {
        let (log, dir) = log("rotation");
        let day = DAY_MS;
        log.append(
            &[
                entry(NOON - day, "yesterday"),
                entry(NOON, "today"),
                entry(NOON + day, "tomorrow"),
            ],
            NOON + day,
        )
        .unwrap();
        assert_eq!(
            files(&dir),
            ["2026-10-27.jsonl", "2026-10-28.jsonl", "2026-10-29.jsonl"]
        );
        let all = log.read(0, DEFAULT_LIMIT);
        assert_eq!(
            all.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["tomorrow", "today", "yesterday"]
        );
        // A read since today skips yesterday's file altogether.
        let recent = log.read(NOON - 1, DEFAULT_LIMIT);
        assert_eq!(recent.len(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn days_older_than_thirty_are_deleted_and_never_written() {
        let (log, dir) = log("prune");
        fs::create_dir_all(&dir).unwrap();
        for day in ["2026-09-28", "2026-09-29", "2026-09-30", "2026-10-27"] {
            fs::write(dir.join(format!("{day}.jsonl")), "").unwrap();
        }
        fs::write(dir.join("notes.txt"), "mine").unwrap();
        // Today is the 28th of October: the 30 days kept begin on the 29th of September.
        log.append(
            &[entry(NOON, "today"), entry(NOON - 31 * DAY_MS, "too old")],
            NOON,
        )
        .unwrap();
        assert_eq!(
            files(&dir),
            [
                "2026-09-29.jsonl",
                "2026-09-30.jsonl",
                "2026-10-27.jsonl",
                "2026-10-28.jsonl",
                "notes.txt",
            ]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_read_stops_at_its_cap_keeping_the_newest() {
        let (log, dir) = log("cap");
        let many: Vec<Entry> = (0..10).map(|i| entry(NOON + i, &format!("n{i}"))).collect();
        log.append(&many, NOON + 100).unwrap();
        log.append(&[entry(NOON - DAY_MS, "yesterday")], NOON + 100)
            .unwrap();
        let read = log.read(0, 4);
        assert_eq!(
            read.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["n9", "n8", "n7", "n6"]
        );
        // Zero is read as one; the ceiling holds however much is asked.
        assert_eq!(log.read(0, 0).len(), 1);
        let big: Vec<Entry> = (0..(MAX_LIMIT as u64 + 50))
            .map(|i| entry(NOON + i, "x"))
            .collect();
        log.append(&big, NOON + 5000).unwrap();
        assert_eq!(log.read(0, usize::MAX).len(), MAX_LIMIT);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_line_is_skipped_and_a_long_text_is_cut() {
        let (log, dir) = log("damage");
        log.append(&[entry(NOON, &"é".repeat(MAX_TEXT + 100))], NOON)
            .unwrap();
        let path = dir.join("2026-10-28.jsonl");
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("{not json\n");
        fs::write(&path, text).unwrap();
        log.append(&[entry(NOON + 1, "after")], NOON).unwrap();
        let read = log.read(0, DEFAULT_LIMIT);
        assert_eq!(read.len(), 2);
        assert_eq!(read[1].text.chars().count(), MAX_TEXT);
        let _ = fs::remove_dir_all(dir);
    }
}
