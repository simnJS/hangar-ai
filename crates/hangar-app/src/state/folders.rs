//! Navigator order (`src/lib/folders.ts`, tests in folders.test.ts), and the
//! `placeBefore` the moves of store.tsx use.

use std::collections::HashSet;

use super::types::{Workspace, WorkspaceFolder};

/// The workspaces as the navigator lists them: those at the top level, then
/// each folder's in folder order (`sidebarOrder`).
///
/// The shortcuts that step through workspaces follow this rather than the
/// stored order, so Alt+3 is the third row on screen. A collapsed folder still
/// counts: numbering that shifted each time one was folded would be no use.
pub fn sidebar_order<'a>(
    workspaces: &'a [Workspace],
    folders: &[WorkspaceFolder],
) -> Vec<&'a Workspace> {
    let known: HashSet<&str> = folders.iter().map(|folder| folder.id.as_str()).collect();
    let loose = workspaces.iter().filter(|ws| match &ws.folder_id {
        None => true,
        Some(id) => !known.contains(id.as_str()),
    });
    let filed = folders.iter().flat_map(|folder| {
        workspaces
            .iter()
            .filter(move |ws| ws.folder_id.as_deref() == Some(folder.id.as_str()))
    });
    loose.chain(filed).collect()
}

/// Something with an id, for `place_before`.
pub trait Keyed {
    fn key(&self) -> &str;
}

impl Keyed for Workspace {
    fn key(&self) -> &str {
        &self.id
    }
}

impl Keyed for WorkspaceFolder {
    fn key(&self) -> &str {
        &self.id
    }
}

impl Keyed for super::types::SavedCommand {
    fn key(&self) -> &str {
        &self.id
    }
}

/// `item` moved, or added, just before `before`, or last when there is no
/// such entry (`placeBefore`).
pub fn place_before<T: Keyed>(list: &mut Vec<T>, item: T, before: Option<&str>) {
    list.retain(|entry| entry.key() != item.key());
    let at = before.and_then(|before| list.iter().position(|entry| entry.key() == before));
    match at {
        Some(at) => list.insert(at, item),
        None => list.push(item),
    }
}

#[cfg(test)]
mod tests {
    //! folders.test.ts, case by case, and placeBefore.

    use super::*;

    fn ws(id: &str, folder: Option<&str>) -> Workspace {
        Workspace {
            id: id.into(),
            folder_id: folder.map(Into::into),
            ..Workspace::default()
        }
    }
    fn folder(id: &str) -> WorkspaceFolder {
        WorkspaceFolder {
            id: id.into(),
            name: id.into(),
            ..WorkspaceFolder::default()
        }
    }
    fn ids(list: &[&Workspace]) -> Vec<String> {
        list.iter().map(|ws| ws.id.clone()).collect()
    }

    #[test]
    fn lists_the_loose_workspaces_then_each_folder_s_in_folder_order() {
        let workspaces = [
            ws("loose1", None),
            ws("b1", Some("B")),
            ws("a1", Some("A")),
            ws("loose2", None),
            ws("a2", Some("A")),
        ];
        assert_eq!(
            ids(&sidebar_order(&workspaces, &[folder("A"), folder("B")])),
            ["loose1", "loose2", "a1", "a2", "b1"]
        );
    }

    #[test]
    fn follows_the_folders_when_they_are_reordered() {
        let workspaces = [ws("a1", Some("A")), ws("b1", Some("B"))];
        assert_eq!(
            ids(&sidebar_order(&workspaces, &[folder("B"), folder("A")])),
            ["b1", "a1"]
        );
    }

    #[test]
    fn keeps_a_workspace_whose_folder_is_gone_with_the_loose_ones() {
        let workspaces = [
            ws("orphan", Some("deleted")),
            ws("a1", Some("A")),
            ws("loose", None),
        ];
        assert_eq!(
            ids(&sidebar_order(&workspaces, &[folder("A")])),
            ["orphan", "loose", "a1"]
        );
    }

    #[test]
    fn lists_every_workspace_exactly_once() {
        let workspaces = [
            ws("a1", Some("A")),
            ws("x", Some("gone")),
            ws("y", None),
            ws("b1", Some("B")),
        ];
        let mut ordered = ids(&sidebar_order(
            &workspaces,
            &[folder("A"), folder("B"), folder("empty")],
        ));
        ordered.sort();
        let mut all: Vec<String> = workspaces.iter().map(|ws| ws.id.clone()).collect();
        all.sort();
        assert_eq!(ordered, all);
    }

    #[test]
    fn is_the_stored_order_when_there_are_no_folders() {
        let workspaces = [ws("c", None), ws("a", None), ws("b", None)];
        assert_eq!(ids(&sidebar_order(&workspaces, &[])), ["c", "a", "b"]);
    }

    #[test]
    fn places_an_item_before_another_or_last() {
        let mut list = vec![folder("a"), folder("b"), folder("c")];
        place_before(&mut list, folder("c"), Some("a"));
        let order: Vec<&str> = list.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(order, ["c", "a", "b"]);
        place_before(&mut list, folder("c"), None);
        let order: Vec<&str> = list.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(order, ["a", "b", "c"]);
        place_before(&mut list, folder("n"), Some("missing"));
        assert_eq!(list.last().unwrap().id, "n");
    }
}
