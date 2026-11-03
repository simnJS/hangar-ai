//! A kit's native tab in a window of its own, answered by a pretend Hangar:
//! two panes, a data folder in memory, toasts written under the page.
//!
//! ```sh
//! examples/kits/hello-native/build.sh
//! cargo run -p hangar-kit-host --example preview -- examples/kits/hello-native/tabs/hello.wasm
//! cargo run -p hangar-kit-host --example preview -- path/to/tab.wasm --light --fr
//! ```
//!
//! Every permission is granted, so a tab's denied states show only in its
//! tests. The window is 360 points wide, the panel's narrowest but one.
//!
//! `HANGAR_PREVIEW_SHOT=page.rgba` takes a picture of the window after two
//! seconds and quits: the raw RGBA, its width and height first on a line of
//! their own, for a script to turn into an image.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use iced::widget::{column, container, text};
use iced::{Element, Length, Subscription, Task};
use serde_json::{Value, json};

use hangar_kit_host::ui::abi::{KitInfo, TabInfo};
use hangar_kit_host::ui::api::{KitError, Scheme, Theme};
use hangar_kit_host::{
    CallFuture, HostCall, KitApi, KitMessage, KitRuntime, KitStyle, KitTab, Limits, TabStart, ready,
};

/// What the window shows of the pretend Hangar's answers.
type Said = Arc<Mutex<Vec<String>>>;

struct Pretend {
    kit: PathBuf,
    data: Mutex<HashMap<String, String>>,
    said: Said,
}

impl Pretend {
    fn say(&self, line: String) {
        println!("{line}");
        let mut said = self.said.lock().unwrap();
        said.push(line);
        let excess = said.len().saturating_sub(4);
        said.drain(..excess);
    }
}

impl KitApi for Pretend {
    fn granted(&self) -> Vec<String> {
        [
            "workspace.read",
            "panes.read",
            "panes.send",
            "files.read",
            "data.read",
            "data.write",
            "preview.show",
            "notify",
        ]
        .map(String::from)
        .to_vec()
    }

    fn call(&self, call: HostCall) -> CallFuture {
        ready(match call {
            HostCall::WorkspaceInfo => {
                Ok(json!({ "id": "preview", "name": "Preview", "folder": "/tmp/preview" }))
            }
            HostCall::PanesList => Ok(json!([
                { "name": "Ava", "agent": "claude", "role": "orchestrator", "state": "working", "canSend": true },
                { "name": "Bo", "agent": "claude", "role": "worker", "state": "idle", "canSend": true },
                { "name": "logs", "agent": "shell", "role": null, "state": null, "canSend": false }
            ])),
            HostCall::PanesSend { pane, text } => {
                self.say(format!("panes.send to {pane}: {text}"));
                Ok(json!({ "queued": true, "pane": pane }))
            }
            HostCall::DataRead { path } => match self.data.lock().unwrap().get(&path) {
                Some(text) => Ok(json!({ "text": text })),
                None => Err(KitError::new(
                    KitError::NOT_FOUND,
                    format!("no {path} in the kit's data"),
                )),
            },
            HostCall::DataWrite { path, text } => {
                self.say(format!("data.write {path}: {} bytes", text.len()));
                self.data.lock().unwrap().insert(path, text);
                Ok(json!({}))
            }
            HostCall::Notify { title, body } => {
                self.say(format!("notify: {title} {body}"));
                Ok(Value::Null)
            }
            HostCall::Toast(toast) => {
                self.say(format!("toast ({:?}): {}", toast.kind, toast.message));
                Ok(json!({ "action": false }))
            }
            HostCall::Confirm(question) => {
                self.say(format!("confirm, answered yes: {}", question.message));
                Ok(json!(true))
            }
            HostCall::Copy { text } => {
                self.say(format!("copied: {text}"));
                Ok(json!({}))
            }
            HostCall::TabParams => Ok(json!({ "params": {}, "caller": null })),
            HostCall::TabSetTitle { title } => {
                self.say(format!("title: {title}"));
                Ok(json!({}))
            }
            HostCall::TabSetBadge { badge } => {
                self.say(format!("badge: {badge:?}"));
                Ok(json!({}))
            }
            other => Err(KitError::new(
                KitError::UNAVAILABLE,
                format!("the preview does not do {}", other.method()),
            )),
        })
    }

    fn image(&self, source: &hangar_kit_host::ui::ImageSource) -> Result<Vec<u8>, KitError> {
        match source {
            hangar_kit_host::ui::ImageSource::Kit(path) => std::fs::read(self.kit.join(path))
                .map_err(|why| KitError::new(KitError::NOT_FOUND, why.to_string())),
            hangar_kit_host::ui::ImageSource::Data(_) => Err(KitError::new(
                KitError::NOT_FOUND,
                "no image in the preview's data",
            )),
        }
    }
}

struct Preview {
    tab: Result<KitTab, String>,
    style: KitStyle,
    said: Said,
    shot: Option<PathBuf>,
}

#[derive(Debug, Clone)]
enum Message {
    Kit(KitMessage),
    Shoot,
    Shot(iced::window::Screenshot),
}

/// The kit's root: the folder holding `hangar-kit.json` above the module.
fn kit_root(module: &Path) -> PathBuf {
    module
        .ancestors()
        .skip(1)
        .find(|folder| folder.join("hangar-kit.json").is_file())
        .unwrap_or_else(|| module.parent().unwrap_or(Path::new(".")))
        .to_path_buf()
}

fn boot() -> (Preview, Task<Message>) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let module = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples/kits/hello-native/tabs/hello.wasm"));
    let light = args.iter().any(|arg| arg == "--light");
    let locale = if args.iter().any(|arg| arg == "--fr") {
        "fr"
    } else {
        "en"
    };
    let style = if light {
        KitStyle::light()
    } else {
        KitStyle::dark()
    }
    .with_locale(hangar_i18n::Locale::from_tag(locale).unwrap_or_default());
    let said = Said::default();
    let shot = std::env::var_os("HANGAR_PREVIEW_SHOT").map(PathBuf::from);
    let started = (|| {
        let bytes = std::fs::read(&module).map_err(|why| format!("{}: {why}", module.display()))?;
        let runtime = KitRuntime::new(Limits::default()).map_err(|why| why.to_string())?;
        let compiled = runtime.compile(&bytes).map_err(|why| why.to_string())?;
        let pretend = Arc::new(Pretend {
            kit: kit_root(&module),
            data: Mutex::default(),
            said: said.clone(),
        });
        let start = TabStart {
            hangar: env!("CARGO_PKG_VERSION").into(),
            kit: KitInfo {
                id: "preview".into(),
                name: "Preview".into(),
                version: "0.0.0".into(),
            },
            tab: TabInfo {
                id: "tab".into(),
                params: Default::default(),
                caller: None,
            },
            theme: Theme {
                scheme: if light { Scheme::Light } else { Scheme::Dark },
                locale: locale.into(),
            },
            visible: true,
        };
        Ok::<_, String>(KitTab::start(&runtime, &compiled, start, pretend))
    })();
    match started {
        Ok((tab, effects)) => (
            Preview {
                tab: Ok(tab),
                style,
                said,
                shot,
            },
            effects.task().map(Message::Kit),
        ),
        Err(why) => (
            Preview {
                tab: Err(why),
                style,
                said,
                shot,
            },
            Task::none(),
        ),
    }
}

fn update(preview: &mut Preview, message: Message) -> Task<Message> {
    match (message, &mut preview.tab) {
        (Message::Kit(message), Ok(tab)) => tab.update(message).task().map(Message::Kit),
        (Message::Kit(_), Err(_)) => Task::none(),
        (Message::Shoot, _) => iced::window::latest()
            .and_then(iced::window::screenshot)
            .map(Message::Shot),
        (Message::Shot(shot), _) => {
            if let Some(path) = &preview.shot {
                let mut bytes = format!("{} {}\n", shot.size.width, shot.size.height).into_bytes();
                bytes.extend_from_slice(&shot.rgba);
                if let Err(why) = std::fs::write(path, bytes) {
                    eprintln!("{}: {why}", path.display());
                }
            }
            iced::exit()
        }
    }
}

fn view(preview: &Preview) -> Element<'_, Message> {
    let page: Element<'_, Message> = match &preview.tab {
        Ok(tab) => tab.draw(preview.style).map(Message::Kit),
        Err(why) => container(text(why.as_str())).padding(16).into(),
    };
    let said = preview.said.lock().unwrap().join("\n");
    let palette = preview.style.palette;
    column![
        container(page).height(Length::Fill),
        container(
            text(said)
                .size(11)
                .font(preview.style.mono)
                .color(palette.text_dim)
        )
        .padding([6, 12])
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(palette.well)),
            ..Default::default()
        }),
    ]
    .into()
}

fn subscription(preview: &Preview) -> Subscription<Message> {
    let tab = match &preview.tab {
        Ok(tab) => tab.subscription().map(Message::Kit),
        Err(_) => Subscription::none(),
    };
    let shot = match preview.shot {
        Some(_) => iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::Shoot),
        None => Subscription::none(),
    };
    Subscription::batch([tab, shot])
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .font(hangar_theme::fonts::INTER_VARIABLE)
        .default_font(hangar_theme::fonts::ui())
        .title("Hangar kit preview")
        .subscription(subscription)
        .window_size((360.0, 720.0))
        .run()
}
