//! Everything a pane knows about the agent in it, beyond the four-way state
//! of `agent_state` (`lib/paneInsight.ts`): what the inspector, the
//! toolbar's activity line and the palette show about the focused pane.
//!
//! The pane is the only writer (it is the one hearing from the hangar-bridge
//! mod and reading the transcripts) and anything else only reads.

use std::collections::HashMap;
use std::sync::LazyLock;

use hangar_i18n::{Translator, args};

use crate::state::live::{EpochMs, ExitWhat, PaneEvent, PaneEventDetail, PaneInsight, TurnOutcome};
use crate::state::types::PaneId;

/// Enough history to read back over a coffee, not a transcript.
pub const MAX_EVENTS: usize = 40;
pub const MAX_EDITED_FILES: usize = 50;

/// The insight of a pane that never reported (`EMPTY_INSIGHT`).
pub static EMPTY_INSIGHT: LazyLock<PaneInsight> = LazyLock::new(PaneInsight::default);

/// Who a queued prompt is from, as shown: `None` for the person's own, the
/// sending agent's pane name, or Hangar for its own events.
pub fn queued_by(from: Option<&str>) -> Option<String> {
    let from = from.filter(|from| !from.is_empty())?;
    Some(if from == "hangar" { "Hangar" } else { from }.to_string())
}

/// The line the inspector and the toolbar show for an event.
pub fn pane_event_text(event: &PaneEventDetail, t: Translator) -> String {
    use hangar_i18n::Key;
    match event {
        PaneEventDetail::Session { id, resumed } => {
            if *resumed {
                t.t_with(Key::PaneEventResumed, &args!("id" => id.as_str()))
                    .into_owned()
            } else {
                t.t(Key::PaneEventNewSession).into_owned()
            }
        }
        PaneEventDetail::TurnStart => t.t(Key::PaneEventTurnStart).into_owned(),
        PaneEventDetail::TurnEnd { outcome, answer } => {
            if *outcome == TurnOutcome::Aborted {
                return t.t(Key::PaneEventAborted).into_owned();
            }
            // The answer's first line says more than "Finished" ever could.
            match answer.as_deref().filter(|answer| !answer.is_empty()) {
                Some(answer) => answer.to_string(),
                None if *outcome == TurnOutcome::Answer => t.t(Key::PaneEventTurnEnd).into_owned(),
                None => t.t(Key::PaneEventFailed).into_owned(),
            }
        }
        PaneEventDetail::Permission { tool } => {
            match tool.as_deref().filter(|tool| !tool.is_empty()) {
                Some(tool) => t
                    .t_with(Key::PaneEventPermission, &args!("tool" => tool))
                    .into_owned(),
                None => t.t(Key::PaneEventPermissionAny).into_owned(),
            }
        }
        PaneEventDetail::Edit { file, overlap } => {
            match overlap.as_deref().filter(|overlap| !overlap.is_empty()) {
                Some(name) => t
                    .t_with(
                        Key::PaneEventEditShared,
                        &args!("file" => file.as_str(), "name" => name),
                    )
                    .into_owned(),
                None => t
                    .t_with(Key::PaneEventEdit, &args!("file" => file.as_str()))
                    .into_owned(),
            }
        }
        PaneEventDetail::Queued { text, from } => match queued_by(from.as_deref()) {
            Some(name) => t
                .t_with(
                    Key::PaneEventQueuedBy,
                    &args!("name" => name.as_str(), "text" => text.as_str()),
                )
                .into_owned(),
            None => t
                .t_with(Key::PaneEventQueued, &args!("text" => text.as_str()))
                .into_owned(),
        },
        PaneEventDetail::Dropped { text } => t
            .t_with(Key::PaneEventDropped, &args!("text" => text.as_str()))
            .into_owned(),
        PaneEventDetail::Exit { what } => t
            .t(match what {
                ExitWhat::Agent => Key::PaneEventEnd,
                ExitWhat::Shell => Key::PaneEventExit,
            })
            .into_owned(),
    }
}

/// The insight of every pane that reported something.
#[derive(Debug, Default)]
pub struct Insights {
    panes: HashMap<PaneId, PaneInsight>,
    /// Bumped by every change that was one, so a reader can cache on it (and
    /// a test can tell that nothing happened).
    version: u64,
}

impl Insights {
    #[cfg(test)]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The pane's insight, or the empty one for a pane that never reported.
    pub fn get(&self, pane: &str) -> &PaneInsight {
        self.panes.get(pane).unwrap_or(&EMPTY_INSIGHT)
    }

    #[cfg(test)]
    pub fn contains(&self, pane: &str) -> bool {
        self.panes.contains_key(pane)
    }

    /// Edits what changed. An edit that changes nothing notifies nobody, so
    /// the pane can report on every event without a reader redrawing for it.
    /// Whether anything changed.
    pub fn update(&mut self, pane: &str, edit: impl FnOnce(&mut PaneInsight)) -> bool {
        let before = self.get(pane);
        let mut next = before.clone();
        edit(&mut next);
        if next == *before {
            return false;
        }
        self.panes.insert(pane.to_string(), next);
        self.version += 1;
        true
    }

    /// Adds a line to the pane's history, newest first.
    pub fn record_event(&mut self, pane: &str, detail: PaneEventDetail, at: EpochMs) {
        let entry = self.panes.entry(pane.to_string()).or_default();
        entry.events.insert(0, PaneEvent { at, detail });
        entry.events.truncate(MAX_EVENTS);
        self.version += 1;
    }

    /// Moves a file to the top of the pane's edited files, listing it once.
    pub fn record_edited_file(&mut self, pane: &str, file: &str) {
        if self.get(pane).edited_files.first().map(String::as_str) == Some(file) {
            return;
        }
        let entry = self.panes.entry(pane.to_string()).or_default();
        entry.edited_files.retain(|known| known != file);
        entry.edited_files.insert(0, file.to_string());
        entry.edited_files.truncate(MAX_EDITED_FILES);
        self.version += 1;
    }

    /// Forgets a pane: it closed, or restarted under a new id. Whether it
    /// knew it.
    pub fn clear(&mut self, pane: &str) -> bool {
        let known = self.panes.remove(pane).is_some();
        if known {
            self.version += 1;
        }
        known
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::live::ContextGauge;
    use hangar_i18n::{Key, Locale};

    fn en() -> Translator {
        Translator::new(Locale::En)
    }

    fn text(event: PaneEventDetail) -> String {
        pane_event_text(&event, en())
    }

    fn ends(outcome: TurnOutcome, answer: Option<&str>) -> String {
        text(PaneEventDetail::TurnEnd {
            outcome,
            answer: answer.map(str::to_string),
        })
    }

    /// The dictionary's own sentence for a key and its parameters, so a test
    /// reads which sentence was picked rather than repeating its wording.
    fn said(key: Key, args: &[(&str, hangar_i18n::Arg)]) -> String {
        en().t_with(key, args).into_owned()
    }

    #[test]
    fn words_a_session_as_new_or_resumed() {
        assert_eq!(
            text(PaneEventDetail::Session {
                id: "01J8Z4".into(),
                resumed: false
            }),
            en().t(Key::PaneEventNewSession)
        );
        assert_eq!(
            text(PaneEventDetail::Session {
                id: "01J8Z4".into(),
                resumed: true
            }),
            said(Key::PaneEventResumed, &args!("id" => "01J8Z4"))
        );
    }

    #[test]
    fn shows_a_turns_answer_or_how_it_ended_when_there_is_none() {
        assert_eq!(
            text(PaneEventDetail::TurnStart),
            en().t(Key::PaneEventTurnStart)
        );
        assert_eq!(ends(TurnOutcome::Answer, Some("Tests pass")), "Tests pass");
        assert_eq!(
            ends(TurnOutcome::Answer, None),
            en().t(Key::PaneEventTurnEnd)
        );
        assert_eq!(
            ends(TurnOutcome::Failed, None),
            en().t(Key::PaneEventFailed)
        );
        assert_eq!(
            ends(TurnOutcome::Failed, Some("Rate limited")),
            "Rate limited"
        );
        // An interrupted turn says so, whatever it had written by then.
        assert_eq!(
            ends(TurnOutcome::Aborted, Some("Half an answer")),
            en().t(Key::PaneEventAborted)
        );
    }

    #[test]
    fn names_what_a_permission_is_for() {
        let ask = |tool: Option<&str>| {
            text(PaneEventDetail::Permission {
                tool: tool.map(str::to_string),
            })
        };
        assert_eq!(
            ask(Some("Edit")),
            said(Key::PaneEventPermission, &args!("tool" => "Edit"))
        );
        assert_eq!(ask(None), en().t(Key::PaneEventPermissionAny));
    }

    #[test]
    fn says_when_an_edit_followed_another_panes_on_the_same_file() {
        assert_eq!(
            text(PaneEventDetail::Edit {
                file: "NOTES.md".into(),
                overlap: Some("Ava".into())
            }),
            said(
                Key::PaneEventEditShared,
                &args!("file" => "NOTES.md", "name" => "Ava")
            )
        );
        assert_eq!(
            text(PaneEventDetail::Edit {
                file: "NOTES.md".into(),
                overlap: None
            }),
            said(Key::PaneEventEdit, &args!("file" => "NOTES.md"))
        );
    }

    #[test]
    fn words_the_rest_with_their_one_parameter() {
        let queued = |text: &str, from: Option<&str>| {
            self::text(PaneEventDetail::Queued {
                text: text.into(),
                from: from.map(str::to_string),
            })
        };
        assert_eq!(
            text(PaneEventDetail::Edit {
                file: "i18n.ts".into(),
                overlap: None
            }),
            said(Key::PaneEventEdit, &args!("file" => "i18n.ts"))
        );
        assert_eq!(
            queued("run the tests", None),
            said(Key::PaneEventQueued, &args!("text" => "run the tests"))
        );
        assert_eq!(
            queued("task done", Some("hangar")),
            said(
                Key::PaneEventQueuedBy,
                &args!("name" => "Hangar", "text" => "task done")
            )
        );
        assert_eq!(
            queued("review it", Some("Ava")),
            said(
                Key::PaneEventQueuedBy,
                &args!("name" => "Ava", "text" => "review it")
            )
        );
        assert_eq!(
            text(PaneEventDetail::Dropped {
                text: "run the tests".into()
            }),
            said(Key::PaneEventDropped, &args!("text" => "run the tests"))
        );
        assert_eq!(
            text(PaneEventDetail::Exit {
                what: ExitWhat::Agent
            }),
            en().t(Key::PaneEventEnd)
        );
        assert_eq!(
            text(PaneEventDetail::Exit {
                what: ExitWhat::Shell
            }),
            en().t(Key::PaneEventExit)
        );
    }

    #[test]
    fn words_an_event_in_the_language_of_the_moment_it_is_shown() {
        let event = PaneEventDetail::Exit {
            what: ExitWhat::Shell,
        };
        let fr = pane_event_text(&event, Translator::new(Locale::Fr));
        assert_ne!(fr, pane_event_text(&event, en()));
    }

    #[test]
    fn names_who_queued_a_prompt_and_nobody_for_the_persons_own() {
        assert_eq!(queued_by(None), None);
        assert_eq!(queued_by(Some("")), None);
        assert_eq!(queued_by(Some("Ava")).as_deref(), Some("Ava"));
        assert_eq!(queued_by(Some("hangar")).as_deref(), Some("Hangar"));
    }

    #[test]
    fn is_empty_for_a_pane_that_never_reported() {
        let insights = Insights::default();
        assert_eq!(insights.get("nobody"), &*EMPTY_INSIGHT);
        assert!(!insights.contains("nobody"));
    }

    #[test]
    fn notifies_nobody_when_a_patch_changes_nothing() {
        let mut insights = Insights::default();
        let set_model = |i: &mut PaneInsight| {
            i.model = Some("claude-opus-5".into());
            i.cost_usd = Some(1.2);
        };
        assert!(insights.update("a", set_model));
        let version = insights.version();
        let before = insights.get("a").clone();
        assert!(!insights.update("a", set_model));
        assert_eq!(insights.version(), version);
        assert_eq!(insights.get("a"), &before);

        let gauge = |i: &mut PaneInsight| {
            i.context = Some(ContextGauge {
                pct: 42.0,
                used_tokens: 84_000,
                window: 200_000,
                model: None,
            });
        };
        assert!(insights.update("a", gauge));
        let with_gauge = insights.version();
        assert!(!insights.update("a", gauge));
        assert_eq!(insights.version(), with_gauge);
        assert!(insights.clear("a"));
    }

    #[test]
    fn an_edit_that_changes_nothing_on_a_new_pane_creates_nothing() {
        let mut insights = Insights::default();
        assert!(!insights.update("a", |_| {}));
        assert!(!insights.contains("a"));
        assert_eq!(insights.version(), 0);
    }

    #[test]
    fn lists_events_newest_first_and_caps_the_history() {
        let mut insights = Insights::default();
        for i in 0..MAX_EVENTS + 5 {
            insights.record_event(
                "b",
                PaneEventDetail::Edit {
                    file: format!("e{i}"),
                    overlap: None,
                },
                i as u64,
            );
        }
        let events = &insights.get("b").events;
        assert_eq!(events.len(), MAX_EVENTS);
        assert_eq!(
            events[0],
            PaneEvent {
                at: (MAX_EVENTS + 4) as u64,
                detail: PaneEventDetail::Edit {
                    file: format!("e{}", MAX_EVENTS + 4),
                    overlap: None
                }
            }
        );
        assert_eq!(events.last().unwrap().at, 5);
    }

    #[test]
    fn keeps_what_happened_not_a_sentence_about_it() {
        let mut insights = Insights::default();
        insights.record_event(
            "d",
            PaneEventDetail::TurnEnd {
                outcome: TurnOutcome::Answer,
                answer: Some("Done".into()),
            },
            1,
        );
        let event = &insights.get("d").events[0];
        assert_eq!(event.detail.kind(), "turn-end");
        assert_eq!(pane_event_text(&event.detail, en()), "Done");
    }

    #[test]
    fn lists_an_edited_file_once_at_the_top() {
        let mut insights = Insights::default();
        insights.record_edited_file("c", "src/a.ts");
        insights.record_edited_file("c", "src/b.ts");
        insights.record_edited_file("c", "src/a.ts");
        assert_eq!(insights.get("c").edited_files, ["src/a.ts", "src/b.ts"]);
        // Again the same file on top: nothing happened.
        let version = insights.version();
        insights.record_edited_file("c", "src/a.ts");
        assert_eq!(insights.version(), version);
        assert!(insights.clear("c"));
        assert_eq!(insights.get("c"), &*EMPTY_INSIGHT);
    }

    #[test]
    fn caps_the_edited_files() {
        let mut insights = Insights::default();
        for i in 0..MAX_EDITED_FILES + 3 {
            insights.record_edited_file("c", &format!("f{i}"));
        }
        let files = &insights.get("c").edited_files;
        assert_eq!(files.len(), MAX_EDITED_FILES);
        assert_eq!(files[0], format!("f{}", MAX_EDITED_FILES + 2));
    }
}
