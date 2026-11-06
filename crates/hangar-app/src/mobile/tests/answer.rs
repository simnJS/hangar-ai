//! remoteAnswer.test.ts, case for case.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use hangar_core::chat::{
    Answer, Body, ChatItem, ChatPage, PendingQuestion, Question, QuestionOption,
};

use crate::control::port::Port;
use crate::state::live::{BridgeEvent, ToolPhase};
use crate::state::{AgentId, AppState, Pane};

use super::super::answer::{
    AnswerIo, AnswerStep, Choice, NOT_ON_SCREEN, NOT_REVIEWED, choices_of, dialog_of, lost_at,
    pending_questions, steps_for, type_answers, untouched,
};
use super::super::contract::RemoteAnswered;
use super::super::engine::{ChatRead, Mobile, locked, run};
use super::fake::{App, Fakes, Screen, workspace};

fn question(header: &str, text: &str, labels: &[&str], multi: bool) -> Question {
    Question {
        question: text.into(),
        header: header.into(),
        multi_select: multi,
        options: labels
            .iter()
            .map(|label| QuestionOption {
                label: (*label).into(),
                description: format!("The {}", label.to_lowercase()),
            })
            .collect(),
    }
}

fn color() -> Question {
    question(
        "Color",
        "Which color do you prefer?",
        &["Red", "Green", "Blue"],
        false,
    )
}
fn fruits() -> Question {
    question(
        "Fruits",
        "Which fruits do you like?",
        &["Apple", "Banana", "Cherry", "Date"],
        true,
    )
}
fn pet() -> Question {
    question("Pet", "Which pet would you adopt?", &["Cat", "Dog"], false)
}
fn size() -> Question {
    question("Size", "Which size?", &["Small", "Large"], false)
}
fn tools() -> Question {
    question("Tools", "Which tools?", &["Hammer", "Saw", "Drill"], true)
}

fn rule(n: usize) -> String {
    "─".repeat(n)
}

/// What the dialog recorded once submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Submitted {
    Answers(Vec<(String, String)>),
    Cancelled,
}

fn answers(pairs: &[(&str, &str)]) -> Option<Submitted> {
    Some(Submitted::Answers(
        pairs
            .iter()
            .map(|(q, a)| ((*q).to_string(), (*a).to_string()))
            .collect(),
    ))
}

#[derive(Debug)]
struct DialogState {
    questions: Vec<Question>,
    lose: Vec<usize>,
    tab: usize,
    cursor: usize,
    reviewing: bool,
    submitted: Option<Submitted>,
    keys: Vec<String>,
    picked: Vec<Option<String>>,
    ticked: Vec<BTreeSet<usize>>,
    typed: Vec<String>,
    typed_ticked: Vec<bool>,
    done: Vec<bool>,
}

/// Claude Code's AskUserQuestion dialog as a pane shows it, row for row as
/// 2.1.312 draws it (wrapped rows joined back, as pane_read gives them), and
/// how it takes keys and pasted text. The transcript above it mentions the
/// questions and has a todo on a header, as a real screen may.
struct Dialog(Mutex<DialogState>);

impl Dialog {
    /// `lose`: the indexes of keys that never reach it.
    fn new(questions: Vec<Question>, lose: &[usize]) -> Arc<Dialog> {
        let n = questions.len();
        Arc::new(Dialog(Mutex::new(DialogState {
            questions,
            lose: lose.to_vec(),
            tab: 0,
            cursor: 0,
            reviewing: false,
            submitted: None,
            keys: Vec::new(),
            picked: vec![None; n],
            ticked: vec![BTreeSet::new(); n],
            typed: vec![String::new(); n],
            typed_ticked: vec![false; n],
            done: vec![false; n],
        })))
    }

    fn submitted(&self) -> Option<Submitted> {
        locked(&self.0).submitted.clone()
    }

    fn keys(&self) -> Vec<String> {
        locked(&self.0).keys.clone()
    }

    fn screen(&self) -> String {
        locked(&self.0).screen()
    }
}

impl DialogState {
    fn lone(&self) -> bool {
        self.questions.len() == 1 && !self.questions[0].multi_select
    }

    fn answer_of(&self, index: usize) -> String {
        let q = &self.questions[index];
        if !q.multi_select {
            return self.picked[index].clone().unwrap_or_default();
        }
        let words = &self.typed[index];
        let mut parts: Vec<String> = q
            .options
            .iter()
            .enumerate()
            .filter(|(at, _)| self.ticked[index].contains(at))
            .map(|(_, option)| option.label.clone())
            .collect();
        if self.typed_ticked[index] && !words.is_empty() {
            parts.push(if words.contains(", ") {
                format!("\"{words}\"")
            } else {
                words.clone()
            });
        }
        parts.join(", ")
    }

    fn move_on(&mut self) {
        self.done[self.tab] = true;
        if self.lone() {
            return self.submit();
        }
        self.tab += 1;
        self.cursor = 0;
        if self.tab == self.questions.len() {
            self.reviewing = true;
        }
    }

    fn submit(&mut self) {
        let answers = (0..self.questions.len())
            .map(|at| (self.questions[at].question.clone(), self.answer_of(at)))
            .collect();
        self.submitted = Some(Submitted::Answers(answers));
    }

    fn key(&mut self, name: &str) {
        self.keys.push(name.to_string());
        if self.lose.contains(&(self.keys.len() - 1)) || self.submitted.is_some() {
            return;
        }
        let q = self.questions.get(self.tab).cloned();
        let type_row = q.as_ref().map_or(0, |q| q.options.len());
        let last = if self.reviewing {
            1
        } else if q.as_ref().is_some_and(|q| q.multi_select) {
            type_row + 2
        } else {
            type_row + 1
        };
        match name {
            "down" => self.cursor = last.min(self.cursor + 1),
            "up" => self.cursor = self.cursor.saturating_sub(1),
            "escape" => self.submitted = Some(Submitted::Cancelled),
            "enter" if self.reviewing => {
                if self.cursor == 0 {
                    self.submit();
                } else {
                    self.submitted = Some(Submitted::Cancelled);
                }
            }
            "enter" if self.cursor == last => panic!("\"Chat about this\" was chosen."),
            "enter" => {
                let q = q.expect("a question on screen");
                if !q.multi_select {
                    if self.cursor < type_row {
                        self.picked[self.tab] = Some(q.options[self.cursor].label.clone());
                    } else if !self.typed[self.tab].is_empty() {
                        self.picked[self.tab] = Some(self.typed[self.tab].clone());
                    } else {
                        return;
                    }
                    self.move_on();
                } else if self.cursor < type_row {
                    let cursor = self.cursor;
                    let ticked = &mut self.ticked[self.tab];
                    if !ticked.remove(&cursor) {
                        ticked.insert(cursor);
                    }
                } else if self.cursor == type_row {
                    self.typed_ticked[self.tab] = !self.typed_ticked[self.tab];
                } else {
                    self.move_on();
                }
            }
            other => panic!("The dialog takes no \"{other}\"."),
        }
    }

    /// Text lands in "Type something" alone, and ticks it.
    fn paste(&mut self, text: &str) {
        if self.submitted.is_some() || self.reviewing {
            return;
        }
        if self.cursor != self.questions[self.tab].options.len() {
            return;
        }
        self.typed[self.tab].push_str(text);
        self.typed_ticked[self.tab] = true;
    }

    fn screen(&self) -> String {
        let all: Vec<&str> = self.questions.iter().map(|q| q.question.as_str()).collect();
        let mut lines = vec![
            format!("❯ Ask me: {}", all.join(" ")),
            String::new(),
            format!("  ☐ {} the todo list", self.questions[0].header),
            String::new(),
        ];
        if self.submitted.is_some() {
            lines.extend([
                "● User answered Claude's questions:".to_string(),
                String::new(),
                rule(80),
                "❯ ".to_string(),
                rule(80),
            ]);
            return lines.join("\n");
        }
        let tabs: Vec<String> = self
            .questions
            .iter()
            .enumerate()
            .map(|(at, q)| format!("{} {}", if self.done[at] { "☒" } else { "☐" }, q.header))
            .collect();
        let tabs = tabs.join("  ");
        lines.push(if self.lone() {
            format!("{} {tabs} ", rule(80))
        } else {
            format!("{}←  {tabs}  ✔ Submit  →", rule(80))
        });
        let cur = |row: usize| if self.cursor == row { "❯" } else { " " };
        if self.reviewing {
            lines.extend([
                "".to_string(),
                "Review your answers".to_string(),
                "  ".to_string(),
            ]);
            for (at, q) in self.questions.iter().enumerate() {
                lines.push(format!(" ● {}", q.question));
                lines.push(format!("   → {}", self.answer_of(at)));
            }
            lines.extend([
                String::new(),
                "Ready to submit your answers?".to_string(),
                String::new(),
                format!("{} 1. Submit answers  2. Cancel", cur(0)),
            ]);
            return lines.join("\n");
        }
        let q = &self.questions[self.tab];
        let type_row = q.options.len();
        lines.extend([String::new(), q.question.clone(), String::new()]);
        for (at, option) in q.options.iter().enumerate() {
            let boxed = if q.multi_select {
                format!(
                    "[{}] ",
                    if self.ticked[self.tab].contains(&at) {
                        "✔"
                    } else {
                        " "
                    }
                )
            } else {
                String::new()
            };
            lines.push(format!("{} {}. {boxed}{}", cur(at), at + 1, option.label));
            lines.push(format!("     {}", option.description));
        }
        let words = &self.typed[self.tab];
        if q.multi_select {
            let shown = if words.is_empty() {
                "Type something"
            } else {
                words
            };
            let tick = if self.typed_ticked[self.tab] {
                "✔"
            } else {
                " "
            };
            lines.push(format!(
                "{} {}. [{tick}] {shown}",
                cur(type_row),
                type_row + 1
            ));
            let next = if self.tab == self.questions.len() - 1 {
                "Submit"
            } else {
                "Next"
            };
            lines.push(format!("{}    {next}", cur(type_row + 1)));
            lines.push(format!("{}  {}. Chat about this", rule(80), type_row + 2));
        } else {
            let shown = if words.is_empty() {
                "Type something."
            } else {
                words
            };
            lines.push(format!("{} {}. {shown}", cur(type_row), type_row + 1));
            lines.push(rule(80));
            lines.push(format!("  {}. Chat about this", type_row + 2));
        }
        lines.extend([
            String::new(),
            "Enter to select · Tab/Arrow keys to navigate · Esc to cancel".to_string(),
            format!("{} Ava (Shop) ─", rule(56)),
        ]);
        lines.join("\n")
    }
}

impl Screen for Dialog {
    fn paste(&self, text: &str) {
        locked(&self.0).paste(text);
    }

    fn key(&self, name: &str) {
        locked(&self.0).key(name);
    }

    fn lines(&self) -> Vec<String> {
        self.screen().split('\n').map(str::to_string).collect()
    }
}

/// The dialog read and typed into directly (`ioFor`).
struct DialogIo {
    dialog: Arc<Dialog>,
    typed: Mutex<Vec<AnswerStep>>,
}

type Io<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

impl AnswerIo for DialogIo {
    fn read(&self) -> Io<'_, Result<String, String>> {
        let screen = self.dialog.screen();
        Box::pin(async move { Ok(screen) })
    }

    fn type_step(&self, step: AnswerStep) -> Io<'_, Result<(), String>> {
        locked(&self.typed).push(step.clone());
        if let Some(text) = &step.text {
            self.dialog.paste(text);
        }
        for key in &step.keys {
            self.dialog.key(key);
        }
        Box::pin(async { Ok(()) })
    }

    fn sleep(&self, _ms: u64) -> Io<'_, ()> {
        Box::pin(async {})
    }
}

fn io_for(dialog: &Arc<Dialog>) -> DialogIo {
    DialogIo {
        dialog: dialog.clone(),
        typed: Mutex::new(Vec::new()),
    }
}

async fn answer(
    questions: &[Question],
    given: Value,
    dialog: &Arc<Dialog>,
) -> Result<Option<Submitted>, String> {
    let choices = choices_of(questions, Some(&given))?;
    type_answers(&io_for(dialog), questions, &choices).await?;
    Ok(dialog.submitted())
}

fn keys(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

fn step(names: &[&str]) -> AnswerStep {
    AnswerStep {
        text: None,
        keys: keys(names),
    }
}

fn typed(text: &str, names: &[&str]) -> AnswerStep {
    AnswerStep {
        text: Some(text.into()),
        keys: keys(names),
    }
}

fn choice(picked: &[usize], other: Option<&str>) -> Choice {
    Choice {
        picked: picked.to_vec(),
        other: other.map(str::to_string),
    }
}

/* ── The keys for one question ────────────────────────────────── */

#[test]
fn keys_a_single_select_option_down_to_it_enter() {
    assert_eq!(
        steps_for(&color(), &choice(&[1], None)),
        [step(&["down", "enter"])]
    );
    assert_eq!(steps_for(&color(), &choice(&[0], None)), [step(&["enter"])]);
}

#[test]
fn keys_a_single_select_question_s_own_words_down_to_type_something_paste_enter() {
    assert_eq!(
        steps_for(&pet(), &choice(&[], Some("A hamster"))),
        [step(&["down", "down"]), typed("A hamster", &["enter"])]
    );
}

#[test]
fn keys_multi_select_enter_on_each_option_then_down_to_next_and_enter() {
    assert_eq!(
        steps_for(&fruits(), &choice(&[0, 2], None)),
        [step(&[
            "enter", "down", "down", "enter", "down", "down", "down", "enter"
        ])]
    );
    assert_eq!(
        steps_for(&fruits(), &choice(&[3], None)),
        [step(&[
            "down", "down", "down", "enter", "down", "down", "enter"
        ])]
    );
}

#[test]
fn keys_multi_select_with_the_person_s_own_words_they_tick_type_something_as_they_land() {
    assert_eq!(
        steps_for(&fruits(), &choice(&[0, 2], Some("Mango, ripe"))),
        [
            step(&["enter", "down", "down", "enter", "down", "down"]),
            typed("Mango, ripe", &["down", "enter"])
        ]
    );
    assert_eq!(
        steps_for(&fruits(), &choice(&[], Some("Kiwi"))),
        [
            step(&["down", "down", "down", "down"]),
            typed("Kiwi", &["down", "enter"])
        ]
    );
}

/* ── Answering the dialog ─────────────────────────────────────── */

#[tokio::test]
async fn dialog_several_questions_multi_select_and_own_words_each_tab_then_the_review_s_submit() {
    let questions = [color(), fruits(), pet()];
    let dialog = Dialog::new(questions.to_vec(), &[]);
    let io = io_for(&dialog);
    let choices = choices_of(
        &questions,
        Some(&json!([{"selected": ["Green"]}, {"selected": ["Cherry", "Apple"], "other": "Mango, ripe"}, {"selected": [], "other": "A \"small\" hamster"}])),
    )
    .unwrap();
    type_answers(&io, &questions, &choices).await.unwrap();
    // What Claude Code 2.1.312 recorded for these keys in a real session.
    assert_eq!(
        dialog.submitted(),
        answers(&[
            ("Which color do you prefer?", "Green"),
            (
                "Which fruits do you like?",
                "Apple, Cherry, \"Mango, ripe\""
            ),
            ("Which pet would you adopt?", "A \"small\" hamster"),
        ])
    );
    assert_eq!(
        *locked(&io.typed),
        [
            step(&["down", "enter"]),
            step(&["enter", "down", "down", "enter", "down", "down"]),
            typed("Mango, ripe", &["down", "enter"]),
            step(&["down", "down"]),
            typed("A \"small\" hamster", &["enter"]),
            step(&["enter"]),
        ]
    );
}

#[tokio::test]
async fn dialog_a_lone_single_select_question_its_enter_submits_once_the_screen_shows_the_choice() {
    let dialog = Dialog::new(vec![size()], &[]);
    let io = io_for(&dialog);
    let choices = choices_of(&[size()], Some(&json!([{"selected": ["Large"]}]))).unwrap();
    type_answers(&io, &[size()], &choices).await.unwrap();
    assert_eq!(dialog.submitted(), answers(&[("Which size?", "Large")]));
    assert_eq!(*locked(&io.typed), [step(&["down"]), step(&["enter"])]);

    let small = Dialog::new(vec![size()], &[]);
    assert_eq!(
        answer(&[size()], json!([{"selected": ["Small"]}]), &small)
            .await
            .unwrap(),
        answers(&[("Which size?", "Small")])
    );
    let own = Dialog::new(vec![size()], &[]);
    assert_eq!(
        answer(
            &[size()],
            json!([{"selected": [], "other": "Medium, please"}]),
            &own
        )
        .await
        .unwrap(),
        answers(&[("Which size?", "Medium, please")])
    );
}

#[tokio::test]
async fn dialog_a_lone_multi_select_question_goes_through_the_review() {
    let one = Dialog::new(vec![tools()], &[]);
    assert_eq!(
        answer(&[tools()], json!([{"selected": ["Drill", "Hammer"]}]), &one)
            .await
            .unwrap(),
        answers(&[("Which tools?", "Hammer, Drill")])
    );
    let two = Dialog::new(vec![tools()], &[]);
    assert_eq!(
        answer(
            &[tools()],
            json!([{"selected": ["Saw"], "other": "A level"}]),
            &two
        )
        .await
        .unwrap(),
        answers(&[("Which tools?", "Saw, A level")])
    );
}

#[test]
fn dialog_is_found_below_its_tab_bar_not_a_todo_or_a_prompt_naming_the_same_words() {
    let screen = Dialog::new(vec![color(), pet()], &[]).screen();
    let lines = dialog_of(&screen, &[color(), pet()]).unwrap();
    assert_eq!(lines[1], "Which color do you prefer?");
    assert!(untouched(&lines, &color()));
    assert!(
        dialog_of(
            "❯ Which color do you prefer?\n  ☐ Something else",
            &[color()]
        )
        .is_none()
    );
}

#[tokio::test]
async fn dialog_types_nothing_into_a_dialog_someone_at_the_pc_has_started_on() {
    for touch in [vec!["down"], vec!["enter"], vec!["down", "down", "down"]] {
        let dialog = Dialog::new(vec![fruits(), pet()], &[]);
        for key in &touch {
            dialog.key(key);
        }
        let before = dialog.keys().len();
        let refused = answer(
            &[fruits(), pet()],
            json!([{"selected": ["Apple"]}, {"selected": ["Cat"]}]),
            &dialog,
        )
        .await;
        assert_eq!(refused, Err(NOT_ON_SCREEN.to_string()));
        assert_eq!(dialog.keys().len(), before);
    }
    // Already past the first question.
    let dialog = Dialog::new(vec![color(), pet()], &[]);
    dialog.key("enter");
    let refused = answer(
        &[color(), pet()],
        json!([{"selected": ["Red"]}, {"selected": ["Cat"]}]),
        &dialog,
    )
    .await;
    assert_eq!(refused, Err(NOT_ON_SCREEN.to_string()));
}

#[tokio::test]
async fn dialog_types_nothing_when_the_dialog_is_gone_from_the_screen_or_shows_other_options() {
    let answered = Dialog::new(vec![color()], &[]);
    answered.key("enter");
    assert_eq!(
        answered.submitted(),
        answers(&[("Which color do you prefer?", "Red")])
    );
    assert_eq!(
        answer(&[color()], json!([{"selected": ["Blue"]}]), &answered).await,
        Err(NOT_ON_SCREEN.to_string())
    );
    assert_eq!(answered.keys(), ["enter"]);

    let other = Dialog::new(
        vec![question(
            "Color",
            "Which color do you prefer?",
            &["Red", "Blue", "Green"],
            false,
        )],
        &[],
    );
    assert_eq!(
        answer(&[color()], json!([{"selected": ["Blue"]}]), &other).await,
        Err(NOT_ON_SCREEN.to_string())
    );
    assert!(other.keys().is_empty());
}

#[tokio::test]
async fn dialog_submits_nothing_when_a_key_was_lost_on_the_way() {
    // The Enter that ticks Cherry: the review shows Apple alone.
    let lost = Dialog::new(vec![color(), fruits()], &[5]);
    assert_eq!(
        answer(
            &[color(), fruits()],
            json!([{"selected": ["Green"]}, {"selected": ["Apple", "Cherry"]}]),
            &lost
        )
        .await,
        Err(NOT_REVIEWED.to_string())
    );
    assert_eq!(lost.submitted(), None);

    // The Enter that moves past the first question: the second never comes up.
    let stuck = Dialog::new(vec![color(), pet()], &[1]);
    assert_eq!(
        answer(
            &[color(), pet()],
            json!([{"selected": ["Green"]}, {"selected": ["Dog"]}]),
            &stuck
        )
        .await,
        Err(lost_at(1))
    );
    assert_eq!(stuck.submitted(), None);

    // A lone question whose cursor did not reach the answer.
    let short = Dialog::new(vec![size()], &[0]);
    assert_eq!(
        answer(&[size()], json!([{"selected": ["Large"]}]), &short).await,
        Err(NOT_REVIEWED.to_string())
    );
    assert_eq!(short.submitted(), None);
}

/* ── The phone's answers ──────────────────────────────────────── */

#[test]
fn answers_are_option_labels_exactly_as_given_and_the_person_s_own_words_cleaned_of_line_breaks() {
    assert_eq!(
        choices_of(
            &[color(), fruits()],
            Some(
                &json!([{"selected": ["Blue"]}, {"selected": ["Date", "Apple", "Date"], "other": " Kiwi\nand\tlime\u{0007} "}])
            )
        ),
        Ok(vec![
            choice(&[2], None),
            choice(&[0, 3], Some("Kiwi and lime"))
        ])
    );
    assert_eq!(
        choices_of(&[pet()], Some(&json!([{"other": "Fish"}]))),
        Ok(vec![choice(&[], Some("Fish"))])
    );
}

#[test]
fn answers_refuse_a_label_that_is_not_among_the_options() {
    assert_eq!(
        choices_of(&[color()], Some(&json!([{"selected": ["Purple"]}]))),
        Err("\"Purple\" is not one of the options of \"Color\".".into())
    );
    assert_eq!(
        choices_of(
            &[color(), pet()],
            Some(&json!([{"selected": ["Red"]}, {"selected": ["cat"]}]))
        ),
        Err("\"cat\" is not one of the options of Question 2 (\"Pet\").".into())
    );
}

#[track_caller]
fn refused(answer: Result<Vec<Choice>, String>, part: &str) {
    let error = answer.expect_err("a refusal");
    assert!(error.contains(part), "{error:?} does not say {part:?}");
}

#[test]
fn answers_refuse_a_single_select_question_given_two_answers_and_a_question_given_none() {
    refused(
        choices_of(&[color()], Some(&json!([{"selected": ["Red", "Blue"]}]))),
        "takes one answer",
    );
    refused(
        choices_of(
            &[color()],
            Some(&json!([{"selected": ["Red"], "other": "Teal"}])),
        ),
        "takes one answer",
    );
    refused(
        choices_of(&[fruits()], Some(&json!([{"selected": [], "other": "  "}]))),
        "\"Fruits\" has no answer.",
    );
}

#[test]
fn answers_refuse_answers_that_do_not_match_the_questions_one_for_one() {
    refused(
        choices_of(&[color(), pet()], Some(&json!([{"selected": ["Red"]}]))),
        "one answer per question: 2",
    );
    refused(
        choices_of(&[color()], Some(&json!("Red"))),
        "Give one answer",
    );
    refused(
        choices_of(&[color()], Some(&json!([{"selected": "Red"}]))),
        "\"selected\" is a list",
    );
    refused(
        choices_of(&[color()], Some(&json!([{"other": 3}]))),
        "\"other\" is the person's own words",
    );
}

/* ── Which question the pane waits on ─────────────────────────── */

fn asked(tool_use_id: &str, answer: Option<Vec<Answer>>) -> ChatItem {
    ChatItem {
        id: tool_use_id.into(),
        at: 1,
        body: Body::Question {
            tool_use_id: tool_use_id.into(),
            questions: vec![color()],
            answer,
        },
    }
}

fn pending(id: &str, questions: Vec<Question>) -> PendingQuestion {
    PendingQuestion {
        tool_use_id: id.into(),
        questions,
    }
}

#[test]
fn waits_on_the_pending_one_by_its_tool_use_id() {
    assert_eq!(
        pending_questions(
            Some(&pending("q2", vec![color()])),
            &[asked("q2", None)],
            "q2"
        ),
        Ok(vec![color()])
    );
}

#[test]
fn waits_refuses_a_question_already_answered_dismissed_or_replaced_by_another() {
    let answered = [asked(
        "q1",
        Some(vec![Answer {
            selected: vec!["Red".into()],
            other: None,
        }]),
    )];
    let said = |answer: Result<Vec<Question>, String>| answer.unwrap_err();
    assert_eq!(
        said(pending_questions(None, &answered, "q1")),
        "This question was already answered."
    );
    assert!(
        said(pending_questions(
            Some(&pending("q2", vec![pet()])),
            &answered,
            "q1"
        ))
        .contains("already answered")
    );
    assert!(
        said(pending_questions(None, &[asked("q1", None)], "q1"))
            .contains("no longer waits on this question")
    );
    assert!(
        said(pending_questions(
            Some(&pending("q2", vec![pet()])),
            &[],
            "q1"
        ))
        .contains("now waits on another question")
    );
    assert!(said(pending_questions(None, &[], "q1")).contains("is not waiting on this question"));
}

/* ── remote_answer ────────────────────────────────────────────── */

struct AnswerRig {
    fakes: Arc<Fakes>,
    mobile: Arc<Mobile>,
    port: Port,
}

impl AnswerRig {
    async fn run(&self, args: Value) -> Result<Value, String> {
        let mut args = args;
        args["pane"] = json!("Ava");
        run("remote_answer", &args, &self.port, &self.mobile).await
    }
}

/// A window with one Claude Code pane, "Ava", showing `dialog`, and its
/// transcript.
fn answer_rig(dialog: &Arc<Dialog>, page: ChatPage, agent: AgentId) -> AnswerRig {
    let ava = Pane {
        id: "a".into(),
        name: "Ava".into(),
        agent,
        session_id: Some("s1".into()),
        ..Pane::default()
    };
    let app = App::new(AppState {
        workspaces: vec![workspace("shop", "Shop", vec![ava])],
        ..AppState::default()
    });
    *locked(&app.drawn) = Some(dialog.clone() as Arc<dyn Screen>);
    let fakes = Fakes::new(app.clone());
    *locked(&fakes.chat) = Box::new(move |_| Ok(Some(page.clone())));
    let mobile = fakes.mobile();
    AnswerRig {
        fakes,
        mobile,
        port: app.port(),
    }
}

fn page_with(question: Option<PendingQuestion>, items: Vec<ChatItem>) -> ChatPage {
    ChatPage {
        items,
        older: false,
        cursor: None,
        question,
        open_call: None,
    }
}

#[tokio::test]
async fn remote_answer_types_the_answers_into_the_pane_and_answers_answered_true() {
    let dialog = Dialog::new(vec![color(), fruits(), pet()], &[]);
    let rig = answer_rig(
        &dialog,
        page_with(Some(pending("q1", vec![color(), fruits(), pet()])), vec![]),
        AgentId::Claude,
    );
    let given =
        json!([{"selected": ["Blue"]}, {"selected": ["Banana", "Date"]}, {"selected": ["Dog"]}]);
    assert_eq!(
        rig.run(json!({"toolUseId": "q1", "answers": given})).await,
        Ok(json!({"answered": true}))
    );
    assert_eq!(
        dialog.submitted(),
        answers(&[
            ("Which color do you prefer?", "Blue"),
            ("Which fruits do you like?", "Banana, Date"),
            ("Which pet would you adopt?", "Dog"),
        ])
    );
    assert_eq!(
        *locked(&rig.fakes.chats),
        [ChatRead {
            session_id: "s1".into(),
            cwd: "C:\\dev\\shop".into(),
            before: None,
            limit: None,
        }]
    );
}

#[tokio::test]
async fn remote_answer_refuses_a_stale_phone_screen_an_answered_question_a_wrong_label_typing_nothing()
 {
    let answered = ChatItem {
        id: "q0".into(),
        at: 1,
        body: Body::Question {
            tool_use_id: "q0".into(),
            questions: vec![color()],
            answer: Some(vec![Answer {
                selected: vec!["Red".into()],
                other: None,
            }]),
        },
    };
    let cases = [
        (
            page_with(Some(pending("q1", vec![color()])), vec![]),
            json!({"toolUseId": "q0", "answers": [{"selected": ["Red"]}]}),
            "now waits on another question",
        ),
        (
            page_with(None, vec![answered]),
            json!({"toolUseId": "q0", "answers": [{"selected": ["Blue"]}]}),
            "already answered",
        ),
        (
            page_with(Some(pending("q1", vec![color()])), vec![]),
            json!({"toolUseId": "q1", "answers": [{"selected": ["Teal"]}]}),
            "\"Teal\" is not one of the options",
        ),
        (
            page_with(Some(pending("q1", vec![color()])), vec![]),
            json!({"answers": [{"selected": ["Red"]}]}),
            "Name the question: its \"toolUseId\"",
        ),
    ];
    for (page, args, error) in cases {
        let dialog = Dialog::new(vec![color()], &[]);
        let said = answer_rig(&dialog, page, AgentId::Claude)
            .run(args)
            .await
            .unwrap_err();
        assert!(said.contains(error), "{said:?} does not say {error:?}");
        assert!(dialog.keys().is_empty());
    }
}

#[tokio::test]
async fn remote_answer_answers_the_question_the_pane_s_mod_reports_which_the_transcript_does_not_have_yet()
 {
    let dialog = Dialog::new(vec![color(), pet()], &[]);
    let rig = answer_rig(&dialog, page_with(None, vec![]), AgentId::Claude);
    rig.mobile.note_bridge_event(
        "a",
        &BridgeEvent::Tool(ToolPhase::Start {
            tool_use_id: "q9".into(),
            tool: "AskUserQuestion".into(),
            input: json!({"questions": [color(), pet()]}),
        }),
        0,
    );
    let said = rig
        .run(json!({"toolUseId": "q8", "answers": [{"selected": ["Red"]}, {"selected": ["Cat"]}]}))
        .await;
    assert!(said.unwrap_err().contains("now waits on another question"));
    assert!(dialog.keys().is_empty());
    assert_eq!(
        rig.run(
            json!({"toolUseId": "q9", "answers": [{"selected": ["Red"]}, {"other": "A parrot"}]})
        )
        .await,
        Ok(json!({"answered": true}))
    );
    assert_eq!(
        dialog.submitted(),
        answers(&[
            ("Which color do you prefer?", "Red"),
            ("Which pet would you adopt?", "A parrot")
        ])
    );
    rig.mobile.note_bridge_event(
        "a",
        &BridgeEvent::Tool(ToolPhase::End {
            tool_use_id: "q9".into(),
        }),
        0,
    );
    let said = rig
        .run(json!({"toolUseId": "q9", "answers": [{"selected": ["Red"]}, {"selected": ["Cat"]}]}))
        .await;
    assert!(said.unwrap_err().contains("not waiting on this question"));
}

#[tokio::test]
async fn remote_answer_refuses_when_the_transcript_still_waits_but_the_pc_s_screen_no_longer_shows_the_question()
 {
    let dialog = Dialog::new(vec![color()], &[]);
    dialog.key("down");
    dialog.key("enter");
    let rig = answer_rig(
        &dialog,
        page_with(Some(pending("q1", vec![color()])), vec![]),
        AgentId::Claude,
    );
    assert_eq!(
        rig.run(json!({"toolUseId": "q1", "answers": [{"selected": ["Red"]}]}))
            .await,
        Err(NOT_ON_SCREEN.to_string())
    );
    assert_eq!(
        dialog.submitted(),
        answers(&[("Which color do you prefer?", "Green")])
    );
}

#[tokio::test]
async fn remote_answer_refuses_a_pane_without_a_claude_question_and_a_second_answer_while_one_is_typed()
 {
    let page = page_with(Some(pending("q1", vec![color()])), vec![]);
    let shell = answer_rig(
        &Dialog::new(vec![color()], &[]),
        page.clone(),
        AgentId::Shell,
    );
    assert_eq!(
        shell.run(json!({"toolUseId": "q1", "answers": []})).await,
        Err("\"Ava\" has no Claude Code question to answer.".into())
    );

    let dialog = Dialog::new(vec![color()], &[]);
    let rig = answer_rig(&dialog, page, AgentId::Claude);
    let (first, second) = tokio::join!(
        rig.run(json!({"toolUseId": "q1", "answers": [{"selected": ["Blue"]}]})),
        rig.run(json!({"toolUseId": "q1", "answers": [{"selected": ["Red"]}]})),
    );
    assert_eq!(
        second,
        Err("An answer is being typed into \"Ava\" already.".into())
    );
    assert_eq!(first, Ok(json!({"answered": true})));
    assert_eq!(
        dialog.submitted(),
        answers(&[("Which color do you prefer?", "Blue")])
    );
}

/* ── desk::answer, the window's typed call ────────────────────── */

#[tokio::test]
async fn desk_answer_types_the_answers_step_by_step_and_refuses_what_does_not_fit() {
    use super::super::desk;
    let given = |selected: &[&str], other: Option<&str>| Answer {
        selected: selected.iter().map(|label| (*label).to_string()).collect(),
        other: other.map(str::to_string),
    };
    let dialog = Dialog::new(vec![color(), pet()], &[]);
    let rig = answer_rig(
        &dialog,
        page_with(Some(pending("q1", vec![color(), pet()])), vec![]),
        AgentId::Claude,
    );
    let refused = desk::answer(
        &rig.port,
        &rig.mobile,
        "shop",
        "a",
        "q1",
        &[given(&["Teal"], None), given(&["Cat"], None)],
    )
    .await;
    assert_eq!(
        refused,
        Err("\"Teal\" is not one of the options of Question 1 (\"Color\").".into())
    );
    let refused = desk::answer(
        &rig.port,
        &rig.mobile,
        "shop",
        "a",
        "q1",
        &[given(&["Red"], None)],
    )
    .await;
    assert_eq!(
        refused,
        Err("Give one answer per question: 2 of them.".into())
    );
    assert!(dialog.keys().is_empty());

    let answered = desk::answer(
        &rig.port,
        &rig.mobile,
        "shop",
        "a",
        "q1",
        &[given(&["Blue"], None), given(&[], Some("A\nparrot"))],
    )
    .await;
    assert_eq!(
        answered,
        Ok(RemoteAnswered {
            answered: true,
            queued: None
        })
    );
    // Blue: two down, Enter; the pet's own words: down past Cat and Dog,
    // pasted, Enter; then the review's Submit.
    assert_eq!(
        dialog.keys(),
        keys(&["down", "down", "enter", "down", "down", "enter", "enter"])
    );
    assert_eq!(
        dialog.submitted(),
        answers(&[
            ("Which color do you prefer?", "Blue"),
            ("Which pet would you adopt?", "A parrot"),
        ])
    );
    assert!(rig.mobile.book().answering.is_empty());
}
