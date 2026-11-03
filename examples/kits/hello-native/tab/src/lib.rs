//! Hello native: a kit's tab in Rust, to read before writing one.
//!
//! - **State**: the tab is a struct, `update` changes it, `view` draws it.
//! - **Inputs**: a name to greet, a note of several lines.
//! - **A table**: the workspace's panes, sorted by Hangar, a row to pick.
//! - **Host calls**: `panes.list`, `panes.send`, `data.read`, `data.write`,
//!   a toast with an action, the clipboard; each answer is a message.
//! - **Subscriptions**: `panes.changed` reads the panes again, a clock
//!   ticks while the tab is in front, the language follows Hangar's.
//!
//! Every refusal is a state of the page, drawn, never a panic: `denied` says
//! where the permission is given.

use std::time::Duration;

use hangar_kit_sdk::api::{DataText, Done, Pane, Queued, Theme, ToastClosed};
use hangar_kit_sdk::prelude::*;
use hangar_kit_sdk::ui::{SelectOption, TableColumn};

/// Where the note is kept, in the kit's own data folder.
const NOTE: &str = "notes/hello.txt";

fn strings(locale: &str) -> Strings {
    Strings::new(locale)
        .add(
            "en",
            &[
                ("title", "Hello, native"),
                ("reading", "Reading the panes…"),
                ("panes_one", "{n} pane"),
                ("panes_other", "{n} panes"),
                ("read", "read {ago} ago"),
                ("name", "Your name"),
                ("greet", "Greet"),
                ("greeted", "Hello, {name}!"),
                ("undo", "Undo"),
                ("greetings_one", "{n} greeting"),
                ("greetings_other", "{n} greetings"),
                ("noPane", "No pane yet"),
                (
                    "noPaneText",
                    "Open a terminal in this workspace and it shows here.",
                ),
                ("denied", "This tab may not list the panes"),
                (
                    "deniedText",
                    "Grant panes.read to Hello native in Settings › Kits.",
                ),
                ("ping", "Ping"),
                ("pinged", "Sent to {pane}."),
                ("note", "Note"),
                (
                    "noteHint",
                    "Kept in the kit's data folder, the same in every workspace.",
                ),
                ("save", "Save"),
                ("saved", "Saved"),
                ("unsaved", "Not saved"),
            ],
        )
        .add(
            "fr",
            &[
                ("title", "Bonjour, en natif"),
                ("reading", "Lecture des panneaux…"),
                ("panes_one", "{n} panneau"),
                ("panes_other", "{n} panneaux"),
                ("read", "lus il y a {ago}"),
                ("name", "Votre nom"),
                ("greet", "Saluer"),
                ("greeted", "Bonjour, {name} !"),
                ("undo", "Annuler"),
                ("greetings_one", "{n} salut"),
                ("greetings_other", "{n} saluts"),
                ("noPane", "Aucun panneau"),
                (
                    "noPaneText",
                    "Ouvrez un terminal dans cet espace : il apparaît ici.",
                ),
                ("denied", "Cet onglet ne peut pas lister les panneaux"),
                (
                    "deniedText",
                    "Accordez panes.read à Hello native dans Réglages › Kits.",
                ),
                ("ping", "Relancer"),
                ("pinged", "Envoyé à {pane}."),
                ("note", "Note"),
                (
                    "noteHint",
                    "Gardée dans le dossier de données du kit, la même partout.",
                ),
                ("save", "Enregistrer"),
                ("saved", "Enregistrée"),
                ("unsaved", "Non enregistrée"),
            ],
        )
}

enum Panes {
    Reading,
    Read(Vec<Pane>),
    Refused(KitError),
}

pub struct Hello {
    t: Strings,
    name: String,
    greetings: u32,
    panes: Panes,
    /// The pane picked in the table, by name: it outlives a new list.
    picked: Option<String>,
    note: String,
    saved: Option<String>,
    read_at: Option<i64>,
    now: i64,
    visible: bool,
    scope: String,
}

#[derive(Clone, Debug)]
pub enum Message {
    Name(String),
    Greet,
    Greeted(Result<ToastClosed, KitError>),
    Refresh,
    Panes(Result<Vec<Pane>, KitError>),
    Pick(usize),
    Ping,
    Pinged(Result<Queued, KitError>),
    Note(String),
    NoteRead(Result<DataText, KitError>),
    Save,
    Saved(Result<Done, KitError>),
    Scope(String),
    Menu(String),
    Tick,
    Visible(bool),
    Theme(Theme),
}

impl Hello {
    fn panes_shown(&self) -> Vec<&Pane> {
        match &self.panes {
            Panes::Read(panes) => panes
                .iter()
                .filter(|pane| self.scope == "all" || pane.agent == "claude")
                .collect(),
            _ => Vec::new(),
        }
    }
}

impl Kit for Hello {
    type Message = Message;

    fn init(start: &Start, cx: &mut Context<Message>) -> Self {
        cx.call(api::PanesList {}, Message::Panes);
        cx.call(api::DataRead { path: NOTE.into() }, Message::NoteRead);
        Hello {
            t: strings(&start.theme.locale),
            name: String::new(),
            greetings: 0,
            panes: Panes::Reading,
            picked: None,
            note: String::new(),
            saved: None,
            read_at: None,
            now: cx.now(),
            visible: start.visible,
            scope: "all".into(),
        }
    }

    fn update(&mut self, message: Message, cx: &mut Context<Message>) {
        match message {
            Message::Name(name) => self.name = name,
            Message::Greet => {
                self.greetings += 1;
                let name = if self.name.trim().is_empty() {
                    "you"
                } else {
                    self.name.trim()
                };
                cx.call(
                    api::Toast {
                        message: self.t.tv("greeted", &[("name", name)]),
                        kind: Notice::Success,
                        action: Some(self.t.t("undo")),
                    },
                    Message::Greeted,
                );
            }
            // The toast's Undo takes the greeting back; its going by itself
            // leaves it.
            Message::Greeted(Ok(ToastClosed { action: true })) => {
                self.greetings = self.greetings.saturating_sub(1)
            }
            Message::Greeted(_) => {}
            Message::Refresh => cx.call(api::PanesList {}, Message::Panes),
            Message::Panes(answer) => {
                self.read_at = Some(cx.now());
                self.panes = match answer {
                    Ok(panes) => Panes::Read(panes),
                    Err(error) => Panes::Refused(error),
                };
            }
            Message::Pick(index) => {
                self.picked = self.panes_shown().get(index).map(|pane| pane.name.clone());
            }
            Message::Ping => {
                if let Some(pane) = self.picked.clone() {
                    cx.call(
                        api::PanesSend {
                            pane,
                            text: "Hello from the hello-native kit: where are you at?".into(),
                        },
                        Message::Pinged,
                    );
                }
            }
            Message::Pinged(Ok(queued)) => {
                cx.toast(Notice::Info, self.t.tv("pinged", &[("pane", &queued.pane)]))
            }
            // Not a Claude Code agent, no bridge, too many a minute: the
            // message says which, in Hangar's words.
            Message::Pinged(Err(error)) => cx.toast(Notice::Error, error.message),
            Message::Note(note) => self.note = note,
            Message::NoteRead(Ok(read)) => {
                self.note = read.text.clone();
                self.saved = Some(read.text);
            }
            // Nothing saved yet is no failure.
            Message::NoteRead(Err(error)) if error.is(KitError::NOT_FOUND) => {
                self.saved = Some(String::new())
            }
            Message::NoteRead(Err(error)) => cx.toast(Notice::Error, error.message),
            Message::Save => {
                let text = self.note.clone();
                cx.call(
                    api::DataWrite {
                        path: NOTE.into(),
                        text,
                    },
                    Message::Saved,
                );
            }
            Message::Saved(Ok(_)) => self.saved = Some(self.note.clone()),
            Message::Saved(Err(error)) => cx.toast(Notice::Error, error.message),
            Message::Scope(scope) => self.scope = scope,
            Message::Menu(item) => match item.as_str() {
                "copy" => {
                    let names: Vec<&str> = self
                        .panes_shown()
                        .iter()
                        .map(|pane| pane.name.as_str())
                        .collect();
                    cx.copy(names.join("\n"));
                }
                "refresh" => cx.call(api::PanesList {}, Message::Panes),
                _ => {}
            },
            Message::Tick => self.now = cx.now(),
            Message::Visible(visible) => {
                self.visible = visible;
                self.now = cx.now();
            }
            Message::Theme(theme) => self.t.set_locale(theme.locale),
        }
    }

    fn view(&self) -> Element<Message> {
        let t = &self.t;
        let state: Element<Message> = match &self.panes {
            Panes::Reading => state_label(Status::Running, t.t("reading")).into(),
            Panes::Read(panes) => {
                state_label(Status::Success, t.tn("panes", panes.len() as i64, &[])).into()
            }
            Panes::Refused(error) => state_label(Status::Failure, error.code.clone()).into(),
        };
        let ago = self.read_at.map(|at| {
            t.tv(
                "read",
                &[(
                    "ago",
                    &format::duration((self.now - at).max(0) as f64, t.locale()),
                )],
            )
        });
        let header = header(t.t("title"))
            .state(state)
            .state(text(ago.unwrap_or_default()).small().muted())
            .action(
                menu([
                    menu_item("refresh", "Refresh").into(),
                    separator(),
                    menu_item("copy", "Copy the names").into(),
                ])
                .on_select(Message::Menu),
            );

        let greet = section(
            t.t("greet"),
            [
                row![
                    text_input(t.t("name"), &self.name)
                        .on_input(Message::Name)
                        .on_submit(Message::Greet),
                    button(t.t("greet")).primary().on_press(Message::Greet),
                ]
                .spacing(8.0)
                .align(Align::Center)
                .into(),
                text(t.tn("greetings", self.greetings as i64, &[]))
                    .small()
                    .dim()
                    .into(),
            ],
        );

        let panes: Element<Message> = match &self.panes {
            Panes::Reading => loading(t.t("reading")).into(),
            Panes::Refused(error) if error.is(KitError::DENIED) => {
                denied(t.t("denied")).text(t.t("deniedText")).into()
            }
            Panes::Refused(error) => failed(error.message.clone())
                .action(button("Try again").small().on_press(Message::Refresh))
                .into(),
            Panes::Read(_) if self.panes_shown().is_empty() => {
                empty(t.t("noPane")).text(t.t("noPaneText")).into()
            }
            Panes::Read(_) => {
                let shown = self.panes_shown();
                let picked = shown
                    .iter()
                    .position(|pane| Some(&pane.name) == self.picked.as_ref());
                let rows = shown.iter().map(|pane| {
                    let state = pane.state.clone().unwrap_or_else(|| "?".into());
                    let status = match state.as_str() {
                        "working" => Status::Running,
                        "waiting" => Status::Waiting,
                        "yours" => Status::Success,
                        _ => Status::Neutral,
                    };
                    table_row([
                        cell(text(pane.name.clone()).truncate()),
                        cell(chip(pane.agent.clone())).key(pane.agent.as_str()),
                        cell(state_label(status, state.clone())).key(state),
                    ])
                });
                let mut table = table(
                    [
                        TableColumn::new("Pane").sortable(),
                        TableColumn::new("Agent").sortable(),
                        TableColumn::new("State").sortable(),
                    ],
                    rows,
                )
                .id("panes")
                .on_row(Message::Pick);
                if let Some(picked) = picked {
                    table = table.selected(picked as u32);
                }
                col![table, bars(by_agent(&shown)).max(shown.len() as f32)]
                    .spacing(12.0)
                    .into()
            }
        };
        let can_ping = self.picked.as_ref().is_some_and(|picked| {
            self.panes_shown()
                .iter()
                .any(|pane| &pane.name == picked && pane.can_send)
        });
        let panes = section(
            "Panes",
            [
                row![
                    select(
                        [
                            SelectOption::new("all", "Every pane"),
                            SelectOption::new("claude", "Claude Code")
                        ],
                        Some(self.scope.clone())
                    )
                    .on_select(Message::Scope),
                    fill(),
                    button(t.t("ping"))
                        .small()
                        .on_press_maybe(can_ping.then_some(Message::Ping)),
                ]
                .spacing(8.0)
                .align(Align::Center)
                .into(),
                panes,
            ],
        );

        let dirty = self.saved.as_ref() != Some(&self.note);
        let note = section(
            t.t("note"),
            [
                text_area("note", &self.note)
                    .label(t.t("note"))
                    .hint(t.t("noteHint"))
                    .on_input(Message::Note)
                    .into(),
                row![
                    button(t.t("save"))
                        .small()
                        .on_press_maybe(dirty.then_some(Message::Save)),
                    chip(if dirty { t.t("unsaved") } else { t.t("saved") }).tone(if dirty {
                        Tone::Warn
                    } else {
                        Tone::Ok
                    }),
                ]
                .spacing(8.0)
                .align(Align::Center)
                .into(),
            ],
        );

        page![header, greet, panes, note].into()
    }

    fn subscriptions(&self) -> Subscriptions<Message> {
        let mut subscriptions = Subscriptions::none()
            .panes_changed(Message::Refresh)
            .visibility(Message::Visible)
            .theme(Message::Theme);
        // The clock only says how long ago the panes were read: behind
        // another tab, nobody reads it.
        if self.visible {
            subscriptions = subscriptions.every("clock", Duration::from_secs(1), Message::Tick);
        }
        subscriptions
    }
}

/// How many panes run each agent, most first.
fn by_agent(panes: &[&Pane]) -> Vec<hangar_kit_sdk::ui::BarItem> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for pane in panes {
        match counts.iter_mut().find(|(agent, _)| *agent == pane.agent) {
            Some((_, count)) => *count += 1,
            None => counts.push((pane.agent.clone(), 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts
        .into_iter()
        .map(|(agent, count)| bar_item(agent, count as f32).text(count.to_string()))
        .collect()
}

export_kit!(Hello);

#[cfg(test)]
mod tests {
    use serde_json::json;

    use hangar_kit_sdk::testing::{self, Harness, StartExt};

    use super::*;

    fn panes() -> serde_json::Value {
        json!([
            { "name": "Ava", "agent": "claude", "role": "worker", "state": "working", "canSend": true },
            { "name": "shell", "agent": "shell", "role": null, "state": null, "canSend": false }
        ])
    }

    fn started() -> Harness<Hello> {
        let mut tab = Harness::<Hello>::new(testing::start().grant([
            "panes.read",
            "panes.send",
            "data.read",
            "data.write",
        ]));
        tab.reply_to("panes.list", Ok(panes()));
        tab.reply_to(
            "data.read",
            Err(KitError::new(KitError::NOT_FOUND, "no such file")),
        );
        tab
    }

    #[test]
    fn it_reads_the_panes_and_the_note() {
        let tab = started();
        assert!(tab.shows("2 panes"));
        assert!(tab.shows("Saved"));
        assert!(tab.calls().is_empty());
    }

    #[test]
    fn a_refusal_says_where_the_permission_is() {
        let mut tab = Harness::<Hello>::new(testing::start());
        tab.reply_to(
            "panes.list",
            Err(KitError::new(
                KitError::DENIED,
                "this tab was not granted panes.read",
            )),
        );
        assert!(tab.shows("This tab may not list the panes"));
    }

    #[test]
    fn greeting_toasts_and_undo_takes_it_back() {
        let mut tab = started();
        tab.type_into("Your name", "Zoé");
        tab.press("Greet");
        assert_eq!(tab.kit().greetings, 1);
        let toast = tab.reply_to("ui.toast", Ok(json!({ "action": true })));
        assert_eq!(toast.params["message"], "Hello, Zoé!");
        assert_eq!(tab.kit().greetings, 0);
    }

    #[test]
    fn the_note_is_saved_in_the_data_folder() {
        let mut tab = started();
        tab.type_into("note", "Ship it");
        assert!(tab.shows("Not saved"));
        tab.press("Save");
        let write = tab.reply_to("data.write", Ok(json!({})));
        assert_eq!(write.params, json!({ "path": NOTE, "text": "Ship it" }));
        assert!(tab.shows("Saved"));
    }

    #[test]
    fn subscriptions_follow_visibility() {
        let mut tab = started();
        assert!(
            tab.subscriptions()
                .events
                .contains(&"panes.changed".to_owned())
        );
        assert_eq!(tab.subscriptions().timers.len(), 1);
        tab.event("tab.visibility", json!({ "visible": false }));
        assert!(tab.subscriptions().timers.is_empty());
        tab.event("panes.changed", json!({}));
        assert_eq!(tab.methods(), vec!["panes.list"]);
    }
}
