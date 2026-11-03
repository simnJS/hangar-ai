//! What a kit's tab may touch of the workspace it is shown in, through the
//! window (src/lib/kitApi.ts): a text file it reads (`files.read`), and a
//! file it opens in the preview (`preview.show`). The window checks the
//! path a tab gives first; these commands check it again on disk, where
//! links are followed:
//!
//! - relative to the workspace folder, either slash, no `..`, no drive or
//!   stream (`:`), no hidden file or folder (`.env`, `.git/…`);
//! - inside the folder once its links and junctions are followed;
//! - for `files.read`, a UTF-8 text file of 1 MB at most.

use std::io::Read;
use std::path::{Path, PathBuf};

/// `KIT_FILE_MAX_BYTES` in src/lib/kits.ts.
pub const MAX_READ_BYTES: u64 = 1024 * 1024;

/// The longest path a tab may give.
const MAX_PATH_CHARS: usize = 1024;

/// The parts of a path a tab gave, relative to the workspace folder.
fn relative_parts(raw: &str) -> Result<Vec<&str>, String> {
    if raw.trim().is_empty() {
        return Err("the path is empty".into());
    }
    if raw.chars().count() > MAX_PATH_CHARS {
        return Err(format!(
            "the path is longer than {MAX_PATH_CHARS} characters"
        ));
    }
    if raw.contains('\0') {
        return Err("the path holds a NUL character".into());
    }
    if raw.starts_with(['/', '\\']) {
        return Err("the path must be relative to the workspace folder".into());
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err("the path must not climb out with `..`".into()),
            part if part.contains(':') => {
                return Err("the path must be relative to the workspace folder".into())
            }
            part if part.starts_with('.') => {
                return Err("hidden files and folders are not readable by kits".into())
            }
            part if part.ends_with(['.', ' ']) => {
                return Err("a path part must not end with a dot or a space".into())
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err("the path must name a file inside the workspace folder".into());
    }
    Ok(parts)
}

/// The file `raw` names inside the workspace folder `root`, canonical.
pub(crate) fn resolve(root: &str, raw: &str) -> Result<PathBuf, String> {
    let parts = relative_parts(raw)?;
    let root = dunce::canonicalize(root)
        .map_err(|_| "the workspace folder is not there any more".to_string())?;
    let joined = parts
        .iter()
        .fold(root.clone(), |path, part| path.join(part));
    let real = dunce::canonicalize(&joined)
        .map_err(|_| format!("{raw} was not found in the workspace folder"))?;
    if !real.starts_with(&root) {
        return Err(format!("{raw} leads outside the workspace folder"));
    }
    // A hidden folder reached through a link counts as hidden all the same.
    let hidden = real
        .strip_prefix(&root)
        .map(|inside| {
            inside
                .components()
                .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
        })
        .unwrap_or(true);
    if hidden {
        return Err("hidden files and folders are not readable by kits".into());
    }
    if !real.is_file() {
        return Err(format!("{raw} is not a file"));
    }
    Ok(real)
}

/// The text of a file inside the workspace folder.
pub(crate) fn read_text(root: &str, raw: &str) -> Result<String, String> {
    let path = resolve(root, raw)?;
    let too_big = || format!("{raw} is larger than 1 MB");
    let len = std::fs::metadata(&path)
        .map_err(|err| format!("{raw} cannot be read: {err}"))?
        .len();
    if len > MAX_READ_BYTES {
        return Err(too_big());
    }
    let mut bytes = Vec::with_capacity(len as usize);
    std::fs::File::open(&path)
        .and_then(|file| file.take(MAX_READ_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|err| format!("{raw} cannot be read: {err}"))?;
    // Grown since it was measured.
    if bytes.len() as u64 > MAX_READ_BYTES {
        return Err(too_big());
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("{raw} is not a UTF-8 text file"))?;
    if text.contains('\0') {
        return Err(format!("{raw} is not a text file"));
    }
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `files.read`: the text of `path`, relative to the workspace folder `root`.
#[tauri::command]
pub async fn kit_file_read(root: String, path: String) -> Result<String, String> {
    crate::git::off_thread(move || read_text(&root, &path)).await
}

/// `preview.show`: the canonical path of the file `path` names inside the
/// workspace folder `root`, for the preview to open.
#[tauri::command]
pub async fn kit_workspace_path(root: String, path: String) -> Result<String, String> {
    crate::git::off_thread(move || resolve(&root, &path).map(|real| display(&real))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{link, temp_dir, write};

    fn workspace(tag: &str) -> PathBuf {
        let root = dunce::canonicalize(temp_dir(tag)).unwrap();
        write(&root, "README.md", "# Hello\nworld\n".as_bytes());
        write(&root, "src/main.luau", b"print('hi')");
        root
    }

    fn root_of(root: &Path) -> String {
        display(root)
    }

    #[test]
    fn a_text_file_inside_the_workspace_is_read() {
        let root = workspace("read");
        assert_eq!(
            read_text(&root_of(&root), "README.md").unwrap(),
            "# Hello\nworld\n"
        );
        for path in [
            "src/main.luau",
            "src\\main.luau",
            "./src/./main.luau",
            "src//main.luau",
        ] {
            assert_eq!(
                read_text(&root_of(&root), path).unwrap(),
                "print('hi')",
                "{path}"
            );
        }
        write(&root, "bom.txt", "\u{feff}text".as_bytes());
        assert_eq!(read_text(&root_of(&root), "bom.txt").unwrap(), "text");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paths_that_leave_the_workspace_are_refused() {
        let root = workspace("escape");
        let outside = temp_dir("escape-outside");
        write(&outside, "secret.txt", b"secret");
        let sibling = root.parent().unwrap().join("secret-sibling.txt");
        std::fs::write(&sibling, b"secret").unwrap();
        let absolute = display(&outside.join("secret.txt"));
        for path in [
            "",
            "  ",
            "../secret-sibling.txt",
            "src/../../secret-sibling.txt",
            "src\\..\\..\\secret-sibling.txt",
            "/etc/passwd",
            "\\\\server\\share\\x.txt",
            "C:/Windows/win.ini",
            "C:\\Windows\\win.ini",
            absolute.as_str(),
            "README.md:stream",
            "README.md\0",
            ".",
            "src/..",
            "README.md.",
        ] {
            assert!(read_text(&root_of(&root), path).is_err(), "{path:?}");
            assert!(resolve(&root_of(&root), path).is_err(), "{path:?}");
        }
        let long = "a/".repeat(600) + "x.txt";
        assert!(read_text(&root_of(&root), &long)
            .unwrap_err()
            .contains("longer"));
        if link(&outside.join("secret.txt"), &root.join("leak.txt")) {
            let err = read_text(&root_of(&root), "leak.txt").unwrap_err();
            assert!(err.contains("outside"), "{err}");
        }
        if link(&outside, &root.join("away")) {
            assert!(read_text(&root_of(&root), "away/secret.txt").is_err());
        }
        for dir in [&root, &outside] {
            let _ = std::fs::remove_dir_all(dir);
        }
        let _ = std::fs::remove_file(sibling);
    }

    #[test]
    fn hidden_files_folders_and_missing_files_are_refused() {
        let root = workspace("hidden");
        write(&root, ".env", b"TOKEN=secret");
        write(&root, ".git/config", b"[remote]");
        write(&root, "src/.secrets/key.txt", b"key");
        for path in [
            ".env",
            ".git/config",
            "src/.secrets/key.txt",
            "missing.txt",
            "src",
        ] {
            assert!(read_text(&root_of(&root), path).is_err(), "{path}");
        }
        let err = read_text(&root_of(&root), ".env").unwrap_err();
        assert!(err.contains("hidden"), "{err}");
        // Reached through a link that is not hidden itself.
        if link(&root.join(".git"), &root.join("gitdir")) {
            assert!(read_text(&root_of(&root), "gitdir/config").is_err());
        }
        assert!(read_text(&root_of(&root.join("gone")), "README.md").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_text_up_to_a_megabyte_is_read() {
        let root = workspace("text");
        write(&root, "image.png", b"\x89PNG\r\n\x1a\n\xff\xfe");
        write(&root, "zero.txt", b"text\0more");
        let at_cap = vec![b'a'; MAX_READ_BYTES as usize];
        write(&root, "cap.txt", &at_cap);
        let over = vec![b'a'; MAX_READ_BYTES as usize + 1];
        write(&root, "over.txt", &over);
        assert!(read_text(&root_of(&root), "image.png")
            .unwrap_err()
            .contains("UTF-8"));
        assert!(read_text(&root_of(&root), "zero.txt")
            .unwrap_err()
            .contains("not a text file"));
        assert_eq!(
            read_text(&root_of(&root), "cap.txt").unwrap().len(),
            MAX_READ_BYTES as usize
        );
        assert!(read_text(&root_of(&root), "over.txt")
            .unwrap_err()
            .contains("1 MB"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_preview_gets_the_canonical_path() {
        let root = workspace("preview");
        let real = resolve(&root_of(&root), "src/./main.luau").unwrap();
        assert_eq!(real, root.join("src").join("main.luau"));
        assert!(resolve(&root_of(&root), "src").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
