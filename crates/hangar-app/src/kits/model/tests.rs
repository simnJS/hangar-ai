//! The cases of `lib/kits.test.ts`, one test per `it`.

use serde_json::json;

use hangar_core::kits::{IssueReason, LaunchIssue};
use hangar_i18n::{Locale, Translator};

use super::*;
use crate::kits::testing::{COMMANDS, HOOKS, OTHER_HOOKS, entry, folder, git, installed};
use crate::state::types::{Settings, Workspace};

fn ws(kits: Vec<WorkspaceKit>) -> Workspace {
    Workspace {
        kits: Some(kits),
        ..Workspace::default()
    }
}

fn settings(kits: Vec<WorkspaceKit>) -> Settings {
    Settings {
        kits_everywhere: Some(kits),
        ..Settings::default()
    }
}

fn ids(kits: &[WorkspaceKit]) -> Vec<&str> {
    kits.iter().map(|kit| kit.id.as_str()).collect()
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// -- workspaceKits ------------------------------------------------------------

#[test]
fn reads_the_kits_a_workspace_enabled_with_what_they_were_granted() {
    let w = ws(vec![
        entry("roblox", &["panes.read", "notify"]),
        entry("unreal-5", &[]),
    ]);
    assert_eq!(
        workspace_kits(Some(&w)),
        vec![
            entry("roblox", &["panes.read", "notify"]),
            entry("unreal-5", &[])
        ]
    );
}

#[test]
fn reads_a_workspace_saved_before_kits_or_without_any_as_none() {
    assert!(workspace_kits(Some(&Workspace::default())).is_empty());
    assert!(workspace_kits(None).is_empty());
    // What a hand-edited file may hold in the place of the list.
    assert!(read_kit_entries_json(&json!({ "roblox": true })).is_empty());
    assert!(read_kit_entries_json(&json!("roblox")).is_empty());
    assert!(read_kit_entries_json(&json!(null)).is_empty());
}

#[test]
fn takes_anything_malformed_as_no_kit() {
    let raw = json!([
        null,
        "roblox",
        { "id": "roblox" },
        { "id": "Roblox", "granted": [] },
        { "id": "../escape", "granted": [] },
        { "id": "r", "granted": [] },
        { "id": 42, "granted": [] },
        { "id": "studio", "granted": "panes.read" },
        { "id": "ok", "granted": [] },
        { "id": "ok", "granted": ["notify"] },
    ]);
    assert_eq!(read_kit_entries_json(&raw), vec![entry("ok", &[])]);
}

#[test]
fn never_grants_a_permission_hangar_does_not_know() {
    let raw = json!([{ "id": "roblox", "granted": ["notify", "files.delete", 3, "notify"] }]);
    assert_eq!(
        read_kit_entries_json(&raw),
        vec![entry("roblox", &["notify"])]
    );
}

#[test]
fn keeps_a_local_server_s_permission_and_accepted_hooks_and_only_well_formed_ones() {
    let w = ws(vec![entry(
        "roblox",
        &[
            "data.read",
            "loopback:8765",
            "hooks:0123456789abcdef",
            "loopback:80",
            "loopback:abc",
            "hooks:short",
            "hooks:0123456789ABCDEF",
        ],
    )]);
    assert_eq!(
        workspace_kits(Some(&w)),
        vec![entry(
            "roblox",
            &["data.read", "loopback:8765", "hooks:0123456789abcdef"]
        )]
    );
}

// -- permissions ----------------------------------------------------------------

#[test]
fn reads_a_local_server_s_port_as_the_manifest_does() {
    assert_eq!(kit_loopback_port("loopback:8765"), Some(8765));
    assert_eq!(kit_loopback_port("loopback:1024"), Some(1024));
    assert_eq!(kit_loopback_port("loopback:65535"), Some(65535));
    for name in [
        "loopback:80",
        "loopback:1023",
        "loopback:0",
        "loopback:abc",
        "loopback:65536",
        "loopback:",
        "loopback:08765",
        "loopback:+8765",
        "loopback:8765 ",
        "Loopback:8765",
        "loopback:99999999999999999999999",
        "notify",
    ] {
        assert_eq!(kit_loopback_port(name), None, "{name}");
    }
}

#[test]
fn knows_the_closed_set_and_the_pattern_and_hooks_apart_from_both() {
    assert!(is_kit_permission("data.read"));
    assert!(is_kit_permission("data.write"));
    assert!(is_kit_permission("loopback:8765"));
    assert!(!is_kit_permission("loopback:80"));
    assert!(is_kit_permission("files.write"));
    assert!(!is_kit_permission("files.delete"));
    assert!(!is_kit_permission("hooks:0123456789abcdef"));
    assert!(is_kit_hooks_grant("hooks:0123456789abcdef"));
    assert!(!is_kit_hooks_grant("hooks:0123"));
    assert!(!is_kit_hooks_grant(""));
}

#[test]
fn orders_them_the_closed_set_s_way_then_by_port() {
    assert_eq!(
        kit_permissions_in([
            "loopback:9000",
            "notify",
            "hooks:0123456789abcdef",
            "loopback:8765",
            "files.delete",
            "panes.read",
            "notify",
        ]),
        strs(&["panes.read", "notify", "loopback:8765", "loopback:9000"])
    );
}

#[test]
fn names_a_command_by_the_rule_the_manifest_holds() {
    assert_eq!(kit_command_name("command:ci-status"), Some("ci-status"));
    assert_eq!(kit_command_name("command:r"), Some("r"));
    assert_eq!(kit_command_name("command:ci-2"), Some("ci-2"));
    for name in [
        "command:",
        "command:CI",
        "command:1ci",
        "command:-ci",
        "command:ci status",
        "command:ci_status",
        "command:ci.status",
        "command:ci-status ",
        "commands:0123456789abcdef",
        "ci-status",
        KIT_COMMAND_PATTERN,
    ] {
        assert_eq!(kit_command_name(name), None, "{name}");
    }
    assert!(is_kit_command_name("ci-status"));
    assert!(!is_kit_command_name("CI"));
    assert!(!is_kit_command_name(""));
}

#[test]
fn knows_a_command_s_permission_and_the_grant_of_a_kit_s_commands_apart_from_it() {
    assert!(is_kit_permission("command:ci-status"));
    assert!(!is_kit_permission("command:"));
    assert!(!is_kit_permission("commands:0123456789abcdef"));
    assert!(is_kit_commands_grant("commands:0123456789abcdef"));
    assert!(!is_kit_commands_grant("command:ci-status"));
    assert!(!is_kit_commands_grant("commands:0123"));
    assert!(!is_kit_commands_grant("commands:0123456789ABCDEF"));
    assert!(!is_kit_commands_grant("hooks:0123456789abcdef"));
    assert!(!is_kit_hooks_grant("commands:0123456789abcdef"));
}

#[test]
fn orders_commands_last_by_name_and_leaves_the_grant_of_a_kit_s_commands_out() {
    assert_eq!(
        kit_permissions_in([
            "command:view",
            "loopback:8765",
            "commands:0123456789abcdef",
            "command:list",
            "notify",
            "command:Bad",
            "command:view",
        ]),
        strs(&["notify", "loopback:8765", "command:list", "command:view"])
    );
}

// -- a kit's commands, on a workspace ---------------------------------------------

#[test]
fn keeps_what_was_accepted_through_the_defensive_reader_on_a_workspace_and_for_all_of_them() {
    let raw = entry(
        "roblox",
        &["command:view", COMMANDS, "command:Bad", "commands:nope"],
    );
    let kept = vec![entry("roblox", &["command:view", COMMANDS])];
    assert_eq!(workspace_kits(Some(&ws(vec![raw.clone()]))), kept);
    assert_eq!(everywhere_kits(Some(&settings(vec![raw.clone()]))), kept);
    assert_eq!(
        effective_kits(Some(&Workspace::default()), Some(&settings(vec![raw]))),
        kept
    );
}

#[test]
fn asks_a_github_kit_that_has_commands_and_no_other() {
    use crate::kits::testing::command_kit;
    let git_kit = command_kit(Some(COMMANDS), git("https://github.com/o/kits", None));
    assert_eq!(kit_commands_grant(&git_kit), Some(COMMANDS));
    let none = command_kit(None, git("https://github.com/o/kits", None));
    assert_eq!(kit_commands_grant(&none), None);
    let nonsense = command_kit(Some("nonsense"), git("https://github.com/o/kits", None));
    assert_eq!(kit_commands_grant(&nonsense), None);
    assert_eq!(
        kit_commands_grant(&command_kit(Some(COMMANDS), folder())),
        None
    );
}

#[test]
fn does_not_hold_a_kit_out_of_the_panes_for_its_commands_only_its_tabs_wait() {
    let mut kit = installed("roblox");
    kit.commands_permission = Some(COMMANDS.into());
    let w = ws(vec![entry("roblox", &[])]);
    assert_eq!(
        pane_kits(Some(&w), Some(&[kit]), None),
        PaneKits {
            ids: strs(&["roblox"]),
            held: vec![]
        }
    );
}

// -- the kits a pane starts with ----------------------------------------------------

fn consenting(id: &str, hooks: &str) -> KitInfo {
    KitInfo {
        hooks_permission: Some(hooks.into()),
        hooks_consent: true,
        ..installed(id)
    }
}

#[test]
fn the_kits_a_pane_starts_with_are_its_own_workspace_s_and_no_other_s() {
    let roblox = ws(vec![entry("roblox", &[])]);
    let other = Workspace::default();
    assert_eq!(workspace_kit_ids(Some(&roblox), None), strs(&["roblox"]));
    assert!(workspace_kit_ids(Some(&other), None).is_empty());
    assert_eq!(
        pane_kits(Some(&roblox), Some(&[]), None),
        PaneKits {
            ids: strs(&["roblox"]),
            held: vec![]
        }
    );
    assert_eq!(
        pane_kits(Some(&other), Some(&[]), None),
        PaneKits::default()
    );
}

#[test]
fn leave_out_a_github_kit_whose_hooks_changed_since_they_were_accepted() {
    let w = ws(vec![entry("studio", &["panes.read", HOOKS])]);
    assert_eq!(
        pane_kits(Some(&w), Some(&[consenting("studio", HOOKS)]), None),
        PaneKits {
            ids: strs(&["studio"]),
            held: vec![]
        }
    );
    assert_eq!(
        pane_kits(Some(&w), Some(&[consenting("studio", OTHER_HOOKS)]), None),
        PaneKits {
            ids: vec![],
            held: strs(&["studio"])
        }
    );
}

#[test]
fn leave_out_a_github_kit_whose_hooks_were_never_accepted_here() {
    let w = ws(vec![entry("studio", &["panes.read"])]);
    assert_eq!(
        pane_kits(Some(&w), Some(&[consenting("studio", HOOKS)]), None),
        PaneKits {
            ids: vec![],
            held: strs(&["studio"])
        }
    );
}

#[test]
fn leave_out_a_github_kit_accepted_before_the_hash_covered_what_else_it_starts_once() {
    // Accepted on 1.8.0, with its hooks' hash; it also has an MCP server, so its hash moved.
    let w = ws(vec![entry("studio", &["panes.read", HOOKS])]);
    let moved = consenting("studio", OTHER_HOOKS);
    assert_eq!(
        pane_kits(Some(&w), Some(std::slice::from_ref(&moved)), None),
        PaneKits {
            ids: vec![],
            held: strs(&["studio"])
        }
    );
    let accepted = ws(vec![entry("studio", &["panes.read", OTHER_HOOKS])]);
    assert_eq!(
        pane_kits(Some(&accepted), Some(&[moved]), None),
        PaneKits {
            ids: strs(&["studio"]),
            held: vec![]
        }
    );
}

#[test]
fn keep_a_github_kit_that_only_has_hooks_its_hash_did_not_move() {
    let w = ws(vec![entry("studio", &["panes.read", HOOKS])]);
    assert_eq!(
        pane_kits(Some(&w), Some(&[consenting("studio", HOOKS)]), None).held,
        Vec::<String>::new()
    );
}

#[test]
fn keep_a_linked_folder_kit_whatever_its_hooks_the_person_is_writing_it() {
    let w = ws(vec![entry("studio", &[])]);
    let folder_kit = KitInfo {
        source: folder(),
        ..consenting("studio", HOOKS)
    };
    assert_eq!(kit_hooks_grant(&folder_kit), None);
    assert_eq!(
        pane_kits(Some(&w), Some(&[folder_kit]), None),
        PaneKits {
            ids: strs(&["studio"]),
            held: vec![]
        }
    );
}

#[test]
fn keep_a_kit_without_hooks_it_stores_nothing_and_behaves_as_before() {
    let w = ws(vec![entry("studio", &["notify"])]);
    let kit = KitInfo {
        hooks_permission: None,
        hooks_consent: true,
        ..installed("studio")
    };
    assert_eq!(kit_hooks_grant(&kit), None);
    assert_eq!(
        pane_kits(Some(&w), Some(&[kit]), None).ids,
        strs(&["studio"])
    );
}

#[test]
fn keep_a_github_kit_installed_before_this_was_asked_until_an_update_changes_its_hooks() {
    let w = ws(vec![entry("studio", &["notify"])]);
    let before = KitInfo {
        hooks_permission: Some(HOOKS.into()),
        hooks_consent: false,
        ..installed("studio")
    };
    assert_eq!(
        pane_kits(Some(&w), Some(std::slice::from_ref(&before)), None).ids,
        strs(&["studio"])
    );
    // Nor does a list without these fields, from a backend that predates them.
    assert_eq!(
        pane_kits(Some(&w), Some(&[installed("studio")]), None).ids,
        strs(&["studio"])
    );
    let changed = KitInfo {
        hooks_permission: Some(OTHER_HOOKS.into()),
        hooks_consent: true,
        ..before
    };
    assert_eq!(
        pane_kits(Some(&w), Some(&[changed]), None),
        PaneKits {
            ids: vec![],
            held: strs(&["studio"])
        }
    );
}

#[test]
fn pass_on_a_kit_that_is_missing_or_broken_for_rust_to_say_so() {
    let w = ws(vec![entry("gone", &[]), entry("bad", &[])]);
    let broken = KitInfo {
        error: Some("no manifest".into()),
        hooks_permission: None,
        hooks_consent: true,
        ..installed("bad")
    };
    assert_eq!(
        pane_kits(Some(&w), Some(&[broken]), None),
        PaneKits {
            ids: strs(&["gone", "bad"]),
            held: vec![]
        }
    );
}

#[test]
fn hold_out_one_kit_and_keep_the_others_in_their_order() {
    let w = ws(vec![
        entry("first", &[]),
        entry("held", &[OTHER_HOOKS]),
        entry("last", &[]),
    ]);
    let kits = [
        installed("first"),
        consenting("held", HOOKS),
        KitInfo {
            source: folder(),
            hooks_permission: Some(HOOKS.into()),
            ..installed("last")
        },
    ];
    assert_eq!(
        pane_kits(Some(&w), Some(&kits), None),
        PaneKits {
            ids: strs(&["first", "last"]),
            held: strs(&["held"])
        }
    );
}

// -- read from the kits on disk -------------------------------------------------------

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(future)
}

#[test]
fn reads_nothing_for_a_workspace_without_kits_or_a_pane_that_loads_none() {
    let asked = std::cell::Cell::new(false);
    let list = || {
        asked.set(true);
        async { Ok(Vec::new()) }
    };
    assert_eq!(
        block_on(load_pane_kits_with(Some(&Workspace::default()), None, list)),
        PaneKits::default()
    );
    assert_eq!(
        block_on(load_pane_kits_with(None, None, list)),
        PaneKits::default()
    );
    assert!(!asked.get());
}

#[test]
fn holds_out_what_the_kits_say_now() {
    let w = ws(vec![entry("studio", &[])]);
    let held = block_on(load_pane_kits_with(Some(&w), None, || async {
        Ok(vec![consenting("studio", HOOKS)])
    }));
    assert_eq!(
        held,
        PaneKits {
            ids: vec![],
            held: strs(&["studio"])
        }
    );
}

#[test]
fn hands_the_ids_on_when_the_kits_cannot_be_read_rust_keeps_them_out_and_says_why() {
    let w = ws(vec![entry("studio", &[])]);
    let passed = block_on(load_pane_kits_with(Some(&w), None, || async {
        Err("the kits index cannot be read".to_owned())
    }));
    assert_eq!(
        passed,
        PaneKits {
            ids: strs(&["studio"]),
            held: vec![]
        }
    );
}

// -- the kits enabled on all workspaces ----------------------------------------------

fn everywhere() -> Settings {
    settings(vec![
        entry("ci", &["notify"]),
        entry("studio", &["panes.read"]),
    ])
}

#[test]
fn everywhere_kits_are_added_to_a_workspace_s_own_which_come_first() {
    let w = ws(vec![entry("roblox", &[])]);
    let every = everywhere();
    assert_eq!(
        ids(&effective_kits(Some(&w), Some(&every))),
        ["roblox", "ci", "studio"]
    );
    assert_eq!(ids(&workspace_kits(Some(&w))), ["roblox"]);
    assert_eq!(
        ids(&effective_kits(Some(&Workspace::default()), Some(&every))),
        ["ci", "studio"]
    );
    assert_eq!(ids(&effective_kits(None, Some(&every))), ["ci", "studio"]);
}

#[test]
fn everywhere_kits_lose_to_a_workspace_s_own_entry_for_the_same_kit_grants_included() {
    let w = ws(vec![entry("ci", &[])]);
    assert_eq!(
        effective_kits(Some(&w), Some(&everywhere())),
        vec![entry("ci", &[]), entry("studio", &["panes.read"])]
    );
}

#[test]
fn everywhere_reads_as_none_when_malformed_or_absent() {
    for bad in [
        json!(null),
        json!("ci"),
        json!(3),
        json!({ "ci": true }),
        json!([null, 4, "ci", { "id": "ci" }, { "id": "X", "granted": [] }, { "id": "ci", "granted": "notify" }]),
    ] {
        assert!(read_kit_entries_json(&bad).is_empty(), "{bad}");
    }
    assert!(everywhere_kits(None).is_empty());
    assert!(everywhere_kits(Some(&Settings::default())).is_empty());
    assert_eq!(
        effective_kits(
            Some(&ws(vec![entry("roblox", &[])])),
            Some(&Settings::default())
        ),
        vec![entry("roblox", &[])]
    );
}

#[test]
fn everywhere_keeps_the_good_entries_of_a_half_broken_list_once_each_without_unknown_permissions() {
    let raw = json!([
        { "id": "ci", "granted": ["notify", "files.delete", "notify"] },
        { "id": "ci", "granted": ["panes.read"] },
        null,
    ]);
    assert_eq!(read_kit_entries_json(&raw), vec![entry("ci", &["notify"])]);
}

#[test]
fn everywhere_kits_are_loaded_by_a_pane_of_any_workspace() {
    let kits = [installed("ci"), installed("studio")];
    let every = everywhere();
    for w in [Workspace::default(), ws(vec![entry("roblox", &[])])] {
        let found = pane_kits(Some(&w), Some(&kits), Some(&every)).ids;
        assert!(found.contains(&"ci".to_owned()));
        assert!(found.contains(&"studio".to_owned()));
    }
    assert_eq!(
        pane_kits(Some(&Workspace::default()), Some(&kits), None),
        PaneKits::default()
    );
    assert_eq!(
        workspace_kit_ids(Some(&Workspace::default()), Some(&every)),
        strs(&["ci", "studio"])
    );
}

#[test]
fn everywhere_holds_back_a_kit_whose_hooks_changed_on_every_workspace() {
    let s = settings(vec![entry("ci", &[HOOKS])]);
    let same = consenting("ci", HOOKS);
    let changed = consenting("ci", OTHER_HOOKS);
    let own = ws(vec![entry("roblox", &[])]);
    for w in [Workspace::default(), own] {
        let kept = pane_kits(Some(&w), Some(std::slice::from_ref(&same)), Some(&s));
        assert!(kept.held.is_empty());
        assert!(kept.ids.contains(&"ci".to_owned()));
        let held = pane_kits(Some(&w), Some(std::slice::from_ref(&changed)), Some(&s));
        assert_eq!(held.held, strs(&["ci"]));
        assert!(!held.ids.contains(&"ci".to_owned()));
    }
}

#[test]
fn a_workspace_s_own_accepted_hooks_still_let_the_kit_through() {
    let s = settings(vec![entry("ci", &[])]);
    let kit = consenting("ci", HOOKS);
    assert_eq!(
        pane_kits(
            Some(&Workspace::default()),
            Some(std::slice::from_ref(&kit)),
            Some(&s)
        ),
        PaneKits {
            ids: vec![],
            held: strs(&["ci"])
        }
    );
    let own = ws(vec![entry("ci", &[HOOKS])]);
    assert_eq!(
        pane_kits(Some(&own), Some(&[kit]), Some(&s)),
        PaneKits {
            ids: strs(&["ci"]),
            held: vec![]
        }
    );
}

#[test]
fn everywhere_kits_are_read_from_the_kits_on_disk_for_a_workspace_with_none_of_its_own() {
    let asked = std::cell::Cell::new(0);
    let list = || {
        asked.set(asked.get() + 1);
        async { Ok(vec![consenting("ci", HOOKS)]) }
    };
    let none = Settings::default();
    assert_eq!(
        block_on(load_pane_kits_with(
            Some(&Workspace::default()),
            Some(&none),
            list
        )),
        PaneKits::default()
    );
    assert_eq!(asked.get(), 0);
    let accepted = settings(vec![entry("ci", &[HOOKS])]);
    assert_eq!(
        block_on(load_pane_kits_with(
            Some(&Workspace::default()),
            Some(&accepted),
            list
        )),
        PaneKits {
            ids: strs(&["ci"]),
            held: vec![]
        }
    );
    let unaccepted = settings(vec![entry("ci", &[])]);
    assert_eq!(
        block_on(load_pane_kits_with(None, Some(&unaccepted), list)),
        PaneKits {
            ids: vec![],
            held: strs(&["ci"])
        }
    );
}

// -- kitIssueNotes ---------------------------------------------------------------------

fn issue(id: &str, reason: LaunchReason, detail: Option<&str>) -> KitLaunchIssue {
    KitLaunchIssue {
        id: id.into(),
        reason,
        detail: detail.map(str::to_owned),
    }
}

#[test]
fn says_which_kit_stayed_out_and_why_one_line_each() {
    let en = Translator::new(Locale::En);
    let notes = kit_issue_notes(
        &[
            issue("gone", LaunchReason::Missing, None),
            issue("bad", LaunchReason::Broken, Some("version: is missing")),
        ],
        en,
    );
    assert_eq!(
        notes,
        vec![
            "Kit “gone” is not installed on this machine: the pane starts without it.",
            "Kit “bad” cannot be loaded (version: is missing): the pane starts without it.",
        ]
    );
}

#[test]
fn puts_every_kit_a_policy_kept_out_on_one_line() {
    let en = Translator::new(Locale::En);
    let notes = kit_issue_notes(
        &[
            issue("roblox", LaunchReason::Forbidden, None),
            issue("unreal", LaunchReason::Forbidden, None),
        ],
        en,
    );
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("roblox, unreal"), "{}", notes[0]);
    assert!(kit_issue_notes(&[], en).is_empty());
}

#[test]
fn says_a_kit_stayed_out_for_its_hooks() {
    let notes = kit_issue_notes(
        &[issue("studio", LaunchReason::Hooks, None)],
        Translator::new(Locale::En),
    );
    assert_eq!(notes.len(), 1);
    assert!(notes[0].starts_with("Kit “studio”: what it runs by itself changed"));
}

#[test]
fn turns_core_s_launch_issues_into_its_own() {
    let from_core = LaunchIssue::new("bad", IssueReason::Broken, Some("why".into()));
    assert_eq!(
        KitLaunchIssue::from(&from_core),
        issue("bad", LaunchReason::Broken, Some("why"))
    );
}

// -- the tab protocol --------------------------------------------------------------------

#[test]
fn gates_every_method_and_event_behind_a_known_permission() {
    for (_, permission) in KIT_METHODS.iter().chain(KIT_EVENTS.iter()) {
        if *permission == KIT_LOOPBACK_PATTERN || *permission == KIT_COMMAND_PATTERN {
            continue;
        }
        assert!(KIT_PERMISSIONS.contains(permission), "{permission}");
    }
    let mut methods: Vec<&str> = KIT_METHODS.iter().map(|(name, _)| *name).collect();
    methods.sort_unstable();
    assert_eq!(
        methods,
        [
            "command.run",
            "data.read",
            "data.write",
            "files.list",
            "files.read",
            "files.readBinary",
            "files.write",
            "files.writeBinary",
            "loopback.fetch",
            "notify",
            "panes.list",
            "panes.send",
            "preview.show",
            "workspace.info",
        ]
    );
}

#[test]
fn lets_a_tab_call_only_what_it_was_granted() {
    let may = |method: &str, granted: &[&str]| kit_may_call(method, granted, None);
    assert!(may("panes.list", &["panes.read"]));
    assert!(!may("panes.send", &["panes.read"]));
    assert!(!may("files.read", &[]));
    assert!(may("data.read", &["data.read"]));
    assert!(!may("data.write", &["data.read"]));
    assert!(!may("toString", &["workspace.read"]));
    assert!(!may("shell.run", &KIT_PERMISSIONS));
}

#[test]
fn gives_each_file_method_its_permission() {
    let may = |method: &str, granted: &[&str]| kit_may_call(method, granted, None);
    assert!(may("files.write", &["files.write"]));
    assert!(!may("files.write", &["files.read"]));
    assert!(may("files.writeBinary", &["files.write"]));
    assert!(!may("files.writeBinary", &["files.read"]));
    for method in ["files.read", "files.readBinary", "files.list"] {
        assert!(may(method, &["files.read"]), "{method}");
        assert!(!may(method, &["files.write"]), "{method}");
        assert!(!may(method, &[]), "{method}");
    }
}

#[test]
fn reads_hidden_files_only_for_a_tab_holding_both_file_permissions() {
    assert!(kit_reads_hidden_files(&["files.read", "files.write"]));
    assert!(kit_reads_hidden_files(&[
        "files.write",
        "notify",
        "files.read"
    ]));
    assert!(!kit_reads_hidden_files(&["files.read"]));
    assert!(!kit_reads_hidden_files(&["files.write"]));
    assert!(!kit_reads_hidden_files::<&str>(&[]));
}

#[test]
fn knows_an_argv_that_runs_something_of_the_kit_s_as_an_argument_and_not_as_the_program() {
    assert!(kit_argv_uses_kit_dir(&[
        "node",
        "{kitDir}/tools/audit.js",
        "{id}"
    ]));
    assert!(kit_argv_uses_kit_dir(&["node", "--root={kitDir}"]));
    assert!(kit_argv_uses_kit_dir(&[
        "gh",
        "run",
        "{kitdir}",
        "{{kitDir}}x"
    ]));
    assert!(!kit_argv_uses_kit_dir(&["gh", "run", "{run}"]));
    assert!(!kit_argv_uses_kit_dir(&["{kitDir}"]));
}

#[test]
fn lets_a_tab_reach_a_local_server_only_on_the_port_it_was_granted() {
    let call = |granted: &[&str], params: Option<serde_json::Value>| {
        kit_may_call("loopback.fetch", granted, params.as_ref())
    };
    assert!(call(&["loopback:8765"], Some(json!({ "port": 8765 }))));
    assert!(!call(&["loopback:8765"], Some(json!({ "port": 8766 }))));
    assert!(!call(&["loopback:8765"], Some(json!({ "port": "8765" }))));
    assert!(!call(&["loopback:8765"], None));
    assert!(!call(&["loopback:8765"], Some(json!(null))));
    assert!(!call(&KIT_PERMISSIONS, Some(json!({ "port": 8765 }))));
    // The pattern's own spelling is no permission.
    assert!(!call(
        &[KIT_LOOPBACK_PATTERN],
        Some(json!({ "port": 8765 }))
    ));
    assert!(!call(&["loopback:80"], Some(json!({ "port": 80 }))));
}

#[test]
fn lets_a_tab_run_only_the_command_it_was_granted() {
    let call = |granted: &[&str], params: Option<serde_json::Value>| {
        kit_may_call("command.run", granted, params.as_ref())
    };
    assert!(call(&["command:view"], Some(json!({ "name": "view" }))));
    assert!(!call(&["command:view"], Some(json!({ "name": "list" }))));
    assert!(!call(
        &["command:view"],
        Some(json!({ "name": "view ", "args": {} }))
    ));
    assert!(!call(&["command:view"], Some(json!({ "name": 3 }))));
    assert!(!call(&["command:view"], None));
    assert!(!call(&["command:view"], Some(json!(null))));
    assert!(!call(&KIT_PERMISSIONS, Some(json!({ "name": "view" }))));
    // Neither the pattern's own spelling nor the kit's grant is a permission.
    assert!(!call(
        &[KIT_COMMAND_PATTERN],
        Some(json!({ "name": "view" }))
    ));
    assert!(!call(
        &["commands:0123456789abcdef"],
        Some(json!({ "name": "view" }))
    ));
}

#[test]
fn reads_a_pinned_tag_out_of_a_git_source() {
    let url = "https://github.com/o/r";
    assert_eq!(
        kit_pinned_tag(&git(url, Some("refs/tags/v1.2.0"))),
        Some("v1.2.0")
    );
    assert_eq!(
        kit_pinned_tag(&git(url, Some("refs/tags/release/1"))),
        Some("release/1")
    );
    assert_eq!(kit_pinned_tag(&git(url, None)), None);
    assert_eq!(kit_pinned_tag(&git(url, Some("main"))), None);
    assert_eq!(kit_pinned_tag(&git(url, Some("v1.2.0"))), None);
    assert_eq!(kit_pinned_tag(&folder()), None);
}
