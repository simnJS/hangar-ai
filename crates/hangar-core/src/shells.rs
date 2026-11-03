use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A launchable shell. `program` is an absolute path whenever we probed a known
/// install location, so panes do not depend on PATH ordering at spawn time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellInfo {
    pub id: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

fn on_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

/// Returns the first candidate that exists, so we can list well-known install
/// locations ahead of a bare PATH lookup.
fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

#[cfg(windows)]
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

#[cfg(windows)]
fn discover() -> Vec<ShellInfo> {
    let mut found: Vec<ShellInfo> = Vec::new();

    let system32 = env_path("SystemRoot")
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32");
    let program_files =
        env_path("ProgramFiles").unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let program_files_x86 =
        env_path("ProgramFiles(x86)").unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let local_app_data = env_path("LOCALAPPDATA").unwrap_or_default();

    let mut push = |id: &str, label: &str, program: Option<PathBuf>, args: &[&str]| {
        if let Some(program) = program {
            found.push(ShellInfo {
                id: id.to_string(),
                label: label.to_string(),
                program: program.to_string_lossy().into_owned(),
                args: args.iter().map(|a| a.to_string()).collect(),
            });
        }
    };

    push(
        "pwsh",
        "PowerShell 7",
        first_existing(&[
            program_files.join(r"PowerShell\7\pwsh.exe"),
            local_app_data.join(r"Microsoft\WindowsApps\pwsh.exe"),
        ])
        .or_else(|| on_path("pwsh.exe")),
        &[],
    );

    push(
        "powershell",
        "Windows PowerShell",
        first_existing(&[system32.join(r"WindowsPowerShell\v1.0\powershell.exe")])
            .or_else(|| on_path("powershell.exe")),
        &[],
    );

    push(
        "cmd",
        "Command Prompt",
        first_existing(&[system32.join("cmd.exe")]).or_else(|| on_path("cmd.exe")),
        &[],
    );

    // Git Bash needs a login shell, otherwise the MSYS environment is incomplete.
    push(
        "git-bash",
        "Git Bash",
        first_existing(&[
            program_files.join(r"Git\bin\bash.exe"),
            program_files_x86.join(r"Git\bin\bash.exe"),
            local_app_data.join(r"Programs\Git\bin\bash.exe"),
        ]),
        &["--login", "-i"],
    );

    push(
        "msys2",
        "MSYS2 Bash",
        first_existing(&[
            PathBuf::from(r"C:\msys64\usr\bin\bash.exe"),
            PathBuf::from(r"C:\msys32\usr\bin\bash.exe"),
        ]),
        &["--login", "-i"],
    );

    push(
        "cygwin",
        "Cygwin Bash",
        first_existing(&[
            PathBuf::from(r"C:\cygwin64\bin\bash.exe"),
            PathBuf::from(r"C:\cygwin\bin\bash.exe"),
        ]),
        &["--login", "-i"],
    );

    push(
        "wsl",
        "WSL",
        first_existing(&[system32.join("wsl.exe")]).or_else(|| on_path("wsl.exe")),
        &[],
    );

    push("nu", "Nushell", on_path("nu.exe"), &[]);

    found
}

/// Arguments a POSIX shell is opened with in a pane.
///
/// macOS terminals all open login shells, and here it is closer to required
/// than conventional: Homebrew's `shellenv` is conventionally written to
/// `~/.zprofile`, which only a login shell reads. A pane started without `-l`
/// would come up on a machine full of tools with none of them on PATH.
///
/// Linux desktops go the other way — their terminals open non-login shells, and
/// a login shell there re-runs `~/.profile` in a session that already has it —
/// so this stays a macOS decision.
#[cfg(all(unix, target_os = "macos"))]
const POSIX_ARGS: &[&str] = &["-l"];
#[cfg(all(unix, not(target_os = "macos")))]
const POSIX_ARGS: &[&str] = &[];

#[cfg(not(windows))]
fn discover() -> Vec<ShellInfo> {
    crate::path_env::ensure();

    // Each shell is looked for in every place it is normally installed, before
    // falling back to PATH: macOS keeps its own zsh and bash in /bin, while
    // Homebrew installs to /opt/homebrew on Apple Silicon and /usr/local on
    // Intel. A pane then holds an absolute path rather than a name to resolve.
    let candidates: [(&str, &str, &[&str], &[&str]); 6] = [
        (
            "zsh",
            "Zsh",
            &["/bin/zsh", "/opt/homebrew/bin/zsh"],
            POSIX_ARGS,
        ),
        (
            "bash",
            "Bash",
            // A newer Homebrew bash comes first: the /bin/bash macOS ships is
            // still 3.2, and scripts written this decade tend to notice.
            &["/opt/homebrew/bin/bash", "/usr/local/bin/bash", "/bin/bash"],
            POSIX_ARGS,
        ),
        (
            "fish",
            "Fish",
            &[
                "/opt/homebrew/bin/fish",
                "/usr/local/bin/fish",
                "/usr/bin/fish",
            ],
            POSIX_ARGS,
        ),
        (
            "nu",
            "Nushell",
            &["/opt/homebrew/bin/nu", "/usr/local/bin/nu", "/usr/bin/nu"],
            // Nushell spells it in full and rejects the short form.
            if cfg!(target_os = "macos") {
                &["--login"]
            } else {
                &[]
            },
        ),
        (
            "pwsh",
            "PowerShell",
            &[
                "/opt/homebrew/bin/pwsh",
                "/usr/local/bin/pwsh",
                "/usr/bin/pwsh",
            ],
            &[],
        ),
        ("sh", "sh", &["/bin/sh"], &[]),
    ];

    let mut found: Vec<ShellInfo> = candidates
        .iter()
        .filter_map(|(id, label, paths, args)| {
            let owned: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            let program = first_existing(&owned).or_else(|| on_path(id))?;
            Some(ShellInfo {
                id: id.to_string(),
                label: label.to_string(),
                program: program.to_string_lossy().into_owned(),
                args: args.iter().map(|a| a.to_string()).collect(),
            })
        })
        .collect();

    // Whatever the user actually logs in with wins the default slot. When that
    // is one of the shells above it is moved rather than added, so the list
    // does not carry the same binary twice under two names.
    if let Ok(login) = std::env::var("SHELL") {
        if Path::new(&login).is_file() {
            match found.iter().position(|s| s.program == login) {
                Some(index) => {
                    let shell = found.remove(index);
                    found.insert(0, shell);
                }
                None => found.insert(
                    0,
                    ShellInfo {
                        id: "login".into(),
                        label: format!("{login} (login shell)"),
                        program: login,
                        args: POSIX_ARGS.iter().map(|a| a.to_string()).collect(),
                    },
                ),
            }
        }
    }

    found
}

#[tauri::command]
pub fn detect_shells() -> Vec<ShellInfo> {
    discover()
}

/// Fallback when a pane has no shell recorded, or its recorded shell vanished.
pub fn default_shell() -> Option<ShellInfo> {
    discover().into_iter().next()
}

/// Guards against a stale absolute path from an uninstalled shell.
pub fn program_exists(program: &str) -> bool {
    Path::new(program).is_file() || on_path(program).is_some()
}
