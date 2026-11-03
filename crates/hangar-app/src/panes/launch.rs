//! The name a Claude Code session runs under (`sessionName` of
//! `src/lib/paneNames.ts`), which no merged package provides yet: the shell
//! ports that module in `state::pane_names` (P0b). The command itself is
//! P4's (`panes_logic::agents::launch_command`), the login one P8's
//! (`accounts::LOGIN_COMMAND`). When `state::pane_names` has
//! `session_name`, the launch should call it and this file go.

/// The name a Claude Code session runs under, what other sessions list and
/// message it by: the pane's name, then its workspace's, with what a shell
/// would read as quoting or expansion left out (`sessionName`).
pub fn session_name(pane: &str, workspace: &str) -> String {
    let cleaned: String = workspace
        .chars()
        .filter(|c| !"\"'`$\\%!^&|<>".contains(*c))
        .collect();
    let workspace = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if workspace.is_empty() {
        pane.to_owned()
    } else {
        format!("{pane} ({workspace})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_name_drops_what_a_shell_would_expand() {
        assert_eq!(session_name("Ava", "Hangar"), "Ava (Hangar)");
        assert_eq!(session_name("Ava", "  it's  $HOME  "), "Ava (its HOME)");
        assert_eq!(session_name("Ava", " "), "Ava");
    }
}
