//! Scratch kits for the tests: folders in the temp directory, and git
//! repositories served over `file://` standing in for GitHub.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use super::manifest::{self, Manifest};

/// A folder in the temp directory, removed when dropped.
pub struct Folder {
    pub root: PathBuf,
}

impl Folder {
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "hangar-kits-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        Folder {
            root: dunce::canonicalize(root).unwrap(),
        }
    }

    pub fn file(&self, relative: &str, body: &str) -> &Self {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
        self
    }

    /// A Claude Code plugin named `name` in `relative` (`.` for the root).
    pub fn plugin(&self, relative: &str, name: &str) -> &Self {
        self.file(
            &join(relative, ".claude-plugin/plugin.json"),
            &json!({ "name": name, "version": "0.1.0" }).to_string(),
        )
    }

    pub fn parse(&self, manifest: &Value) -> Result<Manifest, String> {
        manifest::parse(&manifest.to_string(), &self.root)
    }

    /// A whole kit in `relative`: its manifest, and a plugin at its root with
    /// one skill and one MCP server.
    pub fn kit(&self, relative: &str, id: &str, version: &str) -> PathBuf {
        self.manifest(relative, id, version);
        self.plugin(relative, id)
            .file(
                &join(relative, "skills/hello/SKILL.md"),
                "---\nname: hello\ndescription: Says hello.\n---\nHello.\n",
            )
            .file(
                &join(relative, ".mcp.json"),
                &json!({ "mcpServers": { "studio": {
                    "command": "node",
                    "args": ["${CLAUDE_PLUGIN_ROOT}/server.js"]
                } } })
                .to_string(),
            );
        self.root.join(relative)
    }

    pub fn manifest(&self, relative: &str, id: &str, version: &str) -> &Self {
        self.file(
            &join(relative, manifest::FILE),
            &json!({
                "schema": 1,
                "id": id,
                "name": id,
                "version": version,
                "description": "A test kit."
            })
            .to_string(),
        )
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = super::store::remove_tree(&self.root);
    }
}

fn join(relative: &str, rest: &str) -> String {
    if relative == "." || relative.is_empty() {
        rest.to_string()
    } else {
        format!("{relative}/{rest}")
    }
}

/// A repository and the bare one it pushes to, which a kit is cloned from.
pub struct Remote {
    pub folder: Folder,
}

impl Remote {
    pub fn new(tag: &str) -> Self {
        let folder = Folder::new(tag);
        fs::create_dir_all(folder.root.join("work")).unwrap();
        // Its HEAD names `main`, which a fetch of `HEAD` then follows.
        git(
            &folder.root,
            &["init", "-q", "--bare", "-b", "main", "remote.git"],
        );
        git(&folder.work(), &["init", "-q", "-b", "main"]);
        git(
            &folder.work(),
            &[
                "remote",
                "add",
                "origin",
                &file_url(&folder.root.join("remote.git")),
            ],
        );
        Remote { folder }
    }

    /// What the store is given to clone.
    pub fn url(&self) -> String {
        file_url(&self.folder.root.join("remote.git"))
    }

    /// The working copy, where the tests write what they then push.
    pub fn work(&self) -> PathBuf {
        self.folder.work()
    }

    pub fn push(&self, message: &str) {
        let work = self.work();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-q", "-m", message]);
        git(&work, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    }
}

impl Folder {
    fn work(&self) -> PathBuf {
        self.root.join("work")
    }
}

/// `file://` and forward slashes, whatever the OS: git then serves it like a
/// remote, shallow fetches included.
pub fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

/// git for the tests' own repositories, without the user's identity or
/// signing settings.
pub fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Hangar Tests",
            "-c",
            "user.email=tests@hangar.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git is needed by the kits tests");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
