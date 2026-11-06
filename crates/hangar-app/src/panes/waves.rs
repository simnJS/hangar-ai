//! The order resumes go in at a relaunch: a Claude Code resuming a big
//! session costs a few hundred megabytes and a good part of a core while it
//! reads its transcript, and eight of them at once is a peak the person
//! waits through. The panes on screen in the focused window resume at once;
//! the others wait here and go in waves of [`WAVE`], the next wave as soon
//! as the resumes typed before it are under way ([`settling`]: their
//! command ran, and the agent printed its screen and went quiet for
//! [`READY_QUIET`], which a Claude Code does once its transcript is read),
//! [`BUSY_GAP`] after the last one when they printed and print still (an
//! agent at work: what was the least gap before), or [`MAX_GAP`] after it
//! whatever they do. A waiting pane that
//! comes on screen (its window focused, the zoom over it ended, its
//! workspace shown) goes at once.
//!
//! Only resumes wait: a shell or a new agent starts as it always did. A
//! pane that waits loses nothing: its launch is only put off, and the waves
//! always drain.

use std::time::{Duration, Instant};

use crate::state::PaneId;

/// How many waiting resumes go at a time.
pub const WAVE: usize = 2;
/// The most time a wave waits for the last one to be under way.
pub const MAX_GAP: Duration = Duration::from_secs(5);
/// The most time a wave waits for agents that printed and print still.
pub const BUSY_GAP: Duration = Duration::from_secs(2);
/// How long an agent that printed must stay quiet for its start to be over.
pub const READY_QUIET: Duration = Duration::from_millis(500);
/// How often a resume that has printed nothing yet is looked at again.
pub const LOOK: Duration = Duration::from_millis(200);

/// Where a typed resume stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settling {
    /// Under way: it holds no wave back any more.
    Settled,
    /// Its agent printed and prints still: quiet in this long at the soonest.
    Printing(Duration),
    /// Look again in this long.
    In(Duration),
}

/// Whether a typed resume is under way. `checked`: the check of its command
/// answered; `printed`: its agent printed something of its own after the
/// command's echo; `quiet`: the time since the pane last printed.
pub fn settling(checked: bool, printed: bool, quiet: Option<Duration>) -> Settling {
    if !checked || !printed {
        return Settling::In(LOOK);
    }
    match quiet {
        Some(quiet) if quiet >= READY_QUIET => Settling::Settled,
        Some(quiet) => Settling::Printing(READY_QUIET - quiet),
        None => Settling::In(LOOK),
    }
}

/// The resumes waiting their turn, and those typed and not under way yet.
#[derive(Debug, Default)]
pub struct Waves {
    /// The resumes put off, in the order their shells became ready.
    waiting: Vec<PaneId>,
    /// The resumes typed that are not under way yet.
    starting: Vec<PaneId>,
    /// Those of them whose agent printed and prints still.
    printing: Vec<PaneId>,
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

    /// The resumes typed that are not under way yet.
    pub fn starting(&self) -> &[PaneId] {
        &self.starting
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
        self.printing.retain(|id| *id != pane);
        if !self.starting.contains(&pane) {
            self.starting.push(pane);
        }
        self.last = Some(now);
    }

    /// `pane`'s resume is under way ([`settling`]).
    pub fn settled(&mut self, pane: &str) {
        self.starting.retain(|id| id != pane);
        self.printing.retain(|id| id != pane);
    }

    /// `pane`'s agent printed and prints still ([`Settling::Printing`]).
    pub fn printing(&mut self, pane: &str) {
        if self.starting.iter().any(|id| id == pane) && !self.printing.iter().any(|id| id == pane) {
            self.printing.push(pane.to_owned());
        }
    }

    /// `pane` went: it waits for nothing any more.
    pub fn forget(&mut self, pane: &str) {
        self.waiting.retain(|id| id != pane);
        self.settled(pane);
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
    /// time to look again. `settles_in` is the soonest the resumes under
    /// way may be seen settled ([`Settling::In`]).
    pub fn next_look(&self, now: Instant, settles_in: Option<Duration>) -> Option<Duration> {
        if self.waiting.is_empty() {
            return None;
        }
        let left = self.wait_left(now).unwrap_or(Duration::ZERO);
        Some(settles_in.map_or(left, |settles| settles.min(left)))
    }

    /// How long the next wave still has to wait, `None` when it may go: at
    /// once when nothing typed is still starting, [`BUSY_GAP`] after the
    /// last typed when every one starting prints, [`MAX_GAP`] otherwise.
    fn wait_left(&self, now: Instant) -> Option<Duration> {
        if self.starting.is_empty() {
            return None;
        }
        let since = now.saturating_duration_since(self.last?);
        let all_print = self.starting.iter().all(|id| self.printing.contains(id));
        let gap = if all_print { BUSY_GAP } else { MAX_GAP };
        gap.checked_sub(since).filter(|left| !left.is_zero())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<PaneId> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
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
    fn the_panes_on_screen_hold_the_first_wave_until_they_are_under_way() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c", "d", "e"]);
        waves.typed("a".into(), t0);
        waves.typed("b".into(), t0);
        assert!(waves.due(t0 + ms(600)).is_empty(), "a and b still start");
        waves.settled("a");
        assert!(waves.due(t0 + ms(600)).is_empty(), "b still starts");
        waves.settled("b");
        // No timer once they are: the wave goes as soon as they are.
        assert_eq!(waves.due(t0 + ms(600)), ids(&["c", "d"]));
        assert_eq!(waves.starting(), &[] as &[PaneId]);
    }

    #[test]
    fn a_wave_that_never_settles_holds_the_next_one_only_so_long() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c"]);
        waves.typed("a".into(), t0);
        assert!(waves.due(t0 + MAX_GAP - ms(1)).is_empty());
        assert_eq!(waves.due(t0 + MAX_GAP), ids(&["c"]));
    }

    #[test]
    fn each_wave_waits_for_the_one_before_it_and_not_for_older_ones() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c", "d", "e"]);
        waves.typed("a".into(), t0);
        waves.settled("a");
        let first = waves.due(t0 + ms(700));
        assert_eq!(first, ids(&["c", "d"]));
        for pane in &first {
            waves.typed(pane.clone(), t0 + ms(700));
        }
        // c is under way, d not yet: e waits for d.
        waves.settled("c");
        assert!(waves.due(t0 + ms(1400)).is_empty());
        waves.settled("d");
        assert_eq!(waves.due(t0 + ms(1400)), ids(&["e"]));
    }

    #[test]
    fn agents_that_print_still_hold_the_next_wave_as_long_as_before_and_no_longer() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c"]);
        waves.typed("a".into(), t0);
        waves.typed("b".into(), t0);
        waves.printing("a");
        // b has printed nothing yet: the longest gap.
        assert!(waves.due(t0 + BUSY_GAP).is_empty());
        waves.printing("b");
        assert!(waves.due(t0 + BUSY_GAP - ms(1)).is_empty());
        assert_eq!(waves.due(t0 + BUSY_GAP), ids(&["c"]));
        // Typed again (a retry), a pane prints nothing of its new command yet.
        let mut waves = waiting(&["c"]);
        waves.typed("a".into(), t0);
        waves.printing("a");
        waves.typed("a".into(), t0);
        assert!(waves.due(t0 + BUSY_GAP).is_empty());
    }

    #[test]
    fn every_waiting_resume_goes_in_the_end_in_waves_of_two() {
        let mut now = Instant::now();
        let mut waves = waiting(&["a", "b", "c", "d", "e"]);
        let mut went = Vec::new();
        let mut rounds = Vec::new();
        while let Some(look) = waves.next_look(now, None) {
            now += look;
            let wave = waves.due(now);
            assert!(!wave.is_empty(), "a look at its time finds a wave");
            for pane in &wave {
                waves.typed(pane.clone(), now);
            }
            rounds.push(wave.len());
            went.extend(wave);
            // Never settled: the longest gap holds each wave.
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
        assert_eq!(waves.due(t0), ids(&["d"]));
        assert_eq!(waves.next_look(t0, None), None);
    }

    #[test]
    fn the_next_look_is_when_the_wave_may_go_or_a_start_may_settle() {
        let t0 = Instant::now();
        let mut waves = waiting(&["c"]);
        waves.typed("a".into(), t0);
        assert_eq!(waves.next_look(t0, None), Some(MAX_GAP));
        assert_eq!(waves.next_look(t0, Some(LOOK)), Some(LOOK));
        assert_eq!(
            waves.next_look(t0 + MAX_GAP - ms(50), Some(LOOK)),
            Some(ms(50))
        );
        waves.settled("a");
        assert_eq!(waves.next_look(t0 + ms(10), None), Some(Duration::ZERO));
    }

    #[test]
    fn a_resume_is_under_way_once_checked_printed_and_quiet() {
        // Its command not checked yet, or its agent silent: looked at again.
        assert_eq!(settling(false, true, Some(ms(900))), Settling::In(LOOK));
        assert_eq!(settling(true, false, Some(ms(900))), Settling::In(LOOK));
        // Printing still: as soon as the quiet would be long enough.
        assert_eq!(
            settling(true, true, Some(ms(100))),
            Settling::Printing(READY_QUIET - ms(100))
        );
        assert_eq!(settling(true, true, Some(READY_QUIET)), Settling::Settled);
    }
}
