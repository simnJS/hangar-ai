//! control.test.ts: "model paths", "model_show", "model_snapshot",
//! "model_close", "the model tools beside other kinds of files",
//! "preview_show", "preview_close".

use serde_json::json;

use super::super::fake::{
    Fake, Settle, info, lock, open_file, open_model, pane, workspace, worktree,
};
use super::super::port::{EntryStatus, ModelFileInfo};
use super::super::preview::{RootGrants, base64, model_roots};
use super::super::run::DISABLED;
use super::{contains, control, from_ava, from_shop, matches, refusal, with};
use crate::state::AgentId;

/* ── model paths ─────────────────────────────────────────────── */

#[tokio::test]
async fn allows_the_workspace_its_extra_roots_its_panes_folders_and_its_live_worktrees() {
    let fake = Fake::setup();
    let mut ws = workspace(
        "w1",
        "Hangar",
        "C:\\dev\\hangar",
        vec![
            pane("p1", "Ava", AgentId::Claude, None),
            pane(
                "p2",
                "Max",
                AgentId::Shell,
                Some("C:\\dev\\hangar-wt\\feature"),
            ),
            pane("p3", "Leo", AgentId::Shell, Some("tools")),
        ],
    );
    ws.extra_roots = vec!["D:\\art".into()];
    *lock(&fake.trees) = Some(Box::new(|path: &str| {
        if path == "D:\\art" {
            return Err("not a git repository".into());
        }
        let mut bare = worktree("C:\\dev\\hangar.git", false, None);
        bare.bare = true;
        Ok(vec![
            worktree("C:\\dev\\hangar", true, None),
            worktree("C:\\dev\\hangar-wt\\feature", false, None),
            worktree("C:\\dev\\hangar-wt\\review", false, None),
            worktree("C:\\dev\\hangar-wt\\old", false, Some("gone")),
            bare,
        ])
    }));
    let roots = model_roots(&ws, &fake.port()).await;
    assert_eq!(*lock(&fake.listed), vec!["C:\\dev\\hangar", "D:\\art"]);
    assert_eq!(
        roots,
        vec![
            "C:\\dev\\hangar",
            "D:\\art",
            "C:\\dev\\hangar-wt\\feature",
            "C:\\dev\\hangar\\tools",
            "C:\\dev\\hangar-wt\\review",
        ]
    );
}

#[tokio::test]
async fn does_without_worktrees_when_git_cannot_list_them() {
    let fake = Fake::setup();
    *lock(&fake.trees) = Some(Box::new(|_: &str| Err("git is missing".into())));
    let ws = workspace(
        "w1",
        "Hangar",
        "C:\\dev\\hangar",
        vec![pane("p1", "Ava", AgentId::Claude, None)],
    );
    assert_eq!(
        model_roots(&ws, &fake.port()).await,
        vec!["C:\\dev\\hangar"]
    );
}

#[test]
fn sends_core_every_granted_workspaces_folders_once_each_and_forgets_deleted_ones() {
    let mut grants = RootGrants::default();
    let live = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
    let roots = |list: &[&str]| list.iter().map(|root| root.to_string()).collect::<Vec<_>>();
    let sent = [
        grants.grant(
            "w1",
            roots(&["C:\\dev\\hangar", "C:\\dev\\hangar-wt\\a"]),
            &live(&["w1", "w2"]),
        ),
        grants.grant(
            "w2",
            roots(&["C:\\dev\\shop", "c:/dev/hangar/"]),
            &live(&["w1", "w2"]),
        ),
        grants.grant("w2", roots(&["C:\\dev\\shop"]), &live(&["w2"])),
    ];
    assert_eq!(
        sent,
        [
            roots(&["C:\\dev\\hangar", "C:\\dev\\hangar-wt\\a"]),
            roots(&["C:\\dev\\hangar", "C:\\dev\\hangar-wt\\a", "C:\\dev\\shop"]),
            roots(&["C:\\dev\\shop"]),
        ]
    );
}

#[test]
fn a_snapshot_reaches_the_agent_as_base64() {
    assert_eq!(base64(&super::super::fake::PNG), "iVBORw0KGgo=");
    assert_eq!(base64(b"ab"), "YWI=");
    assert_eq!(base64(b""), "");
}

/* ── model_show ──────────────────────────────────────────────── */

#[tokio::test]
async fn opens_a_model_from_the_callers_folder_announces_it_and_answers_with_what_the_viewer_found()
{
    let fake = Fake::setup();
    let result = control(
        "model_show",
        with(from_ava(), json!({"path": "assets\\chair.glb"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        json!({
            "opened": true,
            "path": "C:\\dev\\hangar\\assets\\chair.glb",
            "format": "glb",
            "title": "chair.glb",
            "info": info(),
            "workspace": "Hangar",
            "visible": true,
        })
    );
    let log = fake.log();
    for line in [
        "host:w1:here",
        "show:C:\\dev\\hangar\\assets\\chair.glb:chair.glb:Ava:w1",
        "announce:modelShown:chair.glb:Ava",
        "settle:m1:45000",
        // Showing a model there is using the workspace.
        "touch:w1",
    ] {
        assert!(log.contains(&line.to_string()), "{line} not in {log:?}");
    }
    assert!(!log.contains(&"open:w1".into()));
}

#[tokio::test]
async fn opens_it_in_the_callers_own_workspace_and_says_so_when_that_one_is_not_on_screen() {
    let fake = Fake::setup();
    {
        let mut preview = lock(&fake.preview.state);
        preview.on_screen = false;
        preview.shown = false;
    }
    let result = control(
        "model_show",
        with(from_shop(), json!({"path": "car.glb"})),
        &fake,
    )
    .await
    .unwrap();
    matches(
        &result,
        json!({"opened": true, "workspace": "Shop", "visible": false, "info": info()}),
    );
    contains(
        result["note"].as_str().unwrap(),
        "open in the preview of \"Shop\", which is not the workspace on screen",
    );
    let log = fake.log();
    assert!(log.contains(&"host:w2:here".into()));
    assert!(log.contains(&"show:C:\\dev\\shop\\car.glb:car.glb:Ava:w2".into()));
    // Hangar is the one on screen: the line at the bottom names Shop.
    assert!(log.contains(&"announce:modelShownElsewhere:car.glb:Ava".into()));
}

#[tokio::test]
async fn says_so_when_the_workspace_is_on_screen_but_its_preview_is_not() {
    let fake = Fake::setup();
    lock(&fake.preview.state).shown = false;
    let result = control(
        "model_show",
        with(from_ava(), json!({"path": "chair.glb"})),
        &fake,
    )
    .await
    .unwrap();
    matches(&result, json!({"workspace": "Hangar", "visible": false}));
    let note = result["note"].as_str().unwrap();
    contains(note, "\"Hangar\" is on screen, but its preview is not");
    contains(note, "toolbar button");
}

#[tokio::test]
async fn wakes_a_sleeping_workspace_it_shows_a_model_in_as_pane_send_does() {
    let fake = Fake::setup();
    lock(&fake.asleep).insert("w2".into());
    let result = control(
        "model_show",
        with(
            from_ava(),
            json!({"workspace": "Shop", "path": "C:\\dev\\shop\\car.glb"}),
        ),
        &fake,
    )
    .await
    .unwrap();
    let log = fake.log();
    assert!(log.contains(&"open:w2".into()) && log.contains(&"touch:w2".into()));
    matches(&result, json!({"workspace": "Shop", "woke": true}));
    contains(
        result["note"].as_str().unwrap(),
        "\"Shop\" was asleep: it is waking up now",
    );
}

#[tokio::test]
async fn reads_a_relative_path_from_the_callers_pane_folder_not_from_the_root_its_server_reports() {
    let fake = Fake::setup();
    fake.patch("w1", |ws| {
        ws.panes = vec![
            pane("p1", "Ava", AgentId::Claude, None),
            pane(
                "p2",
                "Max",
                AgentId::Shell,
                Some("C:\\dev\\hangar-wt\\feature"),
            ),
        ]
    });
    let from_max = json!({"caller": {"paneId": "p2", "paneName": "Max", "cwd": "C:\\dev\\hangar"}});
    let result = control(
        "model_show",
        with(
            from_max,
            json!({"path": "out/ship.gltf", "title": "Ship v2"}),
        ),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        result["path"],
        "C:\\dev\\hangar-wt\\feature\\out\\ship.gltf"
    );
    assert_eq!(result["format"], "gltf");
    assert!(
        fake.log()
            .contains(&"show:C:\\dev\\hangar-wt\\feature\\out\\ship.gltf:Ship v2:Max:w1".into())
    );
}

#[tokio::test]
async fn takes_an_absolute_path_in_any_spelling_and_lets_core_in_on_the_workspace_and_its_worktrees()
 {
    let fake = Fake::setup();
    let result = control(
        "model_show",
        with(
            from_ava(),
            json!({"path": "C:/dev/hangar-wt/feature/a.stl"}),
        ),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(result["path"], "C:\\dev\\hangar-wt\\feature\\a.stl");
    assert!(
        fake.log()
            .contains(&"allow:w1:C:\\dev\\hangar|C:\\dev\\hangar-wt\\feature".into())
    );
}

#[tokio::test]
async fn refuses_what_is_not_a_model_before_asking_anything() {
    let fake = Fake::setup();
    let refused = refusal(
        "model_show",
        with(from_ava(), json!({"path": "README.md"})),
        &fake,
    )
    .await;
    contains(&refused, "not a 3D model");
    contains(&refused, "glb, gltf, obj");
    contains(
        &refusal("model_show", from_ava(), &fake).await,
        "Give the path",
    );
    assert!(fake.log().iter().all(|line| !line.starts_with("allow:")
        && !line.starts_with("stat:")
        && !line.starts_with("show:")));
}

#[tokio::test]
async fn refuses_a_model_outside_the_workspace_however_the_path_gets_there() {
    let fake = Fake::setup();
    let refused = refusal(
        "model_show",
        with(from_ava(), json!({"path": "D:\\downloads\\chair.glb"})),
        &fake,
    )
    .await;
    contains(&refused, "outside your workspace");
    contains(&refused, "C:\\dev\\hangar, C:\\dev\\hangar-wt\\feature");
    contains(
        &refusal(
            "model_show",
            with(from_ava(), json!({"path": "..\\shop\\car.glb"})),
            &fake,
        )
        .await,
        "C:\\dev\\shop\\car.glb is outside your workspace",
    );
    assert!(fake.logged("show:").is_empty());
}

#[tokio::test]
async fn refuses_another_workspaces_model_that_core_would_serve() {
    // Shop showed a model earlier, so its folder is allowed too; Ava's
    // workspace is still the one that counts for Ava.
    let fake = Fake::setup();
    *lock(&fake.stat) = Some(Box::new(|path: &str| {
        Some(Ok(ModelFileInfo {
            path: path.into(),
            size: 10,
            modified_ms: 1,
        }))
    }));
    contains(
        &refusal(
            "model_show",
            with(from_ava(), json!({"path": "C:\\dev\\shop\\car.glb"})),
            &fake,
        )
        .await,
        "outside your workspace",
    );
}

#[tokio::test]
async fn passes_on_why_core_refused_a_file_inside_the_workspace_and_refuses_one_too_big() {
    let fake = Fake::setup();
    assert_eq!(
        refusal(
            "model_show",
            with(from_ava(), json!({"path": "missing.glb"})),
            &fake
        )
        .await,
        "No such file: C:\\dev\\hangar\\missing.glb"
    );
    let refused = refusal(
        "model_show",
        with(from_ava(), json!({"path": "huge.fbx"})),
        &fake,
    )
    .await;
    contains(&refused, "huge.fbx is 300 MB");
    contains(&refused, "up to 256 MB");
    assert!(fake.logged("show:").is_empty());
}

#[tokio::test]
async fn answers_that_the_model_is_still_loading_when_the_viewer_takes_too_long() {
    let fake = Fake::setup();
    lock(&fake.preview.state).settle = Settle::Timeout;
    let result = control(
        "model_show",
        with(from_ava(), json!({"path": "big.glb"})),
        &fake,
    )
    .await
    .unwrap();
    matches(
        &result,
        json!({"opened": true, "status": "loading", "path": "C:\\dev\\hangar\\big.glb"}),
    );
    contains(result["note"].as_str().unwrap(), "model_snapshot");
}

#[tokio::test]
async fn hands_back_the_viewers_error() {
    let fake = Fake::setup();
    lock(&fake.preview.state).settle = Settle::Error;
    assert_eq!(
        refusal(
            "model_show",
            with(from_ava(), json!({"path": "chair.glb"})),
            &fake
        )
        .await,
        "The 3D preview could not load chair.glb: not a glTF file"
    );
}

#[tokio::test]
async fn shows_a_detached_workspaces_model_in_that_workspaces_window() {
    let fake = Fake::setup();
    fake.patch("w1", |ws| ws.detached = Some(true));
    control(
        "model_show",
        with(from_ava(), json!({"path": "chair.glb"})),
        &fake,
    )
    .await
    .unwrap();
    let log = fake.log();
    assert!(log.contains(&"host:w1:own window".into()));
    assert!(!log.contains(&"host:w1:here".into()));
}

#[tokio::test]
async fn says_so_when_that_window_does_not_answer() {
    let fake = Fake::setup();
    *lock(&fake.host_answers) = false;
    fake.patch("w1", |ws| ws.detached = Some(true));
    contains(
        &refusal(
            "model_show",
            with(from_ava(), json!({"path": "chair.glb"})),
            &fake,
        )
        .await,
        "\"Hangar\" is shown in a window of its own, and that window is not answering",
    );
    assert!(fake.logged("announce:").is_empty());
}

#[tokio::test]
async fn is_refused_with_every_other_operation_when_pane_control_is_off() {
    let fake = Fake::setup();
    lock(&fake.state).settings.agent_pane_control = false;
    for (op, args) in [
        ("model_show", with(from_ava(), json!({"path": "chair.glb"}))),
        ("model_snapshot", from_ava()),
        ("model_close", from_ava()),
        ("preview_show", with(from_ava(), json!({"path": "a.png"}))),
        ("preview_close", from_ava()),
    ] {
        assert_eq!(refusal(op, args, &fake).await, DISABLED);
    }
    assert!(fake.log().is_empty());
}

/* ── model_snapshot ──────────────────────────────────────────── */

#[tokio::test]
async fn says_to_show_a_model_first_when_none_is_open() {
    let fake = Fake::setup();
    let refused = refusal("model_snapshot", from_ava(), &fake).await;
    contains(&refused, "No 3D model is open");
    contains(&refused, "model_show");
}

#[tokio::test]
async fn renders_the_model_in_front_at_the_default_size_and_answers_with_the_image_and_its_stats() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a.glb", "w1"),
        open_model("m2", "C:\\dev\\hangar\\b.glb", "w1"),
    ]);
    lock(&fake.preview.state).active = Some("m1".into());
    let result = control("model_snapshot", from_ava(), &fake).await.unwrap();
    assert_eq!(
        result,
        json!({
            "png": "iVBORw0KGgo=",
            "mimeType": "image/png",
            "path": "C:\\dev\\hangar\\a.glb",
            "angle": "iso",
            "width": 1024,
            "height": 768,
            "info": info(),
        })
    );
    assert!(fake.log().contains(&"reveal:m1".into()));
    // Loaded and unchanged on disk: nothing to wait for.
    assert!(fake.logged("settle:").is_empty());
}

#[tokio::test]
async fn picks_a_model_by_path_or_by_file_name_and_clamps_the_size() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a.glb", "w1"),
        open_model("m2", "C:\\dev\\hangar\\art\\b.glb", "w1"),
    ]);
    lock(&fake.preview.state).active = Some("m1".into());
    control(
        "model_snapshot",
        with(
            from_ava(),
            json!({"path": "art/b.glb", "angle": "Top", "width": 5000, "height": 3}),
        ),
        &fake,
    )
    .await
    .unwrap();
    control(
        "model_snapshot",
        with(
            from_ava(),
            json!({"path": "b.glb", "width": "800", "height": 600.4}),
        ),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        fake.logged("snapshot:"),
        vec!["snapshot:m2:top:2048x64", "snapshot:m2:iso:800x600"]
    );
    contains(
        &refusal(
            "model_snapshot",
            with(from_ava(), json!({"path": "c.glb"})),
            &fake,
        )
        .await,
        "No open model matches \"c.glb\"",
    );
    let refused = refusal(
        "model_snapshot",
        with(from_ava(), json!({"angle": "under"})),
        &fake,
    )
    .await;
    contains(&refused, "Unknown angle \"under\"");
    contains(&refused, "iso");
}

#[tokio::test]
async fn loads_a_file_rewritten_since_it_was_shown_before_rendering_it() {
    let fake = Fake::setup();
    fake.grant(&["C:\\dev\\hangar"]);
    fake.preview
        .set(vec![open_model("m1", "C:\\dev\\hangar\\a.glb", "w1")]);
    lock(&fake.mtimes).insert("C:\\dev\\hangar\\a.glb".into(), 250);
    control("model_snapshot", from_ava(), &fake).await.unwrap();
    let order: Vec<String> = fake
        .log()
        .into_iter()
        .filter(|line| {
            line.starts_with("reload:")
                || line.starts_with("settle:")
                || line.starts_with("snapshot:")
        })
        .collect();
    assert_eq!(
        order,
        vec![
            "reload:m1:250",
            "settle:m1:40000",
            "snapshot:m1:iso:1024x768"
        ]
    );
}

#[tokio::test]
async fn waits_for_a_model_still_loading_and_gives_up_with_a_clear_word() {
    let fake = Fake::setup();
    let (ws, mut loading) = open_model("m1", "C:\\dev\\hangar\\a.glb", "w1");
    loading.status = EntryStatus::Loading;
    fake.preview.set(vec![(ws, loading)]);
    lock(&fake.preview.state).settle = Settle::Timeout;
    contains(
        &refusal("model_snapshot", from_ava(), &fake).await,
        "still loading after 40 s",
    );
    lock(&fake.preview.state).settle = Settle::Error;
    contains(
        &refusal("model_snapshot", from_ava(), &fake).await,
        "a.glb failed to load: not a glTF file",
    );
}

#[tokio::test]
async fn explains_a_preview_that_did_not_come_up() {
    let fake = Fake::setup();
    fake.preview
        .set(vec![open_model("m1", "C:\\dev\\hangar\\a.glb", "w1")]);
    lock(&fake.preview.state).snapshot_error = Some("the 3D preview is not mounted".into());
    contains(
        &refusal("model_snapshot", from_ava(), &fake).await,
        "did not come up in the window that shows \"Hangar\"",
    );
}

#[tokio::test]
async fn renders_the_callers_workspaces_model_never_another_workspaces() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a.glb", "w1"),
        open_model("m2", "C:\\dev\\shop\\car.glb", "w2"),
    ]);
    lock(&fake.preview.state).active = Some("m2".into());
    let result = control("model_snapshot", from_shop(), &fake).await.unwrap();
    assert_eq!(result["path"], "C:\\dev\\shop\\car.glb");
    assert!(fake.log().contains(&"host:w2:here".into()));
    assert!(fake.log().contains(&"snapshot:m2:iso:1024x768".into()));
    contains(
        &refusal(
            "model_snapshot",
            with(from_shop(), json!({"path": "C:\\dev\\hangar\\a.glb"})),
            &fake,
        )
        .await,
        "No open model matches",
    );
    assert_eq!(fake.logged("snapshot:"), vec!["snapshot:m2:iso:1024x768"]);
}

/* ── model_close ─────────────────────────────────────────────── */

#[tokio::test]
async fn closes_one_model_by_path_and_announces_it() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a.glb", "w1"),
        open_model("m2", "C:\\dev\\hangar\\b.glb", "w1"),
    ]);
    let result = control(
        "model_close",
        with(from_ava(), json!({"path": ".\\b.glb"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        json!({"closed": ["C:\\dev\\hangar\\b.glb"], "open": 1})
    );
    assert!(fake.log().contains(&"closeModels:m2".into()));
    assert!(
        fake.log()
            .contains(&"announce:modelClosed:b.glb:Ava".into())
    );
}

#[tokio::test]
async fn finds_a_model_however_the_agent_spells_its_path_through_cores_canonical_form() {
    let fake = Fake::setup();
    // Two files of the same name: only the canonical path can tell them apart.
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a\\m.glb", "w1"),
        open_model("m2", "C:\\dev\\hangar\\b\\m.glb", "w1"),
    ]);
    *lock(&fake.stat) = Some(Box::new(|path: &str| {
        path.contains("/c/dev/hangar/b/m.glb").then(|| {
            Ok(ModelFileInfo {
                path: "C:\\dev\\hangar\\b\\m.glb".into(),
                size: 1,
                modified_ms: 1,
            })
        })
    }));
    let result = control(
        "model_close",
        with(from_ava(), json!({"path": "/c/dev/hangar/b/m.glb"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        json!({"closed": ["C:\\dev\\hangar\\b\\m.glb"], "open": 1})
    );
    assert!(fake.log().contains(&"closeModels:m2".into()));
}

#[tokio::test]
async fn closes_all_of_the_callers_workspaces_models_and_leaves_the_others() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a.glb", "w1"),
        open_model("m2", "C:\\dev\\shop\\car.glb", "w2"),
        open_model("m3", "C:\\dev\\hangar\\b.glb", "w1"),
    ]);
    let result = control("model_close", from_ava(), &fake).await.unwrap();
    assert_eq!(
        result,
        json!({"closed": ["C:\\dev\\hangar\\a.glb", "C:\\dev\\hangar\\b.glb"], "open": 0})
    );
    assert!(fake.log().contains(&"closeModels:m1,m3".into()));
    assert!(
        fake.log()
            .contains(&"announce:modelClosed:a.glb, b.glb:Ava".into())
    );
    assert_eq!(fake.preview.ids(), vec!["m2"]);
}

#[tokio::test]
async fn closes_in_the_callers_workspace_only_and_names_it_when_it_is_not_on_screen() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\car.glb", "w1"),
        open_model("m2", "C:\\dev\\shop\\car.glb", "w2"),
    ]);
    let result = control(
        "model_close",
        with(from_shop(), json!({"path": "car.glb"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        json!({"closed": ["C:\\dev\\shop\\car.glb"], "open": 0})
    );
    assert!(fake.log().contains(&"closeModels:m2".into()));
    assert!(
        fake.log()
            .contains(&"announce:modelClosedElsewhere:car.glb:Ava".into())
    );
    assert_eq!(fake.preview.ids(), vec!["m1"]);
}

#[tokio::test]
async fn has_nothing_to_do_when_nothing_is_open_but_refuses_a_path_that_is_not() {
    let fake = Fake::setup();
    assert_eq!(
        control("model_close", from_ava(), &fake).await.unwrap(),
        json!({"closed": [], "open": 0})
    );
    contains(
        &refusal(
            "model_close",
            with(from_ava(), json!({"path": "a.glb"})),
            &fake,
        )
        .await,
        "No 3D model is open",
    );
    assert!(fake.logged("announce:").is_empty());
}

/* ── the model tools beside other kinds of files ────────────── */

#[tokio::test]
async fn snapshot_the_last_model_when_a_file_is_in_front_and_close_only_models() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a.glb", "w1"),
        open_file("f1", "C:\\dev\\hangar\\shot.png", "image", "w1"),
    ]);
    lock(&fake.preview.state).active = Some("f1".into());
    matches(
        &control("model_snapshot", from_ava(), &fake).await.unwrap(),
        json!({"path": "C:\\dev\\hangar\\a.glb"}),
    );
    assert!(fake.log().contains(&"snapshot:m1:iso:1024x768".into()));
    contains(
        &refusal(
            "model_snapshot",
            with(from_ava(), json!({"path": "shot.png"})),
            &fake,
        )
        .await,
        "No open model matches \"shot.png\"",
    );
    assert_eq!(
        control("model_close", from_ava(), &fake).await.unwrap(),
        json!({"closed": ["C:\\dev\\hangar\\a.glb"], "open": 0})
    );
    assert_eq!(fake.preview.ids(), vec!["f1"]);
}

#[tokio::test]
async fn say_no_model_is_open_when_only_other_files_are() {
    let fake = Fake::setup();
    fake.preview.set(vec![open_file(
        "f1",
        "C:\\dev\\hangar\\notes.md",
        "markdown",
        "w1",
    )]);
    contains(
        &refusal("model_snapshot", from_ava(), &fake).await,
        "No 3D model is open in \"Hangar\"",
    );
    assert_eq!(
        control("model_close", from_ava(), &fake).await.unwrap(),
        json!({"closed": [], "open": 0})
    );
}

/* ── preview_show ────────────────────────────────────────────── */

#[tokio::test]
async fn opens_a_file_from_the_callers_folder_announces_it_and_answers_with_its_kind_name_and_size()
{
    let fake = Fake::setup();
    let result = control(
        "preview_show",
        with(from_ava(), json!({"path": "out\\shot.png"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        json!({
            "opened": true,
            "kind": "image",
            "name": "shot.png",
            "size": 4096,
            "path": "C:\\dev\\hangar\\out\\shot.png",
            "mime": "image/png",
            "title": "shot.png",
            "workspace": "Hangar",
            "visible": true,
        })
    );
    let log = fake.log();
    for line in [
        "allow:w1:C:\\dev\\hangar|C:\\dev\\hangar-wt\\feature",
        "inspect:C:\\dev\\hangar\\out\\shot.png",
        "show:C:\\dev\\hangar\\out\\shot.png:shot.png:Ava:w1",
        "announce:modelShown:shot.png:Ava",
        "touch:w1",
    ] {
        assert!(log.contains(&line.to_string()), "{line} not in {log:?}");
    }
    // Nothing to load before answering: its viewer reads it on screen.
    assert!(fake.logged("settle:").is_empty());
}

#[tokio::test]
async fn opens_each_kind_in_a_tab_of_its_own_one_per_path_the_last_one_in_front() {
    let fake = Fake::setup();
    for path in [
        "notes.md",
        "report.pdf",
        "site\\index.html",
        "clip.mp4",
        "theme.ogg",
        "run.log",
    ] {
        control(
            "preview_show",
            with(from_ava(), json!({"path": path})),
            &fake,
        )
        .await
        .unwrap();
    }
    let shown: Vec<(String, String, u64)> = lock(&fake.preview.state)
        .entries
        .iter()
        .map(|(_, entry)| {
            let kind = match &entry.what {
                super::super::port::EntryKind::File { kind, .. } => kind.clone(),
                _ => String::new(),
            };
            (kind, entry.title.clone(), entry.modified_ms)
        })
        .collect();
    let expected = [
        ("markdown", "notes.md"),
        ("pdf", "report.pdf"),
        ("html", "index.html"),
        ("video", "clip.mp4"),
        ("audio", "theme.ogg"),
        ("text", "run.log"),
    ];
    assert_eq!(
        shown,
        expected
            .iter()
            // Core's mtime, which the panel watches to read a rewritten file again.
            .map(|(kind, title)| (kind.to_string(), title.to_string(), 100))
            .collect::<Vec<_>>()
    );
    let again = control(
        "preview_show",
        with(from_ava(), json!({"path": "./notes.md", "title": "Notes"})),
        &fake,
    )
    .await
    .unwrap();
    matches(&again, json!({"kind": "markdown", "title": "Notes"}));
    let preview = lock(&fake.preview.state);
    assert_eq!(preview.entries.len(), 6);
    assert_eq!(
        preview.active.as_deref(),
        Some(preview.entries[0].1.id.as_str())
    );
}

#[tokio::test]
async fn sends_a_3d_model_through_model_shows_own_load_and_answers_with_what_the_viewer_found() {
    let fake = Fake::setup();
    let result = control(
        "preview_show",
        with(from_ava(), json!({"path": "assets/chair.glb"})),
        &fake,
    )
    .await
    .unwrap();
    matches(
        &result,
        json!({
            "kind": "model",
            "name": "chair.glb",
            "size": 4096,
            "opened": true,
            "path": "C:\\dev\\hangar\\assets\\chair.glb",
            "format": "glb",
            "info": info(),
        }),
    );
    // What preview_inspect found is the model's stat: nothing is asked twice.
    assert!(fake.logged("stat:").is_empty());
    let log = fake.log();
    assert!(log.contains(&"show:C:\\dev\\hangar\\assets\\chair.glb:chair.glb:Ava:w1".into()));
    assert!(log.contains(&"settle:m1:45000".into()));
}

#[tokio::test]
async fn refuses_without_a_path_outside_the_workspace_and_what_core_will_not_show_or_no_viewer_draws()
 {
    let fake = Fake::setup();
    contains(
        &refusal("preview_show", from_ava(), &fake).await,
        "Give the path of the file",
    );
    contains(
        &refusal(
            "preview_show",
            with(from_ava(), json!({"path": "D:\\downloads\\shot.png"})),
            &fake,
        )
        .await,
        "outside your workspace. The preview only opens files from C:\\dev\\hangar",
    );
    contains(
        &refusal(
            "preview_show",
            with(from_ava(), json!({"path": "..\\shop\\logo.png"})),
            &fake,
        )
        .await,
        "C:\\dev\\shop\\logo.png is outside your workspace",
    );
    assert_eq!(
        refusal(
            "preview_show",
            with(from_ava(), json!({"path": "setup.exe"})),
            &fake
        )
        .await,
        "The preview does not show .exe files."
    );
    assert_eq!(
        refusal(
            "preview_show",
            with(from_ava(), json!({"path": "missing.png"})),
            &fake
        )
        .await,
        "No such file: C:\\dev\\hangar\\missing.png"
    );
    contains(
        &refusal(
            "preview_show",
            with(from_ava(), json!({"path": "dump.xyz"})),
            &fake,
        )
        .await,
        "no viewer for dump.xyz (binary). It shows: model, image, video",
    );
    assert!(
        fake.log()
            .iter()
            .all(|line| !line.starts_with("show:") && !line.starts_with("announce:"))
    );
}

#[tokio::test]
async fn opens_in_the_callers_own_workspace_wakes_it_if_it_sleeps_and_says_it_is_not_on_screen() {
    let fake = Fake::setup();
    lock(&fake.asleep).insert("w2".into());
    {
        let mut preview = lock(&fake.preview.state);
        preview.on_screen = false;
        preview.shown = false;
    }
    let result = control(
        "preview_show",
        with(from_shop(), json!({"path": "logo.png"})),
        &fake,
    )
    .await
    .unwrap();
    matches(
        &result,
        json!({"kind": "image", "workspace": "Shop", "visible": false, "woke": true}),
    );
    let note = result["note"].as_str().unwrap();
    contains(note, "\"Shop\" was asleep");
    contains(note, "open in the preview of \"Shop\"");
    let log = fake.log();
    assert!(log.contains(&"open:w2".into()));
    assert!(log.contains(&"show:C:\\dev\\shop\\logo.png:logo.png:Ava:w2".into()));
    assert!(log.contains(&"announce:modelShownElsewhere:logo.png:Ava".into()));
}

#[tokio::test]
async fn goes_through_a_detached_workspaces_own_window_and_says_so_when_it_does_not_answer() {
    let fake = Fake::setup();
    fake.patch("w1", |ws| ws.detached = Some(true));
    control(
        "preview_show",
        with(from_ava(), json!({"path": "notes.md"})),
        &fake,
    )
    .await
    .unwrap();
    assert!(fake.log().contains(&"host:w1:own window".into()));
    *lock(&fake.host_answers) = false;
    contains(
        &refusal(
            "preview_show",
            with(from_ava(), json!({"path": "notes.md"})),
            &fake,
        )
        .await,
        "\"Hangar\" is shown in a window of its own, and that window is not answering, so the preview",
    );
}

/* ── preview_close ───────────────────────────────────────────── */

#[tokio::test]
async fn closes_one_file_by_path_or_by_name_whatever_its_kind_and_announces_it() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a.glb", "w1"),
        open_file("f1", "C:\\dev\\hangar\\shot.png", "image", "w1"),
        open_file("f2", "C:\\dev\\hangar\\docs\\notes.md", "markdown", "w1"),
    ]);
    assert_eq!(
        control(
            "preview_close",
            with(from_ava(), json!({"path": "docs/notes.md"})),
            &fake
        )
        .await
        .unwrap(),
        json!({"closed": ["C:\\dev\\hangar\\docs\\notes.md"], "open": 2})
    );
    assert!(fake.log().contains(&"closeModels:f2".into()));
    assert!(
        fake.log()
            .contains(&"announce:modelClosed:notes.md:Ava".into())
    );
    assert_eq!(
        control(
            "preview_close",
            with(from_ava(), json!({"path": "a.glb"})),
            &fake
        )
        .await
        .unwrap(),
        json!({"closed": ["C:\\dev\\hangar\\a.glb"], "open": 1})
    );
    assert_eq!(fake.preview.ids(), vec!["f1"]);
}

#[tokio::test]
async fn closes_everything_in_the_callers_workspace_models_included_and_leaves_the_others() {
    let fake = Fake::setup();
    fake.preview.set(vec![
        open_model("m1", "C:\\dev\\hangar\\a.glb", "w1"),
        open_file("f1", "C:\\dev\\shop\\logo.png", "image", "w2"),
        open_file("f2", "C:\\dev\\hangar\\page.html", "html", "w1"),
    ]);
    assert_eq!(
        control("preview_close", from_ava(), &fake).await.unwrap(),
        json!({"closed": ["C:\\dev\\hangar\\a.glb", "C:\\dev\\hangar\\page.html"], "open": 0})
    );
    assert!(fake.log().contains(&"closeModels:m1,f2".into()));
    assert!(
        fake.log()
            .contains(&"announce:modelClosed:a.glb, page.html:Ava".into())
    );
    assert_eq!(fake.preview.ids(), vec!["f1"]);
}

#[tokio::test]
async fn has_nothing_to_close_in_an_empty_preview_and_refuses_a_path_that_is_not_open() {
    let fake = Fake::setup();
    assert_eq!(
        control("preview_close", from_ava(), &fake).await.unwrap(),
        json!({"closed": [], "open": 0})
    );
    assert_eq!(
        refusal(
            "preview_close",
            with(from_ava(), json!({"path": "a.png"})),
            &fake
        )
        .await,
        "Nothing is open in the preview of \"Hangar\", so nothing matches \"a.png\"."
    );
    fake.preview.set(vec![open_file(
        "f1",
        "C:\\dev\\hangar\\shot.png",
        "image",
        "w1",
    )]);
    contains(
        &refusal(
            "preview_close",
            with(from_ava(), json!({"path": "other.png"})),
            &fake,
        )
        .await,
        "No open file matches \"other.png\". Open: C:\\dev\\hangar\\shot.png",
    );
    assert!(fake.logged("announce:").is_empty());
}
