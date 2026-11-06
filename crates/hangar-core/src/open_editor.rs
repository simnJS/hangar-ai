//! « Ouvrir avec VS Code »: starts VS Code on a workspace's folder, or on its
//! `.code-workspace` when it came from one.
//!
//! Nothing goes through a shell. The path is one argument of its own, made
//! absolute so it can never read as an option (a folder called `-n` would
//! otherwise). On Windows `code.cmd` is a batch file, which only a shell can
//! run, so the `Code.exe` beside its `bin` folder is started instead.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// What came of it. Expected outcomes are values: the call itself never fails.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum EditorOutcome {
    Opened,
    /// The workspace's folder is gone.
    MissingFolder,
    /// No VS Code on this machine.
    NotFound,
    Failed {
        error: String,
    },
}

/// Names `code` goes by on this platform, in order of preference.
fn program_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["code.cmd", "code.exe"]
    } else {
        &["code"]
    }
}

/// The first file called one of `names` in the directories of `path_var`, then
/// in `extra`. A directory that does not exist is skipped.
fn find_program(names: &[&str], path_var: &OsStr, extra: &[PathBuf]) -> Option<PathBuf> {
    let dirs = std::env::split_paths(path_var).chain(extra.iter().cloned());
    for dir in dirs {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Where VS Code installs itself when it is not on the PATH.
fn install_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if let Some(local) = var("LOCALAPPDATA") {
            dirs.push(local.join("Programs").join("Microsoft VS Code").join("bin"));
        }
        for name in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = var(name) {
                dirs.push(base.join("Microsoft VS Code").join("bin"));
            }
        }
    } else if cfg!(target_os = "macos") {
        const BIN: &str = "Visual Studio Code.app/Contents/Resources/app/bin";
        dirs.push(PathBuf::from("/Applications").join(BIN));
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Applications").join(BIN));
        }
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
    } else {
        dirs.push(PathBuf::from("/usr/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/snap/bin"));
        dirs.push(PathBuf::from("/var/lib/flatpak/exports/bin"));
    }
    dirs
}

/// The program to start. A Windows `code.cmd` is swapped for the `Code.exe`
/// one folder up, which needs no shell.
fn launcher(found: PathBuf) -> PathBuf {
    let is_cmd = found
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"));
    if !is_cmd {
        return found;
    }
    found
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("Code.exe"))
        .filter(|exe| exe.is_file())
        .unwrap_or(found)
}

/// The arguments VS Code is started with: the target, absolute so that it
/// cannot be taken for an option. `None` for a path that still would be.
fn build_args(target: &Path) -> Option<Vec<OsString>> {
    let absolute = std::path::absolute(target).ok()?;
    if absolute.to_string_lossy().starts_with('-') {
        return None;
    }
    Some(vec![absolute.into_os_string()])
}

/// What to open: the workspace file sitting in the folder when the workspace
/// came from one, else the folder.
fn target_for(cwd: &str, from_code_workspace: bool) -> PathBuf {
    if from_code_workspace {
        if let Some(file) = crate::workspace_file::find_code_workspace(cwd.to_string()) {
            return PathBuf::from(file);
        }
    }
    PathBuf::from(cwd)
}

fn spawn(program: &Path, args: &[OsString]) -> std::io::Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: closing Hangar must not take the editor along.
        command.process_group(0);
    }
    command.spawn().map(drop)
}

fn open(cwd: &str, from_code_workspace: bool) -> EditorOutcome {
    if !Path::new(cwd).is_dir() {
        return EditorOutcome::MissingFolder;
    }
    crate::path_env::ensure();
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let Some(found) = find_program(program_names(), &path_var, &install_dirs()) else {
        return EditorOutcome::NotFound;
    };
    let Some(args) = build_args(&target_for(cwd, from_code_workspace)) else {
        return EditorOutcome::Failed {
            error: "unsafe path".into(),
        };
    };
    match spawn(&launcher(found), &args) {
        Ok(()) => EditorOutcome::Opened,
        Err(err) => EditorOutcome::Failed {
            error: err.to_string(),
        },
    }
}

pub async fn open_in_vscode(cwd: String, from_code_workspace: bool) -> EditorOutcome {
    tokio::task::spawn_blocking(move || open(&cwd, from_code_workspace))
        .await
        .unwrap_or_else(|err| EditorOutcome::Failed {
            error: err.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        crate::testdir::scratch(&format!("open-editor-{name}"))
    }

    #[test]
    fn finds_the_program_on_a_fake_path() {
        let first = scratch("first");
        let second = scratch("second");
        fs::write(second.join("code"), "").unwrap();
        let path = std::env::join_paths([first, second.clone()]).unwrap();
        assert_eq!(
            find_program(&["code"], &path, &[]),
            Some(second.join("code"))
        );
    }

    #[test]
    fn path_wins_over_install_dirs_and_missing_is_none() {
        let on_path = scratch("onpath");
        let extra = scratch("extra");
        fs::write(extra.join("code"), "").unwrap();
        let path = std::env::join_paths([on_path.clone()]).unwrap();
        assert_eq!(
            find_program(&["code"], &path, std::slice::from_ref(&extra)),
            Some(extra.join("code"))
        );
        fs::write(on_path.join("code"), "").unwrap();
        assert_eq!(
            find_program(&["code"], &path, &[extra]),
            Some(on_path.join("code"))
        );
        assert_eq!(find_program(&["code"], OsStr::new(""), &[]), None);
    }

    #[test]
    fn a_directory_called_code_is_not_the_program() {
        let dir = scratch("dircode");
        fs::create_dir(dir.join("code")).unwrap();
        let path = std::env::join_paths([dir]).unwrap();
        assert_eq!(find_program(&["code"], &path, &[]), None);
    }

    #[test]
    fn cmd_becomes_the_exe_one_folder_up() {
        let root = scratch("install");
        fs::create_dir(root.join("bin")).unwrap();
        fs::write(root.join("bin").join("code.cmd"), "").unwrap();
        fs::write(root.join("Code.exe"), "").unwrap();
        assert_eq!(
            launcher(root.join("bin").join("code.cmd")),
            root.join("Code.exe")
        );
        // Without the exe, the cmd is all there is.
        fs::remove_file(root.join("Code.exe")).unwrap();
        assert_eq!(
            launcher(root.join("bin").join("code.cmd")),
            root.join("bin").join("code.cmd")
        );
        assert_eq!(
            launcher(PathBuf::from("/usr/bin/code")),
            PathBuf::from("/usr/bin/code")
        );
    }

    #[test]
    fn a_folder_with_spaces_is_one_argument() {
        let dir = scratch("with spaces & (parens)");
        let args = build_args(&dir).unwrap();
        assert_eq!(args.len(), 1);
        assert_eq!(PathBuf::from(&args[0]), dir);
    }

    #[test]
    fn a_folder_starting_with_a_dash_cannot_read_as_an_option() {
        let args = build_args(Path::new("-new-window")).unwrap();
        assert_eq!(args.len(), 1);
        let shown = args[0].to_string_lossy().into_owned();
        assert!(!shown.starts_with('-'), "{shown}");
        assert!(Path::new(&args[0]).is_absolute());
    }

    #[test]
    fn a_missing_folder_is_reported_before_anything_is_looked_for() {
        let gone = scratch("gone");
        fs::remove_dir_all(&gone).unwrap();
        assert_eq!(
            open(&gone.to_string_lossy(), false),
            EditorOutcome::MissingFolder
        );
    }

    #[test]
    fn a_workspace_that_came_from_a_code_workspace_opens_the_file() {
        let dir = scratch("multi");
        fs::write(dir.join("proj.code-workspace"), "{}").unwrap();
        let cwd = dir.to_string_lossy().into_owned();
        let target = target_for(&cwd, true);
        assert_eq!(
            target.file_name().unwrap(),
            OsStr::new("proj.code-workspace")
        );
        assert_eq!(target_for(&cwd, false), dir);
        // Nothing to find: the folder.
        let bare = scratch("bare");
        assert_eq!(target_for(&bare.to_string_lossy(), true), bare);
    }
}
