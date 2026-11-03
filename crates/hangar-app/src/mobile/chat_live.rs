//! A Claude Code pane's conversation for the phone (`remote_chat`), and
//! what its hangar-bridge mod reports that the transcript cannot say yet
//! (`noteBridgeEvent`, `liveQuestion`, `pendingOf`, `runningOf` of
//! lib/remoteControl.ts): Claude Code writes a call to its transcript only
//! once it has its result, so neither a call running nor the question or the
//! permission the pane waits on is in it.

use std::collections::HashMap;

use serde_json::Value;

use hangar_core::chat::{Body, CallState, ChatItem, ChatPage, PendingQuestion};

use crate::control::port::{Port, activity_of};
use crate::state::live::{BridgeEvent, RateLimit, ToolPhase};
use crate::state::{AgentActivity, AgentId, PaneId};

use super::calls::{asked_questions, describe_call};
use super::contract::{ChatPending, ChatReason, RemoteChat};
use super::engine::{ChatRead, Mobile};
use super::ops::find_pane;

/// A call a pane's mod reported started and not yet ended.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveCall {
    pub tool_use_id: String,
    pub tool: String,
    pub input: Value,
    pub at: u64,
}

/// What one pane's mod reported (`Live`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneLive {
    /// Oldest first.
    pub calls: Vec<LiveCall>,
    /// The permission dialog open, with the input of the call it asks about.
    pub permission: Option<(String, Value)>,
}

/// The plan's rate-limit windows, from whichever session reported last, as
/// the navigator shows them: one account behind every pane (`planLimits`).
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub limits: Vec<RateLimit>,
    pub at: u64,
}

#[derive(Debug, Default)]
pub struct ChatLive {
    panes: HashMap<PaneId, PaneLive>,
    pub plan: Option<Plan>,
}

impl ChatLive {
    /// Keeps what a pane's mod reports of its calls and dialogs, and of the
    /// plan.
    pub fn note(&mut self, pane: &str, event: &BridgeEvent, at: u64) {
        match event {
            BridgeEvent::Usage { rate_limits, .. } => {
                // A session without such limits (an API key) says nothing of
                // the plan.
                if !rate_limits.is_empty() {
                    self.plan = Some(Plan {
                        limits: rate_limits.clone(),
                        at,
                    });
                }
            }
            // A turn that starts or ends, a conversation that starts over:
            // nothing from before still runs or waits.
            BridgeEvent::Hello { .. }
            | BridgeEvent::Session { .. }
            | BridgeEvent::Turn(_)
            | BridgeEvent::End { .. } => {
                self.panes.remove(pane);
            }
            BridgeEvent::Permission {
                waiting,
                tool,
                input,
            } => {
                let live = self.panes.entry(pane.to_string()).or_default();
                live.permission = match tool {
                    Some(tool) if *waiting && !tool.is_empty() => {
                        Some((tool.clone(), input.clone().unwrap_or(Value::Null)))
                    }
                    _ => None,
                };
                self.prune(pane);
            }
            BridgeEvent::Tool(phase) => {
                let live = self.panes.entry(pane.to_string()).or_default();
                let id = match phase {
                    ToolPhase::Start { tool_use_id, .. } | ToolPhase::End { tool_use_id } => {
                        tool_use_id
                    }
                };
                live.calls.retain(|call| call.tool_use_id != *id);
                if let ToolPhase::Start {
                    tool_use_id,
                    tool,
                    input,
                } = phase
                {
                    live.calls.push(LiveCall {
                        tool_use_id: tool_use_id.clone(),
                        tool: tool.clone(),
                        input: if input.is_null() {
                            Value::Object(Default::default())
                        } else {
                            input.clone()
                        },
                        at,
                    });
                }
                self.prune(pane);
            }
            BridgeEvent::Edit { .. } | BridgeEvent::Dropped { .. } => {}
        }
    }

    fn prune(&mut self, pane: &str) {
        if self
            .panes
            .get(pane)
            .is_some_and(|live| live.calls.is_empty() && live.permission.is_none())
        {
            self.panes.remove(pane);
        }
    }

    /// The question a pane's AskUserQuestion call puts to the person, while
    /// it runs (`liveQuestion`).
    pub fn question(&self, pane: &str) -> Option<PendingQuestion> {
        let calls = &self.panes.get(pane)?.calls;
        calls.iter().rev().find_map(|call| {
            if call.tool != "AskUserQuestion" {
                return None;
            }
            let questions = asked_questions(&call.input);
            (!questions.is_empty()).then(|| PendingQuestion {
                tool_use_id: call.tool_use_id.clone(),
                questions,
            })
        })
    }

    /// What the pane waits on the person for: what its mod reports first,
    /// then what the transcript says — a question without its answer, or the
    /// last call without a result while the pane waits (`pendingOf`).
    pub fn pending_of(
        &self,
        pane: &str,
        page: &ChatPage,
        activity: Option<AgentActivity>,
        cwd: &str,
    ) -> Option<ChatPending> {
        if let Some(question) = self.question(pane).or_else(|| page.question.clone()) {
            return Some(ChatPending::Question {
                tool_use_id: question.tool_use_id,
                questions: question.questions,
            });
        }
        if activity != Some(AgentActivity::Waiting) {
            return None;
        }
        if let Some((tool, input)) = self
            .panes
            .get(pane)
            .and_then(|live| live.permission.as_ref())
        {
            let described = describe_call(tool, input, Some(cwd));
            return Some(ChatPending::Permission {
                tool: tool.clone(),
                title: described.ask,
                detail: described.detail,
            });
        }
        page.open_call.as_ref().map(|call| ChatPending::Permission {
            tool: call.tool.clone(),
            title: call.title.clone(),
            detail: call.detail.clone(),
        })
    }

    /// The calls the pane runs now, as actions after the transcript's: not a
    /// question (the pending card shows it), nor the call a permission
    /// dialog holds, which has not run yet (`runningOf`).
    pub fn running_of(
        &self,
        pane: &str,
        page: &ChatPage,
        pending: Option<&ChatPending>,
        cwd: &str,
    ) -> Vec<ChatItem> {
        let Some(live) = self.panes.get(pane) else {
            return Vec::new();
        };
        let mut calls: Vec<&LiveCall> = live
            .calls
            .iter()
            .filter(|call| {
                call.tool != "AskUserQuestion"
                    && !page.items.iter().any(|item| item.id == call.tool_use_id)
            })
            .collect();
        if let Some(ChatPending::Permission { tool, .. }) = pending
            && let Some(at) = calls.iter().rposition(|call| call.tool == *tool)
        {
            calls.remove(at);
        }
        calls
            .into_iter()
            .map(|call| {
                let described = describe_call(&call.tool, &call.input, Some(cwd));
                ChatItem {
                    id: call.tool_use_id.clone(),
                    at: call.at as i64,
                    body: Body::Action {
                        tool: call.tool.clone(),
                        title: described.title,
                        detail: described.detail,
                        result: None,
                        state: CallState::Running,
                    },
                }
            })
            .collect()
    }
}

/// `remote_chat {pane, before?, limit?}`: a page of a Claude Code pane's
/// conversation, the newest unless `before` asks for an older one.
pub async fn remote_chat(port: &Port, mobile: &Mobile, args: &Value) -> Result<RemoteChat, String> {
    let snapshot = port.snapshot();
    let (ws, pane) = find_pane(&snapshot.state, args.get("pane"))?;
    let before = match args.get("before") {
        None => None,
        Some(Value::String(cursor)) => Some(cursor.clone()),
        Some(_) => return Err("\"before\" is a cursor from an earlier answer.".into()),
    };
    let activity = activity_of(&snapshot, &pane.id).map(|entry| entry.activity);
    let chat = |items, older, cursor, pending, reason| RemoteChat {
        pane_id: pane.id.clone(),
        pane: pane.name.clone(),
        workspace: ws.name.clone(),
        agent: pane.agent.as_str().into(),
        activity,
        session_id: pane.session_id.clone(),
        items,
        older,
        cursor,
        pending,
        reason,
    };
    let empty = |reason| chat(Vec::new(), false, None, None, Some(reason));
    if pane.agent != AgentId::Claude {
        return Ok(empty(ChatReason::NotClaude));
    }
    let Some(session_id) = pane.session_id.clone().filter(|id| !id.is_empty()) else {
        return Ok(empty(ChatReason::NoSession));
    };
    let cwd = pane
        .cwd
        .clone()
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or_else(|| ws.cwd.clone());
    let limit = args
        .get("limit")
        .and_then(Value::as_f64)
        .map(|limit| limit.max(0.0).floor() as usize);
    let page = mobile
        .sources()
        .chat(ChatRead {
            session_id,
            cwd: cwd.clone(),
            before: before.clone(),
            limit,
        })
        .await?;
    let Some(page) = page else {
        return Ok(empty(ChatReason::NoTranscript));
    };
    let book = mobile.book();
    let pending = book.chat.pending_of(&pane.id, &page, activity, &cwd);
    // What runs now comes last: on the newest page alone.
    let mut items = page.items.clone();
    if before.is_none() {
        items.extend(
            book.chat
                .running_of(&pane.id, &page, pending.as_ref(), &cwd),
        );
    }
    Ok(chat(items, page.older, page.cursor.clone(), pending, None))
}
