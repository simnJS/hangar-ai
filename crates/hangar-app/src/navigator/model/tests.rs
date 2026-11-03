//! The cases of `NavigatorModel.test.ts` and `folders.test.ts`, with the same
//! workspaces and the same expectations.

use super::*;
use crate::state::layout;
use crate::state::types::{AgentId, SplitDir, SplitNode};

fn pane(id: &str, name: &str, agent: AgentId) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent,
        ..Pane::default()
    }
}

fn claude(id: &str, name: &str) -> Pane {
    pane(id, name, AgentId::Claude)
}

fn ws(id: &str, panes: Vec<Pane>, folder: Option<&str>) -> Workspace {
    Workspace {
        id: id.into(),
        name: id.into(),
        cwd: format!("C:\\dev\\{id}"),
        panes,
        folder_id: folder.map(str::to_string),
        ..Workspace::default()
    }
}

fn folder(id: &str, collapsed: bool) -> WorkspaceFolder {
    WorkspaceFolder {
        id: id.into(),
        name: id.into(),
        collapsed,
        ..WorkspaceFolder::default()
    }
}

fn leaf(id: &str) -> SplitNode {
    SplitNode::Pane { id: id.into() }
}

struct World {
    workspaces: Vec<Workspace>,
    folders: Vec<WorkspaceFolder>,
}

fn world() -> World {
    World {
        workspaces: vec![
            ws(
                "hangar",
                vec![
                    claude("p1", "Ava"),
                    pane("p2", "Ben", AgentId::Shell),
                    claude("p3", "Cléo"),
                    pane("p4", "Dan", AgentId::Shell),
                ],
                None,
            ),
            ws(
                "storefront",
                vec![claude("p5", "Eve"), claude("p6", "Finn")],
                Some("clients"),
            ),
            ws("docs", vec![claude("p7", "Gus")], None),
        ],
        folders: vec![folder("clients", false)],
    }
}

fn tree<'a>(
    world: &'a World,
    query: &str,
    attention_only: bool,
    wants_you: &dyn Fn(&str) -> bool,
) -> NavTree<'a> {
    let all: Vec<&Workspace> = world.workspaces.iter().collect();
    build_nav_tree(&NavInput {
        workspaces: &all,
        folders: &world.folders,
        query,
        attention_only,
        wants_you,
    })
}

fn keys(world: &World, query: &str) -> Vec<String> {
    tree(world, query, false, &|_| false)
        .rows
        .into_iter()
        .map(|row| row.key)
        .collect()
}

fn keys_wanting(world: &World, query: &str, wants: &dyn Fn(&str) -> bool) -> Vec<String> {
    tree(world, query, true, wants)
        .rows
        .into_iter()
        .map(|row| row.key)
        .collect()
}

fn strs(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---- readingOrder -----------------------------------------------------------------------------
//
// The order is `state::layout`'s; these keep the cases the navigator and the
// toolbar rely on from `NavigatorModel.test.ts`, asked of the real function.

fn reading_order(ws: &Workspace) -> Vec<&Pane> {
    layout::reading_order(&ws.panes, ws.tree.as_ref())
}

fn closed_by_preset(ws: &Workspace, count: u8) -> Vec<&Pane> {
    layout::closed_by_preset(&ws.panes, ws.tree.as_ref(), usize::from(count))
}

#[test]
fn reading_order_follows_the_split_tree_rather_than_the_stored_order() {
    let mut w = ws("w", vec![claude("a", "Ava"), claude("b", "Ben")], None);
    w.tree = Some(SplitNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        a: Box::new(leaf("b")),
        b: Box::new(leaf("a")),
    });
    let names: Vec<&str> = reading_order(&w).iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Ben", "Ava"]);
}

#[test]
fn reading_order_is_empty_for_a_workspace_without_panes() {
    assert!(reading_order(&ws("empty", vec![], None)).is_empty());
}

#[test]
fn reading_order_repairs_a_tree_that_no_longer_matches_the_panes() {
    // A leaf that is no pane any more goes, a pane the tree lacks comes last:
    // `normalizeTree` keeps the order the person had.
    let mut w = ws(
        "w",
        vec![claude("a", "Ava"), claude("b", "Ben"), claude("c", "Cleo")],
        None,
    );
    w.tree = Some(SplitNode::Split {
        dir: SplitDir::Row,
        ratio: 0.5,
        a: Box::new(leaf("gone")),
        b: Box::new(SplitNode::Split {
            dir: SplitDir::Col,
            ratio: 0.5,
            a: Box::new(leaf("b")),
            b: Box::new(leaf("a")),
        }),
    });
    let names: Vec<&str> = reading_order(&w).iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Ben", "Ava", "Cleo"]);
}

#[test]
fn a_preset_closes_the_panes_read_last() {
    let w = ws(
        "w",
        (1..=5)
            .map(|n| claude(&format!("p{n}"), &format!("N{n}")))
            .collect(),
        None,
    );
    let closed: Vec<&str> = closed_by_preset(&w, 2)
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(closed, ["p3", "p4", "p5"]);
    assert!(closed_by_preset(&w, 8).is_empty());
    // A size of 0 still keeps one pane.
    assert_eq!(closed_by_preset(&w, 0).len(), 4);
}

// ---- fold ---------------------------------------------------------------------------------------

#[test]
fn fold_ignores_case_and_accents() {
    assert_eq!(fold("Cléo À TOI"), "cleo a toi");
}

// ---- namesakeHints --------------------------------------------------------------------------------

fn at(id: &str, name: &str, cwd: &str) -> Workspace {
    Workspace {
        id: id.into(),
        name: name.into(),
        cwd: cwd.into(),
        ..Workspace::default()
    }
}

fn hints(list: &[Workspace]) -> Vec<(String, String)> {
    let refs: Vec<&Workspace> = list.iter().collect();
    let mut found: Vec<_> = namesake_hints(&refs).into_iter().collect();
    found.sort();
    found
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn says_nothing_for_a_name_no_other_workspace_has() {
    let list = [
        at("a", "api", "C:\\work\\api"),
        at("b", "web", "C:\\work\\web"),
    ];
    assert!(hints(&list).is_empty());
}

#[test]
fn names_the_folder_that_differs_and_only_that_one() {
    let list = [
        at("a", "api", "C:\\work\\api"),
        at("b", "API", "C:\\perso\\api"),
        at("c", "web", "C:\\web"),
    ];
    assert_eq!(hints(&list), pairs(&[("a", "work"), ("b", "perso")]));
}

#[test]
fn passes_over_a_folder_that_only_repeats_the_name() {
    let list = [
        at("a", "api", "C:\\dev\\api"),
        at("b", "api", "C:\\dev\\api-v2"),
    ];
    assert_eq!(hints(&list), pairs(&[("b", "api-v2")]));
}

#[test]
fn numbers_namesakes_opened_on_the_same_folder() {
    let list = [
        at("a", "api", "C:\\dev\\api"),
        at("b", "api", "C:/dev/api/"),
        at("c", "api", "D:\\x\\api"),
    ];
    assert_eq!(hints(&list), pairs(&[("a", "1"), ("b", "2"), ("c", "x")]));
}

// ---- buildNavTree without a filter -----------------------------------------------------------------

#[test]
fn lists_the_loose_workspaces_first_then_each_folder_with_its_own() {
    assert_eq!(
        keys(&world(), ""),
        strs(&["ws:hangar", "ws:docs", "folder:clients", "ws:storefront"])
    );
}

#[test]
fn lists_workspaces_only_never_their_agents() {
    let w = world();
    let rows = tree(&w, "", false, &|_| false).rows;
    assert!(
        rows.iter()
            .all(|row| matches!(row.kind, RowKind::Ws | RowKind::Folder))
    );
    assert!(!rows.iter().any(|row| row.key.starts_with("pane:")));
}

#[test]
fn keeps_an_empty_folder_and_a_collapsed_one_shut() {
    let mut w = world();
    w.folders = vec![folder("clients", true), folder("empty", false)];
    let t = tree(&w, "", false, &|_| false);
    let summary: Vec<(String, bool, usize)> = t
        .folders
        .iter()
        .map(|f| (f.folder.id.clone(), f.expanded, f.size))
        .collect();
    assert_eq!(
        summary,
        vec![("clients".into(), false, 1), ("empty".into(), true, 0)]
    );
    assert!(!t.rows.iter().any(|row| row.key == "ws:storefront"));
}

#[test]
fn files_a_workspace_whose_folder_is_gone_with_the_loose_ones() {
    let w = World {
        workspaces: vec![ws("orphan", vec![], Some("deleted"))],
        folders: vec![folder("clients", false)],
    };
    assert_eq!(keys(&w, ""), strs(&["ws:orphan", "folder:clients"]));
}

#[test]
fn records_each_rows_parent_for_the_keyboard() {
    let w = world();
    let rows = tree(&w, "", false, &|_| false).rows;
    let find = |key: &str| rows.iter().find(|row| row.key == key).unwrap();
    assert_eq!(
        find("ws:storefront").parent.as_deref(),
        Some("folder:clients")
    );
    assert_eq!(find("ws:docs").parent, None);
    let clients = find("folder:clients");
    assert_eq!(clients.parent, None);
    assert!(clients.expanded);
    assert!(clients.foldable);
}

// ---- buildNavTree with a query -------------------------------------------------------------------

#[test]
fn finds_a_workspace_by_its_name_without_case_or_accents() {
    assert_eq!(keys(&world(), "DOCS"), strs(&["ws:docs"]));
    assert_eq!(keys(&world(), "hang"), strs(&["ws:hangar"]));
}

#[test]
fn finds_a_workspace_through_one_of_its_agents_by_name_or_kind() {
    assert_eq!(
        keys(&world(), "eve"),
        strs(&["folder:clients", "ws:storefront"])
    );
    assert_eq!(keys(&world(), "cleo"), strs(&["ws:hangar"]));
    // The shells of Hangar are the only ones.
    assert_eq!(keys(&world(), "SHELL"), strs(&["ws:hangar"]));
}

#[test]
fn shows_a_folder_whose_name_matches_open_with_all_it_holds() {
    let mut w = world();
    w.folders = vec![folder("clients", true)];
    let t = tree(&w, "client", false, &|_| false);
    assert_eq!(t.folders.len(), 1);
    assert!(t.folders[0].expanded);
    assert_eq!(t.folders[0].size, 1);
    let keys: Vec<&str> = t.rows.iter().map(|row| row.key.as_str()).collect();
    assert_eq!(keys, ["folder:clients", "ws:storefront"]);
}

#[test]
fn holds_folders_open_and_unfoldable_while_it_filters() {
    let mut w = world();
    w.folders = vec![folder("clients", true)];
    let t = tree(&w, "eve", false, &|_| false);
    assert_eq!(t.rows[0].key, "folder:clients");
    assert!(t.rows[0].expanded);
    assert!(!t.rows[0].foldable);
}

#[test]
fn hides_folders_with_nothing_that_matches_and_says_it_is_filtering() {
    let w = world();
    let t = tree(&w, "zzz", false, &|_| false);
    assert!(t.folders.is_empty());
    assert!(t.loose.is_empty());
    assert!(t.rows.is_empty());
    assert!(t.filtering);
}

#[test]
fn ignores_blanks_around_the_query() {
    let w = world();
    assert!(!tree(&w, "   ", false, &|_| false).filtering);
}

// ---- buildNavTree with only the agents that want you ----------------------------------------------

#[test]
fn keeps_only_the_workspaces_where_one_wants_you() {
    let w = world();
    let wants = |id: &str| id == "p2" || id == "p5";
    assert_eq!(
        keys_wanting(&w, "", &wants),
        strs(&["ws:hangar", "folder:clients", "ws:storefront"])
    );
}

#[test]
fn drops_the_active_workspace_too_when_nobody_in_it_waits() {
    let w = world();
    assert_eq!(keys_wanting(&w, "", &|id| id == "p7"), strs(&["ws:docs"]));
}

#[test]
fn attention_combines_with_the_query() {
    let w = world();
    let wants = |id: &str| id == "p2" || id == "p5";
    assert_eq!(keys_wanting(&w, "ben", &wants), strs(&["ws:hangar"]));
    assert!(keys_wanting(&w, "gus", &wants).is_empty());
}

#[test]
fn does_not_keep_a_folder_for_its_name_alone() {
    let w = world();
    assert!(keys_wanting(&w, "clients", &|_| false).is_empty());
}
