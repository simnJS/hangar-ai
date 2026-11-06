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

// The chat view (chat_view/) calls these; nothing in the window does yet.
#![allow(dead_code)]

use hangar_core::chat::Answer;

use crate::control::port::Port;
use crate::state::{Pane, Workspace};

use super::answer::{answer_pane, choices_from};
use super::chat_live::chat_pane;
use super::contract::{RemoteAnswered, RemoteChat, RemoteSent};
use super::engine::Mobile;
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
    chat_pane(port, mobile, &ws, &pane, before, limit).await
}
