//! Where a pane's agent stands, from what it prints and from what its
//! hangar-bridge mod reports (TerminalPane.tsx's `onBridgeEvent`,
//! `handBack`, `announce`, the trust look, and `lib/activity.ts`; inventory
//! §4/1.6).
//!
//! Everything here is a state machine over a pane's runtime and a clock
//! handed in, so it is tested without a terminal or a timer; the controller
//! turns what it returns into notifications and store writes.
//!
//! The output watcher duplicates `lib/activity.ts`, which P4 ports with its
//! suite (`panes_logic::activity`): a pane has to know when an agent without
//! the mod handed back, and the two were written at the same time. The
//! constants and the rules are the TS ones; switch to P4's once it lands.

use std::time::{Duration, Instant};

use crate::state::AgentActivity;
use crate::state::live::{BridgeEvent, ContextGauge, TurnPhase};

/// Shortest burst that counts as work rather than terminal noise.
pub const MIN_BUSY: Duration = Duration::from_millis(2500);
/// How often the silence is checked while a burst runs.
pub const TICK: Duration = Duration::from_millis(250);
/// Two hand-backs closer than this are the same one seen twice: a bell
/// followed by the prompt going quiet.
pub const DEBOUNCE: Duration = Duration::from_millis(1500);
/// How long after the last output the screen is read for Claude Code's
/// question about trusting a folder.
pub const TRUST_LOOK: Duration = Duration::from_millis(600);

/// What the output watcher says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watch {
    /// A burst grew long enough to count as work. Once per burst.
    Busy,
    /// A long enough burst fell silent: a hand-back.
    Settle,
}

/// Turns a raw output stream into "this pane handed control back"
/// (`createActivityWatcher`). Agents write continuously while they work and
/// fall silent once the prompt is yours; a silence after a long enough burst
/// is a hand-back.
#[derive(Debug, Clone, Default)]
pub struct ActivityWatcher {
    /// When the current burst started; `None` while no ticker runs.
    burst_started: Option<Instant>,
    last_output: Option<Instant>,
    last_settle: Option<Instant>,
    busy_reported: bool,
}

impl ActivityWatcher {
    /// A chunk of output arrived.
    pub fn push(&mut self, now: Instant) {
        if self.burst_started.is_none() {
            self.burst_started = Some(now);
        }
        self.last_output = Some(now);
    }

    /// Whether a burst is being watched, and the window has to tick.
    pub fn ticking(&self) -> bool {
        self.burst_started.is_some()
    }

    /// The 250 ms check of the TS ticker. `idle` is the setting
    /// (`notifyIdleMs`), read each time so a change applies at once.
    pub fn tick(&mut self, now: Instant, idle: Duration) -> Vec<Watch> {
        let (Some(started), Some(last)) = (self.burst_started, self.last_output) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // The burst is measured up to its last byte, not to now, so the
        // silence itself never counts as working time.
        let worked = last.saturating_duration_since(started) >= MIN_BUSY;
        if !self.busy_reported && worked {
            self.busy_reported = true;
            out.push(Watch::Busy);
        }
        if now.saturating_duration_since(last) < idle {
            return out;
        }
        if worked {
            out.extend(self.settle(now));
        } else {
            self.stop();
        }
        out
    }

    /// A bell is the agent asking for you outright: it settles now.
    pub fn ring(&mut self, now: Instant) -> Option<Watch> {
        self.settle(now)
    }

    fn stop(&mut self) {
        self.burst_started = None;
        self.busy_reported = false;
    }

    fn settle(&mut self, now: Instant) -> Option<Watch> {
        self.stop();
        if self
            .last_settle
            .is_some_and(|at| now.saturating_duration_since(at) < DEBOUNCE)
        {
            return None;
        }
        self.last_settle = Some(now);
        Some(Watch::Settle)
    }
}

/// What a pane's agent state is made of: TerminalPane's `activity`,
/// `bridged` and `bridgeContext`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentWatch {
    pub activity: AgentActivity,
    /// The pane's Claude Code loaded the mod: its turns, dialogs and usage
    /// are reported rather than guessed, and the guesses stand down.
    pub bridged: bool,
    /// The context as the mod reports it.
    pub gauge: Option<ContextGauge>,
    /// The model the mod's hello named.
    model: Option<String>,
    /// The conversation the mod last said the pane is in.
    reported_session: Option<String>,
    /// Claude Code is asking whether to trust the folder.
    pub trust_asked: bool,
}

/// Whose words an announce carries, before translation.
#[derive(Debug, Clone, PartialEq)]
pub enum Words {
    Text(String),
    Settled,
    Aborted,
    Permission(String),
    Dropped(String),
    TrustFolder,
}

/// The same effects with the words still to be translated.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    HandBack(Words),
    Announce(Words),
    Session(String),
}

/// The activity a pane is left in when its mod says hello: a session that
/// just started is not busy yet (`activityAtHello`).
pub fn activity_at_hello(activity: AgentActivity) -> AgentActivity {
    if activity == AgentActivity::Working {
        AgentActivity::Idle
    } else {
        activity
    }
}

impl AgentWatch {
    /// A report of the pane's mod (`onBridgeEvent`). `stored` is the session
    /// id the pane has saved, to tell a resume from a new conversation.
    pub fn bridge(&mut self, event: &BridgeEvent, stored: Option<&str>) -> Vec<Said> {
        if !matches!(event, BridgeEvent::End { .. }) {
            self.bridged = true;
        }
        let mut out = Vec::new();
        match event {
            BridgeEvent::Hello {
                session_id, model, ..
            } => {
                self.activity = activity_at_hello(self.activity);
                self.model = (!model.is_empty()).then(|| model.clone());
                self.reported_session = Some(session_id.clone());
                out.extend(capture(session_id, stored));
            }
            BridgeEvent::Session { session_id, .. } => {
                // /clear, /resume and a branch move the pane to another
                // conversation; the first start repeats what the hello said.
                if self
                    .reported_session
                    .as_deref()
                    .is_some_and(|known| known != session_id)
                {
                    self.reported_session = Some(session_id.clone());
                }
                out.extend(capture(session_id, stored));
            }
            BridgeEvent::Turn(TurnPhase::Start) => self.activity = AgentActivity::Working,
            BridgeEvent::Turn(TurnPhase::End { reason, answer, .. }) => {
                let words = if !answer.is_empty() {
                    Words::Text(answer.clone())
                } else if reason == "aborted" {
                    Words::Aborted
                } else {
                    Words::Settled
                };
                out.push(Said::HandBack(words));
            }
            BridgeEvent::Permission { waiting, tool, .. } => {
                if *waiting {
                    self.activity = AgentActivity::Waiting;
                    out.push(Said::Announce(Words::Permission(
                        tool.clone().unwrap_or_default(),
                    )));
                } else if self.activity == AgentActivity::Waiting {
                    self.activity = AgentActivity::Working;
                }
            }
            BridgeEvent::Usage {
                context,
                rate_limits,
                ..
            } => {
                let pct = context.percent.or_else(|| {
                    context
                        .tokens
                        .filter(|_| context.window > 0)
                        .map(|tokens| tokens as f64 / context.window as f64 * 100.0)
                });
                self.gauge = pct.map(|pct| {
                    let pct = pct.round().clamp(0.0, 100.0);
                    ContextGauge {
                        pct,
                        used_tokens: context.tokens.unwrap_or_else(|| {
                            (pct / 100.0 * context.window as f64).round() as u64
                        }),
                        window: context.window,
                        model: self.model.clone(),
                    }
                });
                // The plan's limits are P4's store (`setPlanLimits`), which
                // reads the same report.
                let _ = rate_limits;
            }
            BridgeEvent::Dropped { text, .. } => {
                out.push(Said::Announce(Words::Dropped(text.clone())));
            }
            BridgeEvent::End { .. } => {
                // Claude Code exited; what runs in the pane next is a shell.
                self.bridged = false;
                self.gauge = None;
                self.activity = AgentActivity::Idle;
                self.reported_session = None;
            }
            // Edits are the inspector's and P4's (paneInsight, overlaps);
            // tool phases were ignored by the pane.
            BridgeEvent::Edit { .. } | BridgeEvent::Tool(_) => {}
        }
        out
    }

    /// The output watcher's verdicts, for a pane the mod does not report on.
    pub fn watched(&mut self, verdict: Watch) -> Option<Said> {
        if self.bridged {
            return None;
        }
        match verdict {
            Watch::Busy => {
                self.activity = AgentActivity::Working;
                None
            }
            Watch::Settle if self.trust_asked => None,
            Watch::Settle => Some(Said::HandBack(Words::Settled)),
        }
    }

    /// `OSC 9` and `OSC 777`: the agent wrote the message itself. A
    /// notification whose text starts with a number is ConEmu's progress or
    /// working directory (`9;4;…`, `9;9;…`), not one for a person.
    pub fn notification(&mut self, title: Option<&str>, body: &str) -> Option<Said> {
        if self.bridged || starts_with_code(body) {
            return None;
        }
        let words = [body.trim(), title.unwrap_or_default().trim()]
            .into_iter()
            .find(|text| !text.is_empty())
            .map_or(Words::Settled, |text| Words::Text(text.to_owned()));
        Some(Said::HandBack(words))
    }

    /// What the screen says a moment after the last output: Claude Code's
    /// question about trusting a folder comes before its session exists, so
    /// no report of the mod can announce it.
    pub fn trust(&mut self, asking: bool) -> Option<Said> {
        if self.bridged {
            return None;
        }
        if asking && !self.trust_asked {
            self.trust_asked = true;
            self.activity = AgentActivity::Waiting;
            return Some(Said::Announce(Words::TrustFolder));
        }
        if !asking && self.trust_asked {
            self.trust_asked = false;
            if self.activity == AgentActivity::Waiting {
                self.activity = AgentActivity::Working;
            }
        }
        None
    }

    /// A key pressed in the pane: you taking it back, or answering what it
    /// was waiting on.
    pub fn key(&mut self) {
        self.activity = match self.activity {
            AgentActivity::Yours => AgentActivity::Idle,
            AgentActivity::Waiting => AgentActivity::Working,
            other => other,
        };
    }

    /// The shell ended: a Claude Code killed mid-turn never says goodbye.
    pub fn exited(&mut self) {
        self.bridged = false;
        self.activity = AgentActivity::Idle;
    }

    /// `handBack`'s state: `yours`, or `idle` if the person is on the pane.
    pub fn hand_back(&mut self, looking: bool) {
        self.activity = if looking {
            AgentActivity::Idle
        } else {
            AgentActivity::Yours
        };
    }
}

/// The session the mod named, unless it is the stored one already or not a
/// plain id: it is typed on the next launch after `--resume`.
fn capture(session: &str, stored: Option<&str>) -> Option<Said> {
    let plain = !session.is_empty()
        && session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    (plain && Some(session) != stored).then(|| Said::Session(session.to_owned()))
}

fn starts_with_code(body: &str) -> bool {
    let digits = body.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && matches!(body[digits..].chars().next(), None | Some(';'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::live::UsageContext;

    const IDLE: Duration = Duration::from_millis(1000);

    /// Output every 100 ms for `ms`, ending on a last chunk, with the
    /// watcher's 250 ms tick running as it would.
    fn output(w: &mut ActivityWatcher, start: Instant, ms: u64) -> (Instant, Vec<Watch>) {
        let mut seen = Vec::new();
        let mut elapsed = 0;
        while elapsed < ms {
            if elapsed % 100 == 0 {
                w.push(start + Duration::from_millis(elapsed));
            }
            if elapsed % 250 == 0 && elapsed > 0 {
                seen.extend(w.tick(start + Duration::from_millis(elapsed), IDLE));
            }
            elapsed += 50;
        }
        let end = start + Duration::from_millis(ms);
        w.push(end);
        (end, seen)
    }

    fn run(w: &mut ActivityWatcher, from: Instant, ms: u64) -> Vec<Watch> {
        let mut seen = Vec::new();
        let mut at = from;
        let end = from + Duration::from_millis(ms);
        while at < end {
            at += TICK;
            seen.extend(w.tick(at, IDLE));
        }
        seen
    }

    #[test]
    fn a_long_burst_that_falls_silent_settles_once() {
        let mut w = ActivityWatcher::default();
        let (end, early) = output(&mut w, Instant::now(), 4000);
        assert!(!early.contains(&Watch::Settle));
        assert!(early.contains(&Watch::Busy));
        let later = run(&mut w, end, 10_000);
        assert_eq!(later.iter().filter(|v| **v == Watch::Settle).count(), 1);
        assert!(!w.ticking());
    }

    #[test]
    fn a_short_burst_is_noise() {
        let mut w = ActivityWatcher::default();
        let (end, early) = output(&mut w, Instant::now(), 1000);
        assert!(early.is_empty());
        assert!(run(&mut w, end, 10_000).is_empty());
        // And it stops ticking once the burst is over.
        assert!(!w.ticking());
    }

    #[test]
    fn a_bell_settles_at_once_but_not_twice_in_a_row() {
        let mut w = ActivityWatcher::default();
        let now = Instant::now();
        w.push(now);
        assert_eq!(w.ring(now), Some(Watch::Settle));
        assert_eq!(w.ring(now + Duration::from_millis(500)), None);
        assert_eq!(
            w.ring(now + Duration::from_millis(2000)),
            Some(Watch::Settle)
        );
    }

    #[test]
    fn the_mods_hello_stands_the_guesses_down_and_names_the_session() {
        let mut a = AgentWatch {
            activity: AgentActivity::Working,
            ..AgentWatch::default()
        };
        let said = a.bridge(
            &BridgeEvent::Hello {
                session_id: "abc-1".into(),
                model: "opus".into(),
                version: "2".into(),
            },
            None,
        );
        assert!(a.bridged);
        assert_eq!(a.activity, AgentActivity::Idle);
        assert_eq!(said, [Said::Session("abc-1".into())]);
        // The stored session is not captured again; a strange id never is.
        assert!(
            a.bridge(
                &BridgeEvent::Session {
                    session_id: "abc-1".into(),
                    source: "startup".into()
                },
                Some("abc-1")
            )
            .is_empty()
        );
        assert!(
            a.bridge(
                &BridgeEvent::Session {
                    session_id: "a b".into(),
                    source: "startup".into()
                },
                None
            )
            .is_empty()
        );
        assert_eq!(a.watched(Watch::Settle), None);
    }

    #[test]
    fn a_turn_end_hands_back_with_the_answer_or_the_reason() {
        let mut a = AgentWatch::default();
        let end = |reason: &str, answer: &str| {
            BridgeEvent::Turn(TurnPhase::End {
                reason: reason.into(),
                duration_ms: 10,
                answer: answer.into(),
            })
        };
        assert_eq!(a.bridge(&BridgeEvent::Turn(TurnPhase::Start), None), []);
        assert_eq!(a.activity, AgentActivity::Working);
        assert_eq!(
            a.bridge(&end("answer", "Done."), None),
            [Said::HandBack(Words::Text("Done.".into()))]
        );
        assert_eq!(
            a.bridge(&end("aborted", ""), None),
            [Said::HandBack(Words::Aborted)]
        );
        assert_eq!(
            a.bridge(&end("error", ""), None),
            [Said::HandBack(Words::Settled)]
        );
    }

    #[test]
    fn a_permission_waits_and_its_answer_resumes_the_work() {
        let mut a = AgentWatch::default();
        let ask = |waiting| BridgeEvent::Permission {
            waiting,
            tool: Some("Bash".into()),
            input: None,
        };
        assert_eq!(
            a.bridge(&ask(true), None),
            [Said::Announce(Words::Permission("Bash".into()))]
        );
        assert_eq!(a.activity, AgentActivity::Waiting);
        a.bridge(&ask(false), None);
        assert_eq!(a.activity, AgentActivity::Working);
    }

    #[test]
    fn the_usage_report_fills_the_gauge() {
        let mut a = AgentWatch::default();
        a.bridge(
            &BridgeEvent::Hello {
                session_id: "s".into(),
                model: "opus".into(),
                version: "1".into(),
            },
            None,
        );
        let usage = |tokens, percent| BridgeEvent::Usage {
            context: UsageContext {
                tokens,
                window: 200_000,
                percent,
            },
            rate_limits: vec![],
            cost_usd: None,
        };
        a.bridge(&usage(Some(50_000), None), None);
        let gauge = a.gauge.clone().unwrap();
        assert_eq!(
            (gauge.pct, gauge.used_tokens, gauge.model.as_deref()),
            (25.0, 50_000, Some("opus"))
        );
        a.bridge(&usage(None, Some(130.0)), None);
        assert_eq!(a.gauge.as_ref().unwrap().pct, 100.0);
        a.bridge(&usage(None, None), None);
        assert_eq!(a.gauge, None);
        a.bridge(
            &BridgeEvent::End {
                reason: "exit".into(),
            },
            None,
        );
        assert!(!a.bridged);
    }

    #[test]
    fn a_notification_that_starts_with_a_code_is_not_for_a_person() {
        let mut a = AgentWatch::default();
        assert_eq!(a.notification(None, "4;3;0"), None);
        assert_eq!(a.notification(None, "9"), None);
        assert_eq!(
            a.notification(None, " Build done "),
            Some(Said::HandBack(Words::Text("Build done".into())))
        );
        assert_eq!(
            a.notification(Some("Claude"), ""),
            Some(Said::HandBack(Words::Text("Claude".into())))
        );
        assert_eq!(
            a.notification(None, ""),
            Some(Said::HandBack(Words::Settled))
        );
    }

    #[test]
    fn the_trust_question_waits_and_holds_the_settle_back() {
        let mut a = AgentWatch::default();
        assert_eq!(a.trust(true), Some(Said::Announce(Words::TrustFolder)));
        assert_eq!(a.activity, AgentActivity::Waiting);
        assert_eq!(a.trust(true), None);
        assert_eq!(a.watched(Watch::Settle), None);
        assert_eq!(a.trust(false), None);
        assert_eq!(a.activity, AgentActivity::Working);
    }

    #[test]
    fn a_key_takes_the_turn_back() {
        let mut a = AgentWatch {
            activity: AgentActivity::Yours,
            ..AgentWatch::default()
        };
        a.key();
        assert_eq!(a.activity, AgentActivity::Idle);
        a.activity = AgentActivity::Waiting;
        a.key();
        assert_eq!(a.activity, AgentActivity::Working);
    }
}
