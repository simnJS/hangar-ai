//! Which shell a pane runs (`lib/shells.ts`).

use crate::state::types::ShellInfo;

/// Resolves the shell for a pane by walking the override chain
/// (pane, then workspace, then global), falling back to the first detected
/// shell. Ids that no longer exist, an uninstalled shell for one, are
/// skipped rather than trusted.
pub fn resolve_shell<'a>(
    shells: &'a [ShellInfo],
    preferred: &[Option<&str>],
) -> Option<&'a ShellInfo> {
    for id in preferred.iter().flatten() {
        if id.is_empty() {
            continue;
        }
        if let Some(shell) = shells.iter().find(|shell| shell.id == *id) {
            return Some(shell);
        }
    }
    shells.first()
}

/// The label of the shell with this id, or `None` when none has it (nothing
/// chosen, or an id that no longer exists). The TS answered "Auto" in that
/// case; the caller words it, because no literal text lives in the logic.
pub fn shell_label<'a>(shells: &'a [ShellInfo], id: Option<&str>) -> Option<&'a str> {
    let id = id?;
    shells
        .iter()
        .find(|shell| shell.id == id)
        .map(|shell| shell.label.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(id: &str) -> ShellInfo {
        ShellInfo {
            id: id.into(),
            label: id.to_uppercase(),
            program: format!("/bin/{id}"),
            args: Vec::new(),
        }
    }

    #[test]
    fn walks_the_chain_and_takes_the_first_shell_that_exists() {
        let shells = [shell("bash"), shell("zsh"), shell("fish")];
        // The pane's own choice wins.
        assert_eq!(
            resolve_shell(&shells, &[Some("zsh"), Some("fish")]).map(|s| s.id.as_str()),
            Some("zsh")
        );
        // An uninstalled shell is skipped, and so is a missing level.
        assert_eq!(
            resolve_shell(&shells, &[Some("pwsh"), None, Some("fish")]).map(|s| s.id.as_str()),
            Some("fish")
        );
        assert_eq!(
            resolve_shell(&shells, &[Some(""), Some("zsh")]).map(|s| s.id.as_str()),
            Some("zsh")
        );
    }

    #[test]
    fn falls_back_on_the_first_detected_shell_or_none() {
        let shells = [shell("bash"), shell("zsh")];
        assert_eq!(
            resolve_shell(&shells, &[Some("nope")]).map(|s| s.id.as_str()),
            Some("bash")
        );
        assert_eq!(
            resolve_shell(&shells, &[]).map(|s| s.id.as_str()),
            Some("bash")
        );
        assert!(resolve_shell(&[], &[Some("bash")]).is_none());
    }

    #[test]
    fn labels_a_shell_by_id() {
        let shells = [shell("bash")];
        assert_eq!(shell_label(&shells, Some("bash")), Some("BASH"));
        assert_eq!(shell_label(&shells, Some("zsh")), None);
        assert_eq!(shell_label(&shells, None), None);
    }
}
