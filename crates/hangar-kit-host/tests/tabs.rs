//! Real modules in the real runtime: the probe kit (tests/probe) and the
//! hello-native example, built for wasm32-unknown-unknown once per run into
//! target/kit-wasm, beside the build that runs this test, so the two never
//! wait on each other's lock. Needs `rustup target add wasm32-unknown-unknown`.
//!
//! By hand: `cargo build --release --target wasm32-unknown-unknown -p hangar-kit-probe -p hello-native-tab`.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use hangar_kit_host::ui::abi::{KitInfo, TabInfo};
use hangar_kit_host::ui::api::{KitError, Theme};
use hangar_kit_host::ui::{Element, HandlerId, UiEvent};
use hangar_kit_host::{
    CallFuture, Effects, FailureKind, HostCall, KitApi, KitMessage, KitModule, KitRuntime,
    KitStyle, KitTab, Limits, LoadError, TabStart, ready,
};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace")
}

/// The two test modules, built once.
fn module_bytes(artifact: &str) -> Vec<u8> {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    let out = BUILT.get_or_init(|| {
        let target = workspace().join("target").join("kit-wasm");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(workspace())
            .args([
                "build",
                "--release",
                "--target",
                "wasm32-unknown-unknown",
                "-j",
                "4",
            ])
            .args(["-p", "hangar-kit-probe", "-p", "hello-native-tab"])
            .arg("--target-dir")
            .arg(&target)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "building the test kits for wasm32-unknown-unknown failed: is the target installed?"
        );
        target.join("wasm32-unknown-unknown").join("release")
    });
    std::fs::read(out.join(format!("{artifact}.wasm"))).expect("the built module")
}

fn runtime(limits: Limits) -> KitRuntime {
    KitRuntime::new(limits).expect("an engine")
}

fn probe(runtime: &KitRuntime) -> KitModule {
    runtime
        .compile(&module_bytes("hangar_kit_probe"))
        .expect("the probe compiles")
}

/// The app's side, faked: what it was asked, and plain answers.
#[derive(Default)]
struct Fake {
    granted: Mutex<Vec<String>>,
    asked: Mutex<Vec<HostCall>>,
}

impl Fake {
    fn granting(permissions: &[&str]) -> Arc<Fake> {
        let fake = Fake::default();
        *fake.granted.lock().unwrap() = permissions
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect();
        Arc::new(fake)
    }

    fn asked(&self) -> Vec<HostCall> {
        self.asked.lock().unwrap().clone()
    }
}

impl KitApi for Fake {
    fn granted(&self) -> Vec<String> {
        self.granted.lock().unwrap().clone()
    }

    fn call(&self, call: HostCall) -> CallFuture {
        self.asked.lock().unwrap().push(call.clone());
        ready(match call {
            HostCall::PanesList => Ok(json!([
                { "name": "Ava", "agent": "claude", "role": null, "state": "idle", "canSend": true }
            ])),
            HostCall::TabParams => Ok(json!({ "params": {}, "caller": null })),
            HostCall::DataRead { .. } => Err(KitError::new(
                KitError::NOT_FOUND,
                "no such file in the kit's data",
            )),
            HostCall::Notify { .. } => Ok(Value::Null),
            _ => Ok(json!({})),
        })
    }
}

fn start(params: &[(&str, &str)]) -> TabStart {
    TabStart {
        hangar: "2.0.0-test".into(),
        kit: KitInfo {
            id: "probe".into(),
            name: "Probe".into(),
            version: "0.0.0".into(),
        },
        tab: TabInfo {
            id: "probe".into(),
            params: params
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect::<BTreeMap<_, _>>(),
            caller: None,
        },
        theme: Theme::default(),
        visible: true,
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

/// Answers every call, and those its answers make, as the app's tasks would.
fn settle(tab: &mut KitTab, effects: Effects) {
    let mut waiting = effects.calls;
    for _ in 0..32 {
        let Some(dispatch) = waiting.pop() else {
            return;
        };
        let answer = block_on(dispatch.future);
        waiting.extend(tab.reply(dispatch.id, answer).calls);
    }
    panic!("calls kept coming");
}

/// Every text and row title of the view.
fn texts(tab: &KitTab) -> Vec<String> {
    let mut texts = Vec::new();
    tab.view()
        .expect("a view")
        .root
        .for_each(&mut |element| match element {
            Element::Text(text) => texts.push(text.text.clone()),
            Element::List(list) => texts.extend(list.items.iter().map(|item| item.title.clone())),
            _ => {}
        });
    texts
}

fn press(tab: &mut KitTab, label: &str) -> Effects {
    let view = tab.view().expect("a view");
    let mut handler: Option<HandlerId> = None;
    view.root.for_each(&mut |element| {
        if let Element::Button(button) = element
            && button.label.as_deref() == Some(label)
        {
            handler = button.on_press;
        }
    });
    let event = UiEvent {
        view: view.seq,
        handler: handler.expect("the button"),
        value: None,
    };
    tab.update(KitMessage::Ui(event))
}

/// The app may move a tab to a worker thread (docs/native/kits-native.md,
/// "Where it runs").
#[test]
fn a_tab_can_move_to_another_thread() {
    fn send<T: Send>() {}
    send::<KitTab>();
    send::<KitRuntime>();
    send::<KitModule>();
}

#[test]
fn a_real_module_starts_calls_and_draws() {
    let runtime = runtime(Limits::default());
    let fake = Fake::granting(&["panes.read"]);
    let (mut tab, effects) = KitTab::start(
        &runtime,
        &probe(&runtime),
        start(&[("calls", "tab.params,panes.list")]),
        fake.clone(),
    );
    assert!(tab.failure().is_none(), "{:?}", tab.failure());
    assert_eq!(effects.calls.len(), 2);
    settle(&mut tab, effects);
    let shown = texts(&tab);
    assert!(shown.contains(&"tab.params: ok".to_owned()), "{shown:?}");
    assert!(shown.contains(&"panes.list: ok".to_owned()), "{shown:?}");
    assert_eq!(fake.asked(), vec![HostCall::TabParams, HostCall::PanesList]);
    // It draws, with nothing to panic on.
    let _ = tab.draw(KitStyle::dark());
}

#[test]
fn permissions_gate_every_call_before_the_app_sees_it() {
    let runtime = runtime(Limits::default());
    let fake = Fake::granting(&["panes.read", "loopback:9000"]);
    let calls = "panes.list,panes.send,notify,files.read,loopback.fetch,command.run,tab.setTitle,ui.copy,files.delete";
    let (mut tab, effects) = KitTab::start(
        &runtime,
        &probe(&runtime),
        start(&[("calls", calls)]),
        fake.clone(),
    );
    settle(&mut tab, effects);
    let shown = texts(&tab);
    for expected in [
        "panes.list: ok",
        "panes.send: denied",
        "notify: denied",
        "files.read: denied",
        "loopback.fetch: denied",
        "command.run: denied",
        "tab.setTitle: ok",
        // From `init`, not from a click: the clipboard is refused.
        "ui.copy: denied",
        "files.delete: unknown",
    ] {
        assert!(
            shown.contains(&expected.to_owned()),
            "{expected} in {shown:?}"
        );
    }
    assert_eq!(
        fake.asked(),
        vec![
            HostCall::PanesList,
            HostCall::TabSetTitle {
                title: "Probe".into()
            }
        ]
    );

    // A click is a gesture: the clipboard is the app's to fill.
    let effects = press(&mut tab, "Copy");
    settle(&mut tab, effects);
    assert_eq!(
        fake.asked().last(),
        Some(&HostCall::Copy {
            text: "from a click".into()
        })
    );

    // Taken back, the grant denies the next call, and the event goes unheard.
    fake.granted.lock().unwrap().clear();
    let effects = tab.event("panes.changed", json!({}));
    assert!(effects.is_empty());
    assert!(texts(&tab).contains(&"events: 0".to_owned()));
}

#[test]
fn events_and_timers_reach_a_tab_that_asked_for_them() {
    let runtime = runtime(Limits::default());
    let fake = Fake::granting(&["panes.read"]);
    let (mut tab, effects) = KitTab::start(&runtime, &probe(&runtime), start(&[]), fake);
    settle(&mut tab, effects);
    assert_eq!(tab.timers().len(), 1);
    assert_eq!(tab.timers()[0].every_ms, 100, "held to the shortest period");
    tab.event("panes.changed", json!({}));
    tab.event("theme", json!({ "scheme": "light", "locale": "fr" }));
    tab.timer("tick");
    tab.timer("not-a-timer");
    let shown = texts(&tab);
    assert!(shown.contains(&"events: 1".to_owned()), "{shown:?}");
    assert!(shown.contains(&"ticks: 1".to_owned()), "{shown:?}");
}

#[test]
fn a_kit_that_loops_forever_is_stopped() {
    let limits = Limits {
        init_deadline: Duration::from_millis(200),
        call_deadline: Duration::from_millis(100),
        ..Limits::default()
    };
    let runtime = runtime(limits);
    // After the build and the compile, which the first test to run waits for.
    let module = probe(&runtime);
    let began = Instant::now();
    let (mut tab, effects) = KitTab::start(
        &runtime,
        &module,
        start(&[("mode", "loop")]),
        Fake::granting(&[]),
    );
    assert!(effects.is_empty());
    let failure = tab.failure().expect("stopped").clone();
    assert_eq!(failure.kind, FailureKind::Timeout, "{failure:?}");
    assert!(failure.reason.contains("200 ms"), "{}", failure.reason);
    assert!(began.elapsed() < Duration::from_secs(5));
    let _ = tab.draw(KitStyle::light());
    // Started again, it loops again, and is stopped again: never in a loop of
    // restarts by itself.
    tab.update(KitMessage::Reload);
    assert_eq!(
        tab.failure().map(|failure| &failure.kind),
        Some(&FailureKind::Timeout)
    );

    // In an update: the click that set it off ends the tab, not Hangar.
    let (mut tab, effects) =
        KitTab::start(&runtime, &probe(&runtime), start(&[]), Fake::granting(&[]));
    settle(&mut tab, effects);
    let began = Instant::now();
    press(&mut tab, "Spin");
    assert_eq!(
        tab.failure().map(|failure| &failure.kind),
        Some(&FailureKind::Timeout)
    );
    assert!(began.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_kit_that_allocates_too_much_fails_cleanly() {
    let runtime = runtime(Limits {
        memory_bytes: 32 * 1024 * 1024,
        ..Limits::default()
    });
    let (tab, _) = KitTab::start(
        &runtime,
        &probe(&runtime),
        start(&[("mode", "alloc")]),
        Fake::granting(&[]),
    );
    let failure = tab.failure().expect("stopped");
    assert_eq!(failure.kind, FailureKind::Memory, "{failure:?}");
    assert!(failure.reason.contains("32 MB"), "{}", failure.reason);

    let (mut tab, effects) =
        KitTab::start(&runtime, &probe(&runtime), start(&[]), Fake::granting(&[]));
    settle(&mut tab, effects);
    press(&mut tab, "Hog");
    assert_eq!(
        tab.failure().map(|failure| &failure.kind),
        Some(&FailureKind::Memory)
    );
    // A new instance starts with the memory of a new one.
    tab.update(KitMessage::Reload);
    assert!(tab.failure().is_none());
}

#[test]
fn a_panic_is_shown_with_its_message() {
    let runtime = runtime(Limits::default());
    let (tab, _) = KitTab::start(
        &runtime,
        &probe(&runtime),
        start(&[("mode", "panic")]),
        Fake::granting(&[]),
    );
    let failure = tab.failure().expect("stopped");
    assert_eq!(failure.kind, FailureKind::Panic, "{failure:?}");
    assert!(
        failure.reason.contains("the probe was told to panic"),
        "{}",
        failure.reason
    );
    assert!(tab.logs().any(|entry| entry.text.contains("told to panic")));

    let (mut tab, effects) =
        KitTab::start(&runtime, &probe(&runtime), start(&[]), Fake::granting(&[]));
    settle(&mut tab, effects);
    press(&mut tab, "Panic");
    assert!(
        tab.failure()
            .is_some_and(|failure| failure.reason.contains("button panics"))
    );
    // Answers to an instance that is gone go nowhere.
    assert!(tab.reply(1, Ok(Value::Null)).is_empty());
}

#[test]
fn modules_that_break_the_abi_do_not_load() {
    let runtime = runtime(Limits::default());
    let wasi = r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))) (memory (export "memory") 1))"#;
    let refused = runtime.compile(wasi.as_bytes()).err().expect("refused");
    assert!(matches!(refused, LoadError::Import { .. }));
    assert!(
        refused.to_string().contains("wasm32-unknown-unknown"),
        "{refused}"
    );

    let no_view = r#"(module (memory (export "memory") 1)
        (func (export "hangar_abi_version") (result i32) i32.const 1)
        (func (export "hangar_alloc") (param i32) (result i32) i32.const 0)
        (func (export "hangar_init") (param i32 i32) (result i64) i64.const 0)
        (func (export "hangar_update") (param i32 i32) (result i64) i64.const 0))"#;
    assert!(matches!(
        runtime.compile(no_view.as_bytes()),
        Err(LoadError::Export {
            name: "hangar_view",
            ..
        })
    ));

    let newer = r#"(module (memory (export "memory") 1)
        (func (export "hangar_abi_version") (result i32) i32.const 2)
        (func (export "hangar_alloc") (param i32) (result i32) i32.const 0)
        (func (export "hangar_init") (param i32 i32) (result i64) i64.const 0)
        (func (export "hangar_update") (param i32 i32) (result i64) i64.const 0)
        (func (export "hangar_view") (result i64) i64.const 0))"#;
    let module = runtime
        .compile(newer.as_bytes())
        .expect("its shape is right");
    let (tab, _) = KitTab::start(&runtime, &module, start(&[]), Fake::granting(&[]));
    let failure = tab.failure().expect("not started");
    assert_eq!(failure.kind, FailureKind::Load);
    assert!(
        failure.reason.contains("needs a newer Hangar"),
        "{}",
        failure.reason
    );

    // An answer that is not JSON stops the tab, which says so.
    let silent = newer.replace("i32.const 2", "i32.const 1");
    let module = runtime
        .compile(silent.as_bytes())
        .expect("its shape is right");
    let (tab, _) = KitTab::start(&runtime, &module, start(&[]), Fake::granting(&[]));
    assert_eq!(
        tab.failure().map(|failure| &failure.kind),
        Some(&FailureKind::Message)
    );

    assert!(matches!(
        runtime.compile(b"not wasm"),
        Err(LoadError::Invalid(_))
    ));
}

#[test]
fn the_hello_native_example_runs() {
    let runtime = runtime(Limits::default());
    let module = runtime
        .compile(&module_bytes("hello_native_tab"))
        .expect("the example compiles");
    let fake = Fake::granting(&["panes.read", "panes.send", "data.read", "data.write"]);
    let mut start = start(&[]);
    start.kit = KitInfo {
        id: "hello-native".into(),
        name: "Hello native".into(),
        version: "0.1.0".into(),
    };
    let (mut tab, effects) = KitTab::start(&runtime, &module, start, fake.clone());
    settle(&mut tab, effects);
    assert!(tab.failure().is_none(), "{:?}", tab.failure());
    assert_eq!(
        fake.asked()[..2],
        [
            HostCall::PanesList,
            HostCall::DataRead {
                path: "notes/hello.txt".into()
            }
        ]
    );
    let mut table_rows = 0;
    tab.view().unwrap().root.for_each(&mut |element| {
        if let Element::Table(table) = element {
            table_rows = table.rows.len();
        }
    });
    assert_eq!(table_rows, 1);
    let effects = press(&mut tab, "Greet");
    assert!(matches!(fake.asked().last(), Some(HostCall::Toast(_))));
    settle(&mut tab, effects);
    let effects = tab.set_theme(Theme {
        scheme: hangar_kit_host::ui::api::Scheme::Light,
        locale: "fr".into(),
    });
    settle(&mut tab, effects);
    let mut title = String::new();
    tab.view().unwrap().root.for_each(&mut |element| {
        if let Element::Header(header) = element {
            title = header.title.clone();
        }
    });
    assert_eq!(title, "Bonjour, en natif");
    let _ = tab.draw(KitStyle::dark());
}
