//! Builders the tests of this package share: kits, manifests, tabs and the
//! workspace entries that go with them.

use std::collections::BTreeMap;

use hangar_core::kits::{GitSource, Inspection, KitInfo, Manifest, Source, Tab};

use crate::state::types::WorkspaceKit;

pub const HOOKS: &str = "hooks:0123456789abcdef";
pub const OTHER_HOOKS: &str = "hooks:fedcba9876543210";
pub const COMMANDS: &str = "commands:0123456789abcdef";
pub const OTHER_COMMANDS: &str = "commands:fedcba9876543210";

/// A tab of an HTML page asking for `permissions`.
pub fn tab(permissions: &[&str]) -> Tab {
    Tab {
        id: "t".into(),
        title: "T".into(),
        entry: "tabs/t/index.html".into(),
        native: None,
        icon: None,
        permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
        params: BTreeMap::new(),
        singleton: false,
    }
}

/// A tab that is a WebAssembly module.
pub fn native_tab(permissions: &[&str]) -> Tab {
    Tab {
        entry: "tabs/t.wasm".into(),
        ..tab(permissions)
    }
}

pub fn manifest(id: &str, version: &str, tabs: Vec<Tab>) -> Manifest {
    Manifest {
        schema: 1,
        id: id.into(),
        name: "Hello kit".into(),
        version: version.into(),
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

pub fn folder() -> Source {
    Source::Folder {
        path: "D:\\kits\\hello-kit".into(),
    }
}

pub fn git(url: &str, git_ref: Option<&str>) -> Source {
    Source::Git(GitSource {
        url: url.into(),
        git_ref: git_ref.map(str::to_owned),
        subdir: None,
    })
}

/// An installed kit with nothing set but its id and where it comes from: a
/// GitHub kit whose manifest could not be read.
pub fn installed(id: &str) -> KitInfo {
    KitInfo {
        id: id.into(),
        source: git("https://github.com/o/kits", None),
        dir: "C:/k".into(),
        installed_version: "1.0.0".into(),
        added_at: 0,
        manifest: None,
        error: None,
        update: None,
        hooks_permission: None,
        hooks_consent: false,
        commands_permission: None,
    }
}

/// `hello-kit` as a linked folder, usable, with these tabs.
pub fn kit(version: &str, tabs: Vec<Tab>) -> KitInfo {
    KitInfo {
        id: "hello-kit".into(),
        source: folder(),
        dir: "D:\\kits\\hello-kit".into(),
        installed_version: version.into(),
        manifest: Some(manifest("hello-kit", version, tabs)),
        ..installed("hello-kit")
    }
}

/// `hello-kit` from GitHub with one tab asking for `panes.read`, and this hash
/// of what its plugin runs by itself.
pub fn git_kit(hooks_permission: Option<&str>, hooks_consent: bool) -> KitInfo {
    KitInfo {
        source: git("https://github.com/o/kits", None),
        hooks_permission: hooks_permission.map(str::to_owned),
        hooks_consent,
        ..kit("1.0.0", vec![tab(&["panes.read"])])
    }
}

/// `hello-kit` with one tab asking for `command:ci-run`, and this hash of its
/// commands.
pub fn command_kit(commands_permission: Option<&str>, source: Source) -> KitInfo {
    KitInfo {
        source,
        commands_permission: commands_permission.map(str::to_owned),
        ..kit("1.0.0", vec![tab(&["command:ci-run"])])
    }
}

pub fn entry(id: &str, granted: &[&str]) -> WorkspaceKit {
    WorkspaceKit {
        id: id.into(),
        granted: granted.iter().map(|g| (*g).to_owned()).collect(),
    }
}

/// What `kit_inspect` would answer for a kit that brings these tabs and nothing
/// else.
pub fn inspection(tabs: Vec<Tab>) -> Inspection {
    Inspection {
        plugin: None,
        skills: Vec::new(),
        commands: Vec::new(),
        agents: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: Vec::new(),
        running: Vec::new(),
        hooks_permission: None,
        tabs,
        setup: Vec::new(),
        declared_commands: Vec::new(),
        commands_permission: None,
    }
}
