//! Following the session a pane's agent is in (`watchForSession` of
//! `lib/agents.ts`).
//!
//! Agents do not report their session id, so the transcript directory is
//! diffed: whatever file appears that the pane did not already know about
//! belongs to it.
//!
//! This runs for the whole life of the pane, not just at launch. Typing /new
//! or /clear inside an agent abandons the current transcript for a fresh
//! one, and a pane that stopped watching would keep pointing at the old chat,
//! and reopen it on the next resume, which is not the conversation you left.
//!
//! `SessionWatch` is the loop's state with the sleeping taken out: the host
//! asks `due`, runs `hunt_listing` off the UI thread when `begin_round` says
//! a look is worth it, and hands the answer to `round_done`.

use std::collections::HashSet;

use hangar_core::sessions::AgentSession;

use super::agents::Claims;
use crate::state::live::EpochMs;
use crate::state::types::AgentId;

/// Brisk while hunting the first id...
pub const HUNT_MS: u64 = 1500;
/// ...patient once the pane has one.
pub const FOLLOW_MS: u64 = 5000;
/// A pane that looked this many times and found nothing new (one launched
/// with `--resume` never does) is as patient as one that found its session.
/// `FOLLOW_MS` is still well inside `FRESH_MS`, so a late transcript is caught.
pub const EMPTY_ROUNDS_BEFORE_FOLLOW: u32 = 3;

/// The listing a hunt round runs off the UI thread: ids and modification
/// times from the directory alone, no transcript opened, so the labels are
/// empty (the hunt never reads them).
pub fn hunt_listing(cwd: String) -> Vec<AgentSession> {
    hangar_core::sessions::list_session_ids(cwd)
        .into_iter()
        .map(|(id, modified_ms)| AgentSession {
            id,
            label: String::new(),
            modified_ms,
        })
        .collect()
}

/// A transcript has to be warm to be ours. Without this a pane could adopt
/// any file it happens not to know about (one left by another window, say)
/// instead of the conversation that just started under it.
pub const FRESH_MS: u64 = 15_000;

#[derive(Debug)]
pub struct SessionWatch {
    pub agent: AgentId,
    pub cwd: String,
    /// The sessions the pane knew when it launched, and every one it adopted
    /// since, so the next round hunts the one after it.
    known: HashSet<String>,
    interval_ms: u64,
    /// When the next look is due; `None` while one is in flight.
    next_at: Option<EpochMs>,
    /// Set by output, consumed once per round. A pane that printed nothing
    /// since the last look cannot have opened a session, so it is skipped
    /// without touching the disk, and a transcript another pane just created
    /// is far less likely to be mistaken for this one's.
    printed: bool,
    /// Rounds in a row that adopted nothing.
    empty_rounds: u32,
}

impl SessionWatch {
    pub fn new(agent: AgentId, cwd: String, known: HashSet<String>, now: EpochMs) -> SessionWatch {
        SessionWatch {
            agent,
            cwd,
            known,
            interval_ms: HUNT_MS,
            next_at: Some(now + HUNT_MS),
            printed: false,
            empty_rounds: 0,
        }
    }

    /// The pane printed something.
    pub fn note_output(&mut self) {
        self.printed = true;
    }

    pub fn due(&self, now: EpochMs) -> bool {
        self.next_at.is_some_and(|at| at <= now)
    }

    /// Starts a round. `false`: nothing was printed since the last look, so
    /// the disk is left alone and the next round is scheduled.
    pub fn begin_round(&mut self, now: EpochMs) -> bool {
        if !std::mem::take(&mut self.printed) {
            self.next_at = Some(now + self.interval_ms);
            return false;
        }
        self.next_at = None;
        true
    }

    /// Passes a round without listing: the pane's session is named by the
    /// mod (bridged), so whatever the disk says would be thrown away. Nothing
    /// is claimed, nothing is learnt, and the next look is scheduled.
    pub fn skip_round(&mut self, now: EpochMs) {
        self.printed = false;
        self.next_at = Some(now + self.interval_ms);
    }

    /// The listing of a round came back (`None`: it failed, which reads as
    /// nothing found). Answers the session to adopt, if there is one.
    /// `sessions` are sorted most recently touched first, so the first match
    /// is the newest.
    pub fn round_done(
        &mut self,
        sessions: Option<&[AgentSession]>,
        now: EpochMs,
        claims: &mut Claims,
    ) -> Option<String> {
        let fresh = sessions.and_then(|sessions| {
            sessions.iter().find(|session| {
                !self.known.contains(&session.id)
                    && !claims.contains(&session.id)
                    && now.saturating_sub(session.modified_ms) < FRESH_MS
            })
        });
        let found = fresh.map(|session| session.id.clone());
        if found.is_none() {
            self.empty_rounds += 1;
            if self.empty_rounds >= EMPTY_ROUNDS_BEFORE_FOLLOW {
                self.interval_ms = FOLLOW_MS;
            }
        } else {
            self.empty_rounds = 0;
        }
        if let Some(id) = &found {
            claims.claim(id);
            // Remembered as ours, so the next round hunts the one after it.
            self.known.insert(id.clone());
            self.interval_ms = FOLLOW_MS;
        }
        self.next_at = Some(now + self.interval_ms);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, modified_ms: u64) -> AgentSession {
        AgentSession {
            id: id.into(),
            label: id.into(),
            modified_ms,
        }
    }

    fn known(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn watch(known_ids: &[&str]) -> SessionWatch {
        SessionWatch::new(AgentId::Claude, "/work".into(), known(known_ids), 0)
    }

    #[test]
    fn looks_every_second_and_a_half_while_hunting_and_every_five_once_it_found_one() {
        let mut claims = Claims::default();
        let mut watch = watch(&[]);
        assert!(!watch.due(HUNT_MS - 1));
        assert!(watch.due(HUNT_MS));

        watch.note_output();
        assert!(watch.begin_round(HUNT_MS));
        // In flight: not due again until the answer is in.
        assert!(!watch.due(HUNT_MS * 10));
        let found = watch.round_done(Some(&[session("s1", HUNT_MS - 100)]), HUNT_MS, &mut claims);
        assert_eq!(found.as_deref(), Some("s1"));
        assert!(!watch.due(HUNT_MS + FOLLOW_MS - 1));
        assert!(watch.due(HUNT_MS + FOLLOW_MS));
    }

    #[test]
    fn keeps_hunting_briskly_while_it_finds_nothing() {
        let mut claims = Claims::default();
        let mut watch = watch(&[]);
        watch.note_output();
        assert!(watch.begin_round(HUNT_MS));
        assert_eq!(watch.round_done(Some(&[]), HUNT_MS, &mut claims), None);
        assert!(watch.due(HUNT_MS * 2));
        // A listing that failed reads as nothing found.
        watch.note_output();
        assert!(watch.begin_round(HUNT_MS * 2));
        assert_eq!(watch.round_done(None, HUNT_MS * 2, &mut claims), None);
        assert!(watch.due(HUNT_MS * 3));
    }

    #[test]
    fn skips_a_round_when_the_pane_printed_nothing_since_the_last_look() {
        let mut watch = watch(&[]);
        assert!(!watch.begin_round(HUNT_MS));
        assert!(!watch.due(HUNT_MS * 2 - 1));
        assert!(watch.due(HUNT_MS * 2));
        // Output counts for one round only.
        watch.note_output();
        assert!(watch.begin_round(HUNT_MS * 2));
        let mut claims = Claims::default();
        watch.round_done(Some(&[]), HUNT_MS * 2, &mut claims);
        assert!(!watch.begin_round(HUNT_MS * 3));
    }

    #[test]
    fn adopts_the_newest_session_it_did_not_know_and_nobody_claimed() {
        let mut claims = Claims::default();
        claims.claim("taken");
        let mut watch = watch(&["old"]);
        watch.note_output();
        watch.begin_round(HUNT_MS);
        let now = 100_000;
        let sessions = [
            session("taken", now - 100),
            session("new", now - 200),
            session("old", now - 300),
            session("newer-but-after", now - 400),
        ];
        let found = watch.round_done(Some(&sessions), now, &mut claims);
        assert_eq!(found.as_deref(), Some("new"));
        // It is the pane's now, for it and for every other pane.
        assert!(claims.contains("new"));
    }

    #[test]
    fn does_not_adopt_a_transcript_that_is_not_warm() {
        let mut claims = Claims::default();
        let mut watch = watch(&[]);
        watch.note_output();
        watch.begin_round(HUNT_MS);
        let now = 100_000;
        let cold = [session("cold", now - FRESH_MS)];
        assert_eq!(watch.round_done(Some(&cold), now, &mut claims), None);
        watch.note_output();
        watch.begin_round(now + HUNT_MS);
        let warm = [session("warm", now - FRESH_MS + 1 + HUNT_MS)];
        assert_eq!(
            watch
                .round_done(Some(&warm), now + HUNT_MS, &mut claims)
                .as_deref(),
            Some("warm")
        );
    }

    #[test]
    fn follows_a_new_session_after_the_first_one() {
        // /new or /clear inside the agent opens another transcript.
        let mut claims = Claims::default();
        let mut watch = watch(&[]);
        let mut now = HUNT_MS;
        watch.note_output();
        watch.begin_round(now);
        assert_eq!(
            watch
                .round_done(Some(&[session("one", now)]), now, &mut claims)
                .as_deref(),
            Some("one")
        );

        now += FOLLOW_MS;
        watch.note_output();
        assert!(watch.begin_round(now));
        // The first is known now: only the second stands out.
        let both = [session("two", now - 10), session("one", now - 20)];
        assert_eq!(
            watch.round_done(Some(&both), now, &mut claims).as_deref(),
            Some("two")
        );
    }

    #[test]
    fn a_resumed_pane_that_finds_nothing_new_settles_to_the_patient_cadence() {
        let mut claims = Claims::default();
        let mut watch = watch(&["resumed"]);
        let mut now = HUNT_MS;
        for round in 1..=EMPTY_ROUNDS_BEFORE_FOLLOW {
            watch.note_output();
            assert!(watch.begin_round(now));
            let only_known = [session("resumed", now - 10)];
            assert_eq!(watch.round_done(Some(&only_known), now, &mut claims), None);
            if round < EMPTY_ROUNDS_BEFORE_FOLLOW {
                assert!(watch.due(now + HUNT_MS));
                now += HUNT_MS;
            }
        }
        assert!(!watch.due(now + FOLLOW_MS - 1));
        assert!(watch.due(now + FOLLOW_MS));
    }

    #[test]
    fn a_skipped_round_claims_nothing_and_is_rescheduled() {
        let mut watch = watch(&[]);
        watch.note_output();
        watch.skip_round(HUNT_MS);
        // The output was spent on the skipped round; the next one is on time.
        assert!(!watch.due(HUNT_MS + HUNT_MS - 1));
        assert!(watch.due(HUNT_MS * 2));
        assert!(!watch.begin_round(HUNT_MS * 2));
    }
}
