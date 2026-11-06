//! The conversation headless (`ICED_TEST_BACKEND=tiny-skia`): each kind of
//! item drawn, the question and permission cards and the desk calls they
//! make, the input and why it is disabled, the empty and loading states,
//! the optimistic message, the older page, and when it reads again.
//!
//! No desk runs here (no Hangar Mobile runtime in a test model): the calls
//! the region asks are read from its log, and what a call answers is handed
//! to it as the message the desk's answer would be.

use hangar_core::chat::{
    Answer, Body, CallResult, CallState, ChatItem, Question, QuestionOption, Sender,
};
use serde_json::json;

use super::*;
use crate::app::Packages;
use crate::mobile::contract::{ChatPending, ChatReason, RemoteAnswered, RemoteSent};
use crate::navigator::testing::{ctx, model_of, pane, simulate, workspace};
use crate::settings::experiments;
use crate::state::types::{AppState, PaneRole};
use crate::state::{Patch, Settings, StoreAction};

const WS: &str = "conv";
const LEO: &str = "leo";

/// A workspace shown as the chat view, its orchestrator Leo (Claude Code),
/// and a worker beside it; the experiment on.
fn shop(orchestrator: bool) -> Model {
    let mut leo = pane(LEO, "Leo", AgentId::Claude);
    if orchestrator {
        leo.role = Some(PaneRole::Orchestrator);
    }
    let mut m = model_of(AppState {
        workspaces: vec![workspace(
            WS,
            "Shop",
            None,
            vec![leo, pane("ava", "Ava", AgentId::Claude)],
        )],
        active_workspace_id: Some(WS.into()),
        ..AppState::default()
    });
    m.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| s.experiments = vec![experiments::CHAT_VIEW.into()],
    )));
    m.apply(StoreAction::SetChatView {
        ws: WS.into(),
        on: true,
    });
    m
}

fn item(id: &str, body: Body) -> ChatItem {
    ChatItem {
        id: id.into(),
        at: 1_000,
        body,
    }
}

fn person(id: &str, words: &str) -> ChatItem {
    item(
        id,
        Body::User {
            text: words.into(),
            from: Sender::Person,
            images: 0,
        },
    )
}

fn page(items: Vec<ChatItem>, pending: Option<ChatPending>) -> ChatView {
    ChatView {
        pane_id: LEO.into(),
        pane: "Leo".into(),
        workspace: "Shop".into(),
        agent: "claude".into(),
        activity: None,
        session_id: Some("s1".into()),
        items,
        older: false,
        cursor: None,
        pending,
        reason: None,
    }
}

/// The region's state after Leo's newest page answered `view`, the desk up.
fn read_into(st: &mut State, m: &mut Model, view: ChatView) {
    let chat = st.chat_mut(WS, LEO);
    chat.reading = true;
    let _ = handle(
        st,
        m,
        Msg::Read {
            ws: WS.into(),
            pane: LEO.into(),
            older: false,
            result: Ok(Box::new(view)),
        },
    );
    st.chat_mut(WS, LEO).desk = DeskState {
        ready: true,
        ..DeskState::default()
    };
}

/// The texts among `wanted` the region shows.
fn shown_texts(st: &State, m: &Model, wanted: &[&str]) -> Vec<String> {
    let pkg = Packages::default();
    let ctx = ctx(m, &pkg);
    let ws = m.workspace(WS).unwrap();
    let mut ui = simulate(view(st, ctx, ws));
    let _ = ui
        .snapshot(&hangar_theme::to_iced(ctx.theme))
        .expect("it draws");
    wanted
        .iter()
        .filter(|words| ui.find(**words).is_ok())
        .map(|words| words.to_string())
        .collect()
}

/// The region's messages after clicking each of `labels` in turn.
fn clicks(st: &State, m: &Model, labels: &[&str]) -> Vec<Msg> {
    let pkg = Packages::default();
    let ctx = ctx(m, &pkg);
    let ws = m.workspace(WS).unwrap();
    let mut ui = simulate(view(st, ctx, ws));
    for label in labels {
        ui.click(*label)
            .unwrap_or_else(|_| panic!("“{label}” is there"));
    }
    ours(ui.into_messages())
}

fn ours(messages: impl Iterator<Item = Message>) -> Vec<Msg> {
    messages
        .filter_map(|message| match message {
            Message::ChatView(crate::chat_view::Msg::Conversation(inner)) => Some(inner),
            _ => None,
        })
        .collect()
}

fn apply(st: &mut State, m: &mut Model, messages: Vec<Msg>) {
    for message in messages {
        let _ = handle(st, m, message);
    }
}

fn calls(st: &State) -> Vec<Call> {
    st.calls().iter().map(|(_, call)| call.clone()).collect()
}

/* ── empty and loading ─────────────────────────────────────────── */

#[test]
fn without_an_orchestrator_the_region_explains_and_offers_to_start_one() {
    let m = shop(false);
    let st = State::default();
    assert_eq!(
        shown_texts(
            &st,
            &m,
            &[
                "No orchestrator in this workspace",
                "Start the orchestrator"
            ]
        ),
        [
            "No orchestrator in this workspace",
            "Start the orchestrator"
        ]
    );
    let asked = clicks(&st, &m, &["Start the orchestrator"]);
    assert!(matches!(asked.as_slice(), [Msg::Start(ws)] if ws == WS));
}

#[test]
fn the_hq_cannot_have_an_orchestrator_and_offers_none() {
    let mut hq = shop(false).state().clone();
    hq.workspaces[0].hq = Some(true);
    let m = model_of(hq);
    let st = State::default();
    assert_eq!(
        shown_texts(
            &st,
            &m,
            &[
                "No orchestrator in this workspace",
                "Start the orchestrator",
                "This workspace cannot have an orchestrator."
            ]
        ),
        [
            "No orchestrator in this workspace",
            "This workspace cannot have an orchestrator."
        ]
    );
}

#[test]
fn before_a_first_read_the_region_says_it_loads() {
    let m = shop(true);
    let st = State::default();
    assert_eq!(
        shown_texts(&st, &m, &["Loading the conversation…"]),
        ["Loading the conversation…"]
    );
}

/* ── items ─────────────────────────────────────────────────────── */

#[test]
fn each_kind_of_item_is_drawn() {
    let mut m = shop(true);
    let mut st = State::default();
    read_into(
        &mut st,
        &mut m,
        page(
            vec![
                person("u1", "Plan the search box"),
                item(
                    "h1",
                    Body::User {
                        text: "[Hangar] Worker Ada delivered.\nline 2\nline 3\nline 4".into(),
                        from: Sender::Hangar,
                        images: 0,
                    },
                ),
                item(
                    "a1",
                    Body::Assistant {
                        text: "## The plan\n\nFirst **index**, then:\n\n```rust\nfn main() {}\n```"
                            .into(),
                    },
                ),
                item(
                    "c1",
                    Body::Action {
                        tool: "mcp__hangar__board_create_task".into(),
                        title: "Used board_create_task".into(),
                        detail: Some(json!({"title": "Search box"}).to_string()),
                        result: Some(CallResult {
                            ok: true,
                            text: "created".into(),
                        }),
                        state: CallState::Done,
                    },
                ),
                item(
                    "c2",
                    Body::Action {
                        tool: "Bash".into(),
                        title: "Ran cargo test".into(),
                        detail: Some("cargo test".into()),
                        result: None,
                        state: CallState::Running,
                    },
                ),
                item(
                    "c3",
                    Body::Action {
                        tool: "Bash".into(),
                        title: "Ran make".into(),
                        detail: None,
                        result: Some(CallResult {
                            ok: false,
                            text: "make: no rule".into(),
                        }),
                        state: CallState::Error,
                    },
                ),
                item(
                    "q1",
                    Body::Question {
                        tool_use_id: "toolu_old".into(),
                        questions: vec![question("Which colour?", false)],
                        answer: Some(vec![Answer {
                            selected: vec!["Red".into()],
                            other: None,
                        }]),
                    },
                ),
                item(
                    "n1",
                    Body::Notice {
                        text: "Conversation compacted".into(),
                    },
                ),
            ],
            None,
        ),
    );
    let wanted = [
        "Plan the search box",
        "Hangar",
        "Created task “Search box”",
        "Used board_create_task",
        "Ran cargo test",
        "Running",
        "Ran make",
        "Failed",
        "Asked: Which colour?",
        "→ Red",
        "Conversation compacted",
        "Show all",
        "Leo",
        "Enter to send, Shift+Enter for a new line",
    ];
    let expected: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|words| *words != "Used board_create_task")
        .collect();
    assert_eq!(shown_texts(&st, &m, &wanted), expected);
    // Claude's words are Markdown, parsed once and drawn as rich text.
    let chat = st.chat(WS).unwrap();
    let (source, content) = &chat.prose["a1"];
    assert!(source.starts_with("## The plan"));
    assert!(matches!(
        content.items(),
        [
            markdown::Item::Heading(..),
            markdown::Item::Paragraph(..),
            markdown::Item::CodeBlock { .. }
        ]
    ));

    // A call's detail opens on a press, its raw input pretty-printed.
    let asked = clicks(&st, &m, &["Created task “Search box”"]);
    assert!(matches!(asked.as_slice(), [Msg::Toggle(_, id)] if id == "c1"));
    apply(&mut st, &mut m, asked);
    assert_eq!(
        shown_texts(&st, &m, &["Result", "created"]),
        ["Result", "created"]
    );

    // Hangar's long message shows whole on « Show all ».
    let asked = clicks(&st, &m, &["Show all"]);
    apply(&mut st, &mut m, asked);
    assert_eq!(shown_texts(&st, &m, &["Show less"]), ["Show less"]);
}

#[test]
fn a_long_action_title_is_drawn_whole_and_wraps() {
    let long = "A very long task title that would have been cut in the middle of a word by the old single line";
    let mut m = shop(true);
    let mut st = State::default();
    read_into(
        &mut st,
        &mut m,
        page(
            vec![item(
                "c1",
                Body::Action {
                    tool: "mcp__hangar__board_create_task".into(),
                    title: "Used board_create_task".into(),
                    detail: Some(json!({ "title": long }).to_string()),
                    result: None,
                    state: CallState::Done,
                },
            )],
            None,
        ),
    );
    let whole = format!("Created task “{long}”");
    assert_eq!(shown_texts(&st, &m, &[whole.as_str()]), [whole.as_str()]);
}

#[test]
fn a_code_block_gets_its_colours_off_the_ui_thread() {
    let mut m = shop(true);
    let mut st = State::default();
    let source = "```rust\nfn main() {}\n```";
    read_into(
        &mut st,
        &mut m,
        page(
            vec![item(
                "a1",
                Body::Assistant {
                    text: source.into(),
                },
            )],
            None,
        ),
    );
    let content = rich::parse(source);
    let blocks = rich::code_blocks(&content);
    assert_eq!(blocks.len(), 1);
    let found = highlight_blocks(blocks);
    assert_eq!(found.len(), 1, "Rust is a language the highlighter knows");
    apply(&mut st, &mut m, vec![Msg::Coloured(WS.into(), found)]);
    assert_eq!(st.chat(WS).unwrap().code.len(), 1);
    // Drawn with its colours.
    let _ = shown_texts(&st, &m, &[]);
}

#[test]
fn a_reason_says_why_there_is_no_conversation() {
    let mut m = shop(true);
    let mut st = State::default();
    let mut empty = page(vec![], None);
    empty.reason = Some(ChatReason::NoSession);
    read_into(&mut st, &mut m, empty);
    assert_eq!(
        shown_texts(
            &st,
            &m,
            &["No conversation with Leo yet. Write below to start one."]
        )
        .len(),
        1
    );
}

/* ── questions ─────────────────────────────────────────────────── */

fn question(asked: &str, multi: bool) -> Question {
    Question {
        question: asked.into(),
        header: "Colour".into(),
        multi_select: multi,
        options: vec![
            QuestionOption {
                label: "Red".into(),
                description: "Warm".into(),
            },
            QuestionOption {
                label: "Blue".into(),
                description: "Cold".into(),
            },
        ],
    }
}

#[test]
fn a_question_card_takes_the_choices_and_answers_through_the_desk() {
    let mut m = shop(true);
    let mut st = State::default();
    let questions = vec![
        question("Which colour?", false),
        Question {
            question: "Which sizes?".into(),
            header: "Sizes".into(),
            multi_select: true,
            options: vec![
                QuestionOption {
                    label: "Small".into(),
                    description: String::new(),
                },
                QuestionOption {
                    label: "Large".into(),
                    description: String::new(),
                },
            ],
        },
    ];
    read_into(
        &mut st,
        &mut m,
        page(
            vec![
                person("u1", "Make a shirt"),
                item(
                    "q1",
                    Body::Question {
                        tool_use_id: "toolu_q".into(),
                        questions: questions.clone(),
                        answer: None,
                    },
                ),
            ],
            Some(ChatPending::Question {
                tool_use_id: "toolu_q".into(),
                questions: questions.clone(),
            }),
        ),
    );
    assert_eq!(
        shown_texts(
            &st,
            &m,
            &[
                "Leo asks",
                "Which colour?",
                "Which sizes?",
                "Pick any that apply",
                "Answer the question above first."
            ]
        )
        .len(),
        5
    );
    assert!(
        shown_texts(&st, &m, &["Asked: Which colour?"]).is_empty(),
        "the question waiting is the card, not an item"
    );

    // Nothing to send before every question has an answer.
    let asked = clicks(&st, &m, &["Send the answer"]);
    assert!(asked.is_empty(), "disabled: {asked:?}");

    let asked = clicks(&st, &m, &["Blue", "Small", "Large"]);
    apply(&mut st, &mut m, asked);
    // "Other" on the multi-select question, with the person's words.
    let pkg = Packages::default();
    let other = {
        let ctx = ctx(&m, &pkg);
        let ws = m.workspace(WS).unwrap();
        let mut ui = simulate(view(&st, ctx, ws));
        // The second "Other": the first question's is above it.
        ui.click("Other").expect("an Other choice");
        ours(ui.into_messages())
    };
    assert!(matches!(other.as_slice(), [Msg::Other { question: 0, .. }]));
    apply(&mut st, &mut m, other);
    apply(
        &mut st,
        &mut m,
        vec![Msg::OtherText {
            ws: WS.into(),
            question: 0,
            text: " Teal ".into(),
        }],
    );
    // "Other" on a single-select question replaces its option.
    let asked = clicks(&st, &m, &["Send the answer"]);
    apply(&mut st, &mut m, asked);
    assert_eq!(
        calls(&st).last(),
        Some(&Call::Answer {
            tool_use_id: "toolu_q".into(),
            answers: vec![
                Answer {
                    selected: vec![],
                    other: Some("Teal".into()),
                },
                Answer {
                    selected: vec!["Small".into(), "Large".into()],
                    other: None,
                },
            ],
        })
    );
    assert_eq!(
        shown_texts(&st, &m, &["Typing your answer…"]),
        ["Typing your answer…"]
    );
    assert_eq!(
        blocked(st.chat(WS).unwrap(), &m.workspace(WS).unwrap().panes[0]),
        Some(Blocked::Answering)
    );

    // Answered: the chat reads again, and the card goes with the question.
    let before = calls(&st).len();
    apply(
        &mut st,
        &mut m,
        vec![Msg::Decided {
            ws: WS.into(),
            result: Ok(RemoteAnswered {
                answered: true,
                queued: None,
            }),
        }],
    );
    assert_eq!(calls(&st)[before..], [Call::Read { before: None }]);
}

/* ── permissions ───────────────────────────────────────────────── */

fn asking(prompt: &str) -> ChatPending {
    ChatPending::Permission {
        tool: "mcp__hangar__pane_send".into(),
        title: "Used pane_send".into(),
        detail: Some(json!({"pane": "Ava", "text": "go"}).to_string()),
        prompt: prompt.into(),
    }
}

#[test]
fn a_permission_card_allows_or_denies_the_prompt_it_shows() {
    let mut m = shop(true);
    let mut st = State::default();
    read_into(&mut st, &mut m, page(vec![], Some(asking("p1"))));
    assert_eq!(
        shown_texts(
            &st,
            &m,
            &[
                "Leo asks to use pane_send",
                "Sent a message to Ava",
                "Allow",
                "Deny",
                "Allow or deny the request above first."
            ]
        )
        .len(),
        5
    );
    let asked = clicks(&st, &m, &["Allow"]);
    apply(&mut st, &mut m, asked);
    assert_eq!(
        calls(&st).last(),
        Some(&Call::Permit {
            allow: true,
            prompt: Some("p1".into())
        })
    );

    // The prompt changed before the click: refused, said, and read again.
    apply(
        &mut st,
        &mut m,
        vec![Msg::Decided {
            ws: WS.into(),
            result: Err("The permission prompt changed since the phone showed it.".into()),
        }],
    );
    assert_eq!(calls(&st).last(), Some(&Call::Read { before: None }));
    assert_eq!(
        shown_texts(
            &st,
            &m,
            &["Not done: The permission prompt changed since the phone showed it."]
        )
        .len(),
        1
    );
    // The new prompt in, denied: it is the one sent.
    read_into(&mut st, &mut m, page(vec![], Some(asking("p2"))));
    let asked = clicks(&st, &m, &["Deny"]);
    apply(&mut st, &mut m, asked);
    assert_eq!(
        calls(&st).last(),
        Some(&Call::Permit {
            allow: false,
            prompt: Some("p2".into())
        })
    );
}

/* ── input ─────────────────────────────────────────────────────── */

#[test]
fn the_input_says_why_it_is_disabled() {
    let mut m = shop(true);
    let mut st = State::default();
    read_into(&mut st, &mut m, page(vec![], None));
    let leo = m.workspace(WS).unwrap().panes[0].clone();
    assert_eq!(blocked(st.chat(WS).unwrap(), &leo), None);

    let cases = [
        (
            DeskState::default(),
            Blocked::Loading,
            "Hangar is starting: wait a moment.",
        ),
        (
            DeskState {
                ready: true,
                answering: true,
                resuming: false,
            },
            Blocked::Answering,
            "An answer is being typed into Leo: wait for it.",
        ),
        (
            DeskState {
                ready: true,
                answering: false,
                resuming: true,
            },
            Blocked::Resuming,
            "Leo is resuming: wait for Claude Code to start.",
        ),
    ];
    for (desk, why, words) in cases {
        st.chat_mut(WS, LEO).desk = desk;
        assert_eq!(blocked(st.chat(WS).unwrap(), &leo), Some(why));
        assert_eq!(shown_texts(&st, &m, &[words]), [words]);
        // Nothing is sent while it is.
        st.chat_mut(WS, LEO).draft = text_editor::Content::with_text("hello");
        let before = calls(&st).len();
        apply(&mut st, &mut m, vec![Msg::Send(WS.into())]);
        assert_eq!(calls(&st).len(), before, "{why:?}");
    }

    let shell = Pane {
        agent: AgentId::Shell,
        ..leo.clone()
    };
    assert_eq!(
        blocked(st.chat(WS).unwrap(), &shell),
        Some(Blocked::NotClaude)
    );
}

#[test]
fn enter_sends_shift_enter_breaks_the_line() {
    let mut m = shop(true);
    let mut st = State::default();
    read_into(&mut st, &mut m, page(vec![], None));
    let pkg = Packages::default();
    let ctx = ctx(&m, &pkg);
    let ws = m.workspace(WS).unwrap();
    let mut ui = simulate(view(&st, ctx, ws));
    ui.click(view::input_id()).expect("the input");
    let _ = ui.typewrite("hi");
    let _ = ui.tap_key(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Enter,
    ));
    let asked = ours(ui.into_messages());
    assert!(
        asked.iter().any(|m| matches!(m, Msg::Send(ws) if ws == WS)),
        "{asked:?}"
    );
    assert!(asked.iter().any(|m| matches!(m, Msg::Edit(..))));
}

#[test]
fn a_message_sent_shows_at_once_and_is_reconciled_with_the_transcript() {
    let mut m = shop(true);
    let mut st = State::default();
    read_into(&mut st, &mut m, page(vec![person("u0", "Hello")], None));
    st.chat_mut(WS, LEO).draft = text_editor::Content::with_text("Ship it\n");
    apply(&mut st, &mut m, vec![Msg::Send(WS.into())]);
    assert_eq!(
        calls(&st).last(),
        Some(&Call::Send {
            text: "Ship it".into()
        })
    );
    assert_eq!(st.chat(WS).unwrap().draft.text().trim(), "", "emptied");
    assert_eq!(
        shown_texts(&st, &m, &["Ship it", "Sending…"]),
        ["Ship it", "Sending…"]
    );

    let at = st.chat(WS).unwrap().thread.outgoing[0].at;
    apply(
        &mut st,
        &mut m,
        vec![Msg::Sent {
            ws: WS.into(),
            at,
            result: Ok(RemoteSent::QUEUED),
        }],
    );
    assert_eq!(calls(&st).last(), Some(&Call::Read { before: None }));
    assert_eq!(
        shown_texts(&st, &m, &["Queued: Leo gets it once Claude Code is up"]).len(),
        1
    );
    // The transcript has it: shown once, as the transcript's.
    let mut said = person("u1", "Ship it");
    said.at = at;
    read_into(
        &mut st,
        &mut m,
        page(vec![person("u0", "Hello"), said], None),
    );
    assert!(st.chat(WS).unwrap().thread.outgoing.is_empty());
    assert!(shown_texts(&st, &m, &["Queued: Leo gets it once Claude Code is up"]).is_empty());

    // A refusal is said under the message.
    st.chat_mut(WS, LEO).draft = text_editor::Content::with_text("again");
    apply(&mut st, &mut m, vec![Msg::Send(WS.into())]);
    let at = st.chat(WS).unwrap().thread.outgoing[0].at;
    apply(
        &mut st,
        &mut m,
        vec![Msg::Sent {
            ws: WS.into(),
            at,
            result: Err("Leo is not up.".into()),
        }],
    );
    assert_eq!(shown_texts(&st, &m, &["Not sent: Leo is not up."]).len(), 1);
}

/* ── reads ─────────────────────────────────────────────────────── */

#[test]
fn older_messages_load_from_the_cursor() {
    let mut m = shop(true);
    let mut st = State::default();
    let mut newest = page(vec![person("u5", "five")], None);
    newest.older = true;
    newest.cursor = Some("c5".into());
    read_into(&mut st, &mut m, newest);
    let asked = clicks(&st, &m, &["Show earlier messages"]);
    apply(&mut st, &mut m, asked);
    assert_eq!(
        calls(&st).last(),
        Some(&Call::Read {
            before: Some("c5".into())
        })
    );
    assert_eq!(
        shown_texts(&st, &m, &["Loading earlier messages…"]).len(),
        1
    );
    // Asked once while it is out.
    let before = calls(&st).len();
    apply(&mut st, &mut m, vec![Msg::LoadOlder(WS.into())]);
    assert_eq!(calls(&st).len(), before);

    let _ = handle(
        &mut st,
        &mut m,
        Msg::Read {
            ws: WS.into(),
            pane: LEO.into(),
            older: true,
            result: Ok(Box::new(page(vec![person("u4", "four")], None))),
        },
    );
    let ids: Vec<&str> = st
        .chat(WS)
        .unwrap()
        .thread
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(ids, ["u4", "u5"]);
    assert!(shown_texts(&st, &m, &["Show earlier messages"]).is_empty());
}

#[test]
fn a_report_of_the_orchestrator_s_mod_reads_again_one_read_at_a_time() {
    let mut m = shop(true);
    let mut st = State::default();
    let report = |pane: &str| CoreEvent::Bridge {
        pane: pane.into(),
        event: json!({"kind": "turn"}),
    };
    assert!(
        on_event(&mut st, &mut m, &report("ava")).is_none(),
        "a worker's"
    );
    assert!(on_event(&mut st, &mut m, &report(LEO)).is_some());
    assert_eq!(calls(&st), [Call::Read { before: None }]);
    // Another while the read is out: one more read once it answers.
    assert!(on_event(&mut st, &mut m, &report(LEO)).is_some());
    assert_eq!(calls(&st).len(), 1);
    let _ = handle(
        &mut st,
        &mut m,
        Msg::Read {
            ws: WS.into(),
            pane: LEO.into(),
            older: false,
            result: Ok(Box::new(page(vec![], None))),
        },
    );
    assert_eq!(calls(&st).len(), 2, "the read asked meanwhile");

    // A workspace shown as its grid hears nothing.
    m.apply(StoreAction::SetChatView {
        ws: WS.into(),
        on: false,
    });
    let _ = handle(
        &mut st,
        &mut m,
        Msg::Read {
            ws: WS.into(),
            pane: LEO.into(),
            older: false,
            result: Ok(Box::new(page(vec![], None))),
        },
    );
    assert!(on_event(&mut st, &mut m, &report(LEO)).is_none());
}

#[test]
fn a_read_refused_is_said_and_what_was_held_stays() {
    let mut m = shop(true);
    let mut st = State::default();
    read_into(&mut st, &mut m, page(vec![person("u1", "kept")], None));
    st.chat_mut(WS, LEO).reading = true;
    let _ = handle(
        &mut st,
        &mut m,
        Msg::Read {
            ws: WS.into(),
            pane: LEO.into(),
            older: false,
            result: Err("No pane \"leo\" in \"Shop\".".into()),
        },
    );
    assert_eq!(
        shown_texts(
            &st,
            &m,
            &[
                "kept",
                "Could not read the conversation: No pane \"leo\" in \"Shop\"."
            ]
        )
        .len(),
        2
    );
}
