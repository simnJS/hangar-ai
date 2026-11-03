use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

/// One `folders[]` entry of a VS Code workspace file, resolved against the
/// machine the file is opened on.
#[derive(Serialize)]
pub struct WorkspaceRoot {
    /// The label the file gives, falling back to the directory name.
    pub name: String,
    pub path: String,
    /// A workspace file outlives the folders it points at: generated trees get
    /// wiped, and the same file opened on another machine describes paths that
    /// were never there. Reported rather than dropped, so the dialog can show
    /// what is missing instead of quietly handing back a shorter list.
    pub exists: bool,
}

#[tauri::command]
pub fn read_code_workspace(path: String) -> Result<Vec<WorkspaceRoot>, String> {
    let file = Path::new(&path);
    let raw = fs::read_to_string(file).map_err(|e| format!("{path}: {e}"))?;
    let parsed = parse_jsonc(&raw).map_err(|e| format!("{path}: {e}"))?;

    let folders = parsed
        .get("folders")
        .and_then(|folders| folders.as_array())
        .ok_or_else(|| format!("{path}: no \"folders\" list"))?;

    // Relative entries are relative to the file, not to whatever directory this
    // process happens to be sitting in.
    let base = file.parent().unwrap_or_else(|| Path::new("."));
    Ok(folders
        .iter()
        .filter_map(|entry| resolve_root(entry, base))
        .collect())
}

/// The `.code-workspace` sitting directly in `dir`, if there is exactly one
/// obvious candidate. Used to offer the workspace file to someone who picked
/// the folder holding it.
#[tauri::command]
pub fn find_code_workspace(dir: String) -> Option<String> {
    let entries = fs::read_dir(&dir).ok()?;
    let mut found: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().is_some_and(|ext| ext == "code-workspace")
        })
        .collect();
    // `read_dir` promises no order, and the dialog would otherwise offer a
    // different file between two runs of the same folder.
    found.sort();
    found.into_iter().next().map(|path| display_path(&path))
}

fn resolve_root(entry: &serde_json::Value, base: &Path) -> Option<WorkspaceRoot> {
    let raw = entry.get("path")?.as_str()?;
    let candidate = Path::new(raw);
    let resolved = clean(&if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    });

    let name = entry
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            resolved
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| raw.to_owned())
        });

    Some(WorkspaceRoot {
        name,
        exists: resolved.is_dir(),
        path: display_path(&resolved),
    })
}

/// Resolves `.` and `..` without touching the disk.
///
/// `canonicalize` would do it, but on Windows it hands back a `\\?\C:\…`
/// verbatim path, and that is what would end up typed into a terminal and shown
/// in the dialog. This only has to tidy what the file wrote — a single `"."`,
/// the usual entry for a one-folder workspace, is the case that matters.
fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            // Only pop a component we appended ourselves: a leading `..` has
            // nothing above it to drop, and eating it would silently point the
            // root somewhere else.
            Component::ParentDir => {
                let above_is_a_folder =
                    matches!(out.components().next_back(), Some(Component::Normal(_)));
                if above_is_a_folder {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Workspace files are written with forward slashes even on Windows. Everything
/// else in the app shows the separator the OS uses, so these paths are lined up
/// with the ones the folder picker returns.
fn display_path(path: &Path) -> String {
    let shown = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        shown.replace('/', "\\")
    } else {
        shown
    }
}

/// VS Code reads its workspace files as JSON with comments, and hand-edited
/// ones use that. Plain JSON is tried first: the strip below only has to run on
/// files `serde_json` already refused, so a valid file can never be mangled by
/// it.
fn parse_jsonc(raw: &str) -> Result<serde_json::Value, String> {
    match serde_json::from_str(raw) {
        Ok(value) => Ok(value),
        Err(strict) => serde_json::from_str(&strip_jsonc(raw))
            .map_err(|lenient| format!("{strict} (and {lenient} without comments)")),
    }
}

/// Drops `//` and `/* */` comments and trailing commas, leaving anything inside
/// a string literal alone — `"https://…"` is the case that a naive pass ruins.
fn strip_jsonc(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    // Where the comma that may turn out to be trailing landed in `out`.
    let mut pending_comma: Option<usize> = None;

    while let Some(current) = chars.next() {
        if in_string {
            out.push(current);
            // Read before it is updated: `\"` closes nothing, and testing the
            // fresh value would end the string on the very quote it escapes.
            let after_backslash = escaped;
            escaped = !escaped && current == '\\';
            if current == '"' && !after_backslash {
                in_string = false;
            }
            continue;
        }

        match current {
            '/' if chars.peek() == Some(&'/') => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        // Kept so line numbers in a later parse error still
                        // point at the line the user is looking at.
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                    }
                    if previous == '*' && skipped == '/' {
                        break;
                    }
                    previous = skipped;
                }
            }
            ',' => {
                pending_comma = Some(out.len());
                out.push(',');
            }
            '}' | ']' => {
                if let Some(at) = pending_comma.take() {
                    out.remove(at);
                }
                out.push(current);
            }
            // Whitespace between a comma and a closing brace is what makes the
            // comma trailing, so it must not count as content.
            c if c.is_whitespace() => out.push(c),
            c => {
                pending_comma = None;
                if c == '"' {
                    in_string = true;
                    escaped = false;
                }
                out.push(c);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_json_is_untouched() {
        let raw = r#"{"folders":[{"path":"C:/code"}]}"#;
        assert_eq!(
            parse_jsonc(raw).unwrap(),
            serde_json::from_str::<serde_json::Value>(raw).unwrap()
        );
    }

    #[test]
    fn comments_and_trailing_commas_are_dropped() {
        let raw = r#"{
            // the project
            "folders": [
                { "path": "src" }, /* and nothing else */
            ],
        }"#;
        let parsed = parse_jsonc(raw).unwrap();
        assert_eq!(parsed["folders"][0]["path"], "src");
    }

    #[test]
    fn slashes_inside_strings_survive() {
        let raw = r#"{"folders":[{"path":"https://example.com/a,"}]}"#;
        let parsed = parse_jsonc(raw).unwrap();
        assert_eq!(parsed["folders"][0]["path"], "https://example.com/a,");
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_string() {
        let raw = r#"{"folders":[{"name":"say \" // now","path":"a"},]}"#;
        let parsed = parse_jsonc(raw).unwrap();
        assert_eq!(parsed["folders"][0]["name"], "say \" // now");
        assert_eq!(parsed["folders"][0]["path"], "a");
    }

    #[test]
    fn relative_entries_resolve_against_the_file() {
        let base = Path::new("/projects/app");
        let entry = serde_json::json!({ "path": "." });
        let root = resolve_root(&entry, base).unwrap();
        assert_eq!(root.path, display_path(Path::new("/projects/app")));

        let entry = serde_json::json!({ "path": "../shared" });
        let root = resolve_root(&entry, base).unwrap();
        assert_eq!(root.path, display_path(Path::new("/projects/shared")));
        assert_eq!(root.name, "shared");
    }

    #[test]
    fn a_workspace_file_resolves_to_its_folders() {
        let dir = std::env::temp_dir().join("hangar-code-workspace-test");
        let present = dir.join("app");
        fs::create_dir_all(&present).unwrap();
        let file = dir.join("demo.code-workspace");
        fs::write(
            &file,
            r#"{
                // one folder that is here, one that is not
                "folders": [
                    { "path": "app" },
                    { "name": "gone", "path": "nowhere" },
                ]
            }"#,
        )
        .unwrap();

        let roots = read_code_workspace(file.to_string_lossy().into_owned()).unwrap();
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].path, display_path(&present));
        assert_eq!(roots[0].name, "app");
        assert!(roots[0].exists);
        assert_eq!(roots[1].name, "gone");
        assert!(!roots[1].exists);

        assert_eq!(
            find_code_workspace(dir.to_string_lossy().into_owned()),
            Some(display_path(&file))
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_named_entry_keeps_its_label() {
        let entry = serde_json::json!({ "name": "/Verse.org", "path": "/tmp/verse" });
        assert_eq!(
            resolve_root(&entry, Path::new("/")).unwrap().name,
            "/Verse.org"
        );
    }
}
