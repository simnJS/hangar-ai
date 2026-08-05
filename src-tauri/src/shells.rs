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

#[cfg(not(windows))]
fn discover() -> Vec<ShellInfo> {
    let candidates: [(&str, &str, &str); 5] = [
        ("bash", "Bash", "/bin/bash"),
        ("zsh", "Zsh", "/bin/zsh"),
        ("fish", "Fish", "/usr/bin/fish"),
        ("nu", "Nushell", "/usr/bin/nu"),
        ("sh", "sh", "/bin/sh"),
    ];

    let mut found: Vec<ShellInfo> = candidates
        .iter()
        .filter_map(|(id, label, path)| {
            let program = first_existing(&[PathBuf::from(path)]).or_else(|| on_path(id))?;
            Some(ShellInfo {
                id: id.to_string(),
                label: label.to_string(),
                program: program.to_string_lossy().into_owned(),
                args: vec![],
            })
        })
        .collect();

    // Whatever the user actually logs in with wins the default slot.
    if let Ok(login) = std::env::var("SHELL") {
        if Path::new(&login).is_file() && !found.iter().any(|s| s.program == login) {
            found.insert(
                0,
                ShellInfo {
                    id: "login".into(),
                    label: format!("{login} (login shell)"),
                    program: login,
                    args: vec![],
                },
            );
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
