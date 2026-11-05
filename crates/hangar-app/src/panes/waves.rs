//! The order resumes go in at a relaunch: a Claude Code resuming a big
//! session costs a few hundred megabytes and a good part of a core while it
//! reads its transcript, and eight of them at once is a peak the person
//! waits through. The panes on screen in the focused window resume at once;
//! the others wait here and go in waves of [`WAVE`], the next wave once the
//! last one's commands have run (no sooner than [`MIN_GAP`] after it) or
//! [`MAX_GAP`] after it whatever they do. A waiting pane that comes on
//! screen (its window focused, the zoom over it ended, its workspace shown)
//! goes at once.
//!
//! Only resumes wait: a shell or a new agent starts as it always did. A
//! pane that waits loses nothing: its launch is only put off, and the waves
//! always drain.

use std::time::{Duration, Instant};

use crate::state::PaneId;

/// How many waiting resumes go at a time.
pub const WAVE: usize = 2;
/// The least time between two waves, for Claude Code to get through the
/// first reads of its transcript.
pub const MIN_GAP: Duration = Duration::from_secs(2);
/// The most time a wave waits for the last one's commands to run.
pub const MAX_GAP: Duration = Duration::from_secs(5);

/// The resumes waiting their turn, and the last wave still starting.
#[derive(Debug, Default)]
pub struct Waves {
    /// The resumes put off, in the order their shells became ready.
    waiting: Vec<PaneId>,
    /// The resumes typed whose check has not answered yet.
    starting: Vec<PaneId>,
    /// When the last resume was typed.
    last: Option<Instant>,
}

impl Waves {
    /// Whether `pane` waits its turn.
    pub fn is_waiting(&self, pane: &str) -> bool {
        self.waiting.iter().any(|id| id == pane)
    }

    /// Whether anything waits.
    pub fn any_waiting(&self) -> bool {
        !self.waiting.is_empty()
    }

    /// `pane`'s resume is put off.
    pub fn wait(&mut self, pane: PaneId) {
        if !self.is_waiting(&pane) {
            self.waiting.push(pane);
        }
    }

    /// `pane`'s resume was typed, at once or in its wave.
    pub fn typed(&mut self, pane: PaneId, now: Instant) {
        self.waiting.retain(|id| *id != pane);
        if !self.starting.contains(&pane) {
            self.starting.push(pane);
        }
        self.last = Some(now);
    }

    /// The check of `pane`'s command answered: it runs, or it was lost.
    pub fn checked(&mut self, pane: &str) {
        self.starting.retain(|id| id != pane);
    }

    /// `pane` went: it waits for nothing any more.
    pub fn forget(&mut self, pane: &str) {
        self.waiting.retain(|id| id != pane);
        self.starting.retain(|id| id != pane);
    }

    /// The waiting panes now on screen, taken out of the wait: they go at
    /// once.
    pub fn shown(&mut self, on_screen: impl Fn(&PaneId) -> bool) -> Vec<PaneId> {
        let (shown, still): (Vec<_>, Vec<_>) = self.waiting.drain(..).partition(|id| on_screen(id));
        self.waiting = still;
        shown
    }

    /// The next wave, if it is due at `now`, taken out of the wait.
    pub fn due(&mut self, now: Instant) -> Vec<PaneId> {
        if self.waiting.is_empty() || self.wait_left(now).is_some() {
            return Vec::new();
        }
        let n = WAVE.min(self.waiting.len());
        self.waiting.drain(..n).collect()
    }

    /// How long until the next wave may be due, while anything waits: the
    /// time to look again.
    pub fn next_look(&self, now: Instant) -> Option<Duration> {
        if self.waiting.is_empty() {
            return None;
        }
        Some(self.wait_left(now).unwrap_or(Duration::ZERO))
    }

    /// How long the next wave still has to wait, `None` when it may go.
    fn wait_left(&self, now: Instant) -> Option<Duration> {
        let since = now.saturating_duration_since(self.last?);
        let gap = if self.starting.is_empty() {
            MIN_GAP
        } else {
            MAX_GAP
        };
        gap.checked_sub(since).filter(|left| !left.is_zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<PaneId> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn waiting(names: &[&str]) -> Waves {
        let mut waves = Waves::default();
        for name in names {
            waves.wait((*name).to_owned());
        }
        waves
    }

    #[test]
    fn with_nothing_typed_before_the_first_wave_goes_at_once() {
        let now = Instant::now();
        let mut waves = waiting(&["a", "b", "c"]);
        assert_eq!(waves.due(now), ids(&["a", "b"]));
        assert!(waves.is_waiting("c"));
    }

    #[test]
    fn the_panes_on_screen_hold_the_first_wave_until_their_commands_run() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c", "d", "e"]);
        waves.typed("a".into(), t0);
        waves.typed("b".into(), t0);
        assert!(waves.due(t0 + MIN_GAP).is_empty(), "a and b still start");
        waves.checked("a");
        assert!(waves.due(t0 + MIN_GAP).is_empty(), "b still starts");
        waves.checked("b");
        assert!(
            waves.due(t0 + MIN_GAP / 2).is_empty(),
            "too soon after them"
        );
        assert_eq!(waves.due(t0 + MIN_GAP), ids(&["c", "d"]));
    }

    #[test]
    fn a_wave_whose_commands_never_answer_holds_the_next_one_only_so_long() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c"]);
        waves.typed("a".into(), t0);
        assert!(
            waves
                .due(t0 + MAX_GAP - Duration::from_millis(1))
                .is_empty()
        );
        assert_eq!(waves.due(t0 + MAX_GAP), ids(&["c"]));
    }

    #[test]
    fn every_waiting_resume_goes_in_the_end_in_waves_of_two() {
        let mut now = Instant::now();
        let mut waves = waiting(&["a", "b", "c", "d", "e"]);
        let mut went = Vec::new();
        let mut rounds = Vec::new();
        while let Some(look) = waves.next_look(now) {
            now += look;
            let wave = waves.due(now);
            assert!(!wave.is_empty(), "a look at its time finds a wave");
            for pane in &wave {
                waves.typed(pane.clone(), now);
            }
            rounds.push(wave.len());
            went.extend(wave);
            // Never checked: the longest gap holds each wave.
        }
        assert_eq!(went, ids(&["a", "b", "c", "d", "e"]));
        assert_eq!(rounds, vec![2, 2, 1]);
        assert!(!waves.any_waiting());
    }

    #[test]
    fn a_waiting_pane_that_comes_on_screen_leaves_the_wait_at_once() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c", "d", "e"]);
        waves.typed("a".into(), t0);
        let shown = waves.shown(|id| id == "d");
        assert_eq!(shown, ids(&["d"]));
        assert!(!waves.is_waiting("d"));
        assert!(waves.is_waiting("c") && waves.is_waiting("e"));
    }

    #[test]
    fn a_pane_that_goes_waits_for_nothing_and_holds_no_wave() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c", "d"]);
        waves.typed("a".into(), t0);
        waves.forget("a");
        waves.forget("c");
        assert_eq!(waves.due(t0 + MIN_GAP), ids(&["d"]));
        assert_eq!(waves.next_look(t0 + MIN_GAP), None);
    }

    #[test]
    fn the_next_look_is_when_the_wave_may_go() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c"]);
        waves.typed("a".into(), t0);
        assert_eq!(waves.next_look(t0), Some(MAX_GAP));
        waves.checked("a");
        assert_eq!(
            waves.next_look(t0 + Duration::from_secs(1)),
            Some(MIN_GAP - Duration::from_secs(1))
        );
        assert_eq!(waves.next_look(t0 + MAX_GAP), Some(Duration::ZERO));
    }
}
