//! What a file dropped on a pane types: every path, translated for where the
//! pane's program runs and quoted for whoever reads it, with a trailing space
//! and no Enter (`dropText` and its helpers in `src/lib/fileDrop.ts`).
//!
//! The pure half of fileDrop.ts is P4's (`panes_logic::file_drop`, with the
//! suite); it is here too because the drop lands in this package's terminals
//! and both were written at once. The cases below are those of
//! fileDrop.test.ts that the drop depends on. Use P4's once it lands.

use crate::state::types::{AgentId, ShellInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    Native,
    Wsl,
    Msys,
    Cygwin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    /// An agent reads the paste: double quotes, `\"` inside.
    Agent,
    Posix,
    /// PowerShell and Nushell: single quotes, a quote doubled inside.
    Single,
    Cmd,
}

fn program_name(shell: Option<&ShellInfo>) -> String {
    shell
        .and_then(|shell| shell.program.rsplit(['\\', '/']).next())
        .unwrap_or_default()
        .to_lowercase()
}

pub fn path_style_of(shell: Option<&ShellInfo>) -> PathStyle {
    let id = shell.map_or("", |shell| shell.id.as_str());
    if id == "wsl" || program_name(shell) == "wsl.exe" {
        PathStyle::Wsl
    } else if id == "git-bash" || id == "msys2" {
        PathStyle::Msys
    } else if id == "cygwin" {
        PathStyle::Cygwin
    } else {
        PathStyle::Native
    }
}

/// An agent reads the paste, not the shell it was started from.
pub fn quote_style_of(agent: AgentId, shell: Option<&ShellInfo>) -> QuoteStyle {
    if agent != AgentId::Shell {
        return QuoteStyle::Agent;
    }
    let id = shell.map_or("", |shell| shell.id.as_str());
    let program = program_name(shell);
    let bare = program.strip_suffix(".exe").unwrap_or(&program);
    if id == "cmd" || program == "cmd.exe" {
        QuoteStyle::Cmd
    } else if matches!(id, "pwsh" | "powershell" | "nu")
        || matches!(bare, "pwsh" | "powershell" | "nu")
    {
        QuoteStyle::Single
    } else {
        QuoteStyle::Posix
    }
}

/// A Windows path as the pane's program writes it. Anything that is not a
/// drive or a WSL path goes through untouched.
pub fn translate_path(path: &str, style: PathStyle) -> String {
    if style == PathStyle::Native {
        return path.to_owned();
    }
    let lower = path.to_lowercase();
    for prefix in ["\\\\wsl.localhost\\", "\\\\wsl$\\"] {
        if lower.starts_with(prefix) {
            let rest = &path[prefix.len()..];
            // `\\wsl$\<distro>\<path>`: the distro, then the path inside it.
            let name_end = rest.find('\\').unwrap_or(rest.len());
            if name_end == 0 || style != PathStyle::Wsl {
                return path.to_owned();
            }
            let inner = &rest[name_end..];
            return if inner.is_empty() {
                "/".into()
            } else {
                inner.replace('\\', "/")
            };
        }
    }
    let bytes = path.as_bytes();
    let is_drive = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/');
    if !is_drive {
        return path.to_owned();
    }
    let letter = (bytes[0] as char).to_ascii_lowercase();
    let rest = if bytes.len() > 3 {
        path[3..].replace('\\', "/")
    } else {
        String::new()
    };
    let root = match style {
        PathStyle::Wsl => format!("/mnt/{letter}"),
        PathStyle::Cygwin => format!("/cygdrive/{letter}"),
        _ => format!("/{letter}"),
    };
    if rest.is_empty() {
        root
    } else {
        format!("{root}/{rest}")
    }
}

fn bare(path: &str, style: QuoteStyle) -> bool {
    let word = |extra: &str| {
        !path.is_empty()
            && path
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || extra.contains(c))
    };
    match style {
        QuoteStyle::Agent => !path.is_empty() && !path.chars().any(char::is_whitespace),
        QuoteStyle::Posix => word("-./:@%+=,"),
        QuoteStyle::Single => word("-./:\\%+=~"),
        QuoteStyle::Cmd => word("-./:\\@+=,~#"),
    }
}

pub fn quote_path(path: &str, style: QuoteStyle) -> String {
    if bare(path, style) {
        return path.to_owned();
    }
    match style {
        QuoteStyle::Agent => format!("\"{}\"", path.replace('"', "\\\"")),
        QuoteStyle::Posix => format!("'{}'", path.replace('\'', "'\\''")),
        QuoteStyle::Single => format!("'{}'", path.replace('\'', "''")),
        // A Windows file name cannot hold a double quote.
        QuoteStyle::Cmd => format!("\"{path}\""),
    }
}

/// What a drop types into a pane (`dropText`).
pub fn drop_text(paths: &[String], agent: AgentId, shell: Option<&ShellInfo>) -> String {
    let path_style = path_style_of(shell);
    let quote_style = quote_style_of(agent, shell);
    let quoted: Vec<String> = paths
        .iter()
        .map(|path| quote_path(&translate_path(path, path_style), quote_style))
        .collect();
    format!("{} ", quoted.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(id: &str, program: &str) -> ShellInfo {
        ShellInfo {
            id: id.into(),
            label: id.into(),
            program: program.into(),
            args: vec![],
        }
    }

    #[test]
    fn paths_are_joined_with_a_trailing_space_and_no_enter() {
        let paths = vec!["/tmp/a.png".to_string(), "/tmp/b c.png".to_string()];
        assert_eq!(
            drop_text(&paths, AgentId::Shell, Some(&shell("bash", "/bin/bash"))),
            "/tmp/a.png '/tmp/b c.png' "
        );
        assert_eq!(
            drop_text(&paths, AgentId::Claude, Some(&shell("bash", "/bin/bash"))),
            "/tmp/a.png \"/tmp/b c.png\" "
        );
    }

    #[test]
    fn each_reader_gets_its_own_quotes() {
        assert_eq!(quote_path("it's", QuoteStyle::Posix), "'it'\\''s'");
        assert_eq!(quote_path("it's", QuoteStyle::Single), "'it''s'");
        assert_eq!(quote_path("a b", QuoteStyle::Cmd), "\"a b\"");
        assert_eq!(
            quote_path("say \"hi\"", QuoteStyle::Agent),
            "\"say \\\"hi\\\"\""
        );
        assert_eq!(quote_path("C:\\x\\y.txt", QuoteStyle::Cmd), "C:\\x\\y.txt");
    }

    #[test]
    fn windows_paths_are_translated_for_the_shell_that_reads_them() {
        assert_eq!(translate_path("C:\\x\\y", PathStyle::Wsl), "/mnt/c/x/y");
        assert_eq!(translate_path("C:\\x\\y", PathStyle::Msys), "/c/x/y");
        assert_eq!(
            translate_path("C:\\x\\y", PathStyle::Cygwin),
            "/cygdrive/c/x/y"
        );
        assert_eq!(translate_path("C:\\x\\y", PathStyle::Native), "C:\\x\\y");
        assert_eq!(
            translate_path("\\\\wsl$\\Ubuntu\\home\\me", PathStyle::Wsl),
            "/home/me"
        );
        assert_eq!(
            translate_path("\\\\wsl.localhost\\Ubuntu", PathStyle::Wsl),
            "/"
        );
        assert_eq!(
            translate_path("\\\\wsl$\\Ubuntu\\home", PathStyle::Msys),
            "\\\\wsl$\\Ubuntu\\home"
        );
        assert_eq!(translate_path("/home/me", PathStyle::Wsl), "/home/me");
    }

    #[test]
    fn the_shell_decides_the_styles() {
        assert_eq!(
            path_style_of(Some(&shell("x", "C:\\Windows\\wsl.exe"))),
            PathStyle::Wsl
        );
        assert_eq!(
            path_style_of(Some(&shell("git-bash", "bash.exe"))),
            PathStyle::Msys
        );
        assert_eq!(path_style_of(None), PathStyle::Native);
        assert_eq!(
            quote_style_of(AgentId::Shell, Some(&shell("x", "pwsh.exe"))),
            QuoteStyle::Single
        );
        assert_eq!(
            quote_style_of(AgentId::Shell, Some(&shell("cmd", "cmd.exe"))),
            QuoteStyle::Cmd
        );
        assert_eq!(
            quote_style_of(AgentId::Codex, Some(&shell("cmd", "cmd.exe"))),
            QuoteStyle::Agent
        );
        assert_eq!(quote_style_of(AgentId::Shell, None), QuoteStyle::Posix);
    }
}
