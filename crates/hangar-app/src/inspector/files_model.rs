//! The inspector's list of changed files (`lib/inspectorFiles.ts`), worked
//! out from two sources that speak different dialects: the paths the agent
//! reported editing (absolute, in whatever separators its platform used) and
//! git's view of the working tree (relative to the repository root, forward
//! slashes, with line counts).
//!
//! Pure, so the matching rules can be tested without a repository.

use std::collections::HashMap;
use std::sync::Arc;

use hangar_core::git::{ChangedFile, Changes};

/// One row of the section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    /// Stable across refreshes: the normalised path the row stands for.
    pub key: String,
    /// What the row reads, relative to the repository when the file is in it.
    pub path: String,
    /// The directory part, slash included; empty for a file at the top.
    pub dir: String,
    pub name: String,
    /// M, A, D, R… as the inspector shows it; `None` when git has nothing on
    /// it.
    pub status: Option<String>,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub binary: bool,
    /// git read the repository and has nothing pending for this file: it was
    /// committed since, or put back. Never set when git could not be asked.
    pub settled: bool,
}

/// One of the five squares of a row's bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSegment {
    Add,
    Del,
    None,
}

/// Forward slashes, no doubled or trailing separator (`normalizePath`).
pub fn normalize_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        let c = if c == '\\' { '/' } else { c };
        if c == '/' && out.ends_with('/') {
            continue;
        }
        out.push(c);
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

/// Windows paths compare without regard to case — `C:/Dev` and `c:/dev` are
/// one folder — and git on Windows may report either spelling.
fn comparable(path: &str) -> String {
    let bytes = path.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    if drive {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

/// A folder's changes, with what the Files section needs of them worked out
/// once, when git answers, rather than at every build of the view: every
/// changed file's path in the form it is compared in, and the rows of the
/// folder (`folder_rows`).
#[derive(Debug)]
pub struct Indexed {
    pub changes: Arc<Changes>,
    /// The index in `changes.files` of each file, by its compared path.
    by_key: HashMap<String, usize>,
    /// The rows of the folder asked about, and how many more there are.
    folder: (Vec<FileRow>, usize),
}

impl Indexed {
    /// `changes`, as the section shows them for `folder`, `limit` rows of it.
    pub fn new(changes: Arc<Changes>, folder: &str, limit: usize) -> Indexed {
        let by_key = index_of(&changes);
        let folder = folder_rows(&changes, folder, limit);
        Indexed {
            changes,
            by_key,
            folder,
        }
    }

    /// The files the agent edited (`edited_rows`).
    pub fn edited_rows(&self, edited: &[String], folder: Option<&str>) -> Vec<FileRow> {
        rows_of_edited(edited, Some((&self.changes, &self.by_key)), folder)
    }

    /// The rows of the folder, and how many more there are (`folder_rows`).
    pub fn folder_rows(&self) -> (&[FileRow], usize) {
        (&self.folder.0, self.folder.1)
    }
}

impl std::ops::Deref for Indexed {
    type Target = Changes;

    fn deref(&self) -> &Changes {
        &self.changes
    }
}

/// Every changed file's index, by its compared path.
fn index_of(changes: &Changes) -> HashMap<String, usize> {
    changes
        .files
        .iter()
        .enumerate()
        .map(|(at, file)| (comparable(&normalize_path(&file.path)), at))
        .collect()
}

/// `path` relative to `root`, or `None` when it is not inside it.
pub fn relative_to(path: &str, root: &str) -> Option<String> {
    let p = normalize_path(path);
    let r = normalize_path(root);
    let cp = comparable(&p);
    let cr = comparable(&r);
    if cp == cr {
        return Some(String::new());
    }
    let prefix = if cr.ends_with('/') {
        cr
    } else {
        format!("{cr}/")
    };
    // The prefix is compared in `comparable` form, which lowercases a
    // Windows path and so keeps its length for any drive path in ASCII; the
    // `get` makes the rare path where it does not simply not match.
    if cp.starts_with(&prefix) {
        p.get(prefix.len()..).map(str::to_string)
    } else {
        None
    }
}

/// The directory and the name of a path (`splitPath`).
pub fn split_path(path: &str) -> (String, String) {
    match path.rfind('/') {
        None => (String::new(), path.to_string()),
        Some(at) => (path[..=at].to_string(), path[at + 1..].to_string()),
    }
}

/// git's letter as a person reads it: an untracked file is a new one, a type
/// change is a modification. Renames, copies and conflicts keep their letter.
pub fn status_letter(status: &str) -> String {
    match status {
        "?" => "A".into(),
        "T" => "M".into(),
        other => other
            .chars()
            .next()
            .map(|first| first.to_uppercase().collect())
            .unwrap_or_else(|| "M".into()),
    }
}

fn row_from_change(file: &ChangedFile, path: &str) -> FileRow {
    let (dir, name) = split_path(path);
    FileRow {
        key: comparable(&normalize_path(path)),
        path: path.to_string(),
        dir,
        name,
        status: Some(status_letter(&file.status)),
        additions: file.additions,
        deletions: file.deletions,
        binary: file.binary,
        settled: false,
    }
}

/// The files the agent edited, newest first, each with what git says about it
/// when it knows. A file git no longer lists — committed since, or put back as
/// it was — stays in the list without counts: the agent did touch it. Paths
/// read relative to the repository, or to `folder` (the pane's) when git
/// could not say where the repository is.
pub fn edited_rows(
    edited: &[String],
    changes: Option<&Changes>,
    folder: Option<&str>,
) -> Vec<FileRow> {
    let index = changes.map(index_of);
    rows_of_edited(edited, changes.zip(index.as_ref()), folder)
}

/// `edited_rows`, with the changes' paths already indexed.
fn rows_of_edited(
    edited: &[String],
    known: Option<(&Changes, &HashMap<String, usize>)>,
    folder: Option<&str>,
) -> Vec<FileRow> {
    let changes = known.map(|(changes, _)| changes);
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for raw in edited {
        let absolute = normalize_path(raw);
        let in_repo = changes.and_then(|changes| relative_to(&absolute, &changes.root));
        let in_folder = folder.and_then(|folder| relative_to(&absolute, folder));
        // The TS chose with `||`, which skips an empty string as well: a
        // path equal to the root has no relative form to show.
        let path = in_repo
            .clone()
            .filter(|path| !path.is_empty())
            .or(in_folder.filter(|path| !path.is_empty()))
            .unwrap_or(absolute);
        let key = comparable(&path);
        if !seen.insert(key.clone()) {
            continue;
        }
        let found =
            known.and_then(|(changes, by_key)| by_key.get(&key).map(|at| &changes.files[*at]));
        rows.push(match found {
            Some(known) => row_from_change(known, &path),
            None => {
                let (dir, name) = split_path(&path);
                FileRow {
                    key,
                    path,
                    dir,
                    name,
                    status: None,
                    additions: None,
                    deletions: None,
                    binary: false,
                    settled: in_repo.is_some(),
                }
            }
        });
    }
    rows
}

/// What changed under the pane's own folder, for a pane whose agent reported
/// nothing: the first `limit`, and how many more there are (`folderRows`).
pub fn folder_rows(changes: &Changes, folder: &str, limit: usize) -> (Vec<FileRow>, usize) {
    let Some(within) = relative_to(folder, &changes.root) else {
        // A folder outside the repository it was asked about has nothing of it.
        return (Vec::new(), 0);
    };
    let prefix = if within.is_empty() {
        String::new()
    } else {
        comparable(&format!("{within}/"))
    };
    let mine: Vec<&ChangedFile> = changes
        .files
        .iter()
        .filter(|file| comparable(&normalize_path(&file.path)).starts_with(&prefix))
        .collect();
    let rows = mine
        .iter()
        .take(limit)
        .map(|file| row_from_change(file, &normalize_path(&file.path)))
        .collect();
    (rows, mine.len().saturating_sub(limit))
}

/// The added and removed lines of the rows that have counts.
pub fn diff_totals(rows: &[FileRow]) -> (u32, u32) {
    rows.iter().fold((0, 0), |(additions, deletions), row| {
        (
            additions + row.additions.unwrap_or(0),
            deletions + row.deletions.unwrap_or(0),
        )
    })
}

/// GitHub's five squares: as many lit as lines changed, up to five, split
/// between additions and deletions by their share. Either side that changed at
/// all keeps one square, so a one-line deletion in a big rewrite still shows.
pub fn diff_segments(additions: u32, deletions: u32, segments: usize) -> Vec<DiffSegment> {
    let (a, d) = (u64::from(additions), u64::from(deletions));
    let total = a + d;
    let lit = total.min(segments as u64);
    // JS `Math.round` rounds halves up; the shares are never negative.
    let mut adds = if total > 0 {
        ((lit * a) as f64 / total as f64 + 0.5).floor() as u64
    } else {
        0
    };
    if a > 0 && adds == 0 {
        adds = 1;
    }
    if d > 0 && adds == lit && lit > 0 {
        adds = lit - 1;
    }
    let dels = lit - adds;
    (0..segments as u64)
        .map(|i| {
            if i < adds {
                DiffSegment::Add
            } else if i < adds + dels {
                DiffSegment::Del
            } else {
                DiffSegment::None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::DiffSegment::{Add, Del, None as Off};
    use super::*;

    fn file(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            old_path: None,
            status: "M".into(),
            additions: Some(1),
            deletions: Some(0),
            binary: false,
        }
    }

    fn counted(path: &str, status: &str, additions: u32, deletions: u32) -> ChangedFile {
        ChangedFile {
            status: status.into(),
            additions: Some(additions),
            deletions: Some(deletions),
            ..file(path)
        }
    }

    fn changes(files: Vec<ChangedFile>) -> Changes {
        Changes {
            root: "C:/dev/hangar-ai".into(),
            base: Some("abc1234".into()),
            files,
            truncated: false,
        }
    }

    fn strings(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    mod paths {
        use super::*;

        #[test]
        fn separators_and_trailing_slashes_are_normalised() {
            assert_eq!(normalize_path("C:\\dev\\hangar-ai\\"), "C:/dev/hangar-ai");
            assert_eq!(normalize_path("/home//you/x/"), "/home/you/x");
            assert_eq!(normalize_path("/"), "/");
        }

        #[test]
        fn a_path_inside_a_root_is_found_ignoring_case_only_on_windows() {
            assert_eq!(
                relative_to("c:\\Dev\\hangar-ai\\src\\a.ts", "C:/dev/hangar-ai"),
                Some("src/a.ts".into())
            );
            assert_eq!(
                relative_to("C:/dev/hangar-ai", "C:\\dev\\hangar-ai\\"),
                Some(String::new())
            );
            assert_eq!(
                relative_to("C:/dev/hangar-ai-2/a.ts", "C:/dev/hangar-ai"),
                None
            );
            assert_eq!(relative_to("/home/You/a.ts", "/home/you"), None);
            assert_eq!(
                relative_to("/home/you/a.ts", "/home/you"),
                Some("a.ts".into())
            );
        }

        #[test]
        fn the_directory_is_split_from_the_name() {
            assert_eq!(
                split_path("src/lib/a.ts"),
                ("src/lib/".to_string(), "a.ts".to_string())
            );
            assert_eq!(
                split_path("README.md"),
                (String::new(), "README.md".to_string())
            );
        }

        #[test]
        fn git_s_letters_read_the_way_a_person_would() {
            assert_eq!(status_letter("?"), "A");
            assert_eq!(status_letter("T"), "M");
            assert_eq!(status_letter("R"), "R");
            assert_eq!(status_letter("D"), "D");
        }
    }

    mod edited {
        use super::*;

        #[test]
        fn what_the_agent_edited_is_enriched_with_git_s_counts() {
            let rows = edited_rows(
                &strings(&[
                    "C:\\dev\\hangar-ai\\src\\store.tsx",
                    "C:\\dev\\hangar-ai\\src\\new.ts",
                ]),
                Some(&changes(vec![
                    counted("src/store.tsx", "M", 3, 1),
                    counted("src/new.ts", "?", 64, 0),
                    file("src/untouched.ts"),
                ])),
                None,
            );
            let summary: Vec<_> = rows
                .iter()
                .map(|row| {
                    (
                        row.path.as_str(),
                        row.status.as_deref(),
                        row.additions,
                        row.deletions,
                    )
                })
                .collect();
            assert_eq!(
                summary,
                vec![
                    ("src/store.tsx", Some("M"), Some(3), Some(1)),
                    ("src/new.ts", Some("A"), Some(64), Some(0)),
                ]
            );
            assert_eq!(rows[0].dir, "src/");
            assert_eq!(rows[0].name, "store.tsx");
        }

        #[test]
        fn a_file_git_no_longer_lists_stays_without_counts() {
            let rows = edited_rows(
                &strings(&["C:/dev/hangar-ai/done.ts"]),
                Some(&changes(vec![])),
                None,
            );
            let row = &rows[0];
            assert_eq!(row.path, "done.ts");
            assert_eq!(row.status, None);
            assert_eq!(row.additions, None);
            assert!(row.settled);
        }

        #[test]
        fn without_git_or_outside_the_repository_nothing_is_called_settled() {
            let bare = edited_rows(&strings(&["C:\\elsewhere\\a.ts"]), None, None);
            assert_eq!(bare[0].path, "C:/elsewhere/a.ts");
            assert!(!bare[0].settled);
            let outside = edited_rows(
                &strings(&["C:/elsewhere/a.ts"]),
                Some(&changes(vec![])),
                None,
            );
            assert_eq!(outside[0].path, "C:/elsewhere/a.ts");
            assert!(!outside[0].settled);
        }

        #[test]
        fn the_pane_s_folder_is_the_fallback_when_git_cannot_place_the_file() {
            let rows = edited_rows(
                &strings(&["C:\\dev\\app\\src\\a.ts"]),
                None,
                Some("C:\\dev\\app"),
            );
            let row = &rows[0];
            assert_eq!(
                (
                    row.path.as_str(),
                    row.dir.as_str(),
                    row.name.as_str(),
                    row.settled
                ),
                ("src/a.ts", "src/", "a.ts", false)
            );
        }

        #[test]
        fn a_file_is_listed_once_however_it_was_spelled() {
            let rows = edited_rows(
                &strings(&["C:/dev/hangar-ai/a.ts", "c:\\DEV\\hangar-ai\\a.ts"]),
                Some(&changes(vec![])),
                None,
            );
            assert_eq!(rows.len(), 1);
        }
    }

    mod folder {
        use super::*;

        fn everything() -> Changes {
            let mut files = vec![file("README.md")];
            files.extend((0..10).map(|i| file(&format!("app/f{i}.ts"))));
            changes(files)
        }

        #[test]
        fn the_changes_under_the_pane_s_folder_are_kept_and_capped() {
            let (rows, more) = folder_rows(&everything(), "C:\\dev\\hangar-ai\\app", 8);
            assert_eq!(rows.len(), 8);
            assert_eq!(rows[0].path, "app/f0.ts");
            assert_eq!(more, 2);
        }

        #[test]
        fn everything_at_the_root_is_taken_and_nothing_outside_it() {
            assert_eq!(
                folder_rows(&everything(), "C:/dev/hangar-ai", 20).0.len(),
                11
            );
            assert_eq!(folder_rows(&everything(), "D:/other", 8).0.len(), 0);
        }
    }

    mod summary {
        use super::*;

        #[test]
        fn the_known_counts_are_added_up() {
            let binary = ChangedFile {
                additions: None,
                deletions: None,
                binary: true,
                ..file("c.png")
            };
            let rows = edited_rows(
                &strings(&[
                    "C:/dev/hangar-ai/a.ts",
                    "C:/dev/hangar-ai/b.ts",
                    "C:/dev/hangar-ai/c.png",
                ]),
                Some(&changes(vec![
                    counted("a.ts", "M", 3, 1),
                    counted("b.ts", "M", 64, 0),
                    binary,
                ])),
                None,
            );
            assert_eq!(diff_totals(&rows), (67, 1));
        }

        #[test]
        fn one_square_is_lit_per_line_up_to_five_shared_by_their_weight() {
            assert_eq!(diff_segments(0, 0, 5), [Off, Off, Off, Off, Off]);
            assert_eq!(diff_segments(1, 0, 5), [Add, Off, Off, Off, Off]);
            assert_eq!(diff_segments(3, 1, 5), [Add, Add, Add, Del, Off]);
            assert_eq!(diff_segments(64, 0, 5), [Add, Add, Add, Add, Add]);
            assert_eq!(diff_segments(0, 9, 5), [Del, Del, Del, Del, Del]);
        }

        #[test]
        fn a_small_side_never_vanishes() {
            assert_eq!(diff_segments(100, 1, 5), [Add, Add, Add, Add, Del]);
            assert_eq!(diff_segments(1, 100, 5), [Add, Del, Del, Del, Del]);
        }
    }

    mod indexed {
        use super::*;

        #[test]
        fn rows_worked_out_once_are_the_ones_worked_out_each_time() {
            let mut files: Vec<ChangedFile> = (0..2000)
                .map(|n| counted(&format!("src/m{n}/f.rs"), "M", n, 1))
                .collect();
            files.push(counted("SRC/Upper.ts", "A", 3, 0));
            let found = changes(files);
            let edited = strings(&[
                "C:\\dev\\hangar-ai\\src\\m7\\f.rs",
                "c:/DEV/hangar-ai/SRC/Upper.ts",
                "C:/dev/hangar-ai/gone.rs",
            ]);
            let indexed = Indexed::new(Arc::new(found.clone()), "C:/dev/hangar-ai/src", 8);
            assert_eq!(
                indexed.edited_rows(&edited, Some("C:/dev/hangar-ai")),
                edited_rows(&edited, Some(&found), Some("C:/dev/hangar-ai"))
            );
            let rows = indexed.edited_rows(&edited, None);
            assert_eq!(rows[0].additions, Some(7));
            assert_eq!(rows[1].status.as_deref(), Some("A"));
            assert!(rows[2].settled);
            let (rows, more) = indexed.folder_rows();
            let (want, want_more) = folder_rows(&found, "C:/dev/hangar-ai/src", 8);
            assert_eq!((rows.to_vec(), more), (want, want_more));
            assert_eq!(more, 1992);
            // It reads as the changes it holds.
            assert_eq!(indexed.root, "C:/dev/hangar-ai");
        }

        #[test]
        fn paths_are_normalised_and_compared_without_a_regex() {
            assert_eq!(normalize_path("a\\\\b//c///"), "a/b/c");
            assert_eq!(normalize_path("//"), "/");
            assert_eq!(normalize_path(""), "");
            assert_eq!(comparable("C:/Dev/X"), "c:/dev/x");
            assert_eq!(comparable("/home/Me"), "/home/Me");
            assert_eq!(comparable("C:"), "C:");
            assert_eq!(comparable("ab:/x"), "ab:/x");
        }
    }
}
