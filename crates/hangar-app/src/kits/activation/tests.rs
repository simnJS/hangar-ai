//! The cases of `lib/kitActivation.test.ts` that are about logic. The ones
//! that drew the view to markup are in `views/activation/tests.rs`, against
//! the view's model.

use hangar_core::kits::Hook;
use hangar_i18n::{Locale, Translator};

use super::*;
use crate::kits::model::{KIT_PERMISSIONS, kit_reads_hidden_files};
use crate::kits::testing::{
    COMMANDS, HOOKS, OTHER_COMMANDS, OTHER_HOOKS, command_kit, entry, folder, git_kit, inspection,
    installed, kit, native_tab, tab,
};
use crate::state::types::{Pane, Workspace};

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn say(locale: Locale) -> Translator {
    Translator::new(locale)
}

/// French typesetting puts no-break spaces before `;` and `:`; the strings are
/// written with plain ones.
fn plain_spaces(text: &str) -> String {
    text.replace(['\u{a0}', '\u{202f}'], " ")
}

// -- permissions --------------------------------------------------------------

#[test]
fn merges_the_tabs_requests_once_each_in_the_closed_set_s_order() {
    let asked = requested_permissions(&[
        tab(&["notify", "panes.read"]),
        tab(&["panes.read", "workspace.read"]),
    ]);
    assert_eq!(asked, strs(&["workspace.read", "panes.read", "notify"]));
}

#[test]
fn asks_for_nothing_when_no_tab_does() {
    assert!(requested_permissions(&[]).is_empty());
    assert!(requested_permissions(&[tab(&[])]).is_empty());
}

#[test]
fn puts_a_local_server_s_permissions_after_the_closed_set_by_port() {
    assert_eq!(
        requested_permissions(&[
            tab(&["loopback:9000", "notify"]),
            tab(&["loopback:8765", "data.write"])
        ]),
        strs(&["data.write", "notify", "loopback:8765", "loopback:9000"])
    );
}

#[test]
fn says_what_a_local_server_s_permission_means_with_its_port_in_both_languages() {
    assert_eq!(
        permission_label(say(Locale::En), "loopback:8765"),
        "talk to the local server on port 8765, and make it do whatever that server allows"
    );
    assert_eq!(
        permission_label(say(Locale::Fr), "loopback:8765"),
        "parler au serveur local sur le port 8765, et lui faire faire tout ce que ce serveur permet"
    );
    assert_eq!(
        permission_label(say(Locale::En), "panes.send"),
        "send messages to your agents"
    );
}

#[test]
fn says_what_changing_files_means_hidden_ones_named_in_both_languages() {
    assert_eq!(
        permission_label(say(Locale::En), "files.write"),
        "change text files in your project, hidden ones like .env included"
    );
    assert_eq!(
        permission_label(say(Locale::Fr), "files.write"),
        "modifier les fichiers texte de votre projet, y compris les fichiers cachés comme .env"
    );
}

#[test]
fn says_what_a_command_s_permission_lets_the_tab_do_by_its_name_in_both_languages() {
    assert_eq!(
        permission_label(say(Locale::En), "command:ci-run"),
        "run the command ci-run on your computer, with your logins"
    );
    assert_eq!(
        permission_label(say(Locale::Fr), "command:ci-run"),
        "lancer la commande ci-run sur votre ordinateur, avec vos identifiants"
    );
}

#[test]
fn splits_what_is_asked_by_what_was_accepted() {
    assert_eq!(
        permission_diff(&["panes.read", "panes.send"], &["panes.read", "notify"]),
        PermissionDiff {
            kept: strs(&["panes.read"]),
            added: strs(&["panes.send"])
        }
    );
}

#[test]
fn describes_every_permission_in_plain_words_in_both_languages() {
    for permission in KIT_PERMISSIONS {
        let (_, key) = PERMISSION_LABEL
            .iter()
            .find(|(name, _)| *name == permission)
            .unwrap_or_else(|| panic!("{permission} has no sentence"));
        for locale in [Locale::En, Locale::Fr] {
            let said = say(locale).t(*key);
            assert_ne!(said, key.id(), "{permission} in {locale:?}");
            assert!(!said.is_empty());
        }
    }
    assert_eq!(
        say(Locale::Fr).t(PERMISSION_LABEL[2].1),
        "envoyer des messages à vos agents"
    );
}

// -- an update that adds files.write -------------------------------------------

#[test]
fn an_update_that_adds_files_write_reads_to_review_until_it_is_accepted() {
    let held = entry("hello-kit", &["files.read", "notify"]);
    assert_eq!(
        kit_state(
            Some(&held),
            kit_requested(&kit("1.0.0", vec![tab(&["files.read", "notify"])])).as_deref()
        ),
        KitState::On
    );
    let updated = kit("1.1.0", vec![tab(&["files.read", "files.write", "notify"])]);
    let requested = kit_requested(&updated).expect("usable");
    assert_eq!(kit_state(Some(&held), Some(&requested)), KitState::Review);
    assert_eq!(
        permission_diff(&requested, &held.granted).added,
        strs(&["files.write"])
    );
    let accepted = entry("hello-kit", &["files.read", "notify", "files.write"]);
    assert_eq!(kit_state(Some(&accepted), Some(&requested)), KitState::On);
}

#[test]
fn an_update_that_adds_files_write_gives_the_tab_nothing_until_then() {
    // What a tab may call is what it asks for and was granted, both (P16's
    // `effectivePermissions`); the point here is the hidden files.
    let asked = strs(&["files.read", "files.write", "notify"]);
    let effective = |granted: &[&str]| {
        kit_permissions_in(asked.iter().filter(|a| granted.contains(&a.as_str())))
    };
    use crate::kits::model::kit_permissions_in;
    let before = effective(&["files.read", "notify"]);
    assert_eq!(before, strs(&["files.read", "notify"]));
    assert!(!kit_reads_hidden_files(&before));
    let after = effective(&["files.read", "notify", "files.write"]);
    assert!(kit_reads_hidden_files(&after));
}

// -- an update that adds permissions ---------------------------------------------

#[test]
fn an_update_that_adds_permissions_lists_the_added_ones() {
    let before = kit("1.0.0", vec![tab(&["panes.read"])]);
    let after = kit("1.1.0", vec![tab(&["panes.read", "panes.send"])]);
    assert_eq!(
        update_outcome(&before, &after),
        UpdateOutcome {
            from: "1.0.0".into(),
            to: "1.1.0".into(),
            new_permissions: strs(&["panes.send"])
        }
    );
}

#[test]
fn shows_the_workspace_that_accepted_the_old_ones_as_needing_a_review() {
    let held = entry("hello-kit", &["panes.read"]);
    assert_eq!(
        kit_state(Some(&held), Some(&strs(&["panes.read"]))),
        KitState::On
    );
    assert_eq!(
        kit_state(Some(&held), Some(&strs(&["panes.read", "panes.send"]))),
        KitState::Review
    );
}

#[test]
fn does_not_ask_again_for_permissions_the_kit_dropped() {
    let held = entry("hello-kit", &["panes.read", "notify"]);
    assert_eq!(
        kit_state(Some(&held), Some(&strs(&["panes.read"]))),
        KitState::On
    );
}

// -- hooks, in the permission diff ----------------------------------------------

#[test]
fn asks_for_a_github_kit_s_hooks_beside_its_tabs_permissions() {
    assert_eq!(
        kit_requested(&git_kit(Some(HOOKS), true)),
        Some(strs(&["panes.read", HOOKS]))
    );
}

#[test]
fn asks_nothing_for_a_kit_without_hooks_a_linked_folder_or_a_kit_from_before() {
    assert_eq!(
        kit_requested(&git_kit(None, true)),
        Some(strs(&["panes.read"]))
    );
    assert_eq!(
        kit_requested(&git_kit(Some(HOOKS), false)),
        Some(strs(&["panes.read"]))
    );
    let linked = KitInfo {
        source: folder(),
        ..git_kit(Some(HOOKS), true)
    };
    assert_eq!(kit_requested(&linked), Some(strs(&["panes.read"])));
    assert_eq!(
        kit_requested(&kit("1.0.0", vec![tab(&["panes.read"])])),
        Some(strs(&["panes.read"]))
    );
}

#[test]
fn a_kit_that_cannot_be_used_asks_for_nothing() {
    let mut broken = git_kit(Some(HOOKS), true);
    broken.error = Some("needs a newer Hangar".into());
    assert_eq!(kit_requested(&broken), None);
    broken.error = None;
    broken.manifest = None;
    assert_eq!(kit_requested(&broken), None);
}

#[test]
fn splits_hooks_like_any_permission_the_very_ones_accepted_are_kept_others_are_added() {
    let requested = strs(&["panes.read", HOOKS]);
    assert_eq!(
        permission_diff(&requested, &["panes.read", HOOKS]),
        PermissionDiff {
            kept: requested.clone(),
            added: vec![]
        }
    );
    let changed = permission_diff(&requested, &["panes.read", OTHER_HOOKS]);
    assert_eq!(
        changed,
        PermissionDiff {
            kept: strs(&["panes.read"]),
            added: strs(&[HOOKS])
        }
    );
    assert!(hooks_to_accept(&changed));
    assert!(!hooks_to_accept(&permission_diff(
        &["panes.read", "notify"],
        &["panes.read"]
    )));
}

#[test]
fn shows_a_workspace_that_accepted_other_hooks_or_none_as_to_review() {
    let requested = kit_requested(&git_kit(Some(HOOKS), true));
    let state =
        |granted: &[&str]| kit_state(Some(&entry("hello-kit", granted)), requested.as_deref());
    assert_eq!(state(&["panes.read", HOOKS]), KitState::On);
    assert_eq!(state(&["panes.read", OTHER_HOOKS]), KitState::Review);
    assert_eq!(state(&["panes.read"]), KitState::Review);
}

#[test]
fn leaves_a_workspace_as_it_was_when_the_kit_has_no_hooks() {
    let requested = kit_requested(&git_kit(None, true));
    assert_eq!(
        kit_state(
            Some(&entry("hello-kit", &["panes.read"])),
            requested.as_deref()
        ),
        KitState::On
    );
    // Hooks gone since they were accepted: nothing is lost by it.
    assert_eq!(
        kit_state(
            Some(&entry("hello-kit", &["panes.read", OTHER_HOOKS])),
            requested.as_deref()
        ),
        KitState::On
    );
}

#[test]
fn stores_what_the_person_was_shown_a_github_kit_s_hash_a_linked_folder_s_nothing() {
    let shown = hangar_core::kits::Inspection {
        hooks_permission: Some(HOOKS.into()),
        ..inspection(vec![tab(&["panes.read", "loopback:8765"])])
    };
    let git = git_kit(Some(HOOKS), true);
    assert_eq!(
        accepted_grants(&git.source, Some(&shown)),
        strs(&["panes.read", "loopback:8765", HOOKS])
    );
    // The kit changed under the open view: what was accepted is what was seen.
    let changed = git_kit(Some(OTHER_HOOKS), true);
    assert_eq!(
        accepted_grants(&changed.source, Some(&shown)),
        strs(&["panes.read", "loopback:8765", HOOKS])
    );
    assert_eq!(
        accepted_grants(&folder(), Some(&shown)),
        strs(&["panes.read", "loopback:8765"])
    );
    let none = hangar_core::kits::Inspection {
        hooks_permission: None,
        ..shown.clone()
    };
    assert_eq!(
        accepted_grants(&git.source, Some(&none)),
        strs(&["panes.read", "loopback:8765"])
    );
    assert!(accepted_grants(&git.source, None).is_empty());
}

#[test]
fn stays_held_after_accepting_hooks_the_kit_has_since_changed() {
    let shown = hangar_core::kits::Inspection {
        hooks_permission: Some(HOOKS.into()),
        ..inspection(vec![tab(&["panes.read"])])
    };
    let seen = accepted_grants(&git_kit(Some(HOOKS), true).source, Some(&shown));
    let stored = enable_kit(&[], "hello-kit", &seen);
    assert_eq!(stored, vec![entry("hello-kit", &["panes.read", HOOKS])]);
    assert_eq!(
        kit_state(
            stored.first(),
            kit_requested(&git_kit(Some(OTHER_HOOKS), true)).as_deref()
        ),
        KitState::Review
    );
}

#[test]
fn tells_an_update_that_changes_the_hooks_apart_from_one_that_does_not() {
    let hooks = |hash: Option<&str>| git_kit(hash, true);
    assert!(
        update_outcome(&hooks(Some(HOOKS)), &hooks(Some(HOOKS)))
            .new_permissions
            .is_empty()
    );
    assert_eq!(
        update_outcome(&hooks(Some(HOOKS)), &hooks(Some(OTHER_HOOKS))).new_permissions,
        strs(&[OTHER_HOOKS])
    );
    assert_eq!(
        update_outcome(&hooks(None), &hooks(Some(HOOKS))).new_permissions,
        strs(&[HOOKS])
    );
    assert!(
        update_outcome(&hooks(Some(HOOKS)), &hooks(None))
            .new_permissions
            .is_empty()
    );
}

// -- a kit's commands, in the permission diff --------------------------------------

fn github() -> Source {
    crate::kits::testing::git("https://github.com/o/kits", None)
}

#[test]
fn merges_the_tabs_commands_once_each_after_everything_else() {
    assert_eq!(
        requested_permissions(&[
            tab(&["command:b", "notify"]),
            tab(&["command:a", "loopback:8765", "command:b"])
        ]),
        strs(&["notify", "loopback:8765", "command:a", "command:b"])
    );
}

#[test]
fn asks_for_a_github_kit_s_commands_beside_its_tabs_permissions() {
    assert_eq!(
        kit_requested(&command_kit(Some(COMMANDS), github())),
        Some(strs(&["command:ci-run", COMMANDS]))
    );
}

#[test]
fn asks_nothing_for_a_kit_without_commands_or_a_linked_folder() {
    assert_eq!(
        kit_requested(&command_kit(None, github())),
        Some(strs(&["command:ci-run"]))
    );
    assert_eq!(
        kit_requested(&command_kit(Some(COMMANDS), folder())),
        Some(strs(&["command:ci-run"]))
    );
}

#[test]
fn splits_commands_like_any_permission() {
    let requested = strs(&["command:ci-run", COMMANDS]);
    assert_eq!(
        permission_diff(&requested, &["command:ci-run", COMMANDS]),
        PermissionDiff {
            kept: requested.clone(),
            added: vec![]
        }
    );
    let changed = permission_diff(&requested, &["command:ci-run", OTHER_COMMANDS]);
    assert_eq!(
        changed,
        PermissionDiff {
            kept: strs(&["command:ci-run"]),
            added: strs(&[COMMANDS])
        }
    );
    assert!(commands_to_accept(&changed));
    assert!(!hooks_to_accept(&changed));
    assert!(!commands_to_accept(&permission_diff(
        &["notify"],
        &[] as &[&str]
    )));
    assert!(!commands_to_accept(&permission_diff(
        &[HOOKS],
        &[] as &[&str]
    )));
}

#[test]
fn tells_a_kit_waiting_for_its_commands_alone_from_one_waiting_for_more() {
    let none: &[&str] = &[];
    assert!(only_commands_to_accept(&permission_diff(&[COMMANDS], none)));
    assert!(only_commands_to_accept(&permission_diff(
        &["notify", COMMANDS],
        &["notify"]
    )));
    assert!(!only_commands_to_accept(&permission_diff(
        &["notify", COMMANDS],
        none
    )));
    assert!(!only_commands_to_accept(&permission_diff(
        &[HOOKS, COMMANDS],
        none
    )));
    assert!(!only_commands_to_accept(&permission_diff(
        &["notify"],
        &["notify"]
    )));
}

#[test]
fn shows_a_workspace_that_accepted_other_commands_or_none_as_to_review() {
    let requested = kit_requested(&command_kit(Some(COMMANDS), github()));
    let state =
        |granted: &[&str]| kit_state(Some(&entry("hello-kit", granted)), requested.as_deref());
    assert_eq!(state(&["command:ci-run", COMMANDS]), KitState::On);
    assert_eq!(state(&["command:ci-run", OTHER_COMMANDS]), KitState::Review);
    assert_eq!(state(&["command:ci-run"]), KitState::Review);
    // A linked folder is on whatever its commands became.
    assert_eq!(
        kit_state(
            Some(&entry("hello-kit", &["command:ci-run"])),
            kit_requested(&command_kit(Some(COMMANDS), folder())).as_deref()
        ),
        KitState::On
    );
}

#[test]
fn stores_what_the_person_was_shown_for_commands_too() {
    let shown = hangar_core::kits::Inspection {
        hooks_permission: Some(HOOKS.into()),
        commands_permission: Some(COMMANDS.into()),
        ..inspection(vec![tab(&["command:ci-run"])])
    };
    let kit = command_kit(Some(COMMANDS), github());
    assert_eq!(
        accepted_grants(&kit.source, Some(&shown)),
        strs(&["command:ci-run", HOOKS, COMMANDS])
    );
    // The kit changed under the open view: what was accepted is what was seen.
    let changed = command_kit(Some(OTHER_COMMANDS), github());
    assert_eq!(
        accepted_grants(&changed.source, Some(&shown)),
        strs(&["command:ci-run", HOOKS, COMMANDS])
    );
    assert_eq!(
        accepted_grants(&folder(), Some(&shown)),
        strs(&["command:ci-run"])
    );
    let none = hangar_core::kits::Inspection {
        hooks_permission: None,
        commands_permission: None,
        ..shown
    };
    assert_eq!(
        accepted_grants(&kit.source, Some(&none)),
        strs(&["command:ci-run"])
    );
}

#[test]
fn stays_to_review_after_accepting_commands_the_kit_has_since_changed() {
    let shown = hangar_core::kits::Inspection {
        commands_permission: Some(COMMANDS.into()),
        ..inspection(vec![tab(&["command:ci-run"])])
    };
    let seen = accepted_grants(&github(), Some(&shown));
    let stored = enable_kit(&[], "hello-kit", &seen);
    assert_eq!(
        stored,
        vec![entry("hello-kit", &["command:ci-run", COMMANDS])]
    );
    assert_eq!(
        kit_state(
            stored.first(),
            kit_requested(&command_kit(Some(OTHER_COMMANDS), github())).as_deref()
        ),
        KitState::Review
    );
}

#[test]
fn tells_an_update_that_changes_the_commands_apart_from_one_that_does_not() {
    let commands = |hash: Option<&str>| command_kit(hash, github());
    assert!(
        update_outcome(&commands(Some(COMMANDS)), &commands(Some(COMMANDS)))
            .new_permissions
            .is_empty()
    );
    assert_eq!(
        update_outcome(&commands(Some(COMMANDS)), &commands(Some(OTHER_COMMANDS))).new_permissions,
        strs(&[OTHER_COMMANDS])
    );
    assert_eq!(
        update_outcome(&commands(None), &commands(Some(COMMANDS))).new_permissions,
        strs(&[COMMANDS])
    );
}

// -- an argv, as the person reads it -------------------------------------------------

#[test]
fn an_argv_is_written_as_it_is_placeholders_included() {
    assert_eq!(
        kit_argv_line(&[
            "gh",
            "run",
            "view",
            "{run}",
            "--repo={repo}",
            "--json",
            "status"
        ]),
        "gh run view {run} --repo={repo} --json status"
    );
    assert_eq!(
        kit_argv_line(&["docker", "ps", "--format", "{{.Names}}", "C:\\tools"]),
        "docker ps --format {{.Names}} C:\\tools"
    );
}

#[test]
fn an_argv_does_not_let_one_item_read_as_two_or_two_as_one() {
    assert_eq!(kit_argv_line(&["gh", "run view"]), "gh \"run view\"");
    assert_eq!(kit_argv_line(&["gh", "", "x"]), "gh \"\" x");
    assert_eq!(kit_argv_line(&["gh", "a\"b"]), "gh \"a\\\"b\"");
    assert_eq!(kit_argv_line(&["gh", "it's"]), "gh \"it's\"");
    assert_eq!(kit_argv_line(&["gh", "a\nb"]), "gh \"a\\nb\"");
}

#[test]
fn an_argv_writes_out_what_would_not_show_or_would_turn_the_line_around() {
    assert_eq!(
        kit_argv_line(&["gh", "safe\u{202E}mr -"]),
        "gh \"safe\\u{202e}mr -\""
    );
    assert_eq!(kit_argv_line(&["gh\u{200B}", "x"]), "\"gh\\u{200b}\" x");
    assert_eq!(kit_rule_text("[a-z]+\u{202E}"), "[a-z]+\\u{202e}");
    assert_eq!(kit_rule_text("^[\\w.-]+/[\\w.-]+$"), "^[\\w.-]+/[\\w.-]+$");
    // The line and paragraph separators too: the web app let them through.
    assert_eq!(kit_rule_text("a\u{2028}b"), "a\\u{2028}b");
}

// -- what runs by itself --------------------------------------------------------------

fn hook(event: &str, matcher: &str, command: &str) -> Hook {
    Hook {
        event: event.into(),
        matcher: matcher.into(),
        command: command.into(),
    }
}

fn item(name: &str) -> RunItem {
    RunItem {
        name: name.into(),
        ..RunItem::default()
    }
}

#[test]
fn the_groups_go_in_the_view_s_order_hooks_modules_first() {
    let groups = running_groups_of(
        &[
            hook("Stop", "", "x.sh"),
            hook("modules", "", "./register.js"),
        ],
        [
            (RunGroupKind::Unknown, item("z")),
            (
                RunGroupKind::Lsp,
                RunItem {
                    argv: strs(&["l"]),
                    ..item("l")
                },
            ),
            (
                RunGroupKind::Mcp,
                RunItem {
                    argv: strs(&["a"]),
                    ..item("a")
                },
            ),
            (
                RunGroupKind::Mcp,
                RunItem {
                    text: Some("https://b".into()),
                    ..item("b")
                },
            ),
        ],
    );
    let kinds: Vec<_> = groups.iter().map(|group| group.kind).collect();
    assert_eq!(
        kinds,
        [
            RunGroupKind::Module,
            RunGroupKind::Mcp,
            RunGroupKind::Lsp,
            RunGroupKind::Unknown
        ]
    );
    assert_eq!(
        groups[0].items,
        vec![RunItem {
            name: "modules".into(),
            text: Some("./register.js".into()),
            ..RunItem::default()
        }]
    );
    let names: Vec<_> = groups[1].items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["a", "b"]);
}

#[test]
fn the_groups_are_empty_for_a_kit_that_runs_nothing_or_before_it_is_read() {
    assert!(kit_running_groups(None).is_empty());
    assert!(running_groups_of(&[], []).is_empty());
    assert!(kit_running_groups(Some(&inspection(vec![]))).is_empty());
}

#[test]
fn keeps_the_hooks_proper_apart_from_the_modules() {
    let shown = hangar_core::kits::Inspection {
        hooks: vec![
            hook("Stop", "", "x.sh"),
            hook("modules", "", "./register.js"),
        ],
        ..inspection(vec![])
    };
    let plain = kit_plain_hooks(Some(&shown));
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].event, "Stop");
    assert!(kit_plain_hooks(None).is_empty());
}

#[test]
fn writes_an_item_s_line_as_it_is_written_what_would_not_show_written_out() {
    let line = |item: &RunItem| kit_running_line(item);
    let argv = RunItem {
        argv: strs(&["node", "a b"]),
        ..RunItem::default()
    };
    assert_eq!(line(&argv).as_deref(), Some("node \"a b\""));
    let text = |text: &str| RunItem {
        text: Some(text.into()),
        ..RunItem::default()
    };
    assert_eq!(
        line(&text("tail -F 'x y' | grep z")).as_deref(),
        Some("tail -F 'x y' | grep z")
    );
    assert_eq!(
        line(&text("safe\u{202E}rm -")).as_deref(),
        Some("safe\\u{202e}rm -")
    );
    assert_eq!(
        line(&text("hooks:\n  Stop: x\u{200B}")).as_deref(),
        Some("hooks:\n  Stop: x\\u{200b}")
    );
    assert_eq!(line(&RunItem::default()), None);
}

#[test]
fn has_a_title_and_a_sentence_for_each_kind() {
    for kind in RUNNING_GROUP_ORDER {
        let (title, when) = kind.label();
        for locale in [Locale::En, Locale::Fr] {
            assert_ne!(say(locale).t(title), title.id(), "{kind:?}");
            assert_ne!(say(locale).t(when), when.id(), "{kind:?}");
        }
    }
}

#[test]
fn turns_core_s_kinds_into_the_view_s_groups() {
    assert_eq!(
        RunGroupKind::from(RunKind::SkillHook),
        RunGroupKind::SkillHook
    );
    assert_eq!(RunGroupKind::from(RunKind::Unknown), RunGroupKind::Unknown);
    // Every kind core has is in the order, so none is left out of the view.
    for kind in [
        RunKind::Mcp,
        RunKind::Monitor,
        RunKind::Lsp,
        RunKind::SkillHook,
        RunKind::Settings,
        RunKind::Grammar,
        RunKind::Bin,
        RunKind::Binary,
        RunKind::Dependency,
        RunKind::Unknown,
    ] {
        assert!(RUNNING_GROUP_ORDER.contains(&RunGroupKind::from(kind)));
    }
}

// -- the diff, when a hash covers more than the hooks ------------------------------------

#[test]
fn asks_again_when_what_else_runs_changed_with_the_same_permissions() {
    let requested = strs(&["panes.read", OTHER_HOOKS]);
    let diff = permission_diff(&requested, &["panes.read", HOOKS]);
    assert_eq!(diff.kept, strs(&["panes.read"]));
    assert_eq!(diff.added, strs(&[OTHER_HOOKS]));
    assert!(hooks_to_accept(&diff));
}

#[test]
fn does_not_ask_when_the_workspace_holds_the_current_hash() {
    let requested = strs(&["panes.read", OTHER_HOOKS]);
    let diff = permission_diff(&requested, &["panes.read", OTHER_HOOKS]);
    assert!(diff.added.is_empty());
    assert!(!hooks_to_accept(&diff));
}

#[test]
fn reads_as_to_review_and_then_as_on() {
    let asked = kit_requested(&git_kit(Some(OTHER_HOOKS), true));
    let state = |granted: &[&str]| kit_state(Some(&entry("hello-kit", granted)), asked.as_deref());
    assert_eq!(state(&["panes.read", HOOKS]), KitState::Review);
    assert_eq!(state(&["panes.read", OTHER_HOOKS]), KitState::On);
}

// -- kitState, enabling, restarting, labels -----------------------------------------------

#[test]
fn a_kit_is_off_without_an_entry_and_broken_when_it_cannot_be_used() {
    assert_eq!(kit_state(None, Some(&[])), KitState::Off);
    assert_eq!(
        kit_state(Some(&entry("hello-kit", &[])), None),
        KitState::Broken
    );
}

#[test]
fn adds_a_kit_with_exactly_the_accepted_permissions_and_replaces_a_grant() {
    let one = enable_kit(&[], "hello-kit", &strs(&["notify"]));
    assert_eq!(one, vec![entry("hello-kit", &["notify"])]);
    assert_eq!(
        enable_kit(&one, "hello-kit", &strs(&["notify", "panes.read"])),
        vec![entry("hello-kit", &["notify", "panes.read"])]
    );
}

#[test]
fn removes_only_the_named_kit() {
    let kits = vec![entry("a-kit", &[]), entry("b-kit", &[])];
    assert_eq!(disable_kit(&kits, "a-kit"), vec![entry("b-kit", &[])]);
}

#[test]
fn compares_two_lists_by_content_not_by_order() {
    assert!(same_kits(
        &[entry("a-kit", &["notify", "panes.read"])],
        &[entry("a-kit", &["panes.read", "notify"])]
    ));
    assert!(!same_kits(&[entry("a-kit", &[])], &[]));
}

#[test]
fn restarts_the_claude_code_panes_only() {
    let pane = |id: &str, agent: AgentId| Pane {
        id: id.into(),
        name: id.into(),
        agent,
        ..Pane::default()
    };
    let ws = Workspace {
        panes: vec![
            pane("a", AgentId::Claude),
            pane("b", AgentId::Shell),
            pane("c", AgentId::Shell),
            pane("d", AgentId::Claude),
        ],
        ..Workspace::default()
    };
    assert_eq!(panes_to_restart(&ws), strs(&["a", "d"]));
}

#[test]
fn names_a_kit_by_its_manifest_or_by_its_id_when_it_is_unreadable() {
    assert_eq!(kit_name(&kit("1.0.0", vec![])), "Hello kit");
    assert_eq!(kit_name(&installed("broken-kit")), "broken-kit");
}

#[test]
fn writes_a_source_in_one_line() {
    assert_eq!(
        source_line(&folder()),
        (SourceKind::Folder, "D:\\kits\\hello-kit".to_owned())
    );
    let url = "https://github.com/o/r";
    assert_eq!(
        source_line(&crate::kits::testing::git(url, None)).1,
        "https://github.com/o/r"
    );
    let with_place = Source::Git(hangar_core::kits::GitSource {
        url: url.into(),
        git_ref: Some("main".into()),
        subdir: Some("roblox".into()),
    });
    assert_eq!(
        source_line(&with_place).1,
        "https://github.com/o/r (main · roblox)"
    );
}

#[test]
fn a_native_tab_asks_the_same_way_an_html_tab_does() {
    assert_eq!(
        requested_permissions(&[native_tab(&["panes.read"]), tab(&["notify"])]),
        strs(&["panes.read", "notify"])
    );
}

#[test]
fn plain_spaces_helper_is_the_one_the_french_cases_use() {
    assert_eq!(plain_spaces("a\u{202f}: b"), "a : b");
}
