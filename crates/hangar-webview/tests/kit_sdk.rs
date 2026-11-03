//! A kit's page with the real SDK (`hangar-core/kit-sdk/kit.js`), in the real
//! helper, under the policy the app serves it with (`kit_scheme::serve`), and
//! answered through the relay: the one path an HTML tab takes that no test had
//! walked end to end (the engine test uses a stand-in SDK and its own policy).
//!
//! It opens a window, so it needs a display, and is left out of a plain
//! `cargo test`; it is a file of its own because it points hangar-core at a
//! kits folder of its own through the environment, which one process holding
//! other tests should not see:
//!
//! ```text
//! cargo test -p hangar-webview --features engine --test kit_sdk -- --ignored
//! ```
#![cfg(feature = "engine")]

use std::path::Path;
use std::time::{Duration, Instant};

use hangar_webview::kit_relay::{self, Incoming};
use hangar_webview::{
    Client, Content, Event, OpenWindow, PageLoadState, SchemeRequest, SchemeResponse,
};
use serde_json::json;

const HELPER: &str = env!("CARGO_BIN_EXE_hangar-webview");
const KIT: &str = "relay-test";
const PATIENCE: Duration = Duration::from_secs(30);

/// What a kit's tab is: the SDK, a call through the relay, and the two things
/// the policy decides, the network (refused) and the kit's own files (allowed).
const PAGE: &str = r#"<!doctype html>
<meta charset="utf-8" />
<script src="/_hangar/kit.js"></script>
<script>
  const say = (text) => window.ipc.postMessage(text);
  say("sdk:" + typeof window.hangar + ":" + typeof (window.hangar && window.hangar.call));
  window.hangar.call("panes.list").then(
    (panes) => say("panes:" + JSON.stringify(panes)),
    (error) => say("failed:" + error.code + ":" + error.message),
  );
  fetch("https://example.com/").then(() => say("net:reached"), () => say("net:refused"));
  fetch("/relay-test/data.json")
    .then((response) => response.json())
    .then((data) => say("own:" + data.ok), () => say("own:failed"));
</script>
"#;

/// Installs a kit in a kits folder of the test's own, which hangar-core finds
/// through `HANGAR_ENDPOINT_FILE`, and returns the folder to clean up.
fn install(root: &Path) {
    let kit = root.join("source");
    std::fs::create_dir_all(kit.join("page")).unwrap();
    std::fs::write(
        kit.join("hangar-kit.json"),
        json!({
            "schema": 1,
            "id": KIT,
            "name": "Relay test",
            "version": "1.0.0",
            "description": "A kit for a test of the relay."
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(kit.join("page/index.html"), PAGE).unwrap();
    std::fs::write(kit.join("data.json"), r#"{"ok":1}"#).unwrap();

    // SAFETY: this file holds the one test of its process, and nothing else
    // has started a thread that reads the environment yet.
    unsafe {
        std::env::set_var(
            "HANGAR_ENDPOINT_FILE",
            root.join("hangar").join("endpoint.json"),
        );
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(hangar_core::kits::kit_add_folder(
            hangar_core::Host::new(),
            kit.to_string_lossy().into_owned(),
        ))
        .expect("the kit is added");
}

/// What the app's `kit_tabs::bridge::answer` does: the request to
/// hangar-core's `kit_scheme::serve`, and its answer back.
fn serve(request: &SchemeRequest) -> SchemeResponse {
    let mut builder = http::Request::builder()
        .method(request.method.as_str())
        .uri(request.uri.as_str());
    for header in &request.headers {
        builder = builder.header(header.name.as_str(), header.value.as_str());
    }
    let served = builder.body(request.body.clone()).expect("a request");
    let response = hangar_core::kit_scheme::serve(&served, &[]);
    let mut out = SchemeResponse::new(request.request, response.status().as_u16());
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            out = out.header(name.as_str(), value);
        }
    }
    out.body(response.into_body())
}

#[test]
#[ignore = "opens a window: needs a display"]
fn a_kit_page_with_the_real_sdk_calls_hangar_through_the_relay_under_its_policy() {
    let root = std::env::temp_dir().join(format!("hangar-kit-sdk-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    install(&root);

    let (client, events) = Client::spawn(HELPER).unwrap();
    let url = kit_relay::kit_file_url(KIT, "page/index.html");
    client
        .open(
            OpenWindow::new("kit", Content::url(url))
                .title("kit sdk")
                .size(400, 300)
                .scheme(kit_relay::SCHEME)
                .kit_bridge()
                .allow_navigation(kit_relay::kit_prefixes(KIT)),
        )
        .unwrap();

    let mut served: Vec<(String, u16)> = Vec::new();
    let mut policy = None;
    let mut said: Vec<String> = Vec::new();
    let mut calls: Vec<String> = Vec::new();
    let mut loaded = false;
    let wanted = ["sdk:", "panes:", "net:", "own:"];
    let deadline = Instant::now() + PATIENCE;
    while !(loaded
        && wanted
            .iter()
            .all(|start| said.iter().any(|line| line.starts_with(start))))
    {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = events
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("the page finishes: served {served:?}, said {said:?}"));
        match event {
            Event::SchemeRequest(request) => {
                let response = serve(&request);
                let path = request.uri.split_once("://").unwrap().1.to_string();
                if path.ends_with("page/index.html") {
                    policy = response
                        .headers
                        .iter()
                        .find(|header| header.name.eq_ignore_ascii_case("content-security-policy"))
                        .map(|header| header.value.clone());
                }
                served.push((path, response.status));
                client.respond(response).unwrap();
            }
            Event::Ipc { window, uri, body } => {
                // As the app's `PageHost::receive` does: only a call of the
                // kit's own document is answered.
                match kit_relay::read(&body).filter(|_| kit_relay::is_kit_document(&uri, KIT)) {
                    Some(Incoming::Call { id, method, .. }) => {
                        calls.push(method.clone());
                        let answer = if method == "panes.list" {
                            kit_relay::result(&id, json!([{ "name": "Ava" }]))
                        } else {
                            kit_relay::error(&id, "unavailable", "no such method")
                        };
                        client.post_message(&window, answer).unwrap();
                    }
                    Some(Incoming::NoMethod { .. }) => {}
                    None => said.push(body),
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

    // The real SDK ran, and its call went through the relay and came back.
    assert!(
        said.contains(&"sdk:object:function".to_string()),
        "{said:?}"
    );
    assert_eq!(calls, ["panes.list"]);
    assert!(
        said.contains(&r#"panes:[{"name":"Ava"}]"#.to_string()),
        "{said:?}"
    );
    // The policy the app serves the page with held: the network is refused and
    // the kit's own files are not.
    assert!(said.contains(&"net:refused".to_string()), "{said:?}");
    assert!(said.contains(&"own:1".to_string()), "{said:?}");
    let policy = policy.expect("the page was served with a policy");
    assert!(policy.starts_with("sandbox allow-scripts"), "{policy}");
    assert!(policy.contains("connect-src"), "{policy}");
    // The SDK itself came from the scheme, as the policy names it.
    assert!(
        served
            .iter()
            .any(|(path, status)| path.ends_with("/_hangar/kit.js") && *status == 200),
        "{served:?}"
    );

    client.quit().unwrap();
    assert!(client.wait(PATIENCE).is_some_and(|status| status.success()));
    let _ = std::fs::remove_dir_all(&root);
}
