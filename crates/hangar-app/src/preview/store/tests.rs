//! `modelStore.test.ts`, case for case. The cases of the cross-window
//! handover (`takeModelScope`, `adoptModelScope`) are not here: a workspace in
//! a window of its own shares this process and so this store.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;

fn store() -> Store {
    Store::with_clock(|| 1_700_000_000_000)
}

fn show(store: &mut Store, path: &str, opened_by: Option<&str>, workspace: &str) -> String {
    store.show_file(ShowFile {
        workspace_id: workspace.into(),
        path: path.into(),
        title: None,
        opened_by: opened_by.map(str::to_string),
        modified_ms: 1,
        what: What::Model {
            format: "glb".into(),
        },
    })
}

fn model(store: &mut Store, path: &str) -> String {
    show(store, path, Some("Ava"), "ws")
}

fn info() -> ModelInfo {
    ModelInfo {
        triangles: 12,
        vertices: 8,
        meshes: 1,
        materials: 1,
        animations: Vec::new(),
        size: [1.0, 1.0, 1.0],
        center: [0.0, 0.0, 0.0],
    }
}

fn file(
    store: &mut Store,
    kind: FileKind,
    path: &str,
    workspace: &str,
    opened_by: Option<&str>,
) -> String {
    store.show_file(ShowFile {
        workspace_id: workspace.into(),
        path: path.into(),
        title: None,
        opened_by: opened_by.map(str::to_string),
        modified_ms: 1,
        what: What::File {
            kind,
            mime: "application/octet-stream".into(),
            size: 100,
        },
    })
}

fn ids(store: &Store, workspace: &str) -> Vec<String> {
    store
        .scope(workspace)
        .entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect()
}

fn titles(store: &Store, workspace: &str) -> Vec<String> {
    store
        .scope(workspace)
        .entries
        .iter()
        .map(|entry| entry.title.clone())
        .collect()
}

fn first<'a>(store: &'a Store, workspace: &str) -> &'a Entry {
    &store.scope(workspace).entries[0]
}

/* ── the store ─────────────────────────────────────────────────── */

#[test]
fn opens_the_panel_on_a_model_and_puts_it_in_front() {
    let mut store = store();
    let id = model(&mut store, "C:/m/chair.glb");
    let scope = store.scope("ws");
    assert!(scope.panel_open);
    assert_eq!(scope.active_id.as_deref(), Some(id.as_str()));
    assert_eq!(scope.entries[0].title, "chair.glb");
    assert!(scope.unseen);
}

#[test]
fn reloads_a_model_shown_again_instead_of_opening_it_twice() {
    let mut store = store();
    let first_id = model(&mut store, "C:/m/chair.glb");
    store.settle_model(&first_id, Ok(info()));
    let again = show(&mut store, "C:/m/chair.glb", Some("Ben"), "ws");
    let entry = first(&store, "ws");
    assert_eq!(again, first_id);
    assert_eq!(store.scope("ws").entries.len(), 1);
    assert_eq!(entry.revision, 1);
    assert_eq!(entry.status, Status::Loading);
    assert_eq!(entry.opened_by.as_deref(), Some("Ben"));
    // A model keeps what it last showed until the new version lands.
    assert_eq!(entry.model_info(), Some(&info()));
}

#[test]
fn closes_the_panel_with_the_last_model() {
    let mut store = store();
    let a = model(&mut store, "C:/m/a.glb");
    let b = model(&mut store, "C:/m/b.glb");
    store.set_model_panel("ws", None, Some(true));
    store.close_model(&b);
    assert_eq!(store.scope("ws").active_id.as_deref(), Some(a.as_str()));
    assert!(store.scope("ws").expanded);
    store.close_model(&a);
    assert!(!store.scope("ws").panel_open);
    assert!(!store.scope("ws").expanded);
    assert!(store.models().is_empty());
}

#[test]
fn tells_a_waiting_caller_how_the_load_ended() {
    let mut store = store();
    let id = model(&mut store, "C:/m/a.glb");
    // Loading: nothing to say yet.
    assert_eq!(store.settled(&id).map(|entry| entry.is_some()), Some(false));
    store.settle_model(&id, Err("not a glTF file".into()));
    let settled = store.settled(&id).unwrap().unwrap();
    assert_eq!(settled.error.as_deref(), Some("not a glTF file"));
    assert_eq!(settled.status, Status::Error);
    assert!(store.settled("no-such-entry").is_none());
}

/* ── one preview per workspace ─────────────────────────────────── */

#[test]
fn keeps_each_workspaces_models_tab_in_front_and_panel_apart() {
    let mut store = store();
    let chair = show(&mut store, "C:/a/chair.glb", Some("Ava"), "A");
    let table = show(&mut store, "C:/a/table.glb", Some("Ava"), "A");
    let car = show(&mut store, "C:/b/car.glb", Some("Ben"), "B");
    store.activate_model(&chair);
    store.set_model_panel("B", Some(false), None);

    assert_eq!(ids(&store, "A"), vec![chair.clone(), table.clone()]);
    assert_eq!(store.scope("A").active_id.as_deref(), Some(chair.as_str()));
    assert!(store.scope("A").panel_open);
    assert_eq!(ids(&store, "B"), vec![car.clone()]);
    assert_eq!(store.scope("B").active_id.as_deref(), Some(car.as_str()));
    assert!(!store.scope("B").panel_open);
    // The window's viewer holds them all.
    let mut held: Vec<String> = store.models().iter().map(|m| m.id.clone()).collect();
    held.sort();
    let mut all = vec![car, chair, table];
    all.sort();
    assert_eq!(held, all);
}

#[test]
fn has_nothing_for_a_workspace_no_model_was_shown_in_and_its_panel_stays_shut() {
    let mut store = store();
    show(&mut store, "C:/a/chair.glb", Some("Ava"), "A");
    assert_eq!(store.scope("B"), &Scope::default());
    store.set_model_panel("B", Some(true), None);
    assert!(!store.scope("B").panel_open);
    assert!(store.scope("nobody").entries.is_empty());
}

#[test]
fn opens_the_same_file_twice_in_two_workspaces_each_its_own() {
    let mut store = store();
    let in_a = show(&mut store, "C:/shared/robot.glb", Some("Ava"), "A");
    let in_b = show(&mut store, "C:/shared/robot.glb", Some("Ben"), "B");
    assert_ne!(in_a, in_b);
    store.settle_model(&in_a, Ok(info()));
    assert_eq!(first(&store, "A").status, Status::Ready);
    assert_eq!(first(&store, "B").status, Status::Loading);
}

#[test]
fn closes_in_one_workspace_without_touching_the_other() {
    let mut store = store();
    let chair = show(&mut store, "C:/a/chair.glb", Some("Ava"), "A");
    let car = show(&mut store, "C:/b/car.glb", Some("Ben"), "B");
    store.set_model_panel("B", None, Some(true));
    store.close_model(&chair);
    assert!(!store.scope("A").panel_open);
    let b = store.scope("B");
    assert_eq!(b.active_id.as_deref(), Some(car.as_str()));
    assert!(b.panel_open);
    // Expanding opens nothing by itself: the panel was open already.
    assert!(b.expanded);
    show(&mut store, "C:/a/lamp.glb", Some("Ava"), "A");
    store.close_workspace_models("B");
    assert!(store.scope("B").entries.is_empty());
    assert_eq!(titles(&store, "A"), vec!["lamp.glb"]);
}

#[test]
fn brings_a_tab_forward_in_its_own_workspace_only() {
    let mut store = store();
    let chair = show(&mut store, "C:/a/chair.glb", Some("Ava"), "A");
    show(&mut store, "C:/a/table.glb", Some("Ava"), "A");
    let car = show(&mut store, "C:/b/car.glb", Some("Ben"), "B");
    store.activate_model(&chair);
    assert_eq!(store.scope("A").active_id.as_deref(), Some(chair.as_str()));
    assert_eq!(store.scope("B").active_id.as_deref(), Some(car.as_str()));
}

#[test]
fn marks_what_an_agent_changed_as_unseen_until_the_workspaces_panel_is_on_screen() {
    let mut store = store();
    let chair = show(&mut store, "C:/a/chair.glb", Some("Ava"), "A");
    show(&mut store, "C:/b/car.glb", Some("Ben"), "B");
    store.mark_models_seen("A");
    assert!(!store.scope("A").unseen);
    assert!(store.scope("B").unseen);
    store.reload_model(&chair, 5, None);
    assert!(store.scope("A").unseen);
}

#[test]
fn forgets_the_workspaces_that_were_deleted() {
    let mut store = store();
    show(&mut store, "C:/a/chair.glb", Some("Ava"), "A");
    show(&mut store, "C:/b/car.glb", Some("Ben"), "B");
    store.prune_models(&["A".into(), "C".into()]);
    assert!(store.scope("B").entries.is_empty());
    assert_eq!(store.scope("A").entries.len(), 1);
    assert_eq!(store.models().len(), 1);
}

/* ── files of every kind beside the models ─────────────────────── */

#[test]
fn opens_each_kind_as_a_tab_of_its_workspace_the_latest_in_front() {
    let mut store = store();
    let chair = model(&mut store, "C:/a/chair.glb");
    let shot = file(
        &mut store,
        FileKind::Image,
        "C:/a/shot.png",
        "ws",
        Some("Ava"),
    );
    let notes = file(
        &mut store,
        FileKind::Markdown,
        "C:/a/NOTES.md",
        "ws",
        Some("Ava"),
    );
    let scope = store.scope("ws");
    let kinds: Vec<(&str, &str)> = scope
        .entries
        .iter()
        .map(|entry| (entry.kind_name(), entry.title.as_str()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("model", "chair.glb"),
            ("image", "shot.png"),
            ("markdown", "NOTES.md")
        ]
    );
    assert_eq!(scope.active_id.as_deref(), Some(notes.as_str()));
    assert!(scope.panel_open);
    // A file is ready at once; only a model waits for the 3D viewer.
    let statuses: Vec<Status> = scope.entries.iter().map(|entry| entry.status).collect();
    assert_eq!(
        statuses,
        vec![Status::Loading, Status::Ready, Status::Ready]
    );
    store.activate_model(&shot);
    assert_eq!(store.scope("ws").active_id.as_deref(), Some(shot.as_str()));
    store.activate_model(&chair);
    assert_eq!(store.scope("ws").active_id.as_deref(), Some(chair.as_str()));
}

#[test]
fn keeps_the_3d_viewers_list_to_the_models_and_the_windows_to_everything() {
    let mut store = store();
    let chair = show(&mut store, "C:/a/chair.glb", Some("Ava"), "A");
    let page = file(
        &mut store,
        FileKind::Html,
        "C:/b/page.html",
        "B",
        Some("Ava"),
    );
    let models: Vec<String> = store.models().iter().map(|m| m.id.clone()).collect();
    assert_eq!(models, vec![chair.clone()]);
    let mut every: Vec<String> = store.entries().map(|e| e.id.clone()).collect();
    every.sort();
    let mut expected = vec![chair, page];
    expected.sort();
    assert_eq!(every, expected);
    // An image opening adds no model.
    file(
        &mut store,
        FileKind::Image,
        "C:/b/shot.png",
        "B",
        Some("Ava"),
    );
    assert_eq!(store.models().len(), 1);
    assert!(store.models().iter().all(|entry| entry.is_model()));
}

#[test]
fn brings_a_file_shown_again_forward_once_with_a_new_revision_for_its_viewer() {
    let mut store = store();
    let log = file(
        &mut store,
        FileKind::Text,
        "C:/a/run.log",
        "ws",
        Some("Ava"),
    );
    file(
        &mut store,
        FileKind::Pdf,
        "C:/a/report.pdf",
        "ws",
        Some("Ava"),
    );
    let again = store.show_file(ShowFile {
        workspace_id: "ws".into(),
        path: "C:/a/run.log".into(),
        title: Some("Run log".into()),
        opened_by: Some("Ben".into()),
        modified_ms: 7,
        what: What::File {
            kind: FileKind::Text,
            mime: "text/plain".into(),
            size: 900,
        },
    });
    assert_eq!(again, log);
    let scope = store.scope("ws");
    assert_eq!(scope.entries.len(), 2);
    assert_eq!(scope.active_id.as_deref(), Some(log.as_str()));
    let entry = &scope.entries[0];
    assert_eq!(entry.title, "Run log");
    assert_eq!(entry.revision, 1);
    assert_eq!(entry.opened_by.as_deref(), Some("Ben"));
    assert_eq!(entry.modified_ms, 7);
    assert!(matches!(entry.body, Body::File { size: 900, .. }));
}

#[test]
fn closes_one_kind_or_another_and_the_last_one_shuts_its_panel() {
    let mut store = store();
    let chair = model(&mut store, "C:/a/chair.glb");
    let clip = file(
        &mut store,
        FileKind::Video,
        "C:/a/clip.mp4",
        "ws",
        Some("Ava"),
    );
    let song = file(
        &mut store,
        FileKind::Audio,
        "C:/a/theme.ogg",
        "ws",
        Some("Ava"),
    );
    store.close_model(&song);
    assert_eq!(store.scope("ws").active_id.as_deref(), Some(clip.as_str()));
    store.close_models(&[chair, clip]);
    let scope = store.scope("ws");
    assert!(scope.entries.is_empty());
    assert!(!scope.panel_open);
    assert_eq!(store.entries().count(), 0);
}

#[test]
fn keeps_each_workspaces_files_apart_and_closes_one_workspaces_only() {
    let mut store = store();
    let page_a = file(
        &mut store,
        FileKind::Html,
        "C:/shared/index.html",
        "A",
        Some("Ava"),
    );
    let page_b = file(
        &mut store,
        FileKind::Html,
        "C:/shared/index.html",
        "B",
        Some("Ava"),
    );
    assert_ne!(page_a, page_b);
    show(&mut store, "C:/b/car.glb", Some("Ben"), "B");
    store.close_workspace_models("B");
    assert!(store.scope("B").entries.is_empty());
    assert_eq!(ids(&store, "A"), vec![page_a]);
}

#[test]
fn answers_a_waiting_caller_at_once_for_a_file_which_has_nothing_to_load() {
    let mut store = store();
    let pdf = file(
        &mut store,
        FileKind::Pdf,
        "C:/a/report.pdf",
        "ws",
        Some("Ava"),
    );
    let settled = store.settled(&pdf).unwrap().unwrap();
    assert_eq!(settled.status, Status::Ready);
}

#[test]
fn settles_models_only_and_has_a_rewritten_file_read_again_ready_all_along() {
    let mut store = store();
    let shot = file(
        &mut store,
        FileKind::Image,
        "C:/a/shot.png",
        "ws",
        Some("Ava"),
    );
    store.settle_model(&shot, Err("not a glTF file".into()));
    let entry = first(&store, "ws");
    assert_eq!((entry.status, entry.revision), (Status::Ready, 0));
    assert_eq!(entry.error, None);
    store.mark_models_seen("ws");
    store.reload_model(&shot, 99, Some(2048));
    let entry = first(&store, "ws");
    assert_eq!(
        (entry.status, entry.revision, entry.modified_ms),
        (Status::Ready, 1, 99)
    );
    assert!(matches!(entry.body, Body::File { size: 2048, .. }));
    assert!(store.scope("ws").unseen);
}

/* ── kit tabs beside the files ─────────────────────────────────── */

fn kit_tab(store: &mut Store, tab_id: &str, workspace: &str, title: &str) -> String {
    store.open_kit_tab(KitTabToOpen::new(workspace, "hello-kit", tab_id, title))
}

#[test]
fn opens_a_kits_tab_in_front_its_panel_open_as_the_persons_own_doing() {
    let mut store = store();
    let id = kit_tab(&mut store, "status", "ws", "Status");
    let scope = store.scope("ws");
    assert_eq!(scope.active_id.as_deref(), Some(id.as_str()));
    assert!(scope.panel_open);
    assert!(!scope.unseen);
    let entry = &scope.entries[0];
    let kit = entry.kit().expect("a kit tab");
    assert_eq!(
        (kit.kit_id.as_str(), kit.tab_id.as_str()),
        ("hello-kit", "status")
    );
    assert_eq!(entry.title, "Status");
    assert_eq!(
        entry.path,
        kit_tab_path("hello-kit", "status", &KitTabParams::new())
    );
    assert_eq!(entry.status, Status::Ready);
    assert_eq!(entry.opened_by, None);
    assert_eq!(entry.revision, 0);
    assert!(entry.is_kit_tab());
    assert!(!entry.is_file());
    assert!(!entry.is_model());
}

#[test]
fn brings_an_open_tab_forward_as_it_is_under_the_manifests_title_of_now() {
    let mut store = store();
    let status = kit_tab(&mut store, "status", "ws", "Status");
    let shot = file(
        &mut store,
        FileKind::Image,
        "C:/a/shot.png",
        "ws",
        Some("Ava"),
    );
    assert_eq!(store.scope("ws").active_id.as_deref(), Some(shot.as_str()));
    assert_eq!(kit_tab(&mut store, "status", "ws", "State"), status);
    let scope = store.scope("ws");
    assert_eq!(scope.entries.len(), 2);
    assert_eq!(scope.active_id.as_deref(), Some(status.as_str()));
    assert_eq!(
        (scope.entries[0].title.as_str(), scope.entries[0].revision),
        ("State", 0)
    );
}

#[test]
fn keeps_each_workspaces_tabs_apart_and_the_windows_list_of_them() {
    let mut store = store();
    let a = kit_tab(&mut store, "status", "A", "status");
    let b = kit_tab(&mut store, "status", "B", "status");
    assert_ne!(a, b);
    let mut listed: Vec<String> = store.kit_tabs().iter().map(|tab| tab.id.clone()).collect();
    listed.sort();
    let mut both = vec![a, b.clone()];
    both.sort();
    assert_eq!(listed, both);
    assert!(store.models().is_empty());
    file(&mut store, FileKind::Text, "C:/a/x.txt", "A", None);
    assert_eq!(store.kit_tabs().len(), 2);
    store.close_workspace_models("A");
    let listed: Vec<String> = store.kit_tabs().iter().map(|tab| tab.id.clone()).collect();
    assert_eq!(listed, vec![b]);
}

#[test]
fn reloads_from_its_own_button_never_from_a_file_watch() {
    let mut store = store();
    let id = kit_tab(&mut store, "status", "ws", "status");
    store.reload_kit_tab(&id);
    assert_eq!(first(&store, "ws").revision, 1);
    store.reload_model(&id, 99, Some(5));
    let entry = first(&store, "ws");
    assert_eq!((entry.revision, entry.modified_ms), (1, 0));
}

#[test]
fn closes_like_any_tab() {
    let mut store = store();
    let status = kit_tab(&mut store, "status", "ws", "status");
    let other = kit_tab(&mut store, "other", "ws", "other");
    store.close_model(&other);
    assert_eq!(
        store.scope("ws").active_id.as_deref(),
        Some(status.as_str())
    );
    store.close_models(&[status]);
    let scope = store.scope("ws");
    assert!(scope.entries.is_empty());
    assert!(!scope.panel_open);
}

mod with_params {
    use super::*;

    fn run(store: &mut Store, run: &str) -> String {
        store.open_kit_tab(KitTabToOpen::new("ws", "ci", "run", "Run").params(&[("run", run)]))
    }

    #[test]
    fn knows_a_set_of_params_whichever_order_its_names_came_in() {
        let params = |pairs: &[(&str, &str)]| -> KitTabParams {
            pairs
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        assert_eq!(
            kit_params_key(&params(&[("a", "1"), ("b", "2")])),
            kit_params_key(&params(&[("b", "2"), ("a", "1")]))
        );
        assert_ne!(
            kit_params_key(&params(&[("a", "1")])),
            kit_params_key(&params(&[("a", "2")]))
        );
        assert_eq!(kit_tab_path("ci", "run", &params(&[])), "kit:ci/run");
        assert_eq!(
            kit_tab_path("ci", "run", &params(&[("b", "2"), ("a", "1")])),
            kit_tab_path("ci", "run", &params(&[("a", "1"), ("b", "2")]))
        );
        assert_ne!(
            kit_tab_path("ci", "run", &params(&[("a", "1")])),
            kit_tab_path("ci", "run", &params(&[]))
        );
    }

    #[test]
    fn holds_a_tab_per_set_of_params_and_brings_the_one_asked_for_forward() {
        let mut store = store();
        let one = run(&mut store, "1");
        let two = run(&mut store, "2");
        assert_ne!(two, one);
        assert_eq!(store.scope("ws").entries.len(), 2);
        assert_eq!(
            store.kit_tab_params(&one),
            KitTabParams::from([("run".to_string(), "1".to_string())])
        );
        assert_eq!(run(&mut store, "1"), one);
        assert_eq!(store.scope("ws").active_id.as_deref(), Some(one.as_str()));
        assert_eq!(store.scope("ws").entries.len(), 2);
    }

    #[test]
    fn the_same_request_opens_through_the_workspaces_host_as_through_the_store() {
        let mut store = store();
        let id = store.open_kit_tab(
            KitTabToOpen::new("ws", "ci", "run", "Run")
                .params(&[("run", "1")])
                .opened_by("Ava"),
        );
        assert_eq!(run(&mut store, "1"), id);
        let entry = first(&store, "ws");
        assert!(entry.is_kit_tab());
        assert_eq!(entry.opened_by.as_deref(), Some("Ava"));
    }

    #[test]
    fn is_news_when_an_agent_opens_it_not_when_the_person_does() {
        let mut store = store();
        run(&mut store, "1");
        assert!(!store.scope("ws").unseen);
        store.close_all_models();
        store.open_kit_tab(
            KitTabToOpen::new("ws", "ci", "run", "Run")
                .params(&[("run", "1")])
                .opened_by("Ava"),
        );
        assert!(store.scope("ws").unseen);
    }
}

mod who_asked {
    use super::*;

    fn run(store: &mut Store, by: Option<&str>) -> String {
        let mut tab = KitTabToOpen::new("ws", "ci", "run", "Run").params(&[("run", "1")]);
        if let Some(by) = by {
            tab = tab.opened_by(by);
        }
        store.open_kit_tab(tab)
    }

    fn caller(pane: &str) -> Option<KitTabCaller> {
        Some(KitTabCaller { pane: pane.into() })
    }

    type Heard = Arc<Mutex<Vec<(KitTabParams, Option<KitTabCaller>)>>>;

    fn listen(store: &mut Store, id: &str) -> (Heard, WatchId) {
        let heard: Heard = Arc::default();
        let sink = heard.clone();
        let watch = store.watch_kit_tab_params(
            id,
            Box::new(move |params, caller| {
                sink.lock().unwrap().push((params.clone(), caller.cloned()));
            }),
        );
        (heard, watch)
    }

    fn params(pairs: &[(&str, &str)]) -> KitTabParams {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn is_nobody_when_the_person_opened_it_and_the_pane_when_an_agent_or_a_hook_did() {
        let mut store = store();
        let id = run(&mut store, None);
        assert_eq!(store.kit_tab_caller(&id), None);
        store.close_all_models();
        let id = run(&mut store, Some("Ava"));
        assert_eq!(store.kit_tab_caller(&id), caller("Ava"));
    }

    #[test]
    fn is_nobody_once_the_tab_is_closed() {
        let mut store = store();
        let id = run(&mut store, Some("Ava"));
        store.close_all_models();
        assert_eq!(store.kit_tab_caller(&id), None);
        assert_eq!(store.kit_tab_caller("no-such-tab"), None);
    }

    #[test]
    fn is_the_latest_to_ask_and_the_page_of_a_tab_with_params_hears_of_it() {
        let mut store = store();
        let id = run(&mut store, Some("Ava"));
        let (heard, watch) = listen(&mut store, &id);

        assert_eq!(run(&mut store, Some("Max")), id);
        assert_eq!(store.kit_tab_caller(&id), caller("Max"));
        assert_eq!(
            *heard.lock().unwrap(),
            vec![(params(&[("run", "1")]), caller("Max"))]
        );

        // Asked again by the same pane: it may be waiting for another answer.
        run(&mut store, Some("Max"));
        assert_eq!(heard.lock().unwrap().len(), 2);

        store.unwatch_kit_tab_params(&id, watch);
        run(&mut store, Some("Ava"));
        assert_eq!(heard.lock().unwrap().len(), 2);
        assert_eq!(store.kit_tab_caller(&id), caller("Ava"));
    }

    #[test]
    fn stays_who_the_page_answers_when_the_person_brings_the_tab_forward() {
        let mut store = store();
        let id = run(&mut store, Some("Ava"));
        let (heard, _) = listen(&mut store, &id);
        assert_eq!(run(&mut store, None), id);
        assert_eq!(store.kit_tab_caller(&id), caller("Ava"));
        assert!(heard.lock().unwrap().is_empty());
    }

    #[test]
    fn tells_only_the_page_of_that_very_tab() {
        let mut store = store();
        let mine = run(&mut store, Some("Ava"));
        let other = store.open_kit_tab(
            KitTabToOpen::new("ws", "ci", "run", "Run")
                .params(&[("run", "2")])
                .opened_by("Ava"),
        );
        let (heard, _) = listen(&mut store, &other);
        run(&mut store, Some("Max"));
        assert!(heard.lock().unwrap().is_empty());
        assert_eq!(store.kit_tab_caller(&mine), caller("Max"));
        assert_eq!(store.kit_tab_caller(&other), caller("Ava"));
    }

    #[test]
    fn comes_along_with_what_a_singleton_takes_and_when_it_takes_nothing_new() {
        let mut store = store();
        let board = |store: &mut Store, view: Option<&str>, by: &str| {
            let mut tab = KitTabToOpen::new("ws", "ci", "board", "Board")
                .singleton()
                .opened_by(by);
            if let Some(view) = view {
                tab = tab.params(&[("view", view)]);
            }
            store.open_kit_tab(tab)
        };
        let id = board(&mut store, Some("a"), "Ava");
        let (heard, _) = listen(&mut store, &id);
        board(&mut store, Some("b"), "Max");
        board(&mut store, None, "Ava");
        assert_eq!(
            *heard.lock().unwrap(),
            vec![
                (params(&[("view", "b")]), caller("Max")),
                (params(&[("view", "b")]), caller("Ava")),
            ]
        );
    }
}

mod a_singleton {
    use super::*;

    fn board(store: &mut Store, params: &[(&str, &str)]) -> String {
        store.open_kit_tab(
            KitTabToOpen::new("ws", "ci", "board", "Board")
                .singleton()
                .params(params),
        )
    }

    fn view(value: &str) -> KitTabParams {
        KitTabParams::from([("view".to_string(), value.to_string())])
    }

    #[test]
    fn is_one_tab_and_its_page_hears_of_new_params_without_reloading() {
        let mut store = store();
        let id = board(&mut store, &[("view", "a")]);
        let heard: Arc<Mutex<Vec<KitTabParams>>> = Arc::default();
        let sink = heard.clone();
        let watch = store.watch_kit_tab_params(
            &id,
            Box::new(move |params, _| sink.lock().unwrap().push(params.clone())),
        );

        assert_eq!(board(&mut store, &[("view", "b")]), id);
        assert_eq!(store.scope("ws").entries.len(), 1);
        assert_eq!(store.kit_tab_params(&id), view("b"));
        let entry = first(&store, "ws");
        assert_eq!(entry.revision, 0);
        assert_eq!(
            entry.path,
            kit_tab_path("ci", "board", &KitTabParams::new())
        );
        assert_eq!(*heard.lock().unwrap(), vec![view("b")]);

        // Nothing new: nothing to hear, and it keeps what it holds.
        board(&mut store, &[("view", "b")]);
        board(&mut store, &[]);
        assert_eq!(*heard.lock().unwrap(), vec![view("b")]);
        assert_eq!(store.kit_tab_params(&id), view("b"));

        store.unwatch_kit_tab_params(&id, watch);
        board(&mut store, &[("view", "c")]);
        assert_eq!(*heard.lock().unwrap(), vec![view("b")]);
    }

    #[test]
    fn tells_only_its_own_page() {
        let mut store = store();
        let mine = board(&mut store, &[("view", "a")]);
        let other = store.open_kit_tab(
            KitTabToOpen::new("ws", "ci", "other", "Other")
                .singleton()
                .params(&[("view", "a")]),
        );
        let heard: Arc<Mutex<Vec<&str>>> = Arc::default();
        let sink = heard.clone();
        store.watch_kit_tab_params(
            &other,
            Box::new(move |_, _| sink.lock().unwrap().push("other")),
        );
        board(&mut store, &[("view", "b")]);
        assert_ne!(mine, other);
        assert!(heard.lock().unwrap().is_empty());
        assert_eq!(store.kit_tab_params(&other), view("a"));
    }
}

mod reveal {
    use super::*;

    fn tab(store: &mut Store, reveal: Option<bool>, tab_id: &str) -> String {
        let mut tab = KitTabToOpen::new("ws", "ci", tab_id, tab_id).opened_by("Ava");
        if let Some(reveal) = reveal {
            tab = tab.reveal(reveal);
        }
        store.open_kit_tab(tab)
    }

    #[test]
    fn opens_the_panel_by_default_the_tab_in_front() {
        let mut store = store();
        let first = tab(&mut store, None, "run");
        let scope = store.scope("ws");
        assert!(scope.panel_open);
        assert_eq!(scope.active_id.as_deref(), Some(first.as_str()));
        store.set_model_panel("ws", Some(false), None);
        let second = tab(&mut store, Some(true), "other");
        let scope = store.scope("ws");
        assert!(scope.panel_open);
        assert_eq!(scope.active_id.as_deref(), Some(second.as_str()));
    }

    #[test]
    fn false_adds_the_tab_without_opening_the_panel_and_says_it_is_there() {
        let mut store = store();
        let id = tab(&mut store, Some(false), "run");
        let scope = store.scope("ws");
        assert!(!scope.panel_open);
        assert_eq!(scope.active_id.as_deref(), Some(id.as_str()));
        assert!(scope.unseen);
        let listed: Vec<String> = store.kit_tabs().iter().map(|e| e.id.clone()).collect();
        assert_eq!(listed, vec![id]);
    }

    #[test]
    fn false_leaves_what_is_in_front_and_an_open_panel_as_they_are() {
        let mut store = store();
        let first = tab(&mut store, None, "run");
        store.mark_models_seen("ws");
        tab(&mut store, Some(false), "other");
        let scope = store.scope("ws");
        assert!(scope.panel_open);
        assert_eq!(scope.active_id.as_deref(), Some(first.as_str()));
        // Brought forward by nobody, and nothing changed: no news either.
        let again = tab(&mut store, Some(false), "other");
        assert_eq!(store.scope("ws").active_id.as_deref(), Some(first.as_str()));
        assert_ne!(again, first);
    }

    #[test]
    fn false_asks_nothing_of_a_tab_already_there() {
        let mut store = store();
        tab(&mut store, None, "run");
        store.set_model_panel("ws", Some(false), None);
        store.mark_models_seen("ws");
        tab(&mut store, Some(false), "run");
        let scope = store.scope("ws");
        assert!(!scope.panel_open);
        assert!(!scope.unseen);
    }
}

mod what_a_page_says_of_itself {
    use super::*;

    fn opened(store: &mut Store) -> String {
        store.open_kit_tab(KitTabToOpen::new("ws", "ci", "run", "Run"))
    }

    fn badge(state: KitBadgeState, text: Option<&str>) -> Option<KitBadge> {
        Some(KitBadge {
            state,
            text: text.map(str::to_string),
        })
    }

    #[test]
    fn names_its_tab_which_asking_for_it_again_leaves_as_it_is() {
        let mut store = store();
        let id = opened(&mut store);
        store.set_kit_tab_title(&id, "Run #4567");
        let entry = first(&store, "ws");
        assert_eq!(entry.title, "Run #4567");
        assert!(entry.kit().unwrap().titled);
        opened(&mut store);
        assert_eq!(first(&store, "ws").title, "Run #4567");
    }

    #[test]
    fn sets_its_state_in_the_strip_until_it_is_taken_away_or_the_page_reloads() {
        let mut store = store();
        let id = opened(&mut store);
        let held = |store: &Store| first(store, "ws").kit().unwrap().badge.clone();
        assert_eq!(held(&store), None);
        store.set_kit_tab_badge(&id, badge(KitBadgeState::Running, Some("2/5")));
        assert_eq!(held(&store), badge(KitBadgeState::Running, Some("2/5")));
        store.set_kit_tab_badge(&id, None);
        assert_eq!(held(&store), None);
        store.set_kit_tab_badge(&id, badge(KitBadgeState::Failure, None));
        store.reload_kit_tab(&id);
        assert_eq!(held(&store), None);
    }

    #[test]
    fn redraws_nothing_when_nothing_changes() {
        let mut store = store();
        let id = opened(&mut store);
        store.set_kit_tab_badge(&id, badge(KitBadgeState::Success, Some("ok")));
        store.set_kit_tab_title(&id, "Run");
        let before = store.revision();
        store.set_kit_tab_badge(&id, badge(KitBadgeState::Success, Some("ok")));
        store.set_kit_tab_title(&id, "Run");
        assert_eq!(store.revision(), before);
    }

    #[test]
    fn reaches_no_other_kind_of_tab() {
        let mut store = store();
        let shot = model(&mut store, "C:/m/chair.glb");
        let before = store.revision();
        store.set_kit_tab_title(&shot, "Nope");
        store.set_kit_tab_badge(&shot, badge(KitBadgeState::Failure, None));
        assert_eq!(store.revision(), before);
    }
}

/* ── a kit's tabs across a restart ─────────────────────────────── */

/// A storage that outlives the store: one fresh store per start, all sharing
/// the same map.
#[derive(Clone, Default)]
struct Memory(Arc<Mutex<std::collections::HashMap<String, Value>>>);

impl SavedTabs for Memory {
    fn read(&self, workspace: &str) -> Option<Value> {
        self.0.lock().unwrap().get(workspace).cloned()
    }

    fn write(&mut self, workspace: &str, tabs: Option<&[SavedKitTab]>) {
        let mut map = self.0.lock().unwrap();
        match tabs {
            Some(tabs) => {
                map.insert(workspace.to_string(), serde_json::to_value(tabs).unwrap());
            }
            None => {
                map.remove(workspace);
            }
        }
    }

    fn workspaces(&self) -> Vec<String> {
        self.0.lock().unwrap().keys().cloned().collect()
    }
}

mod across_a_restart {
    use super::*;

    fn start(saved: &Memory) -> Store {
        Store::with_saved(Box::new(saved.clone()))
    }

    fn ci() -> KitTabToOpen {
        KitTabToOpen::new("ws", "ci", "run", "Run")
    }

    fn run_params(store: &Store, workspace: &str) -> Vec<KitTabParams> {
        store
            .scope(workspace)
            .entries
            .iter()
            .filter_map(|entry| entry.kit().map(|kit| kit.params.clone()))
            .collect()
    }

    fn params(pairs: &[(&str, &str)]) -> KitTabParams {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn keeps_a_tabs_params_and_its_own_title_behind_a_closed_panel() {
        let saved = Memory::default();
        let mut before = start(&saved);
        let id = before.open_kit_tab(ci().params(&[("run", "4567")]).opened_by("Ava"));
        before.set_kit_tab_title(&id, "Run #4567");
        before.set_kit_tab_badge(
            &id,
            Some(KitBadge {
                state: KitBadgeState::Running,
                text: None,
            }),
        );

        let mut after = start(&saved);
        assert!(after.scope("ws").entries.is_empty());
        after.restore_kit_tabs("ws");
        let scope = after.scope("ws").clone();
        assert_eq!(scope.entries.len(), 1);
        let entry = &scope.entries[0];
        let kit = entry.kit().unwrap();
        assert_eq!((kit.kit_id.as_str(), kit.tab_id.as_str()), ("ci", "run"));
        assert_eq!(kit.params, params(&[("run", "4567")]));
        assert_eq!(entry.title, "Run #4567");
        assert!(kit.titled);
        assert_eq!(entry.opened_by.as_deref(), Some("Ava"));
        // The page says it again once it runs.
        assert_eq!(kit.badge, None);
        assert_eq!(
            entry.path,
            kit_tab_path("ci", "run", &params(&[("run", "4567")]))
        );
        assert!(!scope.panel_open);
        assert!(!scope.unseen);
        assert_eq!(scope.active_id.as_deref(), Some(entry.id.as_str()));
        assert_eq!(after.kit_tab_params(&entry.id), params(&[("run", "4567")]));
    }

    #[test]
    fn the_same_params_land_on_the_restored_tab_and_a_singleton_still_takes_new_ones() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci().params(&[("run", "1")]));
        before.open_kit_tab(
            KitTabToOpen::new("ws", "ci", "board", "Run")
                .singleton()
                .params(&[("view", "a")]),
        );

        let mut after = start(&saved);
        after.restore_kit_tabs("ws");
        let entries = after.scope("ws").entries.clone();
        let (run, board) = (&entries[0], &entries[1]);
        assert_eq!(after.open_kit_tab(ci().params(&[("run", "1")])), run.id);
        assert_eq!(
            after.open_kit_tab(
                KitTabToOpen::new("ws", "ci", "board", "Run")
                    .singleton()
                    .params(&[("view", "b")])
            ),
            board.id
        );
        assert_eq!(after.kit_tab_params(&board.id), params(&[("view", "b")]));

        let mut again = start(&saved);
        again.restore_kit_tabs("ws");
        assert_eq!(
            run_params(&again, "ws"),
            vec![params(&[("run", "1")]), params(&[("view", "b")])]
        );
    }

    #[test]
    fn forgets_a_tab_that_was_closed() {
        let saved = Memory::default();
        let mut before = start(&saved);
        let one = before.open_kit_tab(ci().params(&[("run", "1")]));
        before.open_kit_tab(ci().params(&[("run", "2")]));
        before.close_model(&one);

        let mut after = start(&saved);
        after.restore_kit_tabs("ws");
        assert_eq!(run_params(&after, "ws"), vec![params(&[("run", "2")])]);
        after.close_workspace_models("ws");
        assert!(saved.0.lock().unwrap().is_empty());
    }

    #[test]
    fn brings_no_file_back_and_a_file_opened_first_does_not_lose_what_was_saved() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci().params(&[("run", "1")]));

        let mut after = start(&saved);
        model(&mut after, "C:/m/a.glb");
        after.restore_kit_tabs("ws");
        let kinds: Vec<&str> = after
            .scope("ws")
            .entries
            .iter()
            .map(|e| e.kind_name())
            .collect();
        assert_eq!(kinds, vec!["model", "kit"]);

        let mut again = start(&saved);
        again.restore_kit_tabs("ws");
        let kinds: Vec<&str> = again
            .scope("ws")
            .entries
            .iter()
            .map(|e| e.kind_name())
            .collect();
        assert_eq!(kinds, vec!["kit"]);
    }

    #[test]
    fn opening_a_tab_first_still_brings_the_others_back_and_each_workspace_has_its_own() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci().params(&[("run", "1")]));
        before.open_kit_tab(KitTabToOpen::new("other", "ci", "run", "Run").params(&[("run", "9")]));

        let mut after = start(&saved);
        after.open_kit_tab(ci().params(&[("run", "2")]));
        assert_eq!(
            run_params(&after, "ws"),
            vec![params(&[("run", "1")]), params(&[("run", "2")])]
        );
        assert!(after.scope("other").entries.is_empty());
        after.restore_kit_tabs("other");
        assert_eq!(run_params(&after, "other"), vec![params(&[("run", "9")])]);
    }

    #[test]
    fn restores_once_and_what_is_open_already_is_not_opened_twice() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci().params(&[("run", "1")]));

        let mut after = start(&saved);
        after.restore_kit_tabs("ws");
        after.restore_kit_tabs("ws");
        assert_eq!(after.scope("ws").entries.len(), 1);
    }

    #[test]
    fn reads_the_storage_with_care_what_is_not_a_tab_is_no_tab() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci());
        for raw in [
            json!("not json"),
            json!({"a": 1}),
            json!([1, null, {"kitId": "ci"}]),
            json!([{"kitId":"ci","tabId":"run","path":"elsewhere","title":"x","params":{}}]),
            json!([{"kitId":"ci","tabId":"run","path":"kit:ci/run","title":"x","params":{"a":1}}]),
            json!([{"kitId":"ci","tabId":"run","path":"kit:ci/run","title":"x","params":["a"]}]),
        ] {
            saved.0.lock().unwrap().insert("ws".into(), raw.clone());
            let mut after = start(&saved);
            after.restore_kit_tabs("ws");
            assert!(after.scope("ws").entries.is_empty(), "{raw}");
        }
    }

    #[test]
    fn forgets_what_deleted_workspaces_saved_and_nothing_else_in_the_storage() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(KitTabToOpen::new("kept", "ci", "run", "Run").params(&[("run", "1")]));
        before
            .open_kit_tab(KitTabToOpen::new("deleted", "ci", "run", "Run").params(&[("run", "2")]));
        before.forget_saved_kit_tabs(&["kept".to_string()]);

        let mut after = start(&saved);
        after.restore_kit_tabs("kept");
        after.restore_kit_tabs("deleted");
        assert_eq!(after.scope("kept").entries.len(), 1);
        assert!(after.scope("deleted").entries.is_empty());
    }

    #[test]
    fn works_without_any_storage_as_under_test_or_when_it_is_refused() {
        let mut store = store();
        let id = store.open_kit_tab(ci().params(&[("run", "1")]));
        assert_eq!(store.kit_tab_params(&id), params(&[("run", "1")]));
        store.restore_kit_tabs("other");
        assert!(store.scope("other").entries.is_empty());
        store.forget_saved_kit_tabs(&[]);
    }

    #[test]
    fn come_back_flagged_restored_with_their_params_and_a_tab_opened_later_does_not() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci().params(&[("run", "4567")]).opened_by("Ava"));
        let mut after = start(&saved);
        after.restore_kit_tabs("ws");
        let restored = &after.scope("ws").entries[0];
        assert!(restored.kit().unwrap().restored);
        assert_eq!(restored.kit().unwrap().params, params(&[("run", "4567")]));
        let live = after.open_kit_tab(ci().params(&[("run", "9")]).opened_by("Ava"));
        assert!(!after.entry(&live).unwrap().kit().unwrap().restored);
    }

    #[test]
    fn a_tab_opened_by_a_hook_in_a_workspace_not_restored_yet_loads_at_once() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci().params(&[("run", "1")]));
        let mut after = start(&saved);
        let id = after.open_kit_tab(ci().params(&[("run", "2")]).opened_by("Ava"));
        let entries = after.scope("ws").entries.clone();
        assert!(
            !entries
                .iter()
                .find(|e| e.id == id)
                .unwrap()
                .kit()
                .unwrap()
                .restored
        );
        let others: Vec<bool> = entries
            .iter()
            .filter(|e| e.id != id)
            .map(|e| e.kit().unwrap().restored)
            .collect();
        assert_eq!(others, vec![true]);
    }

    #[test]
    fn stop_being_restored_once_shown_and_the_flag_is_not_saved() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci().params(&[("run", "1")]));
        let mut after = start(&saved);
        after.restore_kit_tabs("ws");
        let id = after.scope("ws").entries[0].id.clone();
        after.mark_kit_tab_shown(&id);
        let entry = &after.scope("ws").entries[0];
        assert!(!entry.kit().unwrap().restored);
        assert_eq!(entry.kit().unwrap().params, params(&[("run", "1")]));
        let mut third = start(&saved);
        third.restore_kit_tabs("ws");
        assert!(third.scope("ws").entries[0].kit().unwrap().restored);
    }

    #[test]
    fn an_agent_or_a_hook_asking_for_a_restored_tab_or_a_reload_loads_it() {
        let saved = Memory::default();
        let mut before = start(&saved);
        before.open_kit_tab(ci().params(&[("run", "1")]));
        let mut after = start(&saved);
        after.restore_kit_tabs("ws");
        after.open_kit_tab(ci().params(&[("run", "1")]));
        assert!(after.scope("ws").entries[0].kit().unwrap().restored);
        after.open_kit_tab(ci().params(&[("run", "1")]).opened_by("Ava"));
        assert!(!after.scope("ws").entries[0].kit().unwrap().restored);

        let mut again = start(&saved);
        again.restore_kit_tabs("ws");
        let id = again.scope("ws").entries[0].id.clone();
        again.reload_kit_tab(&id);
        assert!(!again.scope("ws").entries[0].kit().unwrap().restored);
    }

    #[test]
    fn a_tab_comes_back_under_its_own_id_which_is_its_panes() {
        let saved = Memory::default();
        let mut before = start(&saved);
        let id = before.open_kit_tab(ci().params(&[("run", "1")]));
        assert_eq!(
            saved.read("ws").unwrap()[0]["id"],
            serde_json::json!(id),
            "kept in kit-tabs.json"
        );
        let mut after = start(&saved);
        after.restore_kit_tabs("ws");
        assert_eq!(after.scope("ws").entries[0].id, id);
        assert!(after.is_restored("ws"));
        // An id an entry has already is never given twice, the saved one
        // giving way (a run started in the same millisecond as the last).
        let mut third = start(&saved);
        let other = third.open_kit_tab(KitTabToOpen::new("other", "ci", "run", "Run"));
        third.restore_kit_tabs("ws");
        let restored = third.scope("ws").entries[0].id.clone();
        assert_ne!(restored, other);
        assert_eq!(third.entries().filter(|entry| entry.id == other).count(), 1);
    }

    #[test]
    fn a_tab_saved_before_the_tabs_were_panes_comes_back_under_a_new_id() {
        let saved = Memory::default();
        saved.0.lock().unwrap().insert(
            "ws".into(),
            serde_json::json!([{
                "kitId": "ci", "tabId": "run", "path": "kit:ci/run", "title": "Run",
                "titled": false, "params": {}, "openedBy": null
            }]),
        );
        let mut store = start(&saved);
        store.restore_kit_tabs("ws");
        let entry = &store.scope("ws").entries[0];
        assert!(entry.id.starts_with('p'), "{}", entry.id);
        assert!(entry.kit().unwrap().restored);
        // From now on it is saved with it.
        assert_eq!(
            saved.read("ws").unwrap()[0]["id"],
            serde_json::json!(entry.id)
        );
    }
}
