//! The folders the preview may read from, and how a path an agent wrote is
//! resolved against them (`createRootGrants` of `lib/modelHost.ts`,
//! `modelRoots` and `resolvePath` of `lib/control.ts`).
//!
//! hangar-core holds one list for the whole process (`model_allow_roots`
//! replaces it), and a file stays open after the call that showed it: its
//! viewer keeps reading it and polling it for changes. Replacing the list with
//! only the latest caller's folders would cut every other workspace's files
//! off, so the union of what each live workspace was granted is sent.

use std::path::{Component, Path, PathBuf};

use crate::state::Workspace;

/// What each workspace was granted, in the order they asked.
#[derive(Debug, Default)]
pub struct RootGrants {
    granted: Vec<(String, Vec<String>)>,
}

/// A path as it is compared: the same folder written with the other slash, in
/// the other case or with a trailing one is the same folder.
pub fn comparable(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

impl RootGrants {
    /// Remembers `roots` for `workspace`, forgets the workspaces that are not
    /// in `live`, and answers the union to send: each folder once.
    pub fn grant(&mut self, workspace: &str, roots: Vec<String>, live: &[String]) -> Vec<String> {
        self.granted.retain(|(id, _)| live.iter().any(|l| l == id));
        match self.granted.iter_mut().find(|(id, _)| id == workspace) {
            Some((_, held)) => *held = roots,
            None => self.granted.push((workspace.to_string(), roots)),
        }
        let mut seen = std::collections::HashSet::new();
        let mut union = Vec::new();
        for root in self.granted.iter().flat_map(|(_, roots)| roots) {
            if seen.insert(comparable(root)) {
                union.push(root.clone());
            }
        }
        union
    }
}

/// `given` as an absolute path: itself when it is one, else beside `base`,
/// with its `.` and `..` resolved without touching the disk
/// (`resolvePath`). A Windows drive path counts as absolute everywhere, so a
/// path written for the other platform is not glued to the folder.
pub fn resolve_path(base: &str, given: &str) -> String {
    let given = given.trim();
    let drive = given.as_bytes().get(1) == Some(&b':')
        && given.as_bytes()[0].is_ascii_alphabetic()
        && matches!(given.as_bytes().get(2), Some(b'/' | b'\\'));
    if Path::new(given).is_absolute() || drive || given.starts_with("\\\\") {
        return given.to_string();
    }
    normalize(&Path::new(base).join(given))
        .to_string_lossy()
        .into_owned()
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether `path` is one of `roots` or under one.
pub fn inside(path: &str, roots: &[String]) -> bool {
    let path = comparable(path);
    roots.iter().any(|root| {
        let root = comparable(root);
        !root.is_empty() && (path == root || path.starts_with(&format!("{root}/")))
    })
}

/// The folders an agent in `workspace` may show files from: its folder, the
/// other roots of a `.code-workspace`, and the folders its panes work in
/// (a worker's worktree is a pane's folder). The worktrees git lists for
/// them are added by the caller, which has to ask git.
pub fn workspace_roots(workspace: &Workspace) -> Vec<String> {
    let panes = workspace
        .panes
        .iter()
        .filter_map(|pane| pane.cwd.as_deref())
        .filter(|cwd| !cwd.trim().is_empty())
        .map(|cwd| resolve_path(&workspace.cwd, cwd));
    let mut seen = std::collections::HashSet::new();
    std::iter::once(workspace.cwd.clone())
        .chain(workspace.extra_roots.iter().cloned())
        .chain(panes)
        .filter(|root| !root.trim().is_empty() && seen.insert(comparable(root)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Pane;

    fn live(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn sends_the_union_of_what_every_live_workspace_was_granted() {
        let mut grants = RootGrants::default();
        let first = grants.grant("A", vec!["/dev/a".into()], &live(&["A", "B"]));
        assert_eq!(first, ["/dev/a"]);
        let both = grants.grant("B", vec!["/dev/b".into()], &live(&["A", "B"]));
        assert_eq!(both, ["/dev/a", "/dev/b"]);
    }

    #[test]
    fn a_workspace_asking_again_replaces_its_own_folders_only() {
        let mut grants = RootGrants::default();
        grants.grant("A", vec!["/dev/a".into()], &live(&["A", "B"]));
        grants.grant("B", vec!["/dev/b".into()], &live(&["A", "B"]));
        let union = grants.grant("A", vec!["/dev/a2".into()], &live(&["A", "B"]));
        assert_eq!(union, ["/dev/a2", "/dev/b"]);
    }

    #[test]
    fn counts_a_folder_written_another_way_once() {
        let mut grants = RootGrants::default();
        grants.grant("A", vec!["C:\\Dev\\Shop".into()], &live(&["A", "B"]));
        let union = grants.grant(
            "B",
            vec!["c:/dev/shop/".into(), "/other".into()],
            &live(&["A", "B"]),
        );
        assert_eq!(union, ["C:\\Dev\\Shop", "/other"]);
    }

    #[test]
    fn workspaces_that_no_longer_exist_drop_out() {
        let mut grants = RootGrants::default();
        grants.grant("A", vec!["/dev/a".into()], &live(&["A", "B"]));
        grants.grant("B", vec!["/dev/b".into()], &live(&["A", "B"]));
        let union = grants.grant("B", vec!["/dev/b".into()], &live(&["B"]));
        assert_eq!(union, ["/dev/b"]);
    }

    #[test]
    fn resolves_a_path_against_the_agents_folder() {
        assert_eq!(
            resolve_path("/work/app", "out/shot.png"),
            "/work/app/out/shot.png"
        );
        assert_eq!(
            resolve_path("/work/app", "../shared/a.glb"),
            "/work/shared/a.glb"
        );
        assert_eq!(resolve_path("/work/app", "/tmp/x.png"), "/tmp/x.png");
        assert_eq!(
            resolve_path("/work/app", "C:\\shots\\one.png"),
            "C:\\shots\\one.png"
        );
        assert_eq!(resolve_path("/work/app", "./a.md"), "/work/app/a.md");
    }

    #[test]
    fn tells_what_is_inside_a_root_and_what_only_starts_like_one() {
        let roots = vec!["/work/app".to_string()];
        assert!(inside("/work/app", &roots));
        assert!(inside("/work/app/out/a.png", &roots));
        assert!(!inside("/work/application/a.png", &roots));
        assert!(!inside("/work", &roots));
        assert!(inside("C:\\Work\\App\\a.png", &["c:/work/app".to_string()]));
    }

    #[test]
    fn a_workspaces_roots_are_its_folders_and_its_panes() {
        let workspace = Workspace {
            id: "w".into(),
            cwd: "/work/app".into(),
            extra_roots: vec!["/work/lib".into()],
            panes: vec![
                Pane {
                    cwd: Some("../app-worktrees/fix".into()),
                    ..Pane::default()
                },
                Pane {
                    cwd: Some("/work/app".into()),
                    ..Pane::default()
                },
                Pane::default(),
            ],
            ..Workspace::default()
        };
        assert_eq!(
            workspace_roots(&workspace),
            ["/work/app", "/work/lib", "/work/app-worktrees/fix"]
        );
    }
}
