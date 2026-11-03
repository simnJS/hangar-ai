//! The cases of `lib/kitSetup.test.ts`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use hangar_core::kits::StepStatus;

use super::*;

fn step(id: &str, done: bool) -> StepStatus {
    StepStatus {
        id: id.into(),
        title: id.into(),
        description: None,
        command: format!("run {id}"),
        sha256: id.into(),
        done,
    }
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(future)
}

#[test]
fn offers_only_the_steps_not_done_on_this_machine() {
    let steps = [step("a", true), step("b", false)];
    let pending: Vec<_> = pending_steps(&steps)
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(pending, ["b"]);
}

#[test]
fn counts_done_steps_and_no_step_is_complete() {
    assert_eq!(
        setup_summary(&[step("a", true), step("b", false)]),
        SetupSummary {
            done: 1,
            total: 2,
            complete: false
        }
    );
    assert_eq!(
        setup_summary(&[]),
        SetupSummary {
            done: 0,
            total: 0,
            complete: true
        }
    );
}

#[test]
fn what_is_typed_is_the_command_verbatim_plus_enter() {
    assert_eq!(
        setup_keystrokes("npm install --prefix server"),
        "npm install --prefix server\r"
    );
}

#[test]
fn maps_platforms_like_the_rust_side() {
    assert_eq!(os_of_platform("Win32"), KitOs::Windows);
    assert_eq!(os_of_platform("MacIntel"), KitOs::Macos);
    assert_eq!(os_of_platform("Linux x86_64"), KitOs::Linux);
    assert_eq!(KitOs::Macos.name(), "macos");
    // The OS the tests run on is one of the three the manifest knows.
    assert!(["windows", "macos", "linux"].contains(&current_os().name()));
}

/// The pane of a test: the terminal comes up after `up_after` looks, if ever.
struct Fake {
    pane: Option<&'static str>,
    up_after: Option<usize>,
    looks: AtomicUsize,
    write_error: Option<&'static str>,
    tries: usize,
    typed: Mutex<Vec<(String, String)>>,
    closed: Mutex<Vec<String>>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            pane: Some("pane-1"),
            up_after: Some(0),
            looks: AtomicUsize::new(0),
            write_error: None,
            tries: 80,
            typed: Mutex::default(),
            closed: Mutex::default(),
        }
    }
}

impl PaneTyping for Fake {
    async fn alive(&self, _pane: &str) -> bool {
        let looked = self.looks.fetch_add(1, Ordering::SeqCst);
        self.up_after.is_some_and(|after| looked >= after)
    }

    async fn write(&self, pane: &str, data: &str) -> Result<(), String> {
        if let Some(error) = self.write_error {
            return Err(error.to_owned());
        }
        self.typed.lock().unwrap().push((pane.into(), data.into()));
        Ok(())
    }

    async fn wait(&self, _delay: Duration) {}

    fn close_pane(&self, pane: &str) {
        self.closed.lock().unwrap().push(pane.into());
    }

    fn tries(&self) -> usize {
        self.tries
    }
}

impl SetupPaneDeps for Fake {
    fn open_pane(&self) -> Option<String> {
        self.pane.map(str::to_owned)
    }
}

#[test]
fn types_the_command_once_the_terminal_is_up() {
    let deps = Fake {
        up_after: Some(1),
        ..Fake::new()
    };
    assert_eq!(
        block_on(run_setup_command("echo hi", &deps)),
        SetupRunResult::Ok {
            pane: "pane-1".into()
        }
    );
    assert_eq!(
        *deps.typed.lock().unwrap(),
        vec![("pane-1".to_owned(), "echo hi\r".to_owned())]
    );
    assert_eq!(deps.looks.load(Ordering::SeqCst), 2);
}

#[test]
fn types_nothing_when_the_terminal_never_starts() {
    let deps = Fake {
        up_after: None,
        tries: 3,
        ..Fake::new()
    };
    assert_eq!(
        block_on(run_setup_command("echo hi", &deps)),
        SetupRunResult::NotStarted
    );
    assert!(deps.typed.lock().unwrap().is_empty());
    assert_eq!(*deps.closed.lock().unwrap(), vec!["pane-1".to_owned()]);
    assert_eq!(deps.looks.load(Ordering::SeqCst), 3);
}

#[test]
fn types_nothing_when_the_workspace_is_full() {
    let deps = Fake {
        pane: None,
        ..Fake::new()
    };
    assert_eq!(
        block_on(run_setup_command("echo hi", &deps)),
        SetupRunResult::NoPane
    );
    assert!(deps.typed.lock().unwrap().is_empty());
    assert!(deps.closed.lock().unwrap().is_empty());
}

#[test]
fn reports_a_failed_write_and_closes_the_pane() {
    let deps = Fake {
        write_error: Some("pty not found"),
        ..Fake::new()
    };
    assert_eq!(
        block_on(run_setup_command("echo hi", &deps)),
        SetupRunResult::WriteFailed {
            detail: "pty not found".into()
        }
    );
    assert_eq!(*deps.closed.lock().unwrap(), vec!["pane-1".to_owned()]);
}

#[test]
fn selects_a_workspace_that_is_not_the_one_on_screen() {
    assert_eq!(
        show_workspace_selections("w1", Some("w2"), false),
        [Some("w1".to_owned())]
    );
}

#[test]
fn selects_a_sleeping_one() {
    assert_eq!(
        show_workspace_selections("w1", Some("w2"), true),
        [Some("w1".to_owned())]
    );
}

#[test]
fn selects_a_sleeping_workspace_that_is_already_selected_from_nothing_to_wake_it() {
    assert_eq!(
        show_workspace_selections("w1", Some("w1"), true),
        [None, Some("w1".to_owned())]
    );
}

#[test]
fn selects_the_workspace_on_screen_again_without_waking_anything() {
    assert_eq!(
        show_workspace_selections("w1", Some("w1"), false),
        [Some("w1".to_owned())]
    );
    assert_eq!(
        show_workspace_selections("w1", None, false),
        [Some("w1".to_owned())]
    );
}
