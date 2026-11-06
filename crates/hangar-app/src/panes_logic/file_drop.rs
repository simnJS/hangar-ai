//! Files dropped on a pane, typed into it as paths (`lib/fileDrop.ts`).
//!
//! The pure half: how the program in the pane spells a path the host handed
//! over (`translate_path`), how a path that needs it gets quoted for whoever
//! reads it (`quote_path`), and what a drop types (`drop_text`). The events
//! are the window's: iced reports `FileHovered` and `FileDropped` once per
//! file and with no position (the web app's `tauri://drag-*` carried the
//! whole list and a point), so `DropTracker` counts the files and follows the
//! cursor to put a drop back together.

#[cfg(test)]
use iced::Point;

use crate::state::types::{AgentId, ShellInfo};

/// How the program in the pane spells a path the host handed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    Native,
    Wsl,
    Msys,
    Cygwin,
}

/// How a path that needs it gets quoted for whoever reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    /// An agent's prompt: double quotes, as Windows Terminal pastes a drop.
    Agent,
    /// sh, bash, zsh, fish: single quotes, nothing inside them is special.
    Posix,
    /// PowerShell and Nushell: single quotes, a quote doubled inside.
    Single,
    Cmd,
}

fn program_name(shell: Option<&ShellInfo>) -> String {
    shell
        .and_then(|shell| shell.program.rsplit(['\\', '/']).next())
        .unwrap_or("")
        .to_lowercase()
}

pub fn path_style_of(shell: Option<&ShellInfo>) -> PathStyle {
    let id = shell.map_or("", |shell| shell.id.as_str());
    if id == "wsl" || program_name(shell) == "wsl.exe" {
        return PathStyle::Wsl;
    }
    if id == "git-bash" || id == "msys2" {
        return PathStyle::Msys;
    }
    if id == "cygwin" {
        return PathStyle::Cygwin;
    }
    PathStyle::Native
}

/// An agent reads the paste, not the shell it was started from, but the path
/// itself still has to be one the agent's side of WSL can open.
pub fn quote_style_of(agent: AgentId, shell: Option<&ShellInfo>) -> QuoteStyle {
    if agent != AgentId::Shell {
        return QuoteStyle::Agent;
    }
    let id = shell.map_or("", |shell| shell.id.as_str());
    let program = program_name(shell);
    if id == "cmd" || program == "cmd.exe" {
        return QuoteStyle::Cmd;
    }
    let bare = program.strip_suffix(".exe").unwrap_or(&program);
    if matches!(id, "pwsh" | "powershell" | "nu") || matches!(bare, "pwsh" | "powershell" | "nu") {
        return QuoteStyle::Single;
    }
    QuoteStyle::Posix
}

/// What JavaScript's `.` does not match: a path with a line break in it is
/// not one the patterns below were written for, and passes through.
fn has_line_break(text: &str) -> bool {
    text.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

/// `\\wsl$\Distro\rest` or `\\wsl.localhost\Distro\rest`, any case: what
/// Explorer shows for a file of the WSL file system. Answers `\rest` (just `\`
/// for the distro's root).
fn wsl_share(path: &str) -> Option<&str> {
    let lower = path.to_ascii_lowercase();
    let after = lower.strip_prefix("\\\\wsl")?;
    let host = if after.starts_with(".localhost") {
        ".localhost".len()
    } else if after.starts_with('$') {
        1
    } else {
        return None;
    };
    let after = &path["\\\\wsl".len() + host..];
    let after = after.strip_prefix('\\')?;
    // One or more characters of the distribution's name.
    let distro_end = after.find('\\').unwrap_or(after.len());
    let rest = &after[distro_end..];
    if distro_end == 0 || has_line_break(rest) {
        return None;
    }
    Some(if rest.is_empty() { "\\" } else { rest })
}

/// `C:`, `C:\`, `C:\rest` or `C:/rest`: the drive's letter and what follows.
fn drive_path(path: &str) -> Option<(char, &str)> {
    let mut chars = path.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let after = chars.as_str().strip_prefix(':')?;
    if after.is_empty() {
        return Some((letter, ""));
    }
    let rest = after.strip_prefix(['\\', '/'])?;
    if has_line_break(rest) {
        return None;
    }
    Some((letter, rest))
}

/// A Windows path as the pane's program would write it. Anything that is not
/// a drive or WSL path, a POSIX host's or a network share, goes through
/// untouched.
pub fn translate_path(path: &str, style: PathStyle) -> String {
    if style == PathStyle::Native {
        return path.to_string();
    }

    // A file dragged out of the WSL file system, as Explorer shows it.
    if let Some(rest) = wsl_share(path) {
        return if style == PathStyle::Wsl {
            rest.replace('\\', "/")
        } else {
            path.to_string()
        };
    }

    let Some((letter, rest)) = drive_path(path) else {
        return path.to_string();
    };
    let letter = letter.to_ascii_lowercase();
    let rest = rest.replace('\\', "/");
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

/// JavaScript's `\w`: ASCII letters, digits and the underscore.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// JavaScript's `\s`, which counts the byte order mark and Rust's does not.
fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// Whether a reader takes the whole path literally outside quotes: not empty,
/// and nothing in it but the characters of its bare words.
fn is_bare(path: &str, style: QuoteStyle) -> bool {
    if path.is_empty() {
        return false;
    }
    let allowed: fn(char) -> bool = match style {
        QuoteStyle::Agent => |c| !is_space(c),
        QuoteStyle::Posix => |c| is_word(c) || "-./:@%+=,".contains(c),
        QuoteStyle::Single => |c| is_word(c) || "-./:\\%+=~".contains(c),
        QuoteStyle::Cmd => |c| is_word(c) || "-./:\\@+=,~#".contains(c),
    };
    path.chars().all(allowed)
}

pub fn quote_path(path: &str, style: QuoteStyle) -> String {
    if is_bare(path, style) {
        return path.to_string();
    }
    match style {
        QuoteStyle::Agent => format!("\"{}\"", path.replace('"', "\\\"")),
        QuoteStyle::Posix => format!("'{}'", path.replace('\'', "'\\''")),
        QuoteStyle::Single => format!("'{}'", path.replace('\'', "''")),
        // A Windows file name cannot hold a double quote.
        QuoteStyle::Cmd => format!("\"{path}\""),
    }
}

/// What a drop types into a pane: every path, translated and quoted, with a
/// trailing space so the next word can follow, and no Enter.
pub fn drop_text(paths: &[String], agent: AgentId, shell: Option<&ShellInfo>) -> String {
    let path_style = path_style_of(shell);
    let quote_style = quote_style_of(agent, shell);
    let words: Vec<String> = paths
        .iter()
        .map(|path| quote_path(&translate_path(path, path_style), quote_style))
        .collect();
    format!("{} ", words.join(" "))
}

/// Puts a drop back together from the window's per-file events.
///
/// iced reports each hovered file (`FileHovered`), each dropped one
/// (`FileDropped`) and the end of a hover (`FilesHoveredLeft`), and the
/// cursor separately. The tracker counts the hovered files for the overlay
/// ("drop 3 files to paste them"), remembers where the cursor last was, and
/// holds the dropped paths until the events of that drop have all arrived
/// (`take_drop`).
#[cfg(test)]
#[derive(Debug, Default)]
pub struct DropTracker {
    hovered: usize,
    cursor: Option<Point>,
    dropped: Vec<String>,
}

#[cfg(test)]
impl DropTracker {
    pub fn file_hovered(&mut self) {
        self.hovered += 1;
    }

    /// The hover ended without a drop, or the drop landed.
    pub fn hover_left(&mut self) {
        self.hovered = 0;
    }

    pub fn cursor_moved(&mut self, point: Point) {
        self.cursor = Some(point);
    }

    pub fn cursor_left(&mut self) {
        self.cursor = None;
    }

    /// One file of a drop arrived. The hover is over with the first.
    pub fn file_dropped(&mut self, path: String) {
        self.hovered = 0;
        self.dropped.push(path);
    }

    /// What the overlay shows while files hover: where, and how many.
    pub fn over(&self) -> Option<(Point, usize)> {
        if self.hovered == 0 {
            return None;
        }
        Some((self.cursor?, self.hovered))
    }

    /// The paths of a drop and the point it landed on, once; call it when the
    /// events of the drop are done (the next update after the first
    /// `FileDropped`). A drop with no known cursor has nowhere to land.
    pub fn take_drop(&mut self) -> Option<(Vec<String>, Point)> {
        if self.dropped.is_empty() {
            return None;
        }
        let paths = std::mem::take(&mut self.dropped);
        Some((paths, self.cursor?))
    }

    pub fn has_pending_drop(&self) -> bool {
        !self.dropped.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(id: &str, program: &str) -> ShellInfo {
        ShellInfo {
            id: id.into(),
            label: id.into(),
            program: program.into(),
            args: Vec::new(),
        }
    }

    fn pwsh() -> ShellInfo {
        shell("pwsh", "C:\\Program Files\\PowerShell\\7\\pwsh.exe")
    }
    fn cmd() -> ShellInfo {
        shell("cmd", "C:\\Windows\\System32\\cmd.exe")
    }
    fn wsl() -> ShellInfo {
        shell("wsl", "C:\\Windows\\System32\\wsl.exe")
    }
    fn git_bash() -> ShellInfo {
        shell("git-bash", "C:\\Program Files\\Git\\bin\\bash.exe")
    }
    fn cygwin() -> ShellInfo {
        shell("cygwin", "C:\\cygwin64\\bin\\bash.exe")
    }
    fn zsh() -> ShellInfo {
        shell("zsh", "/bin/zsh")
    }

    #[test]
    fn translate_leaves_paths_alone_for_a_native_shell() {
        assert_eq!(
            translate_path("C:\\Users\\me\\shot.png", PathStyle::Native),
            "C:\\Users\\me\\shot.png"
        );
        assert_eq!(
            translate_path("/Users/me/shot.png", PathStyle::Native),
            "/Users/me/shot.png"
        );
    }

    #[test]
    fn translate_mounts_drives_under_mnt_for_wsl() {
        assert_eq!(
            translate_path("C:\\Users\\me\\My Shots\\a.png", PathStyle::Wsl),
            "/mnt/c/Users/me/My Shots/a.png"
        );
        assert_eq!(translate_path("D:\\", PathStyle::Wsl), "/mnt/d");
        assert_eq!(translate_path("E:", PathStyle::Wsl), "/mnt/e");
    }

    #[test]
    fn translate_reads_a_file_from_the_wsl_file_system_back_as_a_linux_path() {
        assert_eq!(
            translate_path("\\\\wsl.localhost\\Ubuntu\\home\\me\\a.txt", PathStyle::Wsl),
            "/home/me/a.txt"
        );
        assert_eq!(
            translate_path("\\\\wsl$\\Debian\\etc\\hosts", PathStyle::Wsl),
            "/etc/hosts"
        );
        // The root of a distribution, and any case of the host name.
        assert_eq!(translate_path("\\\\WSL$\\Debian", PathStyle::Wsl), "/");
        // For the other styles it is not theirs to translate.
        assert_eq!(
            translate_path("\\\\wsl$\\Debian\\etc\\hosts", PathStyle::Msys),
            "\\\\wsl$\\Debian\\etc\\hosts"
        );
    }

    #[test]
    fn translate_uses_the_msys_and_cygwin_drive_prefixes() {
        assert_eq!(
            translate_path("C:\\repo\\a.png", PathStyle::Msys),
            "/c/repo/a.png"
        );
        assert_eq!(
            translate_path("C:\\repo\\a.png", PathStyle::Cygwin),
            "/cygdrive/c/repo/a.png"
        );
    }

    #[test]
    fn translate_does_not_touch_what_it_cannot_map() {
        assert_eq!(
            translate_path("\\\\server\\share\\a.png", PathStyle::Wsl),
            "\\\\server\\share\\a.png"
        );
        assert_eq!(
            translate_path("/home/me/a.png", PathStyle::Wsl),
            "/home/me/a.png"
        );
        // A letter and a colon with no separator is not a drive path.
        assert_eq!(translate_path("C:foo", PathStyle::Wsl), "C:foo");
    }

    #[test]
    fn quote_only_when_it_has_to() {
        assert_eq!(
            quote_path("C:\\Users\\me\\a.png", QuoteStyle::Agent),
            "C:\\Users\\me\\a.png"
        );
        assert_eq!(
            quote_path("C:\\Users\\me\\a.png", QuoteStyle::Single),
            "C:\\Users\\me\\a.png"
        );
        assert_eq!(
            quote_path("C:\\Users\\me\\a.png", QuoteStyle::Cmd),
            "C:\\Users\\me\\a.png"
        );
        assert_eq!(
            quote_path("/mnt/c/Users/me/a.png", QuoteStyle::Posix),
            "/mnt/c/Users/me/a.png"
        );
    }

    #[test]
    fn quote_a_path_with_spaces_for_each_reader() {
        assert_eq!(
            quote_path("C:\\My Shots\\a.png", QuoteStyle::Agent),
            "\"C:\\My Shots\\a.png\""
        );
        assert_eq!(
            quote_path("C:\\My Shots\\a.png", QuoteStyle::Cmd),
            "\"C:\\My Shots\\a.png\""
        );
        assert_eq!(
            quote_path("C:\\My Shots\\a.png", QuoteStyle::Single),
            "'C:\\My Shots\\a.png'"
        );
        assert_eq!(
            quote_path("/mnt/c/My Shots/a.png", QuoteStyle::Posix),
            "'/mnt/c/My Shots/a.png'"
        );
    }

    #[test]
    fn quote_escapes_the_quote_it_wraps_with() {
        assert_eq!(
            quote_path("C:\\Bob's\\a.png", QuoteStyle::Single),
            "'C:\\Bob''s\\a.png'"
        );
        assert_eq!(
            quote_path("/home/bob's/a.png", QuoteStyle::Posix),
            "'/home/bob'\\''s/a.png'"
        );
        assert_eq!(
            quote_path("/home/a b\"c.png", QuoteStyle::Agent),
            "\"/home/a b\\\"c.png\""
        );
        // A quote in a path with no space is still one word to the agent.
        assert_eq!(
            quote_path("/home/a\"b.png", QuoteStyle::Agent),
            "/home/a\"b.png"
        );
    }

    #[test]
    fn quote_what_a_shell_would_expand() {
        assert_eq!(
            quote_path("C:\\$env\\a.png", QuoteStyle::Single),
            "'C:\\$env\\a.png'"
        );
        assert_eq!(
            quote_path("/tmp/a(1).png", QuoteStyle::Posix),
            "'/tmp/a(1).png'"
        );
        assert_eq!(
            quote_path("C:\\a&b\\x.png", QuoteStyle::Cmd),
            "\"C:\\a&b\\x.png\""
        );
        // Nothing at all is not a word either.
        assert_eq!(quote_path("", QuoteStyle::Posix), "''");
    }

    #[test]
    fn styles_come_from_the_shell() {
        assert_eq!(path_style_of(Some(&wsl())), PathStyle::Wsl);
        assert_eq!(path_style_of(Some(&git_bash())), PathStyle::Msys);
        assert_eq!(path_style_of(Some(&cygwin())), PathStyle::Cygwin);
        assert_eq!(path_style_of(Some(&pwsh())), PathStyle::Native);
        assert_eq!(path_style_of(None), PathStyle::Native);
        // The program alone says WSL when the id is something else.
        assert_eq!(
            path_style_of(Some(&shell("ubuntu", "C:\\Windows\\System32\\WSL.EXE"))),
            PathStyle::Wsl
        );
    }

    #[test]
    fn the_agent_reads_the_paste_whatever_shell_started_it() {
        assert_eq!(
            quote_style_of(AgentId::Claude, Some(&pwsh())),
            QuoteStyle::Agent
        );
        assert_eq!(
            quote_style_of(AgentId::Shell, Some(&pwsh())),
            QuoteStyle::Single
        );
        assert_eq!(
            quote_style_of(AgentId::Shell, Some(&cmd())),
            QuoteStyle::Cmd
        );
        assert_eq!(
            quote_style_of(AgentId::Shell, Some(&wsl())),
            QuoteStyle::Posix
        );
        assert_eq!(
            quote_style_of(AgentId::Shell, Some(&zsh())),
            QuoteStyle::Posix
        );
        assert_eq!(
            quote_style_of(AgentId::Shell, Some(&shell("x", "/usr/bin/nu"))),
            QuoteStyle::Single
        );
        assert_eq!(quote_style_of(AgentId::Shell, None), QuoteStyle::Posix);
    }

    #[test]
    fn drop_text_translates_quotes_and_joins_with_a_trailing_space_and_no_enter() {
        let paths = vec![
            "C:\\Users\\me\\My Shots\\a.png".to_string(),
            "C:\\Users\\me\\b.png".to_string(),
        ];
        assert_eq!(
            drop_text(&paths, AgentId::Claude, Some(&wsl())),
            "\"/mnt/c/Users/me/My Shots/a.png\" /mnt/c/Users/me/b.png "
        );
        assert_eq!(
            drop_text(&paths, AgentId::Shell, Some(&pwsh())),
            "'C:\\Users\\me\\My Shots\\a.png' C:\\Users\\me\\b.png "
        );
        assert_eq!(
            drop_text(&paths, AgentId::Claude, Some(&pwsh())),
            "\"C:\\Users\\me\\My Shots\\a.png\" C:\\Users\\me\\b.png "
        );
    }

    #[test]
    fn the_tracker_counts_hovered_files_and_gathers_a_drop() {
        let mut tracker = DropTracker::default();
        assert!(tracker.over().is_none());

        tracker.cursor_moved(Point::new(10.0, 20.0));
        tracker.file_hovered();
        tracker.file_hovered();
        assert_eq!(tracker.over(), Some((Point::new(10.0, 20.0), 2)));
        tracker.cursor_moved(Point::new(30.0, 40.0));
        assert_eq!(tracker.over(), Some((Point::new(30.0, 40.0), 2)));

        // The drop arrives one file at a time.
        tracker.file_dropped("/a.png".into());
        tracker.file_dropped("/b.png".into());
        assert!(tracker.over().is_none());
        assert!(tracker.has_pending_drop());
        let (paths, at) = tracker.take_drop().unwrap();
        assert_eq!(paths, ["/a.png", "/b.png"]);
        assert_eq!(at, Point::new(30.0, 40.0));
        assert!(tracker.take_drop().is_none());
    }

    #[test]
    fn a_hover_that_leaves_shows_nothing_and_a_drop_without_a_cursor_lands_nowhere() {
        let mut tracker = DropTracker::default();
        tracker.cursor_moved(Point::new(1.0, 1.0));
        tracker.file_hovered();
        tracker.hover_left();
        assert!(tracker.over().is_none());

        tracker.cursor_left();
        tracker.file_dropped("/a.png".into());
        assert!(tracker.take_drop().is_none());
    }
}
