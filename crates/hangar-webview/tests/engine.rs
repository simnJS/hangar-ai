//! The real helper, driven through the client: a window, a page served through
//! a custom scheme, ipc, a POST body, a script's result and a navigation that
//! is refused.
//!
//! It opens a window, so it needs a display, and is left out of a plain
//! `cargo test`:
//!
//! ```text
//! cargo test -p hangar-webview --features engine --test engine -- --ignored
//! ```
#![cfg(feature = "engine")]

use std::time::{Duration, Instant};

use hangar_webview::{
    Client, Content, Event, Events, OpenWindow, PageLoadState, SchemeResponse, scheme_url,
};
use serde_json::json;

const HELPER: &str = env!("CARGO_BIN_EXE_hangar-webview");
const SCHEME: &str = "hangar-test";
const PATIENCE: Duration = Duration::from_secs(20);

/// A page that reports what it can do over ipc, one line each.
const PAGE: &str = r#"<!doctype html>
<meta charset="utf-8" />
<script src="/_hangar/kit.js"></script>
<script>
  const say = (text) => window.ipc.postMessage(text);
  say("init:" + window.__fromInit);
  say("bridge:" + (window.parent !== window) + ":" + typeof window.__hangarHost);
  // As kit.js does it: through window.parent, and hearing it as a message event.
  window.addEventListener("message", (event) => {
    if (event.source === window.parent) say("message:" + JSON.stringify(event.data));
  });
  window.parent.postMessage({ hangar: 1, id: "7", method: "ping" }, "*");
  fetch("/echo", { method: "POST", body: "hello" }).then((r) => r.text()).then((text) => say("echo:" + text));
  window.hangar.call("panes.list").then((panes) => say("sdk:" + JSON.stringify(panes)));
</script>
"#;

/// A stand-in for kit.js with the part this test needs of it: `window.hangar.call`
/// over `window.parent`. The real file is what the demo runs.
const SDK: &str = r#"(function () {
  var pending = {}, next = 0;
  window.addEventListener("message", function (event) {
    var m = event.data;
    if (event.source === window.parent && m && m.hangar === 1 && pending[m.id]) { pending[m.id](m.result); delete pending[m.id]; }
  });
  window.hangar = { call: function (method) {
    return new Promise(function (resolve) { var id = "s" + ++next; pending[id] = resolve; window.parent.postMessage({ hangar: 1, id: id, method: method }, "*"); });
  } };
})();"#;

fn wait_for(events: &Events, what: &str, mut matches: impl FnMut(&Event) -> bool) -> Event {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(event) if matches(&event) => return event,
            Ok(_) => {}
            Err(error) => panic!("waiting for {what}: {error:?}"),
        }
    }
}

#[test]
#[ignore = "opens a window: needs a display"]
fn a_page_talks_to_its_parent_through_every_channel() {
    let (client, events) = Client::spawn(HELPER).unwrap();
    wait_for(&events, "ready", |event| {
        matches!(event, Event::Ready { .. })
    });

    client
        .open(
            OpenWindow::new("t", Content::url(scheme_url(SCHEME, "/index.html")))
                .title("engine test")
                .size(400, 300)
                .scheme(SCHEME)
                .init_script("window.__fromInit = 42;")
                .kit_bridge()
                .allow_navigation([scheme_url(SCHEME, "/")]),
        )
        .unwrap();

    // Everything the page says, in the order it says it, and what the parent
    // does about each: serve it, answer it, or nothing.
    let mut said = Vec::new();
    let mut posts = Vec::new();
    let mut loaded = false;
    let deadline = Instant::now() + PATIENCE;
    let heard_all = |said: &[String]| {
        [
            "init:",
            "bridge:",
            "echo:",
            "sdk:",
            r#"message:{"hangar":1,"id":"7""#,
        ]
        .iter()
        .all(|start| said.iter().any(|line| line.starts_with(start)))
    };
    while !heard_all(&said) || !loaded {
        let left = deadline.saturating_duration_since(Instant::now());
        match events
            .recv_timeout(left)
            .expect("the page finishes talking")
        {
            Event::SchemeRequest(request) => {
                let path = request
                    .uri
                    .split_once("://")
                    .unwrap()
                    .1
                    .split_once('/')
                    .unwrap()
                    .1;
                let response = match path {
                    "index.html" => SchemeResponse::ok(request.request, "text/html", PAGE),
                    "_hangar/kit.js" => SchemeResponse::ok(request.request, "text/javascript", SDK),
                    "echo" => {
                        posts.push((request.method.clone(), request.body.clone()));
                        SchemeResponse::ok(
                            request.request,
                            "text/plain",
                            String::from_utf8(request.body).unwrap().to_uppercase(),
                        )
                    }
                    other => panic!("an unexpected request for {other}"),
                };
                client.respond(response).unwrap();
            }
            Event::Ipc { window, uri, body } => {
                assert_eq!(window, "t");
                assert!(uri.starts_with(&scheme_url(SCHEME, "/")), "{uri}");
                if let Ok(message) = serde_json::from_str::<serde_json::Value>(&body)
                    && message["hangar"] == 1
                {
                    // The page's `window.parent.postMessage`, as the parent gets it.
                    let result = if message["method"] == "panes.list" {
                        json!([{ "name": "Ava" }])
                    } else {
                        json!("pong")
                    };
                    client
                        .post_message(
                            "t",
                            json!({ "hangar": 1, "id": message["id"], "result": result }),
                        )
                        .unwrap();
                } else {
                    said.push(body);
                }
            }
            Event::PageLoad {
                state: PageLoadState::Finished,
                ..
            } => loaded = true,
            Event::Error { code, message, .. } => panic!("the helper reported {code:?}: {message}"),
            _ => {}
        }
    }
    // The page's own listener hears the SDK's answer too, so what is checked is
    // that each line is there and not how many there are.
    for line in [
        r#"bridge:true:object"#,
        r#"echo:HELLO"#,
        r#"init:42"#,
        r#"message:{"hangar":1,"id":"7","result":"pong"}"#,
        r#"sdk:[{"name":"Ava"}]"#,
    ] {
        assert!(
            said.iter().any(|said| said == line),
            "the page did not say {line}: {said:?}"
        );
    }
    // The body of the POST got through, which WebKitGTK only does with `linux-body`.
    assert_eq!(posts, [("POST".to_string(), b"hello".to_vec())]);

    client.eval_with_token("t", "6 * 7", "answer").unwrap();
    let Event::EvalResult { result, .. } = wait_for(
        &events,
        "a result",
        |event| matches!(event, Event::EvalResult { token, .. } if token == "answer"),
    ) else {
        unreachable!()
    };
    assert_eq!(result, json!(42));

    // The window may go nowhere but its own scheme.
    client
        .eval("t", "location.href = 'https://example.com/'")
        .unwrap();
    let Event::NavigationBlocked {
        url, new_window, ..
    } = wait_for(&events, "a refused navigation", |event| {
        matches!(event, Event::NavigationBlocked { .. })
    })
    else {
        unreachable!()
    };
    assert_eq!((url.as_str(), new_window), ("https://example.com/", false));

    client.set_title("t", "renamed").unwrap();
    client.close("t").unwrap();
    wait_for(&events, "the window closing", |event| {
        matches!(event, Event::WindowClosed { .. })
    });
    client.quit().unwrap();
    assert_eq!(events.recv(), None);
    assert!(client.wait(PATIENCE).is_some_and(|status| status.success()));
}

#[test]
#[ignore = "opens a window: needs a display"]
fn mistakes_are_reported_and_do_not_stop_the_helper() {
    let (client, events) = Client::spawn(HELPER).unwrap();
    wait_for(&events, "ready", |event| {
        matches!(event, Event::Ready { .. })
    });

    client.navigate("nobody", "https://example.com").unwrap();
    wait_for(&events, "an unknown window", |event| {
        matches!(
            event,
            Event::Error {
                code: hangar_webview::ErrorCode::UnknownWindow,
                ..
            }
        )
    });

    client
        .open(OpenWindow::new("a", Content::html("<p>a</p>")).scheme("http"))
        .unwrap();
    wait_for(&events, "a scheme that is refused", |event| {
        matches!(
            event,
            Event::Error {
                code: hangar_webview::ErrorCode::OpenFailed,
                ..
            }
        )
    });

    client
        .open(OpenWindow::new("a", Content::html("<p>a</p>")))
        .unwrap();
    wait_for(&events, "the window", |event| {
        matches!(event, Event::Opened { .. })
    });
    client
        .open(OpenWindow::new("a", Content::html("<p>b</p>")))
        .unwrap();
    wait_for(&events, "a duplicate", |event| {
        matches!(
            event,
            Event::Error {
                code: hangar_webview::ErrorCode::DuplicateWindow,
                ..
            }
        )
    });

    // Quitting closes the window still open, and says so before it ends.
    client.quit().unwrap();
    assert!(matches!(
        events.recv(),
        Some(Event::WindowClosed {
            reason: hangar_webview::CloseReason::Quit,
            ..
        })
    ));
    assert_eq!(events.recv(), None);
    assert!(client.wait(PATIENCE).is_some_and(|status| status.success()));
}

#[test]
#[ignore = "starts the engine: needs a display"]
fn a_line_that_is_no_command_is_an_error_to_report_not_the_end_of_the_helper() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let mut helper = Command::new(HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = helper.stdin.take().unwrap();
    let mut output = BufReader::new(helper.stdout.take().unwrap()).lines();
    assert!(
        output
            .next()
            .unwrap()
            .unwrap()
            .contains(r#""type":"ready""#)
    );

    writeln!(input, "this is not json").unwrap();
    writeln!(input, r#"{{"type":"teleport"}}"#).unwrap();
    // The helper is still there to answer a good one.
    writeln!(input, r#"{{"type":"focus","window":"nobody"}}"#).unwrap();
    let lines: Vec<String> = (0..3).map(|_| output.next().unwrap().unwrap()).collect();
    assert!(lines[0].contains("bad_command"), "{}", lines[0]);
    assert!(lines[1].contains("bad_command"), "{}", lines[1]);
    assert!(lines[2].contains("unknown_window"), "{}", lines[2]);

    // Closing its input is how a parent that died looks: the helper leaves by itself.
    drop(input);
    assert!(helper.wait().unwrap().success());
}

#[test]
#[ignore = "opens a window: needs a display"]
fn a_window_made_hidden_loads_is_shown_and_hidden_by_command_and_keeps_its_page() {
    let (client, events) = Client::spawn(HELPER).unwrap();
    wait_for(&events, "ready", |event| {
        matches!(event, Event::Ready { .. })
    });

    // Not shown, and not asking for the keyboard: a PDF that waits to be known
    // drawable, a tab that is behind another.
    client
        .open(
            OpenWindow::new("h", Content::html("<title>kept</title><p>page</p>"))
                .size(300, 200)
                .hidden()
                .unfocused(),
        )
        .unwrap();
    // It loads all the same.
    wait_for(&events, "the page loading while hidden", |event| {
        matches!(
            event,
            Event::PageLoad {
                state: PageLoadState::Finished,
                ..
            }
        )
    });
    let ask = |script: &str, token: &str| {
        client.eval_with_token("h", script, token).unwrap();
        match wait_for(
            &events,
            "a result",
            |event| matches!(event, Event::EvalResult { token: t, .. } if t == token),
        ) {
            Event::EvalResult { result, .. } => result,
            _ => unreachable!(),
        }
    };
    // What the engine says of the window is what the page sees of it.
    assert_eq!(ask("document.visibilityState", "a"), json!("hidden"));
    ask("window.__kept = 42", "set");

    client.set_visible("h", true, false).unwrap();
    // The command is queued behind the window system's own: wait for the page
    // to see it.
    let deadline = Instant::now() + PATIENCE;
    while ask("document.visibilityState", "b") != json!("visible") {
        assert!(Instant::now() < deadline, "the window never became visible");
        std::thread::sleep(Duration::from_millis(50));
    }

    client.set_visible("h", false, false).unwrap();
    let deadline = Instant::now() + PATIENCE;
    while ask("document.visibilityState", "c") != json!("hidden") {
        assert!(Instant::now() < deadline, "the window never was hidden");
        std::thread::sleep(Duration::from_millis(50));
    }
    // Hiding is not closing: the page is the one it was.
    assert_eq!(ask("window.__kept", "kept"), json!(42));

    client.close("h").unwrap();
    wait_for(&events, "the window closing", |event| {
        matches!(event, Event::WindowClosed { .. })
    });
    client.quit().unwrap();
    assert!(client.wait(PATIENCE).is_some_and(|status| status.success()));
}
