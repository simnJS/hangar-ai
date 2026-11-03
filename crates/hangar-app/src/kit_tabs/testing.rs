//! Builders the tests of this package share: kits as `kits_list` answers
//! them, their tabs, and the workspace entries that enable them.

use std::collections::BTreeMap;

use hangar_core::kits::{GitSource, KitInfo, Manifest, Source, Tab, TabParam};

use crate::state::types::WorkspaceKit;

pub fn manifest(id: &str, tabs: Vec<Tab>) -> Manifest {
    Manifest {
        schema: 1,
        id: id.into(),
        name: format!("{id} name"),
        version: "1.0.0".into(),
        description: String::new(),
        author: None,
        homepage: None,
        icon: None,
        hangar: None,
        plugin: None,
        tabs,
        setup: Vec::new(),
        commands: BTreeMap::new(),
    }
}

pub fn tab(id: &str, title: &str, entry: &str, permissions: &[&str]) -> Tab {
    Tab {
        id: id.into(),
        title: title.into(),
        entry: entry.into(),
        native: None,
        icon: None,
        permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
        params: BTreeMap::new(),
        singleton: false,
    }
}

pub fn param(pattern: Option<&str>) -> TabParam {
    TabParam {
        description: "x".into(),
        pattern: pattern.map(str::to_owned),
    }
}

pub fn status() -> Tab {
    tab(
        "status",
        "Status",
        "tabs/status/index.html",
        &["panes.read", "notify"],
    )
}

pub fn explorer() -> Tab {
    tab(
        "explorer",
        "Explorer",
        "tabs/explorer/index.html",
        &["files.read", "preview.show", "panes.send"],
    )
}

pub fn folder(id: &str) -> Source {
    Source::Folder {
        path: format!("D:/kits/{id}"),
    }
}

pub fn github() -> Source {
    Source::Git(GitSource {
        url: "https://github.com/o/kits".into(),
        git_ref: None,
        subdir: None,
    })
}

/// A usable kit, linked from a folder, with these tabs.
pub fn kit(id: &str, tabs: Vec<Tab>) -> KitInfo {
    KitInfo {
        id: id.into(),
        source: folder(id),
        dir: format!("D:/kits/{id}"),
        installed_version: "1.0.0".into(),
        added_at: 1,
        manifest: Some(manifest(id, tabs)),
        error: None,
        update: None,
        hooks_permission: None,
        hooks_consent: false,
        commands_permission: None,
    }
}

/// The kits `kitTabs.test.ts` had installed.
pub fn installed() -> Vec<KitInfo> {
    vec![
        kit("hello-kit", vec![status()]),
        kit("roblox", vec![explorer()]),
        KitInfo {
            error: Some("its manifest is invalid".into()),
            manifest: None,
            ..kit("broken-kit", Vec::new())
        },
        KitInfo {
            error: Some("needs Hangar >=9.0.0".into()),
            ..kit("future-kit", vec![status()])
        },
    ]
}

pub fn entry(id: &str, granted: &[&str]) -> WorkspaceKit {
    WorkspaceKit {
        id: id.into(),
        granted: granted.iter().map(|g| (*g).to_owned()).collect(),
    }
}

pub fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}
