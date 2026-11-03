//! A parent for the helper, in the way Hangar's app will be one: it starts
//! `hangar-webview`, opens a window whose page it serves itself through a
//! custom scheme, answers what the page says over ipc, and prints every event.
//!
//! The example does not build the helper. Build it first, then run this:
//!
//! ```text
//! cargo build -p hangar-webview --features engine
//! cargo run -p hangar-webview --example demo                 # a page with a button; close the window to end
//! cargo run -p hangar-webview --example demo -- --auto       # presses the button itself, leaves after the round trip
//! cargo run -p hangar-webview --example demo -- --kit        # a kit page: the real kit.js over the bridge
//! cargo run -p hangar-webview --example demo -- --pdf FILE   # a PDF, in whatever viewer the engine has
//! ```
//!
//! `--seconds N` closes the window after N seconds, `--helper PATH` says where
//! the helper is (it is looked for next to this program otherwise).

use std::error::Error;
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use hangar_webview::{
    Client, Content, Event, OpenWindow, PageLoadState, SchemeRequest, SchemeResponse, file_url,
    find_helper, scheme_url,
};
use serde_json::{Value, json};

const SCHEME: &str = "hangar-demo";
const WINDOW: &str = "demo";

/// The page of the default mode. It speaks to the parent with
/// `window.ipc.postMessage`, and hears it as `message` events on `window`.
const PAGE: &str = r#"<!doctype html>
<meta charset="utf-8" />
<title>Hangar webview demo</title>
<style>
  body { font: 16px system-ui, sans-serif; margin: 2rem; background: #18181b; color: #e4e4e7; }
  button { font: inherit; padding: .5rem 1rem; }
  pre { background: #09090b; padding: 1rem; border-radius: 6px; min-height: 6rem; }
</style>
<h1>Hangar webview demo</h1>
<p>This page was served by the parent, through the custom scheme <code>hangar-demo</code>.</p>
<button id="go">Send a ping over ipc</button>
<pre id="log"></pre>
<script>
  let count = 0;
  const log = (text) => (document.getElementById("log").textContent += text + "\n");
  const go = document.getElementById("go");
  go.onclick = () => {
    count += 1;
    log("-> ping " + count);
    window.ipc.postMessage(JSON.stringify({ ping: count }));
  };
  window.addEventListener("message", (event) => {
    log("<- " + JSON.stringify(event.data));
    if (event.data.pong === 1) window.ipc.postMessage(JSON.stringify({ roundTrip: "done" }));
  });
  if (location.search.includes("auto")) setTimeout(() => go.click(), 300);
</script>
"#;

/// The page of `--kit`: a kit's page as the SDK expects it, which is the point
/// of the bridge, so it loads the same `/_hangar/kit.js` a kit does.
const KIT_PAGE: &str = r#"<!doctype html>
<meta charset="utf-8" />
<title>Demo kit</title>
<link rel="stylesheet" href="/_hangar/kit.css" />
<script src="/_hangar/kit.js"></script>
<script src="/_hangar/kit-ui.js"></script>
<main class="kit-page">
  <h1>Demo kit</h1>
  <pre id="out" class="kit-pre"></pre>
  <button class="kit-button" id="again">Call panes.list</button>
</main>
<script>
  const out = document.getElementById("out");
  const log = (text) => (out.textContent += text + "\n");
  async function list() {
    try {
      log("panes.list -> " + JSON.stringify(await window.hangar.call("panes.list")));
      await window.hangar.call("tab.setTitle", { title: "Demo kit, " + new Date().toLocaleTimeString() });
    } catch (error) {
      log("failed: " + error.code + ": " + error.message);
    }
  }
  window.hangar.on("theme", (data) => log("theme event, scheme " + data.scheme));
  window.hangar.on("panes.changed", () => log("panes.changed"));
  document.getElementById("again").onclick = list;
  list();
</script>
"#;

enum Mode {
    Ping { auto: bool },
    Kit { sdk: PathBuf },
    Pdf(PathBuf),
}

struct Args {
    mode: Mode,
    seconds: Option<u64>,
    helper: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let (mut auto, mut kit, mut pdf, mut seconds, mut helper) = (false, false, None, None, None);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--auto" => auto = true,
            "--kit" => kit = true,
            "--pdf" => pdf = Some(PathBuf::from(value("--pdf")?)),
            "--seconds" => {
                seconds = Some(
                    value("--seconds")?
                        .parse()
                        .map_err(|_| "--seconds needs a number")?,
                )
            }
            "--helper" => helper = Some(PathBuf::from(value("--helper")?)),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let mode = match (kit, pdf) {
        (_, Some(path)) => Mode::Pdf(
            path.canonicalize()
                .map_err(|error| format!("{}: {error}", path.display()))?,
        ),
        (true, None) => {
            // The SDK is read from where hangar-core keeps it, so the demo runs
            // the file kits get, not a copy that could drift from it.
            let sdk = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../hangar-core/kit-sdk");
            if !sdk.join("kit.js").is_file() {
                return Err(format!(
                    "{} has no kit.js: --kit needs hangar-core's kit SDK",
                    sdk.display()
                ));
            }
            Mode::Kit { sdk }
        }
        (false, None) => Mode::Ping { auto },
    };
    Ok(Args {
        mode,
        seconds,
        helper,
    })
}

/// What the scheme answers, for the path of a request.
fn serve(request: &SchemeRequest, mode: &Mode) -> SchemeResponse {
    let path = request
        .uri
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map_or("/", |(_, path)| path);
    let path = format!("/{}", path.split(['?', '#']).next().unwrap_or(""));
    let not_found = || {
        SchemeResponse::new(request.request, 404)
            .header("Content-Type", "text/plain")
            .body("not found")
    };
    match (mode, path.as_str()) {
        (Mode::Ping { .. }, "/index.html") => {
            SchemeResponse::ok(request.request, "text/html; charset=utf-8", PAGE)
        }
        (Mode::Kit { .. }, "/demo-kit/index.html") => kit_file(
            request,
            "text/html; charset=utf-8",
            KIT_PAGE.as_bytes().to_vec(),
        ),
        (Mode::Kit { sdk }, "/_hangar/kit.js") => {
            sdk_file(request, sdk, "kit.js", "text/javascript; charset=utf-8")
        }
        (Mode::Kit { sdk }, "/_hangar/kit-ui.js") => {
            sdk_file(request, sdk, "kit-ui.js", "text/javascript; charset=utf-8")
        }
        (Mode::Kit { sdk }, "/_hangar/kit.css") => {
            sdk_file(request, sdk, "kit.css", "text/css; charset=utf-8")
        }
        (Mode::Kit { sdk }, "/_hangar/inter.woff2") => {
            sdk_file(request, sdk, "inter.woff2", "font/woff2")
        }
        _ => not_found(),
    }
}

fn sdk_file(
    request: &SchemeRequest,
    sdk: &std::path::Path,
    name: &str,
    mime: &str,
) -> SchemeResponse {
    match std::fs::read(sdk.join(name)) {
        Ok(bytes) => kit_file(request, mime, bytes),
        Err(error) => SchemeResponse::new(request.request, 500).body(error.to_string()),
    }
}

/// A kit's file with the policy `kit_scheme.rs` gives every one, cut down to
/// what this page needs: it runs in a sandbox, so its origin is opaque and it
/// has no storage, and it may load its own files and the SDK and nothing else.
fn kit_file(request: &SchemeRequest, mime: &str, bytes: Vec<u8>) -> SchemeResponse {
    let own = scheme_url(SCHEME, "/demo-kit/");
    let sdk = |file: &str| scheme_url(SCHEME, &format!("/_hangar/{file}"));
    let policy = [
        "sandbox allow-scripts".to_string(),
        "default-src 'none'".to_string(),
        format!(
            "script-src 'unsafe-inline' {own} {} {}",
            sdk("kit.js"),
            sdk("kit-ui.js")
        ),
        format!("style-src 'unsafe-inline' {own} {}", sdk("kit.css")),
        format!("font-src {} data:", sdk("inter.woff2")),
        "frame-src 'none'".to_string(),
        "form-action 'none'".to_string(),
    ]
    .join("; ");
    SchemeResponse::ok(request.request, mime, bytes)
        .header("Content-Security-Policy", policy)
        .header("Cache-Control", "no-store")
}

/// What a kit's page asked, answered the way Hangar would: by method, and an
/// error with a code for what this demo does not do.
fn kit_answer(client: &Client, message: &Value) -> Option<Value> {
    if message["hangar"] != 1 {
        return None;
    }
    let id = message.get("id")?.clone();
    let result = match message["method"].as_str()? {
        "panes.list" => {
            json!([{ "name": "Ava", "role": "agent" }, { "name": "Bo", "role": "shell" }])
        }
        "tab.setTitle" => {
            let title = message["params"]["title"].as_str().unwrap_or("Demo kit");
            client.set_title(WINDOW, title).ok()?;
            Value::Null
        }
        method => {
            return Some(
                json!({ "hangar": 1, "id": id, "error": { "code": "unknown", "message": format!("no method {method}") } }),
            );
        }
    };
    Some(json!({ "hangar": 1, "id": id, "result": result }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    let helper = args.helper.or_else(find_helper).ok_or(
        "hangar-webview was not found next to this program: build it with `cargo build -p hangar-webview --features engine`, or pass --helper",
    )?;
    println!("helper: {}", helper.display());
    let (client, events) = Client::spawn(&helper)?;

    let window = match &args.mode {
        Mode::Ping { auto } => {
            let query = if *auto { "?auto" } else { "" };
            OpenWindow::new(
                WINDOW,
                Content::url(scheme_url(SCHEME, &format!("/index.html{query}"))),
            )
            .title("Hangar webview demo")
            .size(640, 420)
            .scheme(SCHEME)
            .devtools()
        }
        Mode::Kit { .. } => OpenWindow::new(
            WINDOW,
            Content::url(scheme_url(SCHEME, "/demo-kit/index.html")),
        )
        .title("Demo kit")
        .size(640, 420)
        .scheme(SCHEME)
        // The page is a kit's, so it gets `window.hangar` as a frame gave it,
        // and it cannot go anywhere but its own files.
        .kit_bridge()
        .allow_navigation([scheme_url(SCHEME, "/demo-kit/")])
        .background(24, 24, 27),
        Mode::Pdf(path) => {
            let url = file_url(path).ok_or("the PDF path is not absolute")?;
            OpenWindow::new(WINDOW, Content::url(url))
                .title("PDF")
                .size(900, 700)
        }
    };
    client.open(window)?;

    let started = Instant::now();
    let limit = args.seconds.map(Duration::from_secs);
    let mut closing = false;
    loop {
        if !closing && limit.is_some_and(|limit| started.elapsed() > limit) {
            println!("-- time is up, closing the window");
            client.close(WINDOW)?;
            closing = true;
        }
        let event = match events.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                println!("-- the helper's output ended");
                break;
            }
        };
        println!("{:>6.2}s  {event:?}", started.elapsed().as_secs_f32());
        match event {
            Event::SchemeRequest(request) => client.respond(serve(&request, &args.mode))?,
            Event::Ipc { uri, body, .. } => {
                // What a page sends is the page's word, so it is read as such:
                // from the page this window opened on, as JSON, and nothing else.
                let Ok(message) = serde_json::from_str::<Value>(&body) else {
                    println!("-- ignored ipc that is not JSON");
                    continue;
                };
                match &args.mode {
                    Mode::Ping { auto } => {
                        if let Some(n) = message["ping"].as_u64() {
                            client.post_message(WINDOW, json!({ "pong": n }))?;
                            client.set_title(WINDOW, &format!("Hangar webview demo: pong {n}"))?;
                        } else if message["roundTrip"] == "done" && *auto {
                            println!("-- round trip done");
                            client.close(WINDOW)?;
                            closing = true;
                        }
                    }
                    Mode::Kit { .. } if uri.starts_with(&scheme_url(SCHEME, "/demo-kit/")) => {
                        if let Some(answer) = kit_answer(&client, &message) {
                            client.post_message(WINDOW, answer)?;
                        }
                    }
                    Mode::Kit { .. } => println!("-- ignored ipc from {uri}, which is not the kit's page"),
                    Mode::Pdf(_) => {}
                }
            }
            Event::PageLoad { state: PageLoadState::Finished, .. } => match &args.mode {
                // A page hears nothing sent before it has loaded, so the theme goes now.
                Mode::Kit { .. } => client.post_message(
                    WINDOW,
                    json!({ "hangar": 1, "event": "theme", "data": { "scheme": "dark", "locale": "en", "tokens": { "--kit-scale": "1" } } }),
                )?,
                Mode::Pdf(_) => client.eval_with_token(WINDOW, "navigator.pdfViewerEnabled", "pdf-viewer")?,
                Mode::Ping { .. } => {}
            },
            Event::EvalResult { token, result, .. } if token == "pdf-viewer" => {
                println!("-- the engine's built-in PDF viewer: {result}");
            }
            Event::WindowClosed { .. } => break,
            _ => {}
        }
    }

    client.quit().ok();
    match client.wait(Duration::from_secs(5)) {
        Some(status) => println!("helper exited: {status}"),
        None => println!("helper still running, it will be stopped"),
    }
    Ok(())
}
