//! Digests in, the page's numbers out.
//!
//! The answer is a table of hourly buckets, one per session, hour and model,
//! rather than a set of finished charts: the page slices it by period and by
//! project itself, so switching either is instant and every chart on screen
//! is drawn from the same rows. An hour is the user's own, in their time
//! zone, so the day and the hour of the week are theirs too.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::parse::{Agent, Digest};
use crate::pricing::{self, CallTokens};

/// The longest a silence inside a session counts for, in active time.
///
/// Between two messages the agent was working, or its user was reading and
/// typing — until the gap gets long enough to mean they went away. A gap over
/// the cap counts as the cap: the reading time after an answer is real, the
/// lunch break after it is not.
pub const ACTIVE_GAP_CAP_MS: i64 = 5 * 60_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub agent: Agent,
    pub id: String,
    pub cwd: String,
    pub title: String,
    pub first_at: i64,
    pub last_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    /// The price table knows it: its tokens are in the cost.
    pub priced: bool,
}

/// One session's hour, for one model. Turns and active time belong to the
/// session rather than to a model, and sit in rows whose `model` is -1.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub session: u32,
    /// Start of the hour, in ms since the epoch.
    pub at: i64,
    pub model: i32,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    /// API-equivalent, in dollars: the total, then its share per kind.
    pub cost: f64,
    pub cost_input: f64,
    pub cost_output: f64,
    pub cost_cache_write: f64,
    pub cost_cache_read: f64,
    pub calls: u32,
    pub turns: u32,
    pub active_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Aggregate {
    pub sessions: Vec<SessionInfo>,
    pub models: Vec<ModelInfo>,
    pub buckets: Vec<Bucket>,
}

/// The period asked for: `from` included, `to` excluded, either open.
#[derive(Debug, Clone, Copy, Default)]
pub struct Window {
    pub from: Option<i64>,
    pub to: Option<i64>,
}

impl Window {
    fn contains(&self, at: i64) -> bool {
        self.from.is_none_or(|from| at >= from) && self.to.is_none_or(|to| at < to)
    }
}

/// A session being put together from its files.
struct Gathering {
    info: SessionInfo,
    /// Whether `cwd` came from the main transcript, which a subagent's file
    /// does not overrule.
    main_cwd: bool,
    title_rank: u8,
    stamps: Vec<i64>,
}

type Buckets = HashMap<(u32, i64, i32), Bucket>;

fn bucket(buckets: &mut Buckets, session: usize, at: i64, model: i32) -> &mut Bucket {
    buckets
        .entry((session as u32, at, model))
        .or_insert_with(|| Bucket {
            session: session as u32,
            at,
            model,
            ..Bucket::default()
        })
}

/// Sums the digests up. `hour_of` maps a time to the start of its hour in
/// the user's zone; the tests pass UTC.
pub fn aggregate<'a>(
    digests: impl IntoIterator<Item = &'a Digest>,
    window: Window,
    hour_of: impl Fn(i64) -> i64,
) -> Aggregate {
    // The original before its copies: a resumed session carries its parent's
    // messages with their first times, and whichever is read first keeps them.
    let mut digests: Vec<&Digest> = digests
        .into_iter()
        .filter(|digest| !digest.session.is_empty())
        .collect();
    digests.sort_by_key(|digest| {
        (
            digest.calls.first().map(|call| call.at).unwrap_or(i64::MAX),
            digest.sub,
            digest.session.clone(),
        )
    });

    let mut index: HashMap<(Agent, &str), usize> = HashMap::new();
    let mut gathered: Vec<Gathering> = Vec::new();
    let mut models: Vec<String> = Vec::new();
    let mut model_index: HashMap<&str, i32> = HashMap::new();
    let mut buckets: Buckets = HashMap::new();
    let mut seen: HashSet<u64> = HashSet::new();

    for digest in &digests {
        let at = *index
            .entry((digest.agent, digest.session.as_str()))
            .or_insert_with(|| {
                gathered.push(Gathering {
                    info: SessionInfo {
                        agent: digest.agent,
                        id: digest.session.clone(),
                        cwd: String::new(),
                        title: String::new(),
                        first_at: 0,
                        last_at: 0,
                    },
                    main_cwd: false,
                    title_rank: 0,
                    stamps: Vec::new(),
                });
                gathered.len() - 1
            });
        let session = &mut gathered[at];
        if !digest.cwd.is_empty()
            && (session.info.cwd.is_empty() || (!digest.sub && !session.main_cwd))
        {
            session.info.cwd = digest.cwd.clone();
            session.main_cwd = !digest.sub;
        }
        if digest.title_rank > session.title_rank {
            session.info.title = digest.title.clone();
            session.title_rank = digest.title_rank;
        }
        session.stamps.extend_from_slice(&digest.stamps);

        for call in &digest.calls {
            if call.key != 0 && !seen.insert(call.key) {
                continue;
            }
            if !window.contains(call.at) {
                continue;
            }
            let model = digest
                .models
                .get(call.model as usize)
                .map(String::as_str)
                .unwrap_or("unknown");
            let model_at = *model_index.entry(model).or_insert_with(|| {
                models.push(model.to_string());
                (models.len() - 1) as i32
            });
            let tokens = CallTokens {
                input: call.input,
                output: call.output,
                cache_write_5m: call.cache_write_5m,
                cache_write_1h: call.cache_write_1h,
                cache_read: call.cache_read,
                fast: call.fast,
            };
            let row = bucket(&mut buckets, at, hour_of(call.at), model_at);
            row.input += call.input;
            row.output += call.output;
            row.cache_write += call.cache_write_5m + call.cache_write_1h;
            row.cache_read += call.cache_read;
            let cost = pricing::call_cost(model, &tokens).unwrap_or_default();
            row.cost += cost.total();
            row.cost_input += cost.input;
            row.cost_output += cost.output;
            row.cost_cache_write += cost.cache_write;
            row.cost_cache_read += cost.cache_read;
            row.calls += 1;
        }

        if !digest.sub {
            for &prompt in &digest.prompts {
                if window.contains(prompt) {
                    bucket(&mut buckets, at, hour_of(prompt), -1).turns += 1;
                }
            }
        }
    }

    for (at, session) in gathered.iter_mut().enumerate() {
        session.stamps.sort_unstable();
        session.stamps.dedup();
        session.info.first_at = session.stamps.first().copied().unwrap_or(0);
        session.info.last_at = session.stamps.last().copied().unwrap_or(0);
        for pair in session.stamps.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            if !window.contains(start) {
                continue;
            }
            bucket(&mut buckets, at, hour_of(start), -1).active_ms +=
                (end - start).min(ACTIVE_GAP_CAP_MS);
        }
    }

    // Only what the period touched goes back: sessions renumbered in the
    // order they started, rows in time order.
    let mut rows: Vec<Bucket> = buckets
        .into_values()
        .filter(|row| row.calls > 0 || row.turns > 0 || row.active_ms > 0)
        .collect();
    let mut used: Vec<usize> = rows.iter().map(|row| row.session as usize).collect();
    used.sort_unstable();
    used.dedup();
    used.sort_by_key(|&at| (gathered[at].info.first_at, at));
    let renumber: HashMap<usize, u32> = used
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new as u32))
        .collect();
    for row in &mut rows {
        row.session = renumber[&(row.session as usize)];
    }
    rows.sort_by_key(|row| (row.at, row.session, row.model));

    Aggregate {
        sessions: used.iter().map(|&at| gathered[at].info.clone()).collect(),
        models: models
            .into_iter()
            .map(|id| ModelInfo {
                priced: pricing::pricing(&id).is_some(),
                id,
            })
            .collect(),
        buckets: rows,
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse::{Call, Digest};
    use super::*;

    const HOUR: i64 = 3_600_000;
    /// 2026-10-01T00:00:00Z.
    const DAY: i64 = 1_790_812_800_000;

    fn utc_hour(at: i64) -> i64 {
        at.div_euclid(HOUR) * HOUR
    }

    fn call(key: u64, at: i64, output: u64) -> Call {
        Call {
            key,
            at,
            model: 0,
            input: 10,
            output,
            cache_write_5m: 100,
            cache_write_1h: 0,
            cache_read: 1000,
            fast: false,
        }
    }

    fn digest(session: &str, calls: Vec<Call>, stamps: Vec<i64>, prompts: Vec<i64>) -> Digest {
        Digest {
            session: session.to_string(),
            cwd: "C:\\dev\\app".to_string(),
            models: vec!["claude-opus-5-5".to_string()],
            calls,
            stamps,
            prompts,
            ..Digest::default()
        }
    }

    #[test]
    fn calls_are_summed_per_session_hour_and_model_and_priced() {
        let main = digest(
            "s-1",
            vec![call(1, DAY + 60_000, 50), call(2, DAY + 120_000, 70)],
            vec![],
            vec![DAY],
        );
        let result = aggregate([&main], Window::default(), utc_hour);
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.models[0].id, "claude-opus-5-5");
        assert!(result.models[0].priced);
        let tokens = result.buckets.iter().find(|row| row.model == 0).unwrap();
        assert_eq!(tokens.at, DAY);
        assert_eq!((tokens.input, tokens.output, tokens.calls), (20, 120, 2));
        assert_eq!((tokens.cache_write, tokens.cache_read), (200, 2000));
        // 20 × 4 + 120 × 20 + 200 × 5 + 2000 × 0.20, per million.
        let expected = (20.0 * 4.0 + 120.0 * 20.0 + 200.0 * 5.0 + 2000.0 * 0.20) / 1e6;
        assert!((tokens.cost - expected).abs() < 1e-12);
        assert!((tokens.cost_output - 120.0 * 20.0 / 1e6).abs() < 1e-12);
        assert!((tokens.cost_cache_read - 2000.0 * 0.20 / 1e6).abs() < 1e-12);
        let turns = result.buckets.iter().find(|row| row.model == -1).unwrap();
        assert_eq!(turns.turns, 1);
    }

    #[test]
    fn a_call_seen_in_two_files_counts_once() {
        let original = digest("s-1", vec![call(7, DAY, 50)], vec![], vec![]);
        let resumed = digest(
            "s-2",
            vec![call(7, DAY, 50), call(8, DAY + HOUR, 5)],
            vec![],
            vec![],
        );
        let result = aggregate([&resumed, &original], Window::default(), utc_hour);
        let output: u64 = result.buckets.iter().map(|row| row.output).sum();
        assert_eq!(output, 55);
        // The copy went to the session it came from.
        let first = &result.sessions[result.buckets[0].session as usize];
        assert_eq!(first.id, "s-1");
    }

    #[test]
    fn active_time_sums_gaps_and_caps_the_long_ones() {
        let minute = 60_000;
        // 2 min, then 1 min, then an hour away (counts 5 min), then 30 s.
        let stamps = vec![
            DAY,
            DAY + 2 * minute,
            DAY + 3 * minute,
            DAY + 63 * minute,
            DAY + 63 * minute + 30_000,
        ];
        let main = digest("s-1", vec![], stamps, vec![]);
        let result = aggregate([&main], Window::default(), utc_hour);
        let active: i64 = result.buckets.iter().map(|row| row.active_ms).sum();
        assert_eq!(active, 2 * minute + minute + ACTIVE_GAP_CAP_MS + 30_000);
        // Each gap lands in the hour it started in.
        let second_hour: i64 = result
            .buckets
            .iter()
            .filter(|row| row.at == DAY + HOUR)
            .map(|row| row.active_ms)
            .sum();
        assert_eq!(second_hour, 30_000);
    }

    #[test]
    fn a_subagent_fills_its_parents_timeline_without_adding_turns() {
        let minute = 60_000;
        // The parent waits 20 minutes on its subagent, which works throughout.
        let parent = digest("s-1", vec![], vec![DAY, DAY + 20 * minute], vec![DAY]);
        let child = Digest {
            sub: true,
            prompts: vec![DAY + minute],
            ..digest(
                "s-1",
                vec![],
                (1..20).map(|n| DAY + n * minute).collect(),
                vec![],
            )
        };
        let result = aggregate([&parent, &child], Window::default(), utc_hour);
        assert_eq!(result.sessions.len(), 1);
        let active: i64 = result.buckets.iter().map(|row| row.active_ms).sum();
        assert_eq!(active, 20 * minute);
        let turns: u32 = result.buckets.iter().map(|row| row.turns).sum();
        assert_eq!(turns, 1);
    }

    #[test]
    fn the_window_keeps_what_happened_inside_it() {
        let early = digest("s-1", vec![call(1, DAY, 10)], vec![DAY], vec![DAY]);
        let late = digest(
            "s-2",
            vec![call(2, DAY + 2 * HOUR, 20)],
            vec![DAY + 2 * HOUR],
            vec![],
        );
        let window = Window {
            from: Some(DAY + HOUR),
            to: Some(DAY + 3 * HOUR),
        };
        let result = aggregate([&early, &late], window, utc_hour);
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.sessions[0].id, "s-2");
        assert_eq!(result.buckets.iter().map(|row| row.output).sum::<u64>(), 20);
    }

    #[test]
    fn an_unknown_model_is_listed_without_a_price() {
        let mut odd = digest("s-1", vec![call(1, DAY, 10)], vec![], vec![]);
        odd.models = vec!["house-model-1".to_string()];
        let result = aggregate([&odd], Window::default(), utc_hour);
        assert!(!result.models[0].priced);
        assert_eq!(result.buckets[0].cost, 0.0);
        assert_eq!(result.buckets[0].output, 10);
    }
}
