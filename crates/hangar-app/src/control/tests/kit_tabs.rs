//! control.test.ts: "a kit's tabs", what control.ts decides around P16's
//! reading of the kits: which workspace, who asked, which tab is opened,
//! focused or replaced, what the agent is told.

use std::sync::Arc;

use serde_json::{Value, json};

use hangar_core::kits::KitInfo;

use super::super::fake::{Fake, lock};
use super::super::port::{EntryKind, KitParams, KitTabCatalog, KitTabToOpen, OpenEntry};
use super::super::run::DISABLED;
use super::super::types::text;
use super::{contains, control, from_ava, from_shop, refusal, with};
use crate::state::types::WorkspaceKit;
use crate::state::{Settings, Workspace};

/// The kit "ci-kit" and its three tabs, as P16's catalog would read them:
/// `run` takes a run's number and a note, `board` is one tab whatever it
/// shows, `plain` takes nothing.
struct CiKit;

const TABS: [(&str, &str, bool, &[&str]); 3] = [
    ("run", "CI run", false, &["run", "note"]),
    ("board", "Board", true, &["view"]),
    ("plain", "Plain", false, &[]),
];

fn enabled(ws: &Workspace, settings: &Settings) -> Vec<String> {
    ws.kits
        .iter()
        .flatten()
        .chain(settings.kits_everywhere.iter().flatten())
        .map(|kit| kit.id.clone())
        .collect()
}

impl KitTabCatalog for CiKit {
    fn listing(&self, ws: &Workspace, settings: &Settings, _installed: &[KitInfo]) -> Vec<Value> {
        if !enabled(ws, settings).contains(&"ci-kit".to_string()) {
            return Vec::new();
        }
        TABS.iter()
            .map(|(tab, title, singleton, params)| {
                json!({"kit": "ci-kit", "kitName": "CI kit", "tab": tab, "title": title,
                       "description": "Shows a CI run.", "singleton": singleton,
                       "params": params.iter().map(|name| (name.to_string(), json!({}))).collect::<serde_json::Map<_, _>>()})
            })
            .collect()
    }

    fn to_open(
        &self,
        ws: &Workspace,
        settings: &Settings,
        _installed: &[KitInfo],
        kit: &str,
        tab: &str,
        raw: Option<&Value>,
    ) -> Result<KitTabToOpen, String> {
        let on = enabled(ws, settings);
        if !on.iter().any(|id| id == kit) {
            let list = if on.is_empty() {
                "No kit is enabled there.".to_string()
            } else {
                format!("Enabled there: {}.", on.join(", "))
            };
            return Err(format!(
                "The kit \"{kit}\" is not enabled on \"{}\". {list} Only the person can enable one.",
                ws.name
            ));
        }
        let Some((_, title, singleton, declared)) = TABS.iter().find(|(id, ..)| *id == tab) else {
            return Err(format!(
                "The kit \"{kit}\" has no tab \"{tab}\". Its tabs: run, board, plain."
            ));
        };
        let mut params = KitParams::new();
        if let Some(Value::Object(raw)) = raw {
            for (name, value) in raw {
                if !declared.contains(&name.as_str()) {
                    return Err(format!("The tab \"{tab}\" declares no param \"{name}\"."));
                }
                params.insert(name.clone(), text(Some(value)).unwrap_or_default());
            }
        }
        Ok(KitTabToOpen {
            title: title.to_string(),
            singleton: *singleton,
            params,
        })
    }
}

/// "Hangar" with the kit enabled, the panel on screen.
fn kit_setup() -> Arc<Fake> {
    let fake = Fake::setup();
    fake.patch("w1", |ws| {
        ws.kits = Some(vec![WorkspaceKit {
            id: "ci-kit".into(),
            granted: vec![],
        }])
    });
    *lock(&fake.catalog) = Some(Arc::new(CiKit));
    fake
}

async fn open(fake: &Arc<Fake>, args: Value) -> Result<Value, String> {
    control(
        "kit_tab_open",
        with(
            with(from_ava(), json!({"kit": "ci-kit", "tab": "run"})),
            args,
        ),
        fake,
    )
    .await
}

async fn close(fake: &Arc<Fake>, args: Value) -> Value {
    control(
        "kit_tab_close",
        with(
            with(from_ava(), json!({"kit": "ci-kit", "tab": "run"})),
            args,
        ),
        fake,
    )
    .await
    .unwrap()
}

fn params_of(entry: &OpenEntry) -> Value {
    match &entry.what {
        EntryKind::Kit { params, .. } => json!(params),
        _ => Value::Null,
    }
}

fn tab_of(entry: &OpenEntry) -> String {
    match &entry.what {
        EntryKind::Kit { tab_id, .. } => tab_id.clone(),
        _ => String::new(),
    }
}

fn active(fake: &Fake) -> Option<String> {
    lock(&fake.preview.state).active.clone()
}

/// `noteModelView(ws, true)`: that workspace on screen with its panel.
fn on_screen(fake: &Fake, here: bool, shown: bool) {
    let mut preview = lock(&fake.preview.state);
    preview.on_screen = here;
    preview.shown = shown;
}

/* ── kit_tab_list ────────────────────────────────────────────── */

#[tokio::test]
async fn lists_the_tabs_of_the_kits_enabled_on_the_callers_workspace() {
    let fake = kit_setup();
    let listed = control("kit_tab_list", from_ava(), &fake).await.unwrap();
    assert_eq!(listed["workspace"], "Hangar");
    let tabs: Vec<_> = listed["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tab| tab["tab"].clone())
        .collect();
    assert_eq!(tabs, vec![json!("run"), json!("board"), json!("plain")]);
}

#[tokio::test]
async fn lists_nothing_for_another_workspace_and_a_kit_on_all_workspaces_everywhere() {
    let fake = kit_setup();
    assert_eq!(
        control("kit_tab_list", from_shop(), &fake).await.unwrap(),
        json!({"workspace": "Shop", "tabs": []})
    );
    lock(&fake.state).settings.kits_everywhere = Some(vec![WorkspaceKit {
        id: "ci-kit".into(),
        granted: vec![],
    }]);
    let listed = control("kit_tab_list", from_shop(), &fake).await.unwrap();
    assert_eq!(listed["tabs"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn is_pane_control_like_the_rest_refused_with_the_setting_off() {
    let fake = kit_setup();
    lock(&fake.state).settings.agent_pane_control = false;
    assert_eq!(refusal("kit_tab_list", from_ava(), &fake).await, DISABLED);
}

/* ── kit_tab_open ────────────────────────────────────────────── */

#[tokio::test]
async fn opens_a_tab_with_its_params_in_the_callers_workspace_the_panel_open() {
    let fake = kit_setup();
    let answer = open(&fake, json!({"params": {"run": "4567"}}))
        .await
        .unwrap();
    assert_eq!(
        answer,
        json!({"outcome": "opened", "kit": "ci-kit", "tab": "run", "params": {"run": "4567"},
               "workspace": "Hangar", "visible": true})
    );
    let tabs = fake.preview.tabs("w1");
    assert_eq!(tabs.len(), 1);
    assert_eq!(params_of(&tabs[0]), json!({"run": "4567"}));
    assert_eq!(tabs[0].title, "CI run");
    assert_eq!(tabs[0].opened_by.as_deref(), Some("Ava"));
    assert_eq!(active(&fake), Some(tabs[0].id.clone()));
    assert!(
        fake.log()
            .contains(&"announce:modelShown:CI run:Ava".into())
    );
}

#[tokio::test]
async fn focuses_the_tab_that_has_the_same_params_whatever_order_they_came_in() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1", "note": "a"}}))
        .await
        .unwrap();
    open(&fake, json!({"params": {"run": "2"}})).await.unwrap();
    let ids: Vec<String> = fake
        .preview
        .tabs("w1")
        .iter()
        .map(|tab| tab.id.clone())
        .collect();
    assert_eq!(active(&fake).as_ref(), Some(&ids[1]));
    let answer = open(&fake, json!({"params": {"note": "a", "run": "1"}}))
        .await
        .unwrap();
    assert_eq!(answer["outcome"], "focused");
    let again: Vec<String> = fake
        .preview
        .tabs("w1")
        .iter()
        .map(|tab| tab.id.clone())
        .collect();
    assert_eq!(again, ids);
    assert_eq!(active(&fake).as_ref(), Some(&ids[0]));
}

#[tokio::test]
async fn opens_a_new_tab_for_other_params_and_one_for_none() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    assert_eq!(
        open(&fake, json!({"params": {"run": "2"}})).await.unwrap()["outcome"],
        "opened"
    );
    assert_eq!(open(&fake, json!({})).await.unwrap()["outcome"], "opened");
    let tabs = fake.preview.tabs("w1");
    assert_eq!(
        tabs.iter().map(params_of).collect::<Vec<_>>(),
        vec![json!({"run": "1"}), json!({"run": "2"}), json!({})]
    );
    let mut paths: Vec<&str> = tabs.iter().map(|tab| tab.path.as_str()).collect();
    paths.dedup();
    assert_eq!(paths.len(), 3);
}

#[tokio::test]
async fn keeps_one_singleton_tab_whose_new_params_replace_the_old() {
    let fake = kit_setup();
    let board = |params: Value| with(json!({"tab": "board"}), params);
    assert_eq!(
        open(&fake, board(json!({"params": {"view": "a"}})))
            .await
            .unwrap()["outcome"],
        "opened"
    );
    let first = fake.preview.tabs("w1")[0].id.clone();
    assert_eq!(
        open(&fake, board(json!({"params": {"view": "b"}})))
            .await
            .unwrap()["outcome"],
        "replaced"
    );
    let tabs = fake.preview.tabs("w1");
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].id, first);
    assert_eq!(params_of(&tabs[0]), json!({"view": "b"}));
    // The same again, or nothing new: brought forward, holding what it holds.
    assert_eq!(
        open(&fake, board(json!({"params": {"view": "b"}})))
            .await
            .unwrap()["outcome"],
        "focused"
    );
    assert_eq!(
        open(&fake, board(json!({}))).await.unwrap()["outcome"],
        "focused"
    );
    assert_eq!(params_of(&fake.preview.tabs("w1")[0]), json!({"view": "b"}));
}

#[tokio::test]
async fn goes_to_the_callers_workspace_whatever_the_request_says() {
    let fake = kit_setup();
    open(
        &fake,
        json!({"workspace": "Shop", "pane": "Ava", "params": {"run": "1"}}),
    )
    .await
    .unwrap();
    assert_eq!(fake.preview.tabs("w1").len(), 1);
    assert!(fake.preview.tabs("w2").is_empty());
}

#[tokio::test]
async fn finds_a_hooks_workspace_by_its_pane_alone_and_names_it_as_who_opened_the_tab() {
    let fake = kit_setup();
    let hook =
        json!({"caller": {"paneId": "p2"}, "kit": "ci-kit", "tab": "run", "params": {"run": "9"}});
    assert_eq!(
        control("kit_tab_open", hook, &fake).await.unwrap()["outcome"],
        "opened"
    );
    assert_eq!(fake.preview.tabs("w1")[0].opened_by.as_deref(), Some("Max"));
}

#[tokio::test]
async fn who_asked_is_what_hangar_knows_of_the_pane_whatever_a_request_claims() {
    let fake = kit_setup();
    control(
        "kit_tab_open",
        json!({"caller": {"paneId": "p2", "paneName": "Boss", "pane": "Boss"}, "pane": "Ava",
               "paneName": "Ava", "openedBy": "Ava", "by": "Ava", "kit": "ci-kit", "tab": "run"}),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(fake.preview.tabs("w1")[0].opened_by.as_deref(), Some("Max"));
}

#[tokio::test]
async fn who_asked_is_nobody_when_the_request_names_no_pane_of_the_workspace() {
    let fake = kit_setup();
    control(
        "kit_tab_open",
        json!({"caller": {"paneId": "p-gone", "paneName": "Boss", "cwd": "C:\\dev\\hangar"},
               "kit": "ci-kit", "tab": "run"}),
        &fake,
    )
    .await
    .unwrap();
    let tabs = fake.preview.tabs("w1");
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].opened_by, None);
}

#[tokio::test]
async fn who_asked_becomes_the_latest_to_ask_when_an_agent_opens_the_same_tab_again() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    let hook =
        json!({"caller": {"paneId": "p2"}, "kit": "ci-kit", "tab": "run", "params": {"run": "1"}});
    assert_eq!(
        control("kit_tab_open", hook, &fake).await.unwrap()["outcome"],
        "focused"
    );
    assert_eq!(fake.preview.tabs("w1")[0].opened_by.as_deref(), Some("Max"));
}

#[tokio::test]
async fn refuses_a_pane_no_workspace_has_without_guessing_one() {
    let fake = kit_setup();
    let hook = json!({"caller": {"paneId": "p-gone"}, "kit": "ci-kit", "tab": "run"});
    contains(
        &refusal("kit_tab_open", hook, &fake).await,
        "The pane \"p-gone\" is in no workspace of Hangar",
    );
    assert!(fake.preview.tabs("w1").is_empty());
}

#[tokio::test]
async fn passes_on_the_catalogs_refusal_of_a_kit_or_a_tab() {
    let fake = kit_setup();
    assert_eq!(
        open(&fake, json!({"kit": "other-kit"})).await.unwrap_err(),
        "The kit \"other-kit\" is not enabled on \"Hangar\". Enabled there: ci-kit. Only the person can enable one."
    );
    fake.patch("w1", |ws| ws.kits = Some(vec![]));
    assert_eq!(
        open(&fake, json!({})).await.unwrap_err(),
        "The kit \"ci-kit\" is not enabled on \"Hangar\". No kit is enabled there. Only the person can enable one."
    );
    let fake = kit_setup();
    assert_eq!(
        open(&fake, json!({"tab": "nope"})).await.unwrap_err(),
        "The kit \"ci-kit\" has no tab \"nope\". Its tabs: run, board, plain."
    );
}

#[tokio::test]
async fn refuses_a_request_without_its_kit_and_tab_or_with_a_reveal_that_is_not_a_boolean() {
    let fake = kit_setup();
    contains(
        &open(&fake, json!({"kit": ""})).await.unwrap_err(),
        "Name the \"kit\" and the \"tab\"",
    );
    contains(
        &open(&fake, json!({"reveal": "no"})).await.unwrap_err(),
        "reveal must be true or false",
    );
    assert!(fake.preview.tabs("w1").is_empty());
}

#[tokio::test]
async fn says_so_when_the_person_cannot_see_it_another_view_or_another_workspace_on_screen() {
    let fake = kit_setup();
    on_screen(&fake, true, false);
    let no_room = open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    assert_eq!(no_room["visible"], false);
    contains(no_room["note"].as_str().unwrap(), "but not its panes");
    on_screen(&fake, false, false);
    let elsewhere = open(&fake, json!({"params": {"run": "2"}})).await.unwrap();
    assert_eq!(elsewhere["visible"], false);
    contains(
        elsewhere["note"].as_str().unwrap(),
        "which is not the workspace on screen",
    );
}

#[tokio::test]
async fn reveal_false_adds_the_tab_behind_a_closed_panel_and_nothing_else_moves() {
    let fake = kit_setup();
    on_screen(&fake, true, false);
    let answer = open(&fake, json!({"params": {"run": "1"}, "reveal": false}))
        .await
        .unwrap();
    assert_eq!(answer["outcome"], "opened");
    assert_eq!(answer["visible"], false);
    assert_eq!(fake.preview.tabs("w1").len(), 1);
    assert!(fake.logged("announce:").is_empty());
}

#[tokio::test]
async fn reveal_false_leaves_what_is_in_front_of_an_open_panel_and_an_open_tab_where_it_is() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    let first = fake.preview.tabs("w1")[0].id.clone();
    open(&fake, json!({"params": {"run": "2"}, "reveal": false}))
        .await
        .unwrap();
    assert_eq!(fake.preview.tabs("w1").len(), 2);
    assert_eq!(active(&fake), Some(first.clone()));
    assert_eq!(
        open(&fake, json!({"params": {"run": "2"}, "reveal": false}))
            .await
            .unwrap()["outcome"],
        "focused"
    );
    assert_eq!(active(&fake), Some(first));
}

#[tokio::test]
async fn reveal_false_still_gives_a_singleton_its_new_params() {
    let fake = kit_setup();
    open(&fake, json!({"tab": "board", "params": {"view": "a"}}))
        .await
        .unwrap();
    on_screen(&fake, true, false);
    let answer = open(
        &fake,
        json!({"tab": "board", "params": {"view": "b"}, "reveal": false}),
    )
    .await
    .unwrap();
    assert_eq!(answer["outcome"], "replaced");
    assert_eq!(params_of(&fake.preview.tabs("w1")[0]), json!({"view": "b"}));
}

/* ── kit_tab_close ───────────────────────────────────────────── */

#[tokio::test]
async fn closes_the_tab_holding_those_very_params() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1", "note": "a"}}))
        .await
        .unwrap();
    open(&fake, json!({"params": {"run": "2"}})).await.unwrap();
    let answer = close(&fake, json!({"params": {"note": "a", "run": "1"}})).await;
    assert_eq!(
        answer,
        json!({"closed": [{"run": "1", "note": "a"}], "open": [{"run": "2"}]})
    );
    assert_eq!(
        fake.preview
            .tabs("w1")
            .iter()
            .map(params_of)
            .collect::<Vec<_>>(),
        vec![json!({"run": "2"})]
    );
    assert!(
        fake.log()
            .contains(&"announce:modelClosed:CI run:Ava".into())
    );
}

#[tokio::test]
async fn without_params_closes_every_tab_of_it_and_nothing_else() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    open(&fake, json!({"params": {"run": "2"}})).await.unwrap();
    open(&fake, json!({"tab": "plain"})).await.unwrap();
    let answer = close(&fake, json!({})).await;
    assert_eq!(answer["closed"].as_array().unwrap().len(), 2);
    assert_eq!(
        fake.preview
            .tabs("w1")
            .iter()
            .map(tab_of)
            .collect::<Vec<_>>(),
        vec!["plain"]
    );
}

#[tokio::test]
async fn closes_nothing_for_params_nothing_holds_and_says_what_is_open() {
    let fake = kit_setup();
    open(&fake, json!({"tab": "board", "params": {"view": "b"}}))
        .await
        .unwrap();
    let answer = close(&fake, json!({"tab": "board", "params": {"view": "a"}})).await;
    assert_eq!(answer, json!({"closed": [], "open": [{"view": "b"}]}));
    assert_eq!(fake.preview.tabs("w1").len(), 1);
    assert!(fake.logged("announce:modelClosed").is_empty());
}

#[tokio::test]
async fn closes_in_the_callers_workspace_whatever_the_request_says() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    let answer = control(
        "kit_tab_close",
        with(from_shop(), json!({"kit": "ci-kit", "tab": "run"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(answer, json!({"closed": [], "open": []}));
    assert_eq!(fake.preview.tabs("w1").len(), 1);
    close(&fake, json!({"workspace": "Shop"})).await;
    assert!(fake.preview.tabs("w1").is_empty());
}

#[tokio::test]
async fn still_closes_the_tab_of_a_kit_taken_off_the_workspace_since() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    fake.patch("w1", |ws| ws.kits = Some(vec![]));
    assert_eq!(
        close(&fake, json!({})).await["closed"],
        json!([{"run": "1"}])
    );
}

/* ── a kit's tab as a pane of the grid ───────────────────────── */

#[tokio::test]
async fn a_tab_an_agent_opens_or_closes_is_a_pane_and_lays_the_workspace_out_again() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    assert_eq!(fake.logged("views:"), ["views:w1:arrange"]);
    // The same tab again is focused where it is: nothing is laid out.
    let again = open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    assert_eq!(again["outcome"], "focused");
    assert_eq!(fake.logged("views:").last().unwrap(), "views:w1:keep");
    close(&fake, json!({"params": {"run": "1"}})).await;
    assert_eq!(fake.logged("views:").last().unwrap(), "views:w1:arrange");
}

#[tokio::test]
async fn a_full_workspace_has_no_room_for_a_new_tab_but_an_open_one_is_still_focused() {
    let fake = kit_setup();
    open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    fake.patch("w1", |ws| {
        while ws.pane_count() < crate::state::types::MAX_PANES {
            let id = format!("v{}", ws.view_panes.len());
            ws.view_panes.push(crate::state::ViewPane::new(id));
        }
    });
    let refused = open(&fake, json!({"params": {"run": "2"}}))
        .await
        .unwrap_err();
    contains(&refused, "is full");
    assert_eq!(fake.preview.tabs("w1").len(), 1, "nothing opened");
    let focused = open(&fake, json!({"params": {"run": "1"}})).await.unwrap();
    assert_eq!(focused["outcome"], "focused");
}
