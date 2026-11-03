//! « Ouvrir avec VS Code » (`lib/openEditor.ts` and its tests): which path a
//! workspace opens, and the one line to show when it did not open.

use hangar_core::open_editor::{self, EditorOutcome};
use hangar_i18n::{Key, Translator, args};

use crate::state::types::Workspace;

/// Opens a workspace in VS Code: its `.code-workspace` when it came from one
/// (it then has other roots), its folder otherwise. A task that panicked comes
/// back as a `Failed` outcome rather than an error, so the caller has one path.
pub async fn open_in_vscode(ws: &Workspace) -> EditorOutcome {
    let cwd = ws.cwd.clone();
    let from_code_workspace = from_code_workspace(ws);
    match tokio::spawn(open_editor::open_in_vscode(cwd, from_code_workspace)).await {
        Ok(outcome) => outcome,
        Err(error) => failed(error),
    }
}

/// Whether the workspace opens through its `.code-workspace`.
pub fn from_code_workspace(ws: &Workspace) -> bool {
    !ws.extra_roots.is_empty()
}

/// The outcome of a call that could not run to its end.
fn failed(error: impl std::fmt::Display) -> EditorOutcome {
    EditorOutcome::Failed {
        error: error.to_string(),
    }
}

/// The one line to show for an outcome, or `None` when there is nothing to say
/// (`editorNotice`).
pub fn editor_notice(outcome: &EditorOutcome, name: &str, tr: Translator) -> Option<String> {
    match outcome {
        EditorOutcome::Opened => None,
        EditorOutcome::MissingFolder => Some(
            tr.t_with(Key::EditorMissingFolder, &args!("name" => name))
                .into_owned(),
        ),
        EditorOutcome::NotFound => Some(tr.t(Key::EditorNotFound).into_owned()),
        EditorOutcome::Failed { error } => Some(
            tr.t_with(Key::EditorFailed, &args!("error" => error.as_str()))
                .into_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use hangar_i18n::Locale;

    use super::*;

    fn workspace(cwd: &str, extra_roots: &[&str]) -> Workspace {
        Workspace {
            cwd: cwd.into(),
            extra_roots: extra_roots.iter().map(|root| root.to_string()).collect(),
            ..Workspace::default()
        }
    }

    const EN: Translator = Translator::new(Locale::En);

    #[test]
    fn opens_the_folder_of_a_plain_workspace() {
        assert!(!from_code_workspace(&workspace("C:/a b", &[])));
    }

    #[test]
    fn asks_for_the_code_workspace_when_the_workspace_has_other_roots() {
        assert!(from_code_workspace(&workspace("/p", &["/q"])));
    }

    #[test]
    fn turns_a_call_that_could_not_finish_into_a_failed_outcome() {
        assert_eq!(
            failed("boom"),
            EditorOutcome::Failed {
                error: "boom".into()
            }
        );
    }

    #[test]
    fn says_nothing_when_it_opened() {
        assert_eq!(editor_notice(&EditorOutcome::Opened, "W", EN), None);
    }

    #[test]
    fn names_the_workspace_whose_folder_is_gone() {
        assert_eq!(
            editor_notice(&EditorOutcome::MissingFolder, "W", EN).as_deref(),
            Some("The folder of W no longer exists.")
        );
    }

    #[test]
    fn says_vs_code_is_missing_and_what_failed_otherwise() {
        assert_eq!(
            editor_notice(&EditorOutcome::NotFound, "W", EN).as_deref(),
            Some("VS Code was not found on this computer.")
        );
        let failed = EditorOutcome::Failed { error: "x".into() };
        assert_eq!(
            editor_notice(&failed, "W", EN).as_deref(),
            Some("VS Code could not be opened: x")
        );
    }

    #[test]
    fn says_the_same_in_french() {
        let fr = Translator::new(Locale::Fr);
        assert_eq!(
            editor_notice(&EditorOutcome::NotFound, "W", fr).as_deref(),
            Some("VS Code est introuvable sur cet ordinateur.")
        );
    }
}
