//! The cases of `kitActivation.test.ts` about `KitEverywhereRow` and the
//! section's list, against its model.

use hangar_core::kits::KitInfo;
use hangar_i18n::{Locale, Translator};

use super::*;
use crate::kits::testing::{HOOKS, OTHER_HOOKS, entry, git_kit, installed, kit, tab};
use crate::state::types::AgentId;

struct Draw {
    workspace: Option<Workspace>,
    value: Vec<WorkspaceKit>,
    kits: Option<Vec<KitInfo>>,
    error: Option<String>,
    everywhere: Vec<WorkspaceKit>,
    restart: Option<Restart>,
    restarted: bool,
}

impl Draw {
    /// A workspace with a Claude Code pane, and the kit `hello-kit` installed.
    fn new() -> Draw {
        Draw {
            workspace: Some(Workspace {
                id: "w1".into(),
                name: "Studio".into(),
                panes: vec![crate::state::types::Pane {
                    id: "p1".into(),
                    name: "Ava".into(),
                    agent: AgentId::Claude,
                    ..Default::default()
                }],
                ..Workspace::default()
            }),
            value: Vec::new(),
            kits: Some(vec![kit("1.0.0", vec![tab(&["panes.read"])])]),
            error: None,
            everywhere: Vec::new(),
            restart: None,
            restarted: false,
        }
    }

    fn model(&self, locale: Locale) -> SectionModel {
        SectionModel::build(
            Translator::new(locale),
            &SectionInput {
                workspace: self.workspace.as_ref(),
                value: &self.value,
                kits: self.kits.as_deref(),
                error: self.error.as_deref(),
                everywhere: &self.everywhere,
                restart: self.restart.as_ref(),
                restarted: self.restarted,
            },
        )
    }

    fn en(&self) -> SectionModel {
        self.model(Locale::En)
    }
}

fn first(model: &SectionModel) -> &KitRow {
    match &model.rows[0] {
        Row::Kit(row) | Row::Everywhere(row) => row,
        Row::Gone(_) => panic!("an installed kit"),
    }
}

#[test]
fn lists_a_kit_off_with_a_switch_that_can_be_used() {
    let model = Draw::new().en();
    let row = first(&model);
    assert!(!row.on);
    assert!(row.switch_enabled);
    assert_eq!(row.switch_label, "Enable Hello kit on this workspace");
    assert_eq!(row.meta, "v1.0.0");
    assert_eq!(row.tags[0].label, "Off");
    assert_eq!(row.open, None);
}

#[test]
fn offers_details_for_a_kit_that_is_on_and_review_for_one_to_review() {
    let mut draw = Draw::new();
    draw.value = vec![entry("hello-kit", &["panes.read"])];
    let on = draw.en();
    assert_eq!(first(&on).tags[0].label, "Enabled");
    assert_eq!(first(&on).tags[0].tone, TagTone::Ok);
    assert_eq!(first(&on).open.as_deref(), Some("Details"));

    draw.value = vec![entry("hello-kit", &[])];
    let review = draw.en();
    assert_eq!(first(&review).tags[0].label, "New permissions to accept");
    assert_eq!(first(&review).tags[0].tone, TagTone::New);
    assert_eq!(first(&review).open.as_deref(), Some("Review"));
}

#[test]
fn says_a_kit_is_kept_out_of_new_panes_when_its_hooks_wait_to_be_accepted() {
    let mut draw = Draw::new();
    draw.kits = Some(vec![git_kit(Some(OTHER_HOOKS), true)]);
    draw.value = vec![entry("hello-kit", &["panes.read", HOOKS])];
    let model = draw.en();
    assert_eq!(
        first(&model).tags[0].label,
        "To review: kept out of new panes"
    );
}

#[test]
fn a_kit_that_cannot_be_used_cannot_be_switched_on() {
    let mut draw = Draw::new();
    let mut broken = kit("1.0.0", vec![]);
    broken.error = Some("its manifest is invalid".into());
    draw.kits = Some(vec![broken.clone()]);
    let off = draw.en();
    assert!(!first(&off).switch_enabled);
    assert!(first(&off).meta_is_error);
    assert_eq!(first(&off).meta, "its manifest is invalid");
    assert_eq!(first(&off).open, None);
    // Once enabled, its switch works: it is how it is turned off.
    draw.value = vec![entry("hello-kit", &[])];
    let on = draw.en();
    assert!(first(&on).switch_enabled);
    assert_eq!(first(&on).tags[0].label, "Unusable");
    assert_eq!(first(&on).open, None);
}

// -- KitEverywhereRow ---------------------------------------------------------------

#[test]
fn shows_the_kit_on_with_its_switch_locked_and_says_where_it_is_managed() {
    let mut draw = Draw::new();
    draw.kits = Some(vec![git_kit(Some(HOOKS), true)]);
    draw.everywhere = vec![entry("hello-kit", &["panes.read", HOOKS])];
    let model = draw.en();
    let Row::Everywhere(row) = &model.rows[0] else {
        panic!("an everywhere row");
    };
    assert!(row.on);
    assert!(!row.switch_enabled);
    assert_eq!(
        row.switch_label,
        "Hello kit is enabled on all workspaces: change it in Settings › Kits"
    );
    assert_eq!(row.meta, "Managed in Settings › Kits.");
    assert_eq!(row.tags.len(), 1);
    assert_eq!(row.tags[0].label, "On all workspaces");
    assert_eq!(row.open, None);
}

#[test]
fn says_so_in_french() {
    let mut draw = Draw::new();
    draw.kits = Some(vec![git_kit(Some(HOOKS), true)]);
    draw.everywhere = vec![entry("hello-kit", &["panes.read", HOOKS])];
    let model = draw.model(Locale::Fr);
    let text = model.text().replace(['\u{a0}', '\u{202f}'], " ");
    assert!(text.contains(
        "Hello kit est activé sur tous les workspaces : modifiez-le dans Réglages › Kits"
    ));
    assert!(text.contains("Géré dans Réglages › Kits."));
}

#[test]
fn says_when_it_waits_for_a_review_or_cannot_be_used() {
    let mut draw = Draw::new();
    draw.kits = Some(vec![git_kit(Some(OTHER_HOOKS), true)]);
    draw.everywhere = vec![entry("hello-kit", &["panes.read", HOOKS])];
    let model = draw.en();
    let Row::Everywhere(row) = &model.rows[0] else {
        panic!("an everywhere row");
    };
    assert_eq!(row.tags[1].tone, TagTone::New);

    let mut broken = kit("1.0.0", vec![]);
    broken.error = Some("its manifest is invalid".into());
    draw.kits = Some(vec![broken]);
    draw.everywhere = vec![entry("hello-kit", &[])];
    let model = draw.en();
    let Row::Everywhere(row) = &model.rows[0] else {
        panic!("an everywhere row");
    };
    assert_eq!(row.meta, "its manifest is invalid");
    assert_eq!(row.tags[1].label, "Unusable");
}

#[test]
fn a_workspace_s_own_entry_wins_over_the_everywhere_one() {
    let mut draw = Draw::new();
    draw.value = vec![entry("hello-kit", &["panes.read"])];
    draw.everywhere = vec![entry("hello-kit", &[])];
    let model = draw.en();
    assert!(matches!(model.rows[0], Row::Kit(_)));
    assert!(first(&model).switch_enabled);
}

// -- the rest of the section ---------------------------------------------------------------

#[test]
fn lists_a_kit_enabled_but_no_longer_installed_with_a_switch_that_removes_it() {
    let mut draw = Draw::new();
    draw.value = vec![entry("gone-kit", &[])];
    let model = draw.en();
    let Row::Gone(gone) = &model.rows[1] else {
        panic!("a gone row");
    };
    assert_eq!(gone.id, "gone-kit");
    assert_eq!(gone.meta, "Unreadable kit (gone-kit)");
    assert_eq!(gone.tag.label, "Unusable");
    assert_eq!(model.none, None);
}

#[test]
fn does_not_call_a_kit_gone_before_the_kits_are_read() {
    let mut draw = Draw::new();
    draw.kits = None;
    draw.value = vec![entry("gone-kit", &[])];
    let model = draw.en();
    assert!(model.rows.is_empty());
    assert_eq!(model.none, None);
}

#[test]
fn says_when_no_kit_is_installed() {
    let mut draw = Draw::new();
    draw.kits = Some(Vec::new());
    assert!(draw.en().none.is_some());
    draw.value = vec![entry("gone-kit", &[])];
    assert_eq!(draw.en().none, None);
}

#[test]
fn shows_why_the_kits_could_not_be_read() {
    let mut draw = Draw::new();
    draw.error = Some("the index cannot be read".into());
    assert_eq!(
        draw.en().error.as_deref(),
        Some("Error: the index cannot be read")
    );
}

#[test]
fn says_the_kits_are_enabled_once_the_workspace_is_created() {
    let mut draw = Draw::new();
    draw.workspace = None;
    assert_eq!(draw.en().pending, None);
    draw.value = vec![entry("hello-kit", &[])];
    assert!(draw.en().pending.is_some());
    // A workspace that exists has nothing pending.
    let existing = Draw::new();
    assert_eq!(existing.en().pending, None);
}

#[test]
fn offers_to_restart_the_claude_code_panes_after_an_accept() {
    let mut draw = Draw::new();
    draw.restart = Some(Restart {
        ws: "w1".into(),
        kit_name: "Hello kit".into(),
    });
    let model = draw.en();
    let restart = model.restart.expect("a notice");
    assert_eq!(restart.title, "Running agents do not have it yet");
    assert!(restart.body.contains("Hello kit"));
    assert!(restart.body.contains("Each one resumes its conversation."));
    assert_eq!(restart.action, "Restart the Claude Code panes (1)");
    assert_eq!(restart.later, "Later");
}

#[test]
fn offers_no_restart_for_a_workspace_being_created() {
    let mut draw = Draw::new();
    draw.workspace = None;
    draw.restart = Some(Restart {
        ws: "w1".into(),
        kit_name: "Hello kit".into(),
    });
    assert_eq!(draw.en().restart, None);
}

#[test]
fn says_the_panes_were_restarted() {
    let mut draw = Draw::new();
    draw.restarted = true;
    assert_eq!(draw.en().restarted.as_deref(), Some("Restarted."));
}

#[test]
fn an_installed_kit_with_no_manifest_is_listed_by_its_id() {
    let mut draw = Draw::new();
    draw.kits = Some(vec![installed("odd-kit")]);
    let model = draw.en();
    let row = first(&model);
    assert_eq!(row.name, "odd-kit");
    assert!(!row.switch_enabled);
}
