//! A folder of its own for a test, gone with it: this crate has no `tempfile`,
//! and the tests of this package write real files (models, captures).

use std::path::{Path, PathBuf};

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "hangar-model3d-view-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        // Canonical, as the folders hangar-core serves are.
        Scratch(dir.canonicalize().expect("a canonical scratch folder"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Writes a file in the folder and returns its path.
    pub fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.join(name);
        std::fs::write(&path, contents).expect("a file is written");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
