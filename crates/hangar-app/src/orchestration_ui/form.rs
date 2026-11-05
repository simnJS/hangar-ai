//! The « Orchestration » settings of a workspace as its panel edits them
//! (`lib/orchestrationForm.ts`): what a field may hold, how what was typed
//! becomes the settings saved, and what is worth saving at all.
//!
//! Reading is `state::roles::orchestration_of`'s (the TS `orchestrationOf` of
//! lib/orchestration): it takes whatever is stored, hand-edited or not, and
//! falls back field by field. This is the other way round, a form the person is filling in, and
//! holds them to a little more: a shape for the names a command line will
//! carry.
//!
//! The TS form also had steppers for the workers at once and the attempts per
//! task. This version sets no such limit: the orchestrator decides both.

use crate::state::roles::{
    ORCHESTRATION_PERMISSION_MODES, orchestration_defaults, orchestration_of,
};
use crate::state::types::{OrchestrationSettings, Workspace};

/// Offered under the model fields; Claude Code takes a full model id as well.
pub const MODEL_SUGGESTIONS: [&str; 3] = ["opus", "sonnet", "haiku"];

/// The default models as text that lives for the whole program, for the
/// placeholders a field keeps (`orchestration_defaults()` allocates its own).
pub const DEFAULT_ORCHESTRATOR_MODEL: &str = "opus";
pub const DEFAULT_WORKER_MODEL: &str = "sonnet";
pub const DEFAULT_REVIEWER_MODEL: &str = "sonnet";

/// The fields as the panel holds them: text stays text until it is saved.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub auto_merge: bool,
    pub shared_build_cache: bool,
    pub check_command: String,
    pub orchestrator_model: String,
    pub worker_model: String,
    pub reviewer_model: String,
    pub branch: String,
    /// Empty for the person's own mode.
    pub permission_mode: String,
}

/// A field of the draft that can be refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    OrchestratorModel,
    WorkerModel,
    ReviewerModel,
    Branch,
}

/// Why a field cannot be saved as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Model,
    Branch,
}

/// Each field that cannot be saved, in the order the panel lists them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Problems(Vec<(Field, Problem)>);

impl Problems {
    pub fn get(&self, field: Field) -> Option<Problem> {
        self.0
            .iter()
            .find(|(of, _)| *of == field)
            .map(|(_, problem)| *problem)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The panel opens on what the workspace runs with now, defaults included.
pub fn draft_of(ws: &Workspace) -> Draft {
    let settings = orchestration_of(ws);
    Draft {
        auto_merge: settings.auto_merge,
        shared_build_cache: settings.shared_build_cache,
        check_command: settings.check_command.unwrap_or_default(),
        orchestrator_model: settings.orchestrator_model,
        worker_model: settings.worker_model,
        reviewer_model: settings.reviewer_model,
        branch: settings.branch.unwrap_or_default(),
        permission_mode: settings.permission_mode.unwrap_or_default(),
    }
}

/// A model name goes on a command line (`--model <name>`): an alias, or an id
/// as Claude Code spells them (`claude-opus-4-1`, `claude-sonnet-4-5[1m]`, a
/// Bedrock `us.anthropic…` id or an ARN). Never a space or a quote.
/// The TS `/^[\w.:/@[\]-]+$/`, `\w` being ASCII there.
fn is_model_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '/' | '@' | '[' | ']' | '-')
        })
}

/// What git refuses in a branch name, the cheap part of `check-ref-format`:
/// whitespace, the characters it reserves, `..`, a leading dash, a trailing
/// dot or slash, `.lock`. The TS `BAD_BRANCH`.
fn is_bad_branch(name: &str) -> bool {
    // JavaScript's `\s` also counts the byte order mark.
    let reserved = |c: char| {
        c.is_whitespace()
            || c == '\u{feff}'
            || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    };
    name.chars().any(reserved)
        || name.contains("..")
        || name.starts_with('-')
        || name.ends_with(['.', '/'])
        || name.ends_with(".lock")
        || name.contains("@{")
}

/// Each field that cannot be saved as typed, and why. Blank is never a
/// problem.
pub fn draft_problems(draft: &Draft) -> Problems {
    let mut problems = Vec::new();
    for (field, value) in [
        (Field::OrchestratorModel, &draft.orchestrator_model),
        (Field::WorkerModel, &draft.worker_model),
        (Field::ReviewerModel, &draft.reviewer_model),
    ] {
        let name = value.trim();
        if !name.is_empty() && !is_model_name(name) {
            problems.push((field, Problem::Model));
        }
    }
    let branch = draft.branch.trim();
    if !branch.is_empty() && is_bad_branch(branch) {
        problems.push((Field::Branch, Problem::Branch));
    }
    Problems(problems)
}

/// The settings a draft stands for: names trimmed, a blank model back to its
/// default, a blank check or branch to none. Fields with a problem
/// (`draft_problems`) are the caller's to refuse; here they fall back to the
/// default like a blank one.
pub fn normalize_draft(draft: &Draft) -> OrchestrationSettings {
    let base = orchestration_defaults();
    let problems = draft_problems(draft);
    let model = |field: Field, value: &str, fallback: &str| {
        let value = value.trim();
        if problems.get(field).is_none() && !value.is_empty() {
            value.to_owned()
        } else {
            fallback.to_owned()
        }
    };
    let optional = |text: &str| {
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    };
    OrchestrationSettings {
        auto_merge: draft.auto_merge,
        shared_build_cache: draft.shared_build_cache,
        // A check is run through the person's shell exactly as typed, inner
        // spaces and all: only the ends are trimmed.
        check_command: optional(&draft.check_command),
        orchestrator_model: model(
            Field::OrchestratorModel,
            &draft.orchestrator_model,
            &base.orchestrator_model,
        ),
        worker_model: model(Field::WorkerModel, &draft.worker_model, &base.worker_model),
        reviewer_model: model(
            Field::ReviewerModel,
            &draft.reviewer_model,
            &base.reviewer_model,
        ),
        branch: if problems.get(Field::Branch).is_some() {
            None
        } else {
            optional(&draft.branch)
        },
        permission_mode: ORCHESTRATION_PERMISSION_MODES
            .contains(&draft.permission_mode.as_str())
            .then(|| draft.permission_mode.clone()),
    }
}

pub fn same_settings(a: &OrchestrationSettings, b: &OrchestrationSettings) -> bool {
    a == b
}

/// What to store on the workspace: nothing at all when every field is its
/// default, so a workspace whose settings were put back is saved as one that
/// never had any, and follows the defaults a later version ships.
pub fn stored_orchestration(settings: &OrchestrationSettings) -> Option<OrchestrationSettings> {
    if same_settings(settings, &orchestration_defaults()) {
        None
    } else {
        Some(settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(patch: impl FnOnce(&mut Draft)) -> Draft {
        let mut draft = draft_of(&Workspace::default());
        patch(&mut draft);
        draft
    }

    fn only(field: Field, problem: Problem) -> Problems {
        Problems(vec![(field, problem)])
    }

    #[test]
    fn the_default_model_names_are_the_stores_defaults() {
        let base = orchestration_defaults();
        assert_eq!(base.orchestrator_model, DEFAULT_ORCHESTRATOR_MODEL);
        assert_eq!(base.worker_model, DEFAULT_WORKER_MODEL);
        assert_eq!(base.reviewer_model, DEFAULT_REVIEWER_MODEL);
    }

    #[test]
    fn opens_a_workspace_that_never_had_settings_on_the_defaults_blanks_for_the_optional_fields() {
        let base = orchestration_defaults();
        assert_eq!(
            draft_of(&Workspace::default()),
            Draft {
                auto_merge: base.auto_merge,
                shared_build_cache: true,
                check_command: String::new(),
                orchestrator_model: base.orchestrator_model,
                worker_model: base.worker_model,
                reviewer_model: base.reviewer_model,
                branch: String::new(),
                permission_mode: String::new(),
            }
        );
    }

    #[test]
    fn opens_on_what_the_workspace_runs_with_a_malformed_field_read_as_its_default() {
        let saved = OrchestrationSettings {
            auto_merge: false,
            check_command: Some("pnpm test".into()),
            worker_model: "  ".into(),
            branch: Some("main".into()),
            ..orchestration_defaults()
        };
        let ws = Workspace {
            orchestration: Some(saved),
            ..Workspace::default()
        };
        let opened = draft_of(&ws);
        assert!(!opened.auto_merge);
        assert_eq!(opened.check_command, "pnpm test");
        assert_eq!(opened.worker_model, "sonnet");
        assert_eq!(opened.branch, "main");
    }

    #[test]
    fn reads_a_mode_nobody_offers_as_the_default() {
        let ws = Workspace {
            orchestration: Some(OrchestrationSettings {
                permission_mode: Some("yolo".into()),
                ..orchestration_defaults()
            }),
            ..Workspace::default()
        };
        assert_eq!(orchestration_of(&ws), orchestration_defaults());
    }

    #[test]
    fn trims_names_and_reads_a_blank_one_as_its_default() {
        let next = normalize_draft(&draft(|d| {
            d.orchestrator_model = "  claude-opus-4-1 ".into();
            d.worker_model = String::new();
            d.reviewer_model = " ".into();
        }));
        assert_eq!(next.orchestrator_model, "claude-opus-4-1");
        assert_eq!(next.worker_model, orchestration_defaults().worker_model);
        assert_eq!(next.reviewer_model, orchestration_defaults().reviewer_model);
    }

    #[test]
    fn reads_a_blank_check_or_branch_as_none_and_keeps_a_checks_inner_spaces() {
        let blank = normalize_draft(&draft(|d| {
            d.check_command = "   ".into();
            d.branch = String::new();
        }));
        assert_eq!(blank.check_command, None);
        assert_eq!(blank.branch, None);
        let spaced = normalize_draft(&draft(|d| {
            d.check_command = "  cargo test --locked && pnpm  test ".into()
        }));
        assert_eq!(
            spaced.check_command.as_deref(),
            Some("cargo test --locked && pnpm  test")
        );
    }

    #[test]
    fn is_what_settings_of_reads_back_once_stored() {
        let next = normalize_draft(&draft(|d| {
            d.reviewer_model = "haiku".into();
            d.auto_merge = false;
            d.check_command = "make check".into();
            d.branch = "develop".into();
        }));
        let ws = Workspace {
            orchestration: Some(next.clone()),
            ..Workspace::default()
        };
        assert_eq!(orchestration_of(&ws), next);
    }

    #[test]
    fn accepts_aliases_and_the_model_ids_claude_code_takes() {
        for name in [
            "opus",
            "claude-sonnet-4-5",
            "claude-sonnet-4-5[1m]",
            "us.anthropic.claude-opus-4-1-v1:0",
            "arn:aws:bedrock:us-east-1:123:inference-profile/x",
            "",
        ] {
            assert!(
                draft_problems(&draft(|d| d.worker_model = name.into())).is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn refuses_a_model_name_a_command_line_would_split_or_quote() {
        assert_eq!(
            draft_problems(&draft(|d| d.orchestrator_model = "claude opus".into())),
            only(Field::OrchestratorModel, Problem::Model)
        );
        assert_eq!(
            draft_problems(&draft(|d| d.reviewer_model = "sonnet\"; rm -rf".into())),
            only(Field::ReviewerModel, Problem::Model)
        );
    }

    #[test]
    fn refuses_a_branch_name_git_would() {
        for name in [
            "feat/orch ui",
            "-main",
            "a..b",
            "wip.",
            "topic/",
            "x.lock",
            "a~1",
            "what?",
            "a@{1}",
        ] {
            assert_eq!(
                draft_problems(&draft(|d| d.branch = name.into())),
                only(Field::Branch, Problem::Branch),
                "{name}"
            );
        }
        for name in ["main", "feat/orch-ui", "release/0.16", ""] {
            assert!(
                draft_problems(&draft(|d| d.branch = name.into())).is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn never_lets_a_refused_field_through_to_what_is_saved() {
        let next = normalize_draft(&draft(|d| {
            d.worker_model = "two words".into();
            d.branch = "bad name".into();
        }));
        assert_eq!(next.worker_model, orchestration_defaults().worker_model);
        assert_eq!(next.branch, None);
    }

    #[test]
    fn stores_nothing_for_settings_that_are_all_the_defaults() {
        assert_eq!(
            stored_orchestration(&normalize_draft(&draft_of(&Workspace::default()))),
            None
        );
        assert_eq!(stored_orchestration(&orchestration_defaults()), None);
    }

    #[test]
    fn stores_every_field_once_one_of_them_differs() {
        let next = OrchestrationSettings {
            auto_merge: false,
            ..orchestration_defaults()
        };
        assert_eq!(stored_orchestration(&next), Some(next));
    }

    #[test]
    fn tells_settings_apart_field_by_field() {
        assert!(same_settings(
            &orchestration_defaults(),
            &orchestration_defaults()
        ));
        assert!(!same_settings(
            &orchestration_defaults(),
            &OrchestrationSettings {
                branch: Some("main".into()),
                ..orchestration_defaults()
            }
        ));
    }

    #[test]
    fn the_panel_opens_on_what_state_roles_reads_back() {
        // The one reader of stored settings is `state::roles::orchestration_of`
        // (the TS `orchestrationOf`): a permission mode is kept only when it
        // is one that is offered, exactly as written.
        let saved = |mode: &str| Workspace {
            orchestration: Some(OrchestrationSettings {
                permission_mode: Some(mode.into()),
                ..orchestration_defaults()
            }),
            ..Workspace::default()
        };
        assert_eq!(
            draft_of(&saved("acceptEdits")).permission_mode,
            "acceptEdits"
        );
        assert_eq!(draft_of(&saved(" acceptEdits ")).permission_mode, "");
        assert_eq!(draft_of(&saved("yolo")).permission_mode, "");
        assert_eq!(
            draft_of(&saved("auto")),
            Draft {
                permission_mode: "auto".into(),
                ..draft_of(&Workspace::default())
            }
        );
    }
}
