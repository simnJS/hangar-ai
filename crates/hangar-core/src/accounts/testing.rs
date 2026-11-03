//! Scratch folders for the tests: a main account and a profile store in the
//! temp directory, never the person's own `~/.claude`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::ClaudeHome;

/// A folder in the temp directory, removed when dropped.
pub struct Scratch {
    pub root: PathBuf,
}

impl Scratch {
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "hangar-accounts-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        Scratch {
            root: dunce::canonicalize(root).unwrap(),
        }
    }

    /// A directory under the scratch root, created.
    pub fn dir(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    pub fn file(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    /// A main account laid out as Claude Code's default one: `home/.claude`
    /// and `home/.claude.json` beside it.
    pub fn main_home(&self) -> ClaudeHome {
        let home = self.dir("home");
        ClaudeHome {
            dir: home.join(".claude"),
            config: home.join(".claude.json"),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Links first, so nothing is ever removed through one.
        unlink_all(&self.root);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn unlink_all(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if super::link::is_link(&path) {
            let _ = super::link::remove_link(&path);
        } else if path.is_dir() {
            unlink_all(&path);
        }
    }
}

pub fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
