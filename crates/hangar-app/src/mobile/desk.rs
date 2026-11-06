//! The window's own way into what the phone does to a Claude Code pane
//! (2.2's chat view): send it text, answer its questions, allow or deny its
//! permission prompts, read its conversation. The same code as the
//! `remote_*` operations, with their guards — nothing typed while an answer
//! is (MOB-18), a pane not up yet held in the queue, a permission typed only
//! on the prompt shown (MOB-02) — called with ids and typed values instead
//! of the phone's JSON.
//!
//! Every call takes the port (`control::State::port`) and the runtime's
//! `Mobile` (`mobile::State::mobile`), cheap to clone into a
//! `Task::future`. Refusals are the phone's sentences.
//!
//! A pane of a remote workspace (or a worker sent to a node) has its
//! transcript on its node: its pages are read there (`nodes::remote::chat`,
//! `transcript_page`), and so are those the guards read — the question an
//! answer is checked against, the permission prompt a click must still
//! match (`pending_now`). What is typed goes to the pane as for a local one:
//! the port writes a remote pane's input over its node's stream, and reads
//! its screen from the terminal that stream feeds here.

// The chat view (chat_view/) is the window's only caller of these.
#![allow(dead_code)]

use serde_json::Value;

use hangar_core::chat::{
    Answer, Body, CallResult, CallState, ChatItem, ChatPage, OpenCall, PendingQuestion, Question,
    QuestionOption, Sender,
};

use crate::control::port::{Port, activity_of};
use crate::nodes::{away, remote};
use crate::state::{AgentActivity, AgentId, Pane, Workspace};

use super::answer::{answer_pane, choices_from};
use super::chat_live::{self, ChatLive, chat_pane};
use super::contract::{ChatPending, ChatReason, RemoteAnswered, RemoteChat, RemoteSent};
use super::engine::{ChatRead, Mobile};
use super::ops::{Input, permit_pane, type_into};

/// A page of a pane's conversation, with what it waits on the person for
/// (`ChatPending`) and the calls it runs now: what `remote_chat` answers.
pub type ChatView = RemoteChat;

/// The pane `pane` of workspace `ws`, as the window holds it now.
fn find(port: &Port, ws: &str, pane: &str) -> Result<(Workspace, Pane), String> {
    let snapshot = port.snapshot();
    let Some(workspace) = snapshot.state.workspaces.iter().find(|w| w.id == ws) else {
        return Err(format!("No workspace with id \"{ws}\" in Hangar."));
    };
    let Some(found) = workspace.panes.iter().find(|p| p.id == pane) else {
        return Err(format!("No pane \"{pane}\" in \"{}\".", workspace.name));
    };
    Ok((workspace.clone(), found.clone()))
}

/// Types `text` into the pane, then Enter, as the person: refused while an
/// answer is typed into it, held in the queue while its Claude Code is not
/// up (`queued`), typed once its shell runs otherwise (`remote_send`).
pub async fn send_text(
    port: &Port,
    mobile: &Mobile,
    ws: &str,
    pane: &str,
    text: &str,
) -> Result<RemoteSent, String> {
    if text.trim().is_empty() {
        return Err("Nothing to send: give text.".into());
    }
    let (ws, pane) = find(port, ws, pane)?;
    let input = Input {
        text: Some(text.to_string()),
        ..Input::default()
    };
    type_into(port, mobile, &ws, &pane, input).await
}

/// Answers the question `tool_use_id` the pane waits on, one answer per
/// question — option labels and the person's own words — typed into its
/// dialog step by step, each checked on the screen (`remote_answer`).
pub async fn answer(
    port: &Port,
    mobile: &Mobile,
    ws: &str,
    pane: &str,
    tool_use_id: &str,
    answers: &[Answer],
) -> Result<RemoteAnswered, String> {
    let (ws, pane) = find(port, ws, pane)?;
    answer_pane(port, mobile, &ws, &pane, tool_use_id, |questions| {
        choices_from(questions, answers)
    })
    .await
}

/// Allows (Enter) or denies (Escape) the permission the pane waits on,
/// unless what it asks is no longer `prompt`, the `ChatPending::Permission`
/// prompt the chat showed (`None` when it showed none) (`remote_permission`).
pub async fn permit(
    port: &Port,
    mobile: &Mobile,
    ws: &str,
    pane: &str,
    allow: bool,
    prompt: Option<&str>,
) -> Result<RemoteAnswered, String> {
    let (ws, pane) = find(port, ws, pane)?;
    permit_pane(port, mobile, &ws, &pane, allow, prompt).await
}

/// A page of the pane's conversation, the newest unless `before` (an
/// earlier page's `cursor`) asks for an older one, merged with what its mod
/// reports: the question or permission it waits on, the calls it runs
/// (`remote_chat`).
pub async fn chat(
    port: &Port,
    mobile: &Mobile,
    ws: &str,
    pane: &str,
    before: Option<String>,
    limit: Option<usize>,
) -> Result<ChatView, String> {
    let (ws, pane) = find(port, ws, pane)?;
    if away::node_of(&ws, &pane).is_none() {
        return chat_pane(port, mobile, &ws, &pane, before, limit).await;
    }
    let activity = activity_of(&port.snapshot(), &pane.id).map(|entry| entry.activity);
    let newest = before.is_none();
    let page = match not_read(&pane) {
        Some(_) => None,
        None => {
            let read = ChatRead {
                session_id: pane.session_id.clone().unwrap_or_default(),
                cwd: cwd_of(&ws, &pane),
                before,
                limit,
            };
            transcript_page(mobile, &ws, &pane, read).await?
        }
    };
    Ok(view_of(
        &ws,
        &pane,
        activity,
        newest,
        page,
        &mobile.book().chat,
    ))
}

/* ── the transcript, here or on the pane's node ──────────────── */

/// Why a pane's conversation is not read at all: not Claude Code, or no
/// session yet.
fn not_read(pane: &Pane) -> Option<ChatReason> {
    if pane.agent != AgentId::Claude {
        return Some(ChatReason::NotClaude);
    }
    match &pane.session_id {
        Some(id) if !id.is_empty() => None,
        _ => Some(ChatReason::NoSession),
    }
}

/// The folder a pane's session runs in: its own, else its workspace's.
fn cwd_of(ws: &Workspace, pane: &Pane) -> String {
    pane.cwd
        .clone()
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or_else(|| ws.cwd.clone())
}

/// What `chat` answers of pane `pane` from the page read (`None`: none was,
/// or the session has no transcript yet), merged with what its mod reports
/// (`live`) as `chat_live::chat_pane` merges it: the question or permission
/// it waits on and, on the newest page alone, the calls it runs now.
fn view_of(
    ws: &Workspace,
    pane: &Pane,
    activity: Option<AgentActivity>,
    newest: bool,
    page: Option<ChatPage>,
    live: &ChatLive,
) -> ChatView {
    let mut view = ChatView {
        pane_id: pane.id.clone(),
        pane: pane.name.clone(),
        workspace: ws.name.clone(),
        agent: pane.agent.as_str().into(),
        activity,
        session_id: pane.session_id.clone(),
        items: Vec::new(),
        older: false,
        cursor: None,
        pending: None,
        reason: None,
    };
    if let Some(reason) = not_read(pane) {
        view.reason = Some(reason);
        return view;
    }
    let Some(page) = page else {
        view.reason = Some(ChatReason::NoTranscript);
        return view;
    };
    let cwd = cwd_of(ws, pane);
    let pending = live.pending_of(&pane.id, &page, activity, &cwd);
    if newest {
        let running = live.running_of(&pane.id, &page, pending.as_ref(), &cwd);
        view.items = page.items;
        view.items.extend(running);
    } else {
        view.items = page.items;
    }
    view.older = page.older;
    view.cursor = page.cursor;
    view.pending = pending;
    view
}

/// A page of pane `pane`'s transcript: on its node for a pane of a remote
/// workspace or a worker sent to one, from this machine's disk otherwise.
/// `None` while the session has none.
pub async fn transcript_page(
    mobile: &Mobile,
    ws: &Workspace,
    pane: &Pane,
    read: ChatRead,
) -> Result<Option<ChatPage>, String> {
    match away::node_of(ws, pane) {
        Some(node) => node_page(&node, &read).await,
        None => mobile.sources().chat(read).await,
    }
}

/// `read` asked of node `node` (`GET /node/chat`).
async fn node_page(node: &str, read: &ChatRead) -> Result<Option<ChatPage>, String> {
    let client = remote::client_of(node)?;
    let page = remote::chat(
        &client,
        &read.session_id,
        &read.cwd,
        read.before.as_deref(),
        read.limit,
    )
    .await?;
    page.map(|page| page_of(&page)).transpose()
}

/// What a Claude Code pane waits on the person for now (`chat_live`'s
/// `pending_now`), its transcript read on its node when it has one: what
/// the guards check a click against.
pub async fn pending_now(
    port: &Port,
    mobile: &Mobile,
    ws: &Workspace,
    pane: &Pane,
) -> Result<Option<ChatPending>, String> {
    let Some(node) = away::node_of(ws, pane) else {
        return chat_live::pending_now(port, mobile, ws, pane).await;
    };
    if not_read(pane).is_some() {
        return Ok(None);
    }
    let cwd = cwd_of(ws, pane);
    let read = ChatRead {
        session_id: pane.session_id.clone().unwrap_or_default(),
        cwd: cwd.clone(),
        before: None,
        limit: None,
    };
    let page = node_page(&node, &read).await?.unwrap_or_else(|| ChatPage {
        items: Vec::new(),
        older: false,
        cursor: None,
        question: None,
        open_call: None,
    });
    let activity = activity_of(&port.snapshot(), &pane.id).map(|entry| entry.activity);
    Ok(mobile
        .book()
        .chat
        .pending_of(&pane.id, &page, activity, &cwd))
}

/* ── a page as the node sends it ─────────────────────────────── */

/// A page of the conversation as the node wrote it: hangar-core's
/// `ChatPage` serialised (camelCase, each item's body tagged by `kind`).
/// An item this version does not know (a newer node) is left out, as the
/// phone leaves it out; a page without its items is refused.
fn page_of(page: &Value) -> Result<ChatPage, String> {
    let items = page
        .get("items")
        .and_then(Value::as_array)
        .ok_or("The node's chat page has no items: is it this version of Hangar?")?;
    Ok(ChatPage {
        items: items.iter().filter_map(item_of).collect(),
        older: page.get("older").and_then(Value::as_bool).unwrap_or(false),
        cursor: text_of(page, "cursor"),
        question: page.get("question").and_then(pending_question_of),
        open_call: page.get("openCall").and_then(open_call_of),
    })
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn item_of(item: &Value) -> Option<ChatItem> {
    let text = || text_of(item, "text");
    let body = match item.get("kind")?.as_str()? {
        "user" => Body::User {
            text: text()?,
            from: match item.get("from").and_then(Value::as_str) {
                Some("hangar") => Sender::Hangar,
                _ => Sender::Person,
            },
            images: item
                .get("images")
                .and_then(Value::as_u64)
                .and_then(|images| u32::try_from(images).ok())
                .unwrap_or(0),
        },
        "assistant" => Body::Assistant { text: text()? },
        "action" => Body::Action {
            tool: text_of(item, "tool")?,
            title: text_of(item, "title")?,
            detail: text_of(item, "detail"),
            result: item.get("result").and_then(|result| {
                Some(CallResult {
                    ok: result.get("ok")?.as_bool()?,
                    text: text_of(result, "text").unwrap_or_default(),
                })
            }),
            state: match item.get("state")?.as_str()? {
                "running" => CallState::Running,
                "done" => CallState::Done,
                "error" => CallState::Error,
                _ => return None,
            },
        },
        "question" => Body::Question {
            tool_use_id: text_of(item, "toolUseId")?,
            questions: questions_of(item.get("questions")?)?,
            answer: match item.get("answer") {
                None | Some(Value::Null) => None,
                Some(answers) => Some(
                    answers
                        .as_array()?
                        .iter()
                        .map(answer_of)
                        .collect::<Option<_>>()?,
                ),
            },
        },
        "notice" => Body::Notice { text: text()? },
        _ => return None,
    };
    Some(ChatItem {
        id: text_of(item, "id")?,
        at: item.get("at").and_then(Value::as_i64).unwrap_or(0),
        body,
    })
}

fn answer_of(answer: &Value) -> Option<Answer> {
    Some(Answer {
        selected: answer
            .get("selected")?
            .as_array()?
            .iter()
            .map(|label| label.as_str().map(str::to_owned))
            .collect::<Option<_>>()?,
        other: text_of(answer, "other"),
    })
}

fn questions_of(questions: &Value) -> Option<Vec<Question>> {
    questions
        .as_array()?
        .iter()
        .map(|question| {
            Some(Question {
                question: text_of(question, "question")?,
                header: text_of(question, "header").unwrap_or_default(),
                multi_select: question
                    .get("multiSelect")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                options: question
                    .get("options")?
                    .as_array()?
                    .iter()
                    .map(|option| {
                        Some(QuestionOption {
                            label: text_of(option, "label")?,
                            description: text_of(option, "description").unwrap_or_default(),
                        })
                    })
                    .collect::<Option<_>>()?,
            })
        })
        .collect()
}

fn pending_question_of(question: &Value) -> Option<PendingQuestion> {
    Some(PendingQuestion {
        tool_use_id: text_of(question, "toolUseId")?,
        questions: questions_of(question.get("questions")?)?,
    })
}

fn open_call_of(call: &Value) -> Option<OpenCall> {
    Some(OpenCall {
        tool: text_of(call, "tool")?,
        title: text_of(call, "title")?,
        detail: text_of(call, "detail"),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use hangar_core::git::{ChangedFile, Changes, FileDiff};
    use hangar_core::remote::ActivityEntry;
    use hangar_core::stats::UsageStats;

    use super::*;
    use crate::control::port::BoxFuture;
    use crate::mobile::engine::Sources;
    use crate::navigator::testing::{pane, workspace};
    use crate::nodes::testing::{answer, listen, read_request};
    use crate::orchestration::manager_feed::ManagerJournalEntry;
    use crate::orchestration::projects::ProjectSummary;
    use crate::state::live::{BridgeEvent, ToolPhase};

    /// This machine's transcripts: each read kept, none ever found.
    #[derive(Default)]
    struct Disk {
        reads: Mutex<Vec<ChatRead>>,
    }

    fn none<T: Send + 'static>(value: T) -> BoxFuture<T> {
        Box::pin(async move { value })
    }

    impl Sources for Disk {
        fn projects(&self) -> BoxFuture<Vec<ProjectSummary>> {
            none(Vec::new())
        }
        fn manager_journal(&self) -> Vec<ManagerJournalEntry> {
            Vec::new()
        }
        fn history(&self, _: u64) -> BoxFuture<Result<Vec<ActivityEntry>, String>> {
            none(Ok(Vec::new()))
        }
        fn chat(&self, read: ChatRead) -> BoxFuture<Result<Option<ChatPage>, String>> {
            self.reads.lock().unwrap().push(read);
            none(Ok(None))
        }
        fn usage(&self) -> BoxFuture<Result<Arc<UsageStats>, String>> {
            none(Err("no statistics here".into()))
        }
        fn worktrees(&self, _: &str) -> BoxFuture<Result<Vec<String>, String>> {
            none(Ok(Vec::new()))
        }
        fn stats_now(&self) -> i64 {
            0
        }
        fn changes(&self, _: &str, _: &str) -> BoxFuture<Result<Changes, String>> {
            none(Err("no git here".into()))
        }
        fn file_diff(
            &self,
            _: &str,
            _: &ChangedFile,
            _: &str,
        ) -> BoxFuture<Result<FileDiff, String>> {
            none(Err("no git here".into()))
        }
        fn wait(&self, _: u64) -> BoxFuture<()> {
            none(())
        }
        fn listening(&self, _: Vec<u16>) -> BoxFuture<Result<Vec<u16>, String>> {
            none(Ok(Vec::new()))
        }
        fn tasks(&self, _: &str) -> BoxFuture<Result<Vec<(String, String)>, String>> {
            none(Ok(Vec::new()))
        }
    }

    fn color() -> Question {
        Question {
            question: "Which color?".into(),
            header: "Color".into(),
            multi_select: false,
            options: vec![
                QuestionOption {
                    label: "Red".into(),
                    description: "Warm".into(),
                },
                QuestionOption {
                    label: "Blue".into(),
                    description: String::new(),
                },
            ],
        }
    }

    /// A page with an item of every kind, as core's chat_page writes one.
    fn full_page() -> ChatPage {
        let item = |id: &str, body| ChatItem {
            id: id.into(),
            at: 1_700,
            body,
        };
        ChatPage {
            items: vec![
                item(
                    "u1",
                    Body::User {
                        text: "fix the build".into(),
                        from: Sender::Person,
                        images: 2,
                    },
                ),
                item(
                    "h1",
                    Body::User {
                        text: "[Hangar] Briefing".into(),
                        from: Sender::Hangar,
                        images: 0,
                    },
                ),
                item(
                    "a1",
                    Body::Assistant {
                        text: "## On it\n\n```rust\nfn main() {}\n```".into(),
                    },
                ),
                item(
                    "t1",
                    Body::Action {
                        tool: "Bash".into(),
                        title: "Ran cargo build".into(),
                        detail: Some("cargo build".into()),
                        result: Some(CallResult {
                            ok: false,
                            text: "error[E0425]".into(),
                        }),
                        state: CallState::Error,
                    },
                ),
                item(
                    "t2",
                    Body::Action {
                        tool: "Read".into(),
                        title: "Read 3 files".into(),
                        detail: None,
                        result: None,
                        state: CallState::Done,
                    },
                ),
                item(
                    "q1",
                    Body::Question {
                        tool_use_id: "toolu_q1".into(),
                        questions: vec![color()],
                        answer: Some(vec![Answer {
                            selected: vec!["Red".into()],
                            other: Some("dark red".into()),
                        }]),
                    },
                ),
                item(
                    "q2",
                    Body::Question {
                        tool_use_id: "toolu_q2".into(),
                        questions: vec![color()],
                        answer: None,
                    },
                ),
                item(
                    "n1",
                    Body::Notice {
                        text: "Conversation compacted".into(),
                    },
                ),
            ],
            older: true,
            cursor: Some("4096".into()),
            question: Some(PendingQuestion {
                tool_use_id: "toolu_q2".into(),
                questions: vec![color()],
            }),
            open_call: Some(OpenCall {
                tool: "Bash".into(),
                title: "Run pnpm test".into(),
                detail: Some("pnpm test".into()),
            }),
        }
    }

    #[test]
    fn a_page_the_node_writes_reads_back_as_core_wrote_it() {
        let page = full_page();
        let written = serde_json::to_value(&page).unwrap();
        assert_eq!(page_of(&written), Ok(page));

        let bare = ChatPage {
            items: Vec::new(),
            older: false,
            cursor: None,
            question: None,
            open_call: None,
        };
        assert_eq!(
            page_of(&serde_json::to_value(&bare).unwrap()),
            Ok(bare.clone())
        );
        // An item of a newer node is left out, the rest kept.
        let newer = json!({"items": [
            {"id": "x", "at": 1, "kind": "hologram", "text": "?"},
            {"id": "n", "at": 2, "kind": "notice", "text": "Session resumed"},
        ], "older": false, "cursor": null});
        let read = page_of(&newer).unwrap();
        assert_eq!(read.items.len(), 1);
        assert_eq!(read.items[0].id, "n");
        // Not a page at all.
        assert!(page_of(&json!({"error": "nope"})).is_err());
    }

    /// Leo, the orchestrator of workspace "far" on node `node`.
    fn far(node: Option<&str>) -> (Workspace, Pane) {
        let mut leo = pane("leo", "Leo", AgentId::Claude);
        leo.session_id = Some("s1".into());
        let mut ws = workspace("far", "Far", None, vec![leo.clone()]);
        ws.cwd = "/home/hangar/my demo".into();
        ws.node = node.map(str::to_owned);
        (ws, leo)
    }

    fn read(before: Option<&str>) -> ChatRead {
        ChatRead {
            session_id: "s1".into(),
            cwd: "/home/hangar/my demo".into(),
            before: before.map(str::to_owned),
            limit: Some(30),
        }
    }

    #[tokio::test]
    async fn a_remote_pane_s_transcript_is_read_on_its_node_a_local_one_on_this_disk() {
        let (listener, client) = listen().await;
        let page = serde_json::to_string(&json!({ "page": full_page() })).unwrap();
        let node = tokio::spawn(async move {
            let mut lines = Vec::new();
            for body in [page.as_str(), r#"{"page":null}"#] {
                let (mut stream, _) = listener.accept().await.unwrap();
                lines.push(read_request(&mut stream).await.line().to_owned());
                answer(&mut stream, 200, body).await;
            }
            lines
        });
        let node_id = "desk-test-node";
        remote::publish(node_id, Some(client));
        let disk = Arc::new(Disk::default());
        let mobile = Mobile::new(disk.clone());

        let (ws, leo) = far(Some(node_id));
        let got = transcript_page(&mobile, &ws, &leo, read(None)).await;
        assert_eq!(got, Ok(Some(full_page())));
        let none = transcript_page(&mobile, &ws, &leo, read(Some("4096"))).await;
        assert_eq!(none, Ok(None));
        assert_eq!(
            node.await.unwrap(),
            [
                "GET /node/chat?session=s1&cwd=%2Fhome%2Fhangar%2Fmy%20demo&limit=30 HTTP/1.1",
                "GET /node/chat?session=s1&cwd=%2Fhome%2Fhangar%2Fmy%20demo&before=4096&limit=30 HTTP/1.1",
            ]
        );
        assert!(disk.reads.lock().unwrap().is_empty(), "this disk untouched");

        // A worker a local workspace sent to the node: its own node counts.
        let (mut ws, mut ava) = far(None);
        ava.node = Some("desk-test-gone".into());
        ws.panes = vec![ava.clone()];
        assert_eq!(
            transcript_page(&mobile, &ws, &ava, read(None)).await,
            Err("node desk-test-gone is not connected".into())
        );
        assert!(disk.reads.lock().unwrap().is_empty());

        // A local pane: this machine's disk, as before.
        let (ws, leo) = far(None);
        assert_eq!(
            transcript_page(&mobile, &ws, &leo, read(None)).await,
            Ok(None)
        );
        assert_eq!(*disk.reads.lock().unwrap(), [read(None)]);
        remote::publish(node_id, None);
    }

    #[test]
    fn a_remote_page_is_merged_with_what_its_mod_reports() {
        let (ws, leo) = far(Some("n1"));
        let mut live = ChatLive::default();
        live.note(
            "leo",
            &BridgeEvent::Tool(ToolPhase::Start {
                tool_use_id: "toolu_r".into(),
                tool: "Bash".into(),
                input: json!({"command": "pnpm build"}),
            }),
            5_000,
        );
        let mut page = full_page();
        page.question = None;
        page.open_call = None;

        // The newest page: the calls running come after the transcript's.
        let view = view_of(
            &ws,
            &leo,
            Some(AgentActivity::Working),
            true,
            Some(page.clone()),
            &live,
        );
        let ids: Vec<&str> = view.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(
            ids,
            ["u1", "h1", "a1", "t1", "t2", "q1", "q2", "n1", "toolu_r"]
        );
        assert_eq!(view.cursor.as_deref(), Some("4096"));
        assert!(view.older);
        assert_eq!(view.pending, None);
        assert_eq!(view.reason, None);
        assert_eq!(view.workspace, "Far");

        // An older page: the transcript's alone.
        let older = view_of(&ws, &leo, None, false, Some(page.clone()), &live);
        assert_eq!(older.items, page.items);

        // Waiting on the call it opened: its permission is pending.
        page.open_call = Some(OpenCall {
            tool: "Bash".into(),
            title: "Run pnpm test".into(),
            detail: None,
        });
        let waiting = view_of(
            &ws,
            &leo,
            Some(AgentActivity::Waiting),
            true,
            Some(page),
            &ChatLive::default(),
        );
        assert!(matches!(
            waiting.pending,
            Some(ChatPending::Permission { ref title, .. }) if title == "Run pnpm test"
        ));

        // No transcript yet, no session, not Claude Code.
        let none = view_of(&ws, &leo, None, true, None, &live);
        assert_eq!(none.reason, Some(ChatReason::NoTranscript));
        let mut fresh = leo.clone();
        fresh.session_id = None;
        let view = view_of(&ws, &fresh, None, true, Some(full_page()), &live);
        assert_eq!(view.reason, Some(ChatReason::NoSession));
        assert!(view.items.is_empty());
        let shell = pane("sh", "Shell", AgentId::Shell);
        let view = view_of(&ws, &shell, None, true, Some(full_page()), &live);
        assert_eq!(view.reason, Some(ChatReason::NotClaude));
    }
}
