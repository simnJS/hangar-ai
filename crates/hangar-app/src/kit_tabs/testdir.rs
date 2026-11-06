//! Scratch folders for tests: made under the system temp folder and removed
//! when the test ends, also when it panics.
//!
//! The test harness runs each test on a thread of its own, so the folders are
//! kept by the thread that asked for them, and go with it. A test that hands a
//! folder to another thread must not outlive the test itself with it.

use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    static KEPT: RefCell<Vec<tempfile::TempDir>> = const { RefCell::new(Vec::new()) };
}

/// A new, empty folder named `hangar-{tag}-…`, removed when the test's thread
/// ends.
pub fn scratch(tag: &str) -> PathBuf {
    let dir = tempfile::Builder::new()
        .prefix(&format!("hangar-{tag}-"))
        .tempdir()
        .expect("a scratch folder");
    let path = dir.path().to_path_buf();
    KEPT.with(|kept| kept.borrow_mut().push(dir));
    path
}
