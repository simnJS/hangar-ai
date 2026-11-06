//! Runs of calls folded: the calls one after another between two messages
//! read as one quiet line ("8 commands, 5 tasks created · 2 failed"), its
//! list shown once pressed. A lone call stays the line it is; a question,
//! a notice or a message ends a run (the questions and permissions waiting
//! are the card, never folded).

use hangar_core::chat::{Body, CallState, ChatItem};
use hangar_i18n::{Key, Translator};

/// What the conversation shows, in order: an item, or a run of calls.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece<'a> {
    One(&'a ChatItem),
    Run(Vec<&'a ChatItem>),
}

impl Piece<'_> {
    /// The key a run is shown whole under: its first call's, so it stays
    /// open as more calls join it.
    pub fn key(first: &ChatItem) -> String {
        format!("run:{}", first.id)
    }
}

/// The items as shown: two calls or more in a row are a run.
pub fn pieces<'a>(items: impl IntoIterator<Item = &'a ChatItem>) -> Vec<Piece<'a>> {
    let mut pieces = Vec::new();
    let mut run: Vec<&ChatItem> = Vec::new();
    let flush = |run: &mut Vec<&'a ChatItem>, pieces: &mut Vec<Piece<'a>>| match run.len() {
        0 => {}
        1 => pieces.push(Piece::One(run.remove(0))),
        _ => pieces.push(Piece::Run(std::mem::take(run))),
    };
    for item in items {
        if matches!(item.body, Body::Action { .. }) {
            run.push(item);
        } else {
            flush(&mut run, &mut pieces);
            pieces.push(Piece::One(item));
        }
    }
    flush(&mut run, &mut pieces);
    pieces
}

/// The kinds of calls a run counts, in the order the line says them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Command,
    Edit,
    Read,
    Search,
    Task,
    Memory,
    Message,
    Other,
}

const KINDS: [Kind; 8] = [
    Kind::Command,
    Kind::Edit,
    Kind::Read,
    Kind::Search,
    Kind::Task,
    Kind::Memory,
    Kind::Message,
    Kind::Other,
];

/// The kind of a call, by its tool.
pub fn kind(tool: &str) -> Kind {
    let name = tool
        .strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
        .map_or(tool, |(_, name)| name);
    match name {
        "Bash" | "PowerShell" | "BashOutput" | "KillShell" => Kind::Command,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => Kind::Edit,
        "Read" => Kind::Read,
        "Grep" | "Glob" | "WebSearch" | "WebFetch" | "ToolSearch" | "memory_search" => Kind::Search,
        "board_create_task" => Kind::Task,
        "memory_write" | "memory_update" => Kind::Memory,
        "pane_send" | "project_send" | "SendMessage" => Kind::Message,
        _ => Kind::Other,
    }
}

/// What a run did, counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    /// By kind, in `KINDS` order, those with none left out.
    pub kinds: Vec<(Kind, usize)>,
    pub failed: usize,
    pub running: usize,
}

/// A failed call: refused, or its result an error.
pub fn failed(item: &ChatItem) -> bool {
    match &item.body {
        Body::Action { state, result, .. } => {
            *state == CallState::Error || result.as_ref().is_some_and(|result| !result.ok)
        }
        _ => false,
    }
}

pub fn tally(run: &[&ChatItem]) -> Tally {
    let mut counts = [0usize; KINDS.len()];
    let mut tally = Tally::default();
    for item in run {
        let Body::Action { tool, state, .. } = &item.body else {
            continue;
        };
        counts[kind(tool) as usize] += 1;
        if failed(item) {
            tally.failed += 1;
        } else if *state == CallState::Running {
            tally.running += 1;
        }
    }
    tally.kinds = KINDS
        .iter()
        .zip(counts)
        .filter(|(_, count)| *count > 0)
        .map(|(kind, count)| (*kind, count))
        .collect();
    tally
}

/// The line's words, without the failures and the calls running (said in
/// their own colour beside it).
pub fn summary(i18n: Translator, tally: &Tally) -> String {
    tally
        .kinds
        .iter()
        .map(|(kind, count)| {
            let key = match kind {
                Kind::Command => Key::ChatConvCallsCommands,
                Kind::Edit => Key::ChatConvCallsEdits,
                Kind::Read => Key::ChatConvCallsReads,
                Kind::Search => Key::ChatConvCallsSearches,
                Kind::Task => Key::ChatConvCallsTasks,
                Kind::Memory => Key::ChatConvCallsMemory,
                Kind::Message => Key::ChatConvCallsMessages,
                Kind::Other => Key::ChatConvCallsOther,
            };
            i18n.tn(key, *count, &[]).into_owned()
        })
        .collect::<Vec<_>>()
        .join(", ")
}
