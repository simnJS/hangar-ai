//! Turns a raw PTY stream into "this pane handed control back" events
//! (`lib/activity.ts`).
//!
//! No CLI agent exposes an "I am done" signal, so this watches the shape of
//! the output instead. Agents write continuously while they work (spinners,
//! elapsed timers, streamed tokens, tool output) and then go completely
//! silent once the prompt is yours again. A silence that follows a long
//! enough burst is therefore a hand-back.
//!
//! The burst threshold is what keeps the noise out: a shell prompt, an echoed
//! keystroke or a `git status` produce a few hundred milliseconds of output
//! and are ignored, while a real turn produces seconds of it.
//!
//! The watcher owns no timer and reads no clock. Output is reported with
//! `push(now)`, and while `is_ticking()` the host calls `poll(now, idle_ms)`
//! about every `TICK_MS`; `poll` runs every check that came due since the
//! last call, each at its own moment, so a host that is late gets the answer
//! the timers of the web app would have given.

use super::EpochMs;

/// Shortest burst that counts as real work rather than terminal noise.
pub const MIN_BUSY_MS: u64 = 2500;

/// How often the silence is checked. Cheap: only runs during a burst.
pub const TICK_MS: u64 = 250;

/// Two hand-backs closer than this are the same one seen twice, typically a
/// bell immediately followed by the prompt going quiet.
pub const DEBOUNCE_MS: u64 = 1500;

/// What the watcher found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// A burst just grew long enough to count as work. Once per burst.
    Busy,
    /// The pane handed control back.
    Settle,
}

#[derive(Debug, Default, Clone)]
pub struct ActivityWatcher {
    /// When the next check is due; `Some` exactly while a burst is followed.
    next_tick: Option<EpochMs>,
    burst_started_at: EpochMs,
    last_output_at: EpochMs,
    /// `None` until the first hand-back, so the first one is never debounced.
    last_settle_at: Option<EpochMs>,
    busy_reported: bool,
}

impl ActivityWatcher {
    /// Whether a burst is being followed, and so whether the host has to
    /// call `poll` every `TICK_MS`. False means no timer needs to run.
    pub fn is_ticking(&self) -> bool {
        self.next_tick.is_some()
    }

    /// Call for every chunk of output; cost is constant per chunk. The first
    /// one of a burst starts it.
    pub fn push(&mut self, now: EpochMs) {
        if self.next_tick.is_none() {
            self.burst_started_at = now;
            self.next_tick = Some(now + TICK_MS);
        }
        self.last_output_at = now;
    }

    /// A bell is an explicit hand-back: it settles now, without waiting.
    pub fn ring(&mut self, now: EpochMs) -> Option<Signal> {
        self.settle(now)
    }

    /// Stops following the burst and forgets that it was reported.
    pub fn dispose(&mut self) {
        self.stop();
    }

    /// Runs the checks due up to `now`. `idle_ms` is read here, on each
    /// check, so changing the setting applies without a restart.
    pub fn poll(&mut self, now: EpochMs, idle_ms: u64) -> Vec<Signal> {
        let mut signals = Vec::new();
        while let Some(due) = self.next_tick {
            if due > now {
                break;
            }
            self.next_tick = Some(due + TICK_MS);
            self.check(due, idle_ms, &mut signals);
        }
        signals
    }

    fn check(&mut self, at: EpochMs, idle_ms: u64, signals: &mut Vec<Signal>) {
        let burst = self.last_output_at.saturating_sub(self.burst_started_at);
        if !self.busy_reported && burst >= MIN_BUSY_MS {
            self.busy_reported = true;
            signals.push(Signal::Busy);
        }
        if at.saturating_sub(self.last_output_at) < idle_ms {
            return;
        }
        // The burst is measured up to its last byte, not to now, so the
        // silence itself never counts as working time.
        if burst >= MIN_BUSY_MS {
            signals.extend(self.settle(at));
        } else {
            self.stop();
        }
    }

    fn settle(&mut self, now: EpochMs) -> Option<Signal> {
        self.stop();
        if self
            .last_settle_at
            .is_some_and(|last| now.saturating_sub(last) < DEBOUNCE_MS)
        {
            return None;
        }
        self.last_settle_at = Some(now);
        self.burst_started_at = 0;
        Some(Signal::Settle)
    }

    fn stop(&mut self) {
        self.next_tick = None;
        self.busy_reported = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE_MS: u64 = 1000;
    /// Any start will do; the watcher only measures differences.
    const START: EpochMs = 1_767_225_600_000;

    /// A virtual clock standing in for the fake timers of the vitest suite:
    /// `advance` moves it and polls the watcher the way a 250 ms timer would.
    struct Sim {
        now: EpochMs,
        idle_ms: u64,
        watcher: ActivityWatcher,
        settled: usize,
        busy: usize,
    }

    impl Sim {
        fn new() -> Sim {
            Sim::with_idle(IDLE_MS)
        }

        fn with_idle(idle_ms: u64) -> Sim {
            Sim {
                now: START,
                idle_ms,
                watcher: ActivityWatcher::default(),
                settled: 0,
                busy: 0,
            }
        }

        fn count(&mut self, signals: impl IntoIterator<Item = Signal>) {
            for signal in signals {
                match signal {
                    Signal::Busy => self.busy += 1,
                    Signal::Settle => self.settled += 1,
                }
            }
        }

        fn push(&mut self) {
            self.watcher.push(self.now);
        }

        fn ring(&mut self) {
            let signal = self.watcher.ring(self.now);
            self.count(signal);
        }

        fn advance(&mut self, ms: u64) {
            self.now += ms;
            let signals = self.watcher.poll(self.now, self.idle_ms);
            self.count(signals);
        }

        /// Output every `every_ms` for `duration_ms`, ending on a last chunk.
        fn output(&mut self, duration_ms: u64, every_ms: u64) {
            let mut elapsed = 0;
            while elapsed < duration_ms {
                self.push();
                self.advance(every_ms);
                elapsed += every_ms;
            }
            self.push();
        }
    }

    #[test]
    fn settles_once_a_long_burst_falls_silent() {
        let mut sim = Sim::new();
        sim.output(4000, 100);

        sim.advance(IDLE_MS - 1);
        assert_eq!(sim.settled, 0);

        // The silence is checked on a 250 ms tick, so it lands within one of them.
        sim.advance(250);
        assert_eq!(sim.settled, 1);

        sim.advance(10_000);
        assert_eq!(sim.settled, 1);
    }

    #[test]
    fn ignores_a_short_burst_the_way_a_prompt_or_an_echoed_key_looks() {
        let mut sim = Sim::new();
        sim.output(1000, 100);
        sim.advance(10_000);
        assert_eq!(sim.settled, 0);
        // And stops ticking once the burst is over.
        assert!(!sim.watcher.is_ticking());
    }

    #[test]
    fn does_not_count_the_silence_itself_as_work() {
        let mut sim = Sim::with_idle(5000);
        // 2 s of output then 5 s of silence: 7 s since the start, 2 s of work.
        sim.output(2000, 100);
        sim.advance(6000);
        assert_eq!(sim.settled, 0);
    }

    #[test]
    fn keeps_waiting_while_output_keeps_coming() {
        let mut sim = Sim::new();
        sim.output(20_000, 900);
        assert_eq!(sim.settled, 0);
        sim.advance(IDLE_MS + 250);
        assert_eq!(sim.settled, 1);
    }

    #[test]
    fn reads_the_idle_delay_on_every_tick() {
        let mut sim = Sim::with_idle(60_000);
        sim.output(4000, 100);
        sim.advance(3000);
        assert_eq!(sim.settled, 0);

        sim.idle_ms = 1000;
        sim.advance(250);
        assert_eq!(sim.settled, 1);
    }

    #[test]
    fn settles_on_a_bell_without_waiting_for_the_silence() {
        let mut sim = Sim::new();
        sim.output(500, 100);
        sim.ring();
        assert_eq!(sim.settled, 1);
        assert!(!sim.watcher.is_ticking());
    }

    #[test]
    fn counts_the_silence_and_a_bell_right_after_it_as_one_hand_back() {
        let mut sim = Sim::new();
        sim.output(4000, 100);
        sim.advance(IDLE_MS + 250);
        assert_eq!(sim.settled, 1);
        // The agent rings as its prompt comes back, just after going quiet.
        sim.ring();
        assert_eq!(sim.settled, 1);
    }

    #[test]
    fn lets_two_bells_through_once_they_are_far_enough_apart() {
        let mut sim = Sim::new();
        sim.ring();
        sim.advance(1000);
        sim.ring();
        assert_eq!(sim.settled, 1);
        sim.advance(600);
        sim.ring();
        assert_eq!(sim.settled, 2);
    }

    #[test]
    fn stops_for_good_once_disposed() {
        let mut sim = Sim::new();
        sim.output(4000, 100);
        sim.watcher.dispose();
        sim.advance(10_000);
        assert_eq!(sim.settled, 0);
        assert!(!sim.watcher.is_ticking());
    }

    mod on_busy {
        use super::*;

        #[test]
        fn fires_once_a_burst_is_long_enough_to_be_work_and_only_once() {
            let mut sim = Sim::new();
            sim.output(2000, 100);
            assert_eq!(sim.busy, 0);

            sim.output(6000, 100);
            assert_eq!(sim.busy, 1);
        }

        #[test]
        fn never_fires_for_a_short_burst() {
            let mut sim = Sim::new();
            sim.output(1500, 100);
            sim.advance(10_000);
            assert_eq!(sim.busy, 0);
        }

        #[test]
        fn fires_again_for_the_next_burst_once_the_last_one_settled() {
            let mut sim = Sim::new();
            sim.output(4000, 100);
            sim.advance(IDLE_MS + 250);
            assert_eq!(sim.settled, 1);

            sim.output(4000, 100);
            assert_eq!(sim.busy, 2);
        }
    }

    #[test]
    fn a_host_that_polls_late_gets_the_answer_the_timers_would_have_given() {
        // One poll, ten seconds after the output stopped, finds the work and
        // the hand-back both, in that order.
        let mut watcher = ActivityWatcher::default();
        for offset in (0..4000).step_by(100) {
            watcher.push(START + offset);
        }
        let signals = watcher.poll(START + 14_000, IDLE_MS);
        assert_eq!(signals, [Signal::Busy, Signal::Settle]);
        assert!(!watcher.is_ticking());
    }
}
