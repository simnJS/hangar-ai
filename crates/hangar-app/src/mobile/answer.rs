//! Answering Claude Code's AskUserQuestion from the phone (`remote_answer`,
//! lib/remoteAnswer.ts): the window types into the question's dialog what
//! the person would type at the PC, so the session gets the answer exactly as
//! if it had been given there.
//!
//! The keys are those of Claude Code's own dialog (2.1.x). It shows one tab
//! per question, each opening with the cursor on its first option. Enter
//! picks a single-select option and moves on to the next tab; on a
//! multi-select question it ticks the option under the cursor, and the row
//! below the options, "Next" ("Submit" on the last), moves on. The row just
//! after the options, "Type something", takes the person's own words as they
//! are pasted, and ticks itself. A review then waits for Enter on "Submit
//! answers", except after a lone single-select question, which its Enter
//! submits.
//!
//! Every step is checked on the screen before the next: nothing is typed
//! unless the dialog shows the question as Claude Code first drew it, and
//! nothing is submitted unless the screen shows what the person chose. A
//! dialog someone is answering at the PC, a key lost on the way, a Claude
//! Code that draws its dialog otherwise: each stops the answer, never
//! submitting what the person did not choose.

use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use hangar_core::chat::{Answer, Body, ChatItem, PendingQuestion, Question};

use crate::control::port::Port;
use crate::control::run_as_person;
use crate::state::{AgentId, Pane, Workspace};

use super::contract::RemoteAnswered;
use super::desk::transcript_page;
use super::engine::{ChatRead, Mobile};
use super::ops::{find_pane, hold, resumes};

/// One write into the pane: `text` pasted, never followed by Enter, then
/// `keys`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnswerStep {
    pub text: Option<String>,
    pub keys: Vec<String>,
}

/// A question's answer, checked against its options: the indexes picked,
/// ascending, and the person's own words.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    pub picked: Vec<usize>,
    pub other: Option<String>,
}

type Io<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What the pane reads and types, through pane_read and pane_send.
pub trait AnswerIo: Send + Sync {
    /// The last lines the pane's terminal shows.
    fn read(&self) -> Io<'_, Result<String, String>>;
    fn type_step(&self, step: AnswerStep) -> Io<'_, Result<(), String>>;
    fn sleep(&self, ms: u64) -> Io<'_, ()>;
}

/// How long a step gets to show on the screen, read again every `SETTLE_MS`.
const SETTLE_MS: u64 = 150;
const SETTLE_TRIES: usize = 20;
/// The question is on the screen already when the transcript says it waits.
const FIRST_TRIES: usize = 3;

pub const NOT_ON_SCREEN: &str = "The question is not on the PC's screen as Claude Code first drew it: someone may be answering it there. Nothing was typed.";
pub const NOT_REVIEWED: &str = "The PC's screen does not show these answers ready to submit, so nothing was submitted: check them in the Terminal view.";

pub fn lost_at(index: usize) -> String {
    format!(
        "Question {} did not come up on the PC as expected, so the rest was not typed: finish in the Terminal view.",
        index + 1
    )
}

/// The questions `tool_use_id` asks, when the pane waits on it; otherwise
/// the reason it does not, for the phone to show: its screen is stale
/// (`pendingQuestions`).
pub fn pending_questions(
    pending: Option<&PendingQuestion>,
    items: &[ChatItem],
    tool_use_id: &str,
) -> Result<Vec<Question>, String> {
    if let Some(pending) = pending
        && pending.tool_use_id == tool_use_id
    {
        return Ok(pending.questions.clone());
    }
    let asked = items.iter().find_map(|item| match &item.body {
        Body::Question {
            tool_use_id: id,
            answer,
            ..
        } if id == tool_use_id => Some(answer.is_some()),
        _ => None,
    });
    Err(match (asked, pending) {
        (Some(true), _) => "This question was already answered.",
        (Some(false), _) => "Claude Code no longer waits on this question: it was dismissed.",
        (None, Some(_)) => "Claude Code now waits on another question.",
        (None, None) => "Claude Code is not waiting on this question.",
    }
    .into())
}

/// Line breaks and control characters out: in the dialog's field a newline
/// would answer.
fn clean_words(text: &str) -> String {
    static BREAKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\r\n\t]+").unwrap());
    let spaced = BREAKS.replace_all(text, " ");
    spaced
        .chars()
        .filter(|c| !matches!(*c as u32, 0x00..=0x1f | 0x7f..=0x9f))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Refused: not one answer per question.
fn count_refused(questions: &[Question]) -> String {
    if questions.len() == 1 {
        "Give one answer: this is one question.".into()
    } else {
        format!("Give one answer per question: {} of them.", questions.len())
    }
}

/// How a refusal names question `index`.
fn name_of(questions: &[Question], index: usize) -> String {
    let question = &questions[index];
    if questions.len() == 1 {
        format!("\"{}\"", question.header)
    } else {
        format!("Question {} (\"{}\")", index + 1, question.header)
    }
}

/// One answer, its labels and the person's own words, held against its
/// question.
fn choice_for(
    name: &str,
    question: &Question,
    selected: &[String],
    other: &str,
) -> Result<Choice, String> {
    let mut unique: Vec<&String> = Vec::new();
    for label in selected {
        if !unique.contains(&label) {
            unique.push(label);
        }
    }
    let mut picked = Vec::new();
    for label in unique {
        let Some(at) = question
            .options
            .iter()
            .position(|option| option.label == *label)
        else {
            return Err(format!("\"{label}\" is not one of the options of {name}."));
        };
        picked.push(at);
    }
    picked.sort_unstable();
    let other = Some(clean_words(other)).filter(|words| !words.is_empty());
    let given = picked.len() + usize::from(other.is_some());
    if given == 0 {
        return Err(format!("{name} has no answer."));
    }
    if !question.multi_select && given > 1 {
        return Err(format!(
            "{name} takes one answer: one option, or your own words."
        ));
    }
    Ok(Choice { picked, other })
}

/// The phone's answers held against the questions; what does not fit is
/// refused (`choicesOf`).
pub fn choices_of(questions: &[Question], answers: Option<&Value>) -> Result<Vec<Choice>, String> {
    let answers = match answers.and_then(Value::as_array) {
        Some(list) if list.len() == questions.len() => list,
        _ => return Err(count_refused(questions)),
    };
    questions
        .iter()
        .zip(answers)
        .enumerate()
        .map(|(index, (question, answer))| {
            let name = name_of(questions, index);
            let selected = match answer.get("selected") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(labels)) if labels.iter().all(Value::is_string) => labels
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                Some(_) => return Err(format!("{name}: \"selected\" is a list of option labels.")),
            };
            let other = match answer.get("other") {
                None => String::new(),
                Some(Value::String(words)) => words.clone(),
                Some(_) => {
                    return Err(format!(
                        "{name}: \"other\" is the person's own words, as text."
                    ));
                }
            };
            choice_for(&name, question, &selected, &other)
        })
        .collect()
}

/// The window's answers held against the questions, as `choices_of` holds
/// the phone's.
pub fn choices_from(questions: &[Question], answers: &[Answer]) -> Result<Vec<Choice>, String> {
    if answers.len() != questions.len() {
        return Err(count_refused(questions));
    }
    questions
        .iter()
        .zip(answers)
        .enumerate()
        .map(|(index, (question, answer))| {
            let other = answer.other.as_deref().unwrap_or("");
            choice_for(
                &name_of(questions, index),
                question,
                &answer.selected,
                other,
            )
        })
        .collect()
}

fn down(count: usize) -> impl Iterator<Item = String> {
    std::iter::repeat_n("down".to_string(), count)
}

/// What answering one question types, from its tab as first drawn: the
/// cursor on the first option. The last key is the Enter that moves on, or
/// submits (`stepsFor`).
pub fn steps_for(question: &Question, choice: &Choice) -> Vec<AnswerStep> {
    let type_row = question.options.len();
    let keys = |keys: Vec<String>| AnswerStep { text: None, keys };
    if !question.multi_select {
        if let Some(other) = &choice.other {
            return vec![
                keys(down(type_row).collect()),
                AnswerStep {
                    text: Some(other.clone()),
                    keys: vec!["enter".into()],
                },
            ];
        }
        let first = choice.picked.first().copied().unwrap_or(0);
        return vec![keys(down(first).chain(["enter".to_string()]).collect())];
    }
    let mut typed = Vec::new();
    let mut at = 0;
    for &index in &choice.picked {
        typed.extend(down(index - at));
        typed.push("enter".into());
        at = index;
    }
    match &choice.other {
        // "Next" is the row after "Type something".
        None => {
            typed.extend(down(type_row + 1 - at));
            typed.push("enter".into());
            vec![keys(typed)]
        }
        Some(other) => {
            typed.extend(down(type_row - at));
            vec![
                keys(typed),
                AnswerStep {
                    text: Some(other.clone()),
                    keys: vec!["down".into(), "enter".into()],
                },
            ]
        }
    }
}

/* ── Reading the dialog off the screen ────────────────────────── */

fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A numbered row, or the unnumbered one that moves on ("Next", "Submit").
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    /// `None`: the row that moves on.
    n: Option<usize>,
    cursor: bool,
    text: String,
    /// Its line in the dialog.
    at: usize,
}

static NUMBERED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([❯>])?\s*(\d+)\.\s+(.*?)\s*$").unwrap());
static MOVE_ON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([❯>])?\s+(Next|Submit)\s*$").unwrap());
static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[(.)\]\s*").unwrap());
static TYPE_SOMETHING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\[ \]\s*)?Type something").unwrap());
static LEFT_OVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^[,"]*$"#).unwrap());

fn rows_of(lines: &[&str]) -> Vec<Row> {
    let mut rows = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        if let Some(caps) = NUMBERED.captures(line) {
            rows.push(Row {
                n: caps[2].parse().ok(),
                cursor: caps.get(1).is_some(),
                text: caps[3].to_string(),
                at,
            });
        }
        if let Some(caps) = MOVE_ON.captures(line) {
            rows.push(Row {
                n: None,
                cursor: caps.get(1).is_some(),
                text: caps[2].to_string(),
                at,
            });
        }
    }
    rows
}

/// The dialog: the lines below its tab bar, the last line of the screen
/// that shows one of the questions' headers as a tab (☐ unanswered, ☒
/// answered). Claude Code draws it at the bottom, in place of the prompt
/// (`dialogOf`).
pub fn dialog_of<'a>(screen: &'a str, questions: &[Question]) -> Option<Vec<&'a str>> {
    let lines: Vec<&str> = screen.split('\n').collect();
    let at = lines.iter().rposition(|line| {
        questions.iter().any(|question| {
            line.contains(&format!("☐ {}", question.header))
                || line.contains(&format!("☒ {}", question.header))
        })
    })?;
    Some(lines[at + 1..].to_vec())
}

/// Whether the dialog shows `question` as first drawn: its options in their
/// order, nothing ticked, nothing typed, the cursor on the first
/// (`untouched`).
pub fn untouched(lines: &[&str], question: &Question) -> bool {
    let rows = rows_of(lines);
    let numbered = |n: usize| rows.iter().find(|row| row.n == Some(n));
    let Some(first) = numbered(1) else {
        return false;
    };
    if !first.cursor || rows.iter().filter(|row| row.cursor).count() != 1 {
        return false;
    }
    if !squash(&lines[..first.at].concat()).contains(&squash(&question.question)) {
        return false;
    }
    let options_shown = question.options.iter().enumerate().all(|(index, option)| {
        let Some(row) = numbered(index + 1) else {
            return false;
        };
        let boxed = BOX.captures(&row.text);
        let ticked_right = match &boxed {
            Some(caps) => question.multi_select && &caps[1] == " ",
            None => !question.multi_select,
        };
        if !ticked_right {
            return false;
        }
        let label = match &boxed {
            Some(caps) => &row.text[caps[0].len()..],
            None => row.text.as_str(),
        };
        squash(label) == squash(&option.label)
    });
    let type_row = numbered(question.options.len() + 1);
    options_shown && type_row.is_some_and(|row| TYPE_SOMETHING.is_match(&row.text))
}

/// Whether a lone single-select question has the cursor on the person's
/// answer: its Enter submits at once, with no review to check first
/// (`chosen`).
pub fn chosen(lines: &[&str], question: &Question, choice: &Choice) -> bool {
    let rows = rows_of(lines);
    let cursor: Vec<&Row> = rows.iter().filter(|row| row.cursor).collect();
    let [row] = cursor.as_slice() else {
        return false;
    };
    let Some(other) = &choice.other else {
        let index = choice.picked.first().copied().unwrap_or(0);
        return row.n == Some(index + 1)
            && question
                .options
                .get(index)
                .is_some_and(|option| squash(&row.text) == squash(&option.label));
    };
    // Words longer than the row run on below it.
    let mut run_on = vec![row.text.as_str()];
    for line in &lines[row.at + 1..] {
        if line.trim().is_empty()
            || line.contains('─')
            || NUMBERED.is_match(line)
            || MOVE_ON.is_match(line)
        {
            break;
        }
        run_on.push(line);
    }
    row.n == Some(question.options.len() + 1) && squash(&run_on.concat()) == squash(other)
}

/// Whether the review shows, for each question, the answer asked for and
/// nothing else, with the cursor on "Submit answers". Claude Code joins a
/// question's answers with ", " and may quote the person's words
/// (`reviewed`).
pub fn reviewed(lines: &[&str], questions: &[Question], choices: &[Choice]) -> bool {
    let text = lines.join("\n");
    if !squash(&text).contains("Reviewyouranswers") {
        return false;
    }
    let rows = rows_of(lines);
    let Some(submit) = rows.iter().find(|row| row.n == Some(1)) else {
        return false;
    };
    if !submit.cursor || !submit.text.starts_with("Submit answers") {
        return false;
    }
    let body = match text.find("Ready to submit") {
        Some(end) => &text[..end],
        None => text.as_str(),
    };
    let parts: Vec<String> = body.split('●').skip(1).map(squash).collect();
    questions.iter().zip(choices).all(|(question, choice)| {
        let asked = format!("{}→", squash(&question.question));
        let Some(part) = parts.iter().find(|part| part.starts_with(&asked)) else {
            return false;
        };
        let mut left = part[asked.len()..].to_string();
        let mut pieces: Vec<String> = choice
            .picked
            .iter()
            .filter_map(|&at| question.options.get(at).map(|option| option.label.clone()))
            .chain(choice.other.clone())
            .map(|piece| squash(&piece))
            .collect();
        // The longest first: a label inside another is found in what is left.
        pieces.sort_by_key(|piece| std::cmp::Reverse(piece.chars().count()));
        for piece in pieces {
            let Some(at) = left.find(&piece) else {
                return false;
            };
            left.replace_range(at..at + piece.len(), "");
        }
        LEFT_OVER.is_match(&left)
    })
}

/* ── Typing it ────────────────────────────────────────────────── */

/// Reads the screen until the dialog passes `check`, a few times.
async fn shows(
    io: &dyn AnswerIo,
    questions: &[Question],
    check: impl Fn(&[&str]) -> bool,
    tries: usize,
) -> Result<bool, String> {
    for attempt in 0..tries {
        if attempt > 0 {
            io.sleep(SETTLE_MS).await;
        }
        let screen = io.read().await?;
        if let Some(lines) = dialog_of(&screen, questions)
            && check(&lines)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Types the answers into the dialog of `questions`, checking the screen
/// before each question and before the Enter that submits. Refused, having
/// submitted nothing, when the screen is not what that step expects
/// (`typeAnswers`).
pub async fn type_answers(
    io: &dyn AnswerIo,
    questions: &[Question],
    choices: &[Choice],
) -> Result<(), String> {
    let lone = questions.len() == 1 && !questions[0].multi_select;
    for (index, (question, choice)) in questions.iter().zip(choices).enumerate() {
        let tries = if index == 0 {
            FIRST_TRIES
        } else {
            SETTLE_TRIES
        };
        let ready = shows(io, questions, |lines| untouched(lines, question), tries).await?;
        if !ready {
            return Err(if index == 0 {
                NOT_ON_SCREEN.to_string()
            } else {
                lost_at(index)
            });
        }
        let mut steps = steps_for(question, choice);
        if lone && let Some(last) = steps.last_mut() {
            // Held back: this Enter submits, so the screen is checked first.
            last.keys.pop();
        }
        for step in steps {
            if step.text.as_deref().is_some_and(|text| !text.is_empty()) || !step.keys.is_empty() {
                io.type_step(step).await?;
            }
        }
    }
    let ready = if lone {
        shows(
            io,
            questions,
            |lines| chosen(lines, &questions[0], &choices[0]),
            SETTLE_TRIES,
        )
        .await?
    } else {
        shows(
            io,
            questions,
            |lines| reviewed(lines, questions, choices),
            SETTLE_TRIES,
        )
        .await?
    };
    if !ready {
        return Err(NOT_REVIEWED.into());
    }
    io.type_step(AnswerStep {
        text: None,
        keys: vec!["enter".into()],
    })
    .await
}

/* ── remote_answer ────────────────────────────────────────────── */

/// Lines read to find a question's dialog: four questions' review fits well
/// within.
const ANSWER_SCREEN: usize = 120;

/// The answers as the person would write them, for a session that resumed
/// after a sleep: its dialog went with the agent the sleep stopped
/// (`answerText`).
pub fn answer_text(questions: &[Question], choices: &[Choice]) -> String {
    questions
        .iter()
        .zip(choices)
        .map(|(question, choice)| {
            let said: Vec<String> = choice
                .picked
                .iter()
                .filter_map(|&at| question.options.get(at).map(|option| option.label.clone()))
                .chain(choice.other.clone())
                .collect();
            format!("{}\n→ {}", question.question, said.join(", "))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The pane's screen and keys, through pane_read and pane_send as the
/// person.
struct PaneIo<'a> {
    port: &'a Port,
    workspace: String,
    pane: String,
}

impl AnswerIo for PaneIo<'_> {
    fn read(&self) -> Io<'_, Result<String, String>> {
        Box::pin(async move {
            let read = run_as_person(
                "pane_read",
                json!({ "workspace": self.workspace, "pane": self.pane, "lines": ANSWER_SCREEN }),
                self.port,
            )
            .await?;
            Ok(read["text"].as_str().unwrap_or("").to_string())
        })
    }

    fn type_step(&self, step: AnswerStep) -> Io<'_, Result<(), String>> {
        Box::pin(async move {
            let mut args = json!({
                "workspace": self.workspace,
                "pane": self.pane,
                "keys": step.keys,
                "submit": false,
            });
            if let Some(text) = step.text {
                args["text"] = json!(text);
            }
            run_as_person("pane_send", args, self.port)
                .await
                .map(|_| ())
        })
    }

    fn sleep(&self, ms: u64) -> Io<'_, ()> {
        self.port.sleep(ms)
    }
}

/// Marks a pane as being answered for as long as it lives.
struct Answering<'a> {
    mobile: &'a Mobile,
    pane: String,
}

impl Drop for Answering<'_> {
    fn drop(&mut self) {
        self.mobile.book().answering.remove(&self.pane);
    }
}

/// `remote_answer {pane, toolUseId, answers}`: answers the question a Claude
/// Code pane waits on, through its dialog on the PC (`answer_pane`).
pub async fn remote_answer(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<RemoteAnswered, String> {
    let (ws, pane) = find_pane(&port.snapshot().state, args.get("pane"))?;
    let tool_use_id = args.get("toolUseId").and_then(Value::as_str).unwrap_or("");
    answer_pane(port, mobile, &ws, &pane, tool_use_id, |questions| {
        choices_of(questions, args.get("answers"))
    })
    .await
}

/// Answers the question `tool_use_id` a Claude Code pane waits on, through
/// its dialog on the PC: only the one its mod reports open, or its
/// transcript shows unanswered, so a screen showing an older one is
/// refused. `choose` holds the answers against its questions. A pane not up
/// yet — asleep, or resuming — gets the answers in the queue.
pub async fn answer_pane(
    port: &Port,
    mobile: &Mobile,
    ws: &Workspace,
    pane: &Pane,
    tool_use_id: &str,
    choose: impl FnOnce(&[Question]) -> Result<Vec<Choice>, String>,
) -> Result<RemoteAnswered, String> {
    if tool_use_id.is_empty() {
        return Err("Name the question: its \"toolUseId\", from the chat.".into());
    }
    let session_id = match (&pane.agent, &pane.session_id) {
        (AgentId::Claude, Some(id)) if !id.is_empty() => id.clone(),
        _ => {
            return Err(format!(
                "\"{}\" has no Claude Code question to answer.",
                pane.name
            ));
        }
    };
    if !mobile.book().answering.insert(pane.id.clone()) {
        return Err(format!(
            "An answer is being typed into \"{}\" already.",
            pane.name
        ));
    }
    let _answering = Answering {
        mobile,
        pane: pane.id.clone(),
    };
    let cwd = pane
        .cwd
        .clone()
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or_else(|| ws.cwd.clone());
    // On its node for a remote pane: the question is checked against the
    // transcript the pane writes.
    let read = ChatRead {
        session_id,
        cwd,
        before: None,
        limit: None,
    };
    let page = transcript_page(mobile, ws, pane, read).await?;
    let live = mobile.book().chat.question(&pane.id);
    let pending = live.or_else(|| page.as_ref().and_then(|page| page.question.clone()));
    let items = page
        .as_ref()
        .map(|page| page.items.as_slice())
        .unwrap_or_default();
    let questions = pending_questions(pending.as_ref(), items, tool_use_id)?;
    let choices = choose(&questions)?;
    if resumes(port, mobile, pane) {
        let text = answer_text(&questions, &choices);
        hold(port, mobile, ws, pane, Some(&text)).await?;
        return Ok(RemoteAnswered {
            answered: true,
            queued: Some(true),
        });
    }
    if port.terminal(&pane.id).is_none() {
        return Err(NOT_ON_SCREEN.into());
    }
    let io = PaneIo {
        port,
        workspace: ws.id.clone(),
        pane: pane.id.clone(),
    };
    type_answers(&io, &questions, &choices).await?;
    Ok(RemoteAnswered {
        answered: true,
        queued: None,
    })
}

/// The screens of a real three-question prompt replayed, step by step, as
/// Claude Code 2.1.319 draws them in a 160-column pane: long questions and
/// descriptions wrapped, French words, a tab bar above each question and the
/// review at the end. What the window types into them is held against the
/// keys that dialog takes.
#[cfg(test)]
mod replay {
    use std::sync::Mutex;

    use hangar_core::chat::QuestionOption;

    use super::*;
    use crate::mobile::engine::locked;

    fn option(label: &str, description: &str) -> QuestionOption {
        QuestionOption {
            label: label.into(),
            description: description.into(),
        }
    }

    const Q1: &str = "Le moteur d'un serveur GMod (srcds) et son Lua tournent sur un seul thread, et le jeu ne peut pas être rendu entièrement multithread sans réécrire le moteur. Quelle approche veux-tu ?";
    const Q2: &str = "Sur quelle plateforme le serveur tourne-t-il ?";
    const Q3: &str = "Qu'est-ce qui ralentit ton serveur aujourd'hui ? Ça me dit quoi optimiser en premier.";

    /// The person's prompt: long descriptions on the first, a recommended
    /// option first on the second, several answers on the third.
    fn questions() -> Vec<Question> {
        vec![
            Question {
                question: Q1.into(),
                header: "Approche".into(),
                multi_select: false,
                options: vec![
                    option(
                        "Module workers Lua (Recommandé)",
                        "Module binaire C++ (gmsv_*.dll/.so) : un pool de threads, chacun avec son propre état Lua isolé, qui échange des messages avec le thread principal (comme les Web Workers). Les addons y délèguent les calculs lourds. Faisable et stable.",
                    ),
                    option(
                        "Opérations async natives",
                        "Module C++ qui exécute certaines opérations lentes sur des threads (fichiers, SQL, HTTP, JSON, compression) et rappelle un callback Lua. Plus simple, mais limité à ces opérations.",
                    ),
                    option(
                        "Les deux",
                        "Les workers Lua plus une bibliothèque d'opérations async. Plus long.",
                    ),
                    option(
                        "Patch du moteur",
                        "Hooker srcds pour paralléliser la physique ou les Think des entités. Il faut faire du reverse engineering, c'est très instable et ça casse à chaque mise à jour de GMod. Déconseillé.",
                    ),
                ],
            },
            Question {
                question: Q2.into(),
                header: "Plateforme".into(),
                multi_select: false,
                options: vec![
                    option(
                        "Windows et Linux (Recommandé)",
                        "On compile les deux binaires (win64 et linux64), avec une branche x86-64 de GMod.",
                    ),
                    option(
                        "Linux seulement",
                        "Serveur dédié Linux (cas le plus courant pour un serveur hébergé).",
                    ),
                    option("Windows seulement", "Serveur dédié Windows."),
                ],
            },
            Question {
                question: Q3.into(),
                header: "Priorité".into(),
                multi_select: true,
                options: vec![
                    option(
                        "Calculs Lua lourds",
                        "Pathfinding, IA, génération, boucles sur beaucoup d'entités.",
                    ),
                    option(
                        "I/O et base de données",
                        "SQL, fichiers, requêtes HTTP qui bloquent le tick.",
                    ),
                    option(
                        "Rien de précis",
                        "C'est une bibliothèque générique pour les développeurs d'addons.",
                    ),
                ],
            },
        ]
    }

    const WIDTH: usize = 160;

    fn rule() -> String {
        "─".repeat(WIDTH)
    }

    /// Words wrapped as Ink wraps them, each line after the first indented.
    fn wrapped(words: &str, indent: &str, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in words.split(' ') {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
        lines
            .into_iter()
            .map(|line| format!("{indent}{line}"))
            .collect()
    }

    /// The conversation above the dialog, as the pane shows it.
    fn above() -> Vec<String> {
        vec![
            "● Avant de choisir, trois questions.".into(),
            String::new(),
            "● User answered Claude's questions:".into(),
            "  ⎿  · Une ancienne question → Oui".into(),
            String::new(),
        ]
    }

    fn tabs(done: &[bool]) -> String {
        let headers = ["Approche", "Plateforme", "Priorité"];
        let tabs: Vec<String> = headers
            .iter()
            .zip(done)
            .map(|(header, done)| format!(" {} {header} ", if *done { "☒" } else { "☐" }))
            .collect();
        format!("← {} ✔ Submit  →", tabs.join(""))
    }

    /// Question `at` as the dialog draws it: the cursor on row `cursor`
    /// (0-based), the options of a multi-select question ticked by `ticked`.
    fn question_screen(
        questions: &[Question],
        at: usize,
        done: &[bool],
        cursor: usize,
        ticked: &[usize],
        typed: &str,
    ) -> String {
        let q = &questions[at];
        let mut lines = above();
        lines.push(rule());
        lines.push(tabs(done));
        lines.push(String::new());
        lines.extend(wrapped(&q.question, "", WIDTH));
        lines.push(String::new());
        let mark = |row: usize| if row == cursor { "❯" } else { " " };
        for (index, option) in q.options.iter().enumerate() {
            let boxed = match (q.multi_select, ticked.contains(&index)) {
                (false, _) => String::new(),
                (true, true) => "[✔] ".into(),
                (true, false) => "[ ] ".into(),
            };
            lines.push(format!("{} {}. {boxed}{}", mark(index), index + 1, option.label));
            lines.extend(wrapped(&option.description, "     ", WIDTH - 5));
        }
        let type_row = q.options.len();
        if q.multi_select {
            let words = if typed.is_empty() { "Type something" } else { typed };
            lines.push(format!("{} {}. [ ] {words}", mark(type_row), type_row + 1));
            let next = if at + 1 == questions.len() {
                "Submit"
            } else {
                "Next"
            };
            lines.push(format!("{}    {next}", mark(type_row + 1)));
            lines.push(rule());
            lines.push(format!("  {}. Chat about this", type_row + 2));
        } else {
            let words = if typed.is_empty() { "Type something." } else { typed };
            lines.push(format!("{} {}. {words}", mark(type_row), type_row + 1));
            lines.push(rule());
            lines.push(format!("  {}. Chat about this", type_row + 2));
        }
        lines.push(String::new());
        lines.push("Enter to select · Tab/Arrow keys to navigate · Esc to cancel".into());
        lines.join("\n")
    }

    /// The review: each question, wrapped, and its answer.
    fn review_screen(questions: &[Question], said: &[&str]) -> String {
        let mut lines = above();
        lines.push(rule());
        lines.push(rule());
        lines.push(tabs(&[true, true, true]));
        lines.push(String::new());
        lines.push("Review your answers".into());
        lines.push(String::new());
        for (q, said) in questions.iter().zip(said) {
            let mut asked = wrapped(&q.question, "   ", WIDTH - 3);
            asked[0] = format!(" ● {}", asked[0].trim_start());
            lines.extend(asked);
            lines.push(format!("   → {said}"));
        }
        lines.push(String::new());
        lines.push("Ready to submit your answers?".into());
        lines.push(String::new());
        lines.push("❯ 1. Submit answers".into());
        lines.push("  2. Cancel".into());
        lines.join("\n")
    }

    /// The screens replayed: the one shown after each count of steps typed
    /// (the last one past the end).
    struct Replay {
        screens: Vec<(usize, String)>,
        typed: Mutex<Vec<AnswerStep>>,
    }

    impl Replay {
        fn typed(&self) -> Vec<AnswerStep> {
            locked(&self.typed).clone()
        }
    }

    impl AnswerIo for Replay {
        fn read(&self) -> Io<'_, Result<String, String>> {
            let count = locked(&self.typed).len();
            let screen = self
                .screens
                .iter()
                .rev()
                .find(|(after, _)| *after <= count)
                .map(|(_, screen)| screen.clone())
                .unwrap_or_default();
            Box::pin(async move { Ok(screen) })
        }

        fn type_step(&self, step: AnswerStep) -> Io<'_, Result<(), String>> {
            locked(&self.typed).push(step);
            Box::pin(async { Ok(()) })
        }

        fn sleep(&self, _ms: u64) -> Io<'_, ()> {
            Box::pin(async {})
        }
    }

    fn keys(names: &[&str]) -> AnswerStep {
        AnswerStep {
            text: None,
            keys: names.iter().map(|name| (*name).to_string()).collect(),
        }
    }

    const OWN: &str = "on va réécrire le moteur";

    /// The window's answers: its own words on the first, the recommended
    /// option on the second, two on the third.
    fn given() -> Vec<Answer> {
        vec![
            Answer {
                selected: vec![],
                other: Some(OWN.into()),
            },
            Answer {
                selected: vec!["Windows et Linux (Recommandé)".into()],
                other: None,
            },
            Answer {
                selected: vec![
                    "I/O et base de données".into(),
                    "Calculs Lua lourds".into(),
                ],
                other: None,
            },
        ]
    }

    fn replay(review: &str) -> Replay {
        let questions = questions();
        let screens = vec![
            (0, question_screen(&questions, 0, &[false; 3], 0, &[], "")),
            (1, question_screen(&questions, 0, &[false; 3], 4, &[], "")),
            (
                2,
                question_screen(&questions, 1, &[true, false, false], 0, &[], ""),
            ),
            (
                3,
                question_screen(&questions, 2, &[true, true, false], 0, &[], ""),
            ),
            (4, review.to_string()),
            (5, String::new()),
        ];
        Replay {
            screens,
            typed: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn the_first_question_is_found_as_drawn_wrapped_and_in_french() {
        let questions = questions();
        let screen = question_screen(&questions, 0, &[false; 3], 0, &[], "");
        let lines = dialog_of(&screen, &questions).expect("the dialog under its tab bar");
        assert!(untouched(&lines, &questions[0]));
        // The tab of another question does not stand for this one.
        assert!(!untouched(&lines, &questions[1]));
        let moved = question_screen(&questions, 0, &[false; 3], 1, &[], "");
        let lines = dialog_of(&moved, &questions).unwrap();
        assert!(!untouched(&lines, &questions[0]), "someone moved the cursor");
    }

    #[tokio::test]
    async fn three_questions_own_words_a_default_and_several_answers_typed_then_submitted() {
        let questions = questions();
        let choices = choices_from(&questions, &given()).unwrap();
        assert_eq!(
            choices,
            [
                Choice {
                    picked: vec![],
                    other: Some(OWN.into())
                },
                Choice {
                    picked: vec![0],
                    other: None
                },
                Choice {
                    picked: vec![0, 1],
                    other: None
                },
            ]
        );
        let review = review_screen(
            &questions,
            &[
                OWN,
                "Windows et Linux (Recommandé)",
                "Calculs Lua lourds, I/O et base de données",
            ],
        );
        let io = replay(&review);
        type_answers(&io, &questions, &choices).await.unwrap();
        assert_eq!(
            io.typed(),
            [
                // 1: down to "Type something.", the words, Enter.
                keys(&["down", "down", "down", "down"]),
                AnswerStep {
                    text: Some(OWN.into()),
                    keys: vec!["enter".into()],
                },
                // 2: the first option, under the cursor.
                keys(&["enter"]),
                // 3: tick the first two, down to "Submit".
                keys(&["enter", "down", "enter", "down", "down", "down", "enter"]),
                // The review's "Submit answers".
                keys(&["enter"]),
            ]
        );
    }

    #[tokio::test]
    async fn a_review_that_shows_another_answer_is_not_submitted() {
        let questions = questions();
        let choices = choices_from(&questions, &given()).unwrap();
        let review = review_screen(
            &questions,
            &[OWN, "Linux seulement", "Calculs Lua lourds, I/O et base de données"],
        );
        let io = replay(&review);
        assert_eq!(
            type_answers(&io, &questions, &choices).await,
            Err(NOT_REVIEWED.to_string())
        );
        assert_eq!(io.typed().len(), 4, "everything but the Enter that submits");
    }

    #[tokio::test]
    async fn a_question_someone_is_answering_at_the_pc_gets_nothing_typed() {
        let questions = questions();
        let choices = choices_from(&questions, &given()).unwrap();
        let io = Replay {
            screens: vec![(
                0,
                question_screen(&questions, 0, &[false; 3], 2, &[], ""),
            )],
            typed: Mutex::new(Vec::new()),
        };
        assert_eq!(
            type_answers(&io, &questions, &choices).await,
            Err(NOT_ON_SCREEN.to_string())
        );
        assert!(io.typed().is_empty());
    }
}
