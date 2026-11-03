//! The cases of `KitsSettings.test.ts` and of the row cases of
//! `kitGallery.test.ts`, against the page's model.

use std::collections::HashMap;

use hangar_core::kits::{KitInfo, UpdateInfo};
use hangar_i18n::{Locale, Translator};

use super::*;
use crate::kits::testing::{entry, folder, git, installed, kit, tab};
use crate::kits::{Page, UpdateNote};

fn manifest_of(id: &str, name: &str, version: &str) -> hangar_core::kits::Manifest {
    let mut manifest = crate::kits::testing::manifest(id, version, vec![]);
    manifest.name = name.into();
    manifest.description = "Build places with agents.".into();
    manifest.tabs = vec![tab(&["panes.send", "notify"])];
    manifest
}

fn git_kit() -> KitInfo {
    KitInfo {
        id: "roblox".into(),
        source: hangar_core::kits::Source::Git(hangar_core::kits::GitSource {
            url: "https://github.com/o/kits".into(),
            git_ref: Some("main".into()),
            subdir: Some("roblox".into()),
        }),
        installed_version: "1.0.0".into(),
        manifest: Some(manifest_of("roblox", "Roblox", "1.0.0")),
        ..installed("roblox")
    }
}

fn folder_kit() -> KitInfo {
    KitInfo {
        id: "hello-kit".into(),
        source: folder(),
        manifest: Some(manifest_of("hello-kit", "Hello kit", "1.0.0")),
        ..git_kit()
    }
}

fn broken_kit() -> KitInfo {
    KitInfo {
        id: "bad-kit".into(),
        manifest: None,
        error: Some("tabs[0].permissions: unknown permission \"files.write\"".into()),
        ..git_kit()
    }
}

struct Draw {
    kits: Option<Vec<KitInfo>>,
    page: Page,
    workspaces: Vec<Workspace>,
    everywhere: Vec<WorkspaceKit>,
    has_active_workspace: bool,
    error: Option<String>,
}

impl Draw {
    fn new() -> Draw {
        Draw {
            kits: Some(vec![git_kit(), folder_kit(), broken_kit()]),
            page: Page::default(),
            workspaces: vec![Workspace {
                id: "w1".into(),
                name: "Studio".into(),
                kits: Some(vec![entry("roblox", &[]), entry("hello-kit", &[])]),
                ..Workspace::default()
            }],
            everywhere: Vec::new(),
            has_active_workspace: true,
            error: None,
        }
    }

    fn kits(mut self, kits: Vec<KitInfo>) -> Self {
        self.kits = Some(kits);
        self
    }

    fn model(&self, locale: Locale) -> PageModel {
        let input = PageInput {
            kits: self.kits.as_deref(),
            error: self.error.as_deref(),
            page: &self.page,
            workspaces: &self.workspaces,
            everywhere: &self.everywhere,
            has_active_workspace: self.has_active_workspace,
        };
        PageModel::build(Translator::new(locale), &input)
    }

    fn en(&self) -> PageModel {
        self.model(Locale::En)
    }
}

fn rows(model: &PageModel) -> &[RowModel] {
    let Loadable::Ready(rows) = &model.installed else {
        panic!("the kits are listed");
    };
    rows
}

#[test]
fn lists_each_kit_with_its_version_and_source_and_shows_a_broken_manifest_s_error() {
    let mut draw = Draw::new();
    draw.workspaces.clear();
    let out = draw.en().text();
    assert!(out.contains("Roblox"));
    assert!(out.contains("v1.0.0"));
    assert!(out.contains("GitHub: https://github.com/o/kits (main · roblox)"));
    assert!(out.contains("Folder: D:\\kits\\hello-kit"));
    assert!(out.contains("linked, read live"));
    assert!(out.contains("This kit cannot be used"));
    assert!(out.contains("unknown permission"));
}

#[test]
fn offers_an_update_check_on_a_github_kit_and_a_reload_on_a_linked_folder() {
    let draw = Draw::new().kits(vec![git_kit(), folder_kit()]);
    let model = draw.en();
    let out = model.text();
    assert!(out.contains("Check for update"));
    assert!(out.contains("Reload"));
    assert!(out.contains("Add from a folder"));
    assert!(out.contains("Add from GitHub"));
    let rows = rows(&model);
    assert!(matches!(
        &rows[0].actions,
        Actions::Normal {
            primary: Primary::Check { enabled: true, .. },
            ..
        }
    ));
    assert!(matches!(
        &rows[1].actions,
        Actions::Normal {
            primary: Primary::Reload { enabled: true, .. },
            ..
        }
    ));
}

#[test]
fn shows_the_old_and_the_new_version_of_an_available_update() {
    let mut draw = Draw::new().kits(vec![git_kit()]);
    draw.page.notes = HashMap::from([(
        "roblox".to_owned(),
        UpdateNote::Available {
            current: "1.0.0".into(),
            latest: "1.1.0".into(),
        },
    )]);
    let model = draw.en();
    assert!(model.text().contains("v1.0.0 → v1.1.0"));
    // The button is « Update ».
    assert!(matches!(
        &rows(&model)[0].actions,
        Actions::Normal { primary: Primary::Update { label, enabled: true }, .. } if label == "Update"
    ));
}

#[test]
fn an_update_the_last_check_found_is_offered_without_asking_again() {
    let mut found = git_kit();
    found.update = Some(UpdateInfo {
        current: "1.0.0".into(),
        latest: "1.2.0".into(),
        available: true,
        checked_at: 1,
    });
    let model = Draw::new().kits(vec![found]).en();
    assert!(model.text().contains("v1.0.0 → v1.2.0"));
    assert!(matches!(
        &rows(&model)[0].actions,
        Actions::Normal {
            primary: Primary::Update { .. },
            ..
        }
    ));
}

#[test]
fn a_finished_update_says_so_and_warns_when_it_asked_for_more() {
    let mut draw = Draw::new().kits(vec![git_kit()]);
    draw.page.notes = HashMap::from([(
        "roblox".to_owned(),
        UpdateNote::Done {
            from: "1.0.0".into(),
            to: "1.1.0".into(),
            new_permissions: 1,
        },
    )]);
    let row = &rows(&draw.en())[0].clone();
    assert_eq!(row.note.as_deref(), Some("Updated: v1.0.0 → v1.1.0."));
    assert!(
        row.note_extra
            .as_deref()
            .unwrap()
            .contains("asks for new permissions")
    );
    draw.page.notes = HashMap::from([(
        "roblox".to_owned(),
        UpdateNote::Done {
            from: "1.0.0".into(),
            to: "1.0.1".into(),
            new_permissions: 0,
        },
    )]);
    assert_eq!(rows(&draw.en())[0].note_extra, None);
}

#[test]
fn says_what_each_update_note_says() {
    let t = Translator::new(Locale::En);
    let said = |note: UpdateNote| note_text(t, &note);
    assert_eq!(said(UpdateNote::Checking), "Checking…");
    assert_eq!(
        said(UpdateNote::None {
            version: "1.0.0".into()
        }),
        "Up to date (v1.0.0)."
    );
    assert_eq!(said(UpdateNote::Updating), "Updating…");
    assert_eq!(
        said(UpdateNote::Folder),
        "A linked folder has no remote: edit it, then reload."
    );
    assert_eq!(
        said(UpdateNote::Reloaded {
            version: "2.0.0".into()
        }),
        "Reloaded: v2.0.0."
    );
    assert_eq!(
        said(UpdateNote::Error("no network".into())),
        "Error: no network"
    );
}

#[test]
fn a_kit_being_checked_or_updated_cannot_be_asked_again() {
    let mut draw = Draw::new().kits(vec![git_kit()]);
    draw.page.notes = HashMap::from([("roblox".to_owned(), UpdateNote::Checking)]);
    assert!(matches!(
        &rows(&draw.en())[0].actions,
        Actions::Normal { primary: Primary::Check { label, enabled: false }, .. } if label == "Checking…"
    ));
    draw.page.notes = HashMap::from([("roblox".to_owned(), UpdateNote::Updating)]);
    assert!(matches!(
        &rows(&draw.en())[0].actions,
        Actions::Normal {
            primary: Primary::Check { enabled: false, .. },
            ..
        }
    ));
}

#[test]
fn a_linked_folder_with_no_manifest_cannot_be_reloaded() {
    let mut gone = folder_kit();
    gone.manifest = None;
    gone.error = Some("no hangar-kit.json".into());
    assert!(matches!(
        &rows(&Draw::new().kits(vec![gone]).en())[0].actions,
        Actions::Normal {
            primary: Primary::Reload { enabled: false, .. },
            ..
        }
    ));
}

#[test]
fn says_a_linked_folder_is_left_untouched_before_removing_it() {
    let mut draw = Draw::new().kits(vec![folder_kit()]);
    draw.page.removing = Some("hello-kit".into());
    let model = draw.en();
    let out = model.text();
    assert!(out.contains("The linked folder is left untouched"));
    assert!(out.contains("Remove the kit"));
    assert!(matches!(&rows(&model)[0].actions, Actions::Confirm { .. }));
    // A kit Hangar cloned says its copy goes.
    let mut git_draw = Draw::new().kits(vec![git_kit()]);
    git_draw.page.removing = Some("roblox".into());
    assert!(
        git_draw
            .en()
            .text()
            .contains("The copy Hangar cloned is deleted.")
    );
}

#[test]
fn shows_the_inline_errors_of_both_ways_of_adding() {
    let mut draw = Draw::new();
    draw.page.folder_error = Some("no hangar-kit.json".into());
    draw.page.git_error = Some("not a GitHub link".into());
    let model = draw.en();
    let out = model.text();
    assert!(out.contains("no hangar-kit.json"));
    assert!(out.contains("not a GitHub link"));
    assert_eq!(model.add.git_error.as_deref(), Some("not a GitHub link"));
}

#[test]
fn adds_from_a_link_only_once_there_is_one_and_nothing_else_is_being_added() {
    let mut draw = Draw::new();
    assert!(!draw.en().add.git_enabled);
    draw.page.git_url = "  https://github.com/o/r ".into();
    let model = draw.en();
    assert!(model.add.git_enabled);
    assert!(model.add.folder_enabled);
    draw.page.adding = Some(Adding::Git);
    let busy = draw.en();
    assert!(!busy.add.git_enabled);
    assert!(!busy.add.folder_enabled);
    assert_eq!(busy.add.git_button, "Adding…");
    assert_eq!(busy.add.folder_button, "Add from a folder");
}

#[test]
fn lists_the_workspaces_to_manage_their_kits_from() {
    let model = Draw::new().en();
    let out = model.text();
    assert!(out.contains("Kits per workspace"));
    assert!(out.contains("Studio"));
    assert!(out.contains("2 kits enabled"));
    assert!(out.contains("Manage kits"));
}

#[test]
fn leaves_out_the_block_of_workspaces_when_there_are_none() {
    let mut draw = Draw::new();
    draw.workspaces.clear();
    assert!(draw.en().by_workspace.is_none());
}

#[test]
fn counts_one_kit_in_the_singular() {
    let mut draw = Draw::new();
    draw.workspaces[0].kits = Some(vec![entry("roblox", &[])]);
    assert!(draw.en().text().contains("1 kit enabled"));
}

#[test]
fn offers_a_switch_on_all_workspaces_on_each_kit_off_by_default() {
    let model = Draw::new().kits(vec![git_kit()]).en();
    let row = &rows(&model)[0];
    assert_eq!(row.everywhere.label, "On all workspaces");
    assert_eq!(
        row.everywhere.switch_label,
        "Enable Roblox on all workspaces"
    );
    assert!(!row.everywhere.on);
    assert!(row.everywhere.switch_enabled);
}

#[test]
fn shows_a_kit_enabled_everywhere_as_on() {
    let mut draw = Draw::new().kits(vec![git_kit()]);
    draw.everywhere = vec![entry("roblox", &["panes.send", "notify"])];
    let model = draw.en();
    let row = &rows(&model)[0];
    assert!(row.everywhere.on);
    assert_eq!(row.everywhere.tags[0].label, "Enabled");
    assert_eq!(row.everywhere.review, None);
}

#[test]
fn marks_an_everywhere_kit_to_review_when_an_update_added_permissions_or_changed_its_hooks() {
    let mut draw = Draw::new().kits(vec![git_kit()]);
    draw.everywhere = vec![entry("roblox", &["notify"])];
    let added = draw.en();
    let out = added.text();
    assert!(out.contains("New permissions to accept"));
    assert_eq!(rows(&added)[0].everywhere.review.as_deref(), Some("Review"));
    assert!(out.contains("kept out of every pane"));

    let hooked = KitInfo {
        hooks_permission: Some("hooks:0123456789abcdef".into()),
        hooks_consent: true,
        ..git_kit()
    };
    let mut held = Draw::new().kits(vec![hooked]);
    held.everywhere = vec![entry("roblox", &["panes.send", "notify"])];
    assert!(
        held.en()
            .text()
            .contains("To review: kept out of new panes")
    );
}

#[test]
fn says_a_review_of_commands_alone_in_its_own_words() {
    let commands = KitInfo {
        commands_permission: Some("commands:0123456789abcdef".into()),
        ..git_kit()
    };
    let mut draw = Draw::new().kits(vec![commands]);
    draw.everywhere = vec![entry("roblox", &["panes.send", "notify"])];
    let model = draw.en();
    let hint = &rows(&model)[0].everywhere.hint;
    assert_ne!(
        hint,
        "Accepted once, for every workspace, the ones you create later included. A workspace's own choice for this kit still wins."
    );
    assert!(!hint.contains("kept out of every pane"));
}

#[test]
fn says_to_open_a_workspace_first_only_when_none_is_active() {
    let mut draw = Draw::new().kits(vec![git_kit()]);
    draw.has_active_workspace = false;
    assert!(draw.en().text().contains("Open a workspace first"));
    draw.has_active_workspace = true;
    assert!(!draw.en().text().contains("Open a workspace first"));
}

#[test]
fn says_running_agents_need_a_restart_after_a_change() {
    let mut draw = Draw::new();
    draw.page.restart_for = Some("Roblox".into());
    let model = draw.en();
    let restart = model.restart.as_ref().expect("a notice");
    assert_eq!(restart.title, "Running agents do not have the change yet");
    assert!(model.text().contains("Roblox"));
    assert_eq!(restart.dismiss, "Got it");
}

#[test]
fn speaks_french_with_accents() {
    let mut draw = Draw::new().kits(vec![folder_kit()]);
    draw.page.removing = Some("hello-kit".into());
    let out = draw.model(Locale::Fr).text();
    assert!(out.contains("Le dossier lié n'est pas touché"));
    assert!(out.contains("Ajouter depuis un dossier"));
    assert!(out.contains("Sur tous les workspaces"));
    let mut restart = Draw::new();
    restart.page.restart_for = Some("Roblox".into());
    assert!(
        restart
            .model(Locale::Fr)
            .text()
            .contains("Les agents en cours n'ont pas encore")
    );
}

#[test]
fn says_the_page_is_loading_or_empty() {
    let mut draw = Draw::new();
    draw.kits = None;
    assert!(matches!(draw.en().installed, Loadable::Loading(_)));
    draw.kits = Some(Vec::new());
    assert!(matches!(draw.en().installed, Loadable::Empty(_)));
    draw.error = Some("the index cannot be read".into());
    assert_eq!(
        draw.en().error.as_deref(),
        Some("Error: the index cannot be read")
    );
}

// -- kitGallery.test.ts: the row of an installed kit ------------------------------------------

fn pinned() -> KitInfo {
    KitInfo {
        source: git(
            "https://github.com/hangar-ai/ci-status",
            Some("refs/tags/v0.1.0"),
        ),
        installed_version: "0.1.0".into(),
        ..kit("0.1.0", vec![])
    }
}

#[test]
fn says_what_a_kit_is_pinned_to_in_words_not_by_git_s_name_for_the_tag() {
    let out = Draw::new().kits(vec![pinned()]).en().text();
    assert!(out.contains("GitHub: https://github.com/hangar-ai/ci-status · pinned to v0.1.0"));
    assert!(!out.contains("refs/tags/"));
}

#[test]
fn keeps_the_folder_of_a_pinned_kit_and_says_nothing_of_a_kit_on_a_branch() {
    let url = "https://github.com/o/r";
    let with_folder = KitInfo {
        source: hangar_core::kits::Source::Git(hangar_core::kits::GitSource {
            url: url.into(),
            git_ref: Some("refs/tags/v1".into()),
            subdir: Some("kit".into()),
        }),
        ..pinned()
    };
    assert!(
        Draw::new()
            .kits(vec![with_folder])
            .en()
            .text()
            .contains("GitHub: https://github.com/o/r (kit) · pinned to v1")
    );
    let branch = KitInfo {
        source: git(url, Some("main")),
        ..pinned()
    };
    let out = Draw::new().kits(vec![branch]).en().text();
    assert!(out.contains("GitHub: https://github.com/o/r (main)"));
    assert!(!out.contains("pinned to"));
}
