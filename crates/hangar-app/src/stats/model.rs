//! Everything the statistics page computes, from the hourly rows hangar-core
//! returns to what each chart draws (`lib/statsModel.ts`). Pure: the clock,
//! the language and the workspaces come in as arguments, so every rule here
//! has a test. Hangar Mobile's `remote_stats` (P20) sums the statistics up
//! with the same `summarize`, over every project.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use hangar_core::stats::{Agent, UsageStats};

use super::time::{add_days, hour_of, start_of_day, start_of_week, weekday_of};
use crate::state::types::Workspace;

/// The periods the page offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Period {
    D7,
    #[default]
    D30,
    D90,
    All,
}

/// `PERIODS`, in the order the control lists them.
pub const PERIODS: [Period; 4] = [Period::D7, Period::D30, Period::D90, Period::All];

impl Period {
    /// The id the phone's `remote_stats` call names it by.
    pub fn id(self) -> &'static str {
        match self {
            Period::D7 => "7d",
            Period::D30 => "30d",
            Period::D90 => "90d",
            Period::All => "all",
        }
    }

    pub fn from_id(id: &str) -> Option<Period> {
        PERIODS.into_iter().find(|period| period.id() == id)
    }

    fn days(self) -> Option<i64> {
        match self {
            Period::D7 => Some(7),
            Period::D30 => Some(30),
            Period::D90 => Some(90),
            Period::All => None,
        }
    }
}

/// What the charts and rankings measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Measure {
    #[default]
    Tokens,
    Cost,
}

// ---------------------------------------------------------------------------
// Days
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub from: i64,
    /// Excluded: the midnight after today.
    pub to: i64,
    pub days: i64,
    /// The same length just before, for the trends; none for "all".
    pub previous: Option<(i64, i64)>,
}

/// Whole days up to and including today; "all" starts on the first day with
/// data.
pub fn period_range(period: Period, now: i64, first_at: Option<i64>) -> Range {
    let to = add_days(start_of_day(now), 1);
    let Some(days) = period.days() else {
        let today = start_of_day(now);
        let from = first_at.map_or(today, |first| start_of_day(first).min(today));
        let mut days = 0;
        let mut day = from;
        while day < to {
            days += 1;
            day = add_days(day, 1);
        }
        return Range {
            from,
            to,
            days,
            previous: None,
        };
    };
    let from = add_days(to, -days);
    Range {
        from,
        to,
        days,
        previous: Some((add_days(from, -days), from)),
    }
}

/// Past this many days, a column per day would be thinner than a hairline:
/// weeks.
pub const WEEKLY_AFTER_DAYS: i64 = 120;

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

/// A folder the sessions of a workspace may have run in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectRoot {
    pub workspace_id: String,
    pub name: String,
    /// Normalized (see [`normalize_path`]).
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Project {
    pub key: String,
    pub name: String,
    /// The folder shown under the name: the workspace's, or the session's own.
    pub path: String,
    pub workspace_id: Option<String>,
}

/// A path in one spelling: forward slashes, no trailing one, and lowercase
/// where the file system ignores case (a Windows drive or share), so that
/// `C:\Dev\App` from one agent and `c:/dev/app/` from another are one place.
pub fn normalize_path(path: &str) -> String {
    let mut clean = path.trim().replace('\\', "/");
    while clean.len() > 1 && clean.ends_with('/') && !is_drive_root(&clean) {
        clean.pop();
    }
    if has_drive(&clean) || clean.starts_with("//") {
        clean.to_lowercase()
    } else {
        clean
    }
}

/// `^[a-z]:/`.
fn has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// `^[a-z]:/$`.
fn is_drive_root(path: &str) -> bool {
    path.len() == 3 && has_drive(path)
}

fn last_segment(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}

/// Every folder a workspace's sessions may have started in: its own, the
/// other roots of a `.code-workspace`, its worktrees, and where its panes
/// run. A pane's folder above the workspace's own (a shell in the home
/// folder) is left out: it would claim every project under it.
pub fn workspace_roots(
    workspaces: &[Workspace],
    worktrees: &HashMap<String, Vec<String>>,
) -> Vec<ProjectRoot> {
    let mut roots = Vec::new();
    for ws in workspaces {
        let own = normalize_path(&ws.cwd);
        let pane_dirs = ws
            .panes
            .iter()
            .filter_map(|pane| pane.cwd.as_deref())
            .filter(|cwd| !cwd.is_empty())
            .filter(|cwd| {
                let dir = normalize_path(cwd);
                !own.starts_with(&format!("{dir}/"))
            });
        let all = std::iter::once(ws.cwd.as_str())
            .chain(ws.extra_roots.iter().map(String::as_str))
            .chain(
                worktrees
                    .get(&ws.id)
                    .into_iter()
                    .flatten()
                    .map(String::as_str),
            )
            .chain(pane_dirs)
            .filter(|path| !path.is_empty())
            .map(normalize_path);
        let mut seen = HashSet::new();
        for path in all {
            if seen.insert(path.clone()) {
                roots.push(ProjectRoot {
                    workspace_id: ws.id.clone(),
                    name: ws.name.clone(),
                    path,
                });
            }
        }
    }
    roots
}

/// The project a session's folder belongs to: the workspace with the deepest
/// root above it, or the folder itself, named after its last segment.
pub fn project_for(cwd: &str, roots: &[ProjectRoot]) -> Project {
    let path = normalize_path(cwd);
    let mut best: Option<&ProjectRoot> = None;
    for root in roots {
        let inside = path == root.path || path.starts_with(&format!("{}/", root.path));
        if inside && best.is_none_or(|current| root.path.len() > current.path.len()) {
            best = Some(root);
        }
    }
    if let Some(root) = best {
        return Project {
            key: format!("ws:{}", root.workspace_id),
            name: root.name.clone(),
            path: root.path.clone(),
            workspace_id: Some(root.workspace_id.clone()),
        };
    }
    Project {
        key: format!("dir:{path}"),
        name: if cwd.is_empty() {
            "?".to_owned()
        } else {
            last_segment(cwd)
        },
        path: cwd.to_owned(),
        workspace_id: None,
    }
}

// ---------------------------------------------------------------------------
// Summing up
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub tokens: u64,
    pub cost: f64,
    pub cost_input: f64,
    pub cost_output: f64,
    pub cost_cache_write: f64,
    pub cost_cache_read: f64,
    pub active_ms: i64,
    pub turns: u64,
    pub calls: u64,
    pub sessions: usize,
}

impl Totals {
    fn add(&mut self, row: &hangar_core::stats::Bucket) {
        self.input += row.input;
        self.output += row.output;
        self.cache_write += row.cache_write;
        self.cache_read += row.cache_read;
        self.tokens += row.input + row.output + row.cache_write + row.cache_read;
        self.cost += row.cost;
        self.cost_input += row.cost_input;
        self.cost_output += row.cost_output;
        self.cost_cache_write += row.cost_cache_write;
        self.cost_cache_read += row.cost_cache_read;
        self.active_ms += row.active_ms;
        self.turns += u64::from(row.turns);
        self.calls += u64::from(row.calls);
    }

    /// The figure a ranking or a chart orders by.
    pub fn measured(&self, measure: Measure) -> f64 {
        match measure {
            Measure::Cost => self.cost,
            Measure::Tokens => self.tokens as f64,
        }
    }
}

/// One column of the chart: a day, or a week past [`WEEKLY_AFTER_DAYS`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Point {
    pub at: i64,
    pub totals: Totals,
}

/// A row of a ranking: a project, an agent, a model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ranked {
    pub key: String,
    pub label: String,
    /// A second line: the project's folder.
    pub detail: String,
    /// Of the measure, 0–1.
    pub share: f64,
    /// A model the price table does not know: no cost, said so.
    pub unpriced: bool,
    pub totals: Totals,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub index: usize,
    pub id: String,
    pub agent: Agent,
    pub title: String,
    pub project: Project,
    /// The model that did most of its work in the period.
    pub model: Option<String>,
    pub last_at: i64,
    pub totals: Totals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Day,
    Week,
}

#[derive(Debug, Clone)]
pub struct Summary {
    pub range: Range,
    pub granularity: Granularity,
    pub totals: Totals,
    /// The period before, for the trends; none for "all".
    pub previous: Option<Totals>,
    pub series: Vec<Point>,
    /// Every project of the period, whatever the filter, so any can be picked.
    pub projects: Vec<Ranked>,
    pub agents: Vec<Ranked>,
    pub models: Vec<Ranked>,
    /// Active time by weekday (Monday first) and hour.
    pub heat: [[i64; 24]; 7],
    pub heat_max: i64,
    /// The heaviest sessions of the period, by the measure.
    pub sessions: Vec<SessionRow>,
    /// Tokens of models without a price, left out of the cost.
    pub unpriced_tokens: u64,
    /// Days of the period with any active time.
    pub active_days: usize,
    /// The first hour with anything in it, over all time.
    pub first_at: Option<i64>,
}

pub struct SummaryOptions<'a> {
    pub period: Period,
    /// A project key, or none for all of them.
    pub project: Option<&'a str>,
    pub measure: Measure,
    pub now: i64,
    /// The project of each session, by index (see [`project_for`]).
    pub projects: &'a [Project],
    /// Display names of the agents.
    pub agent_label: &'a dyn Fn(Agent) -> String,
    /// How many of the heaviest sessions to keep; the page shows eight.
    pub top_sessions: usize,
}

pub const TOP_SESSIONS: usize = 8;

/// An agent's id as the transcripts name it.
pub fn agent_key(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
    }
}

const FAMILIES: [&str; 4] = ["opus", "sonnet", "haiku", "fable"];

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn count_digits(text: &str) -> usize {
    text.bytes().take_while(u8::is_ascii_digit).count()
}

/// `(?:-\d{8})?$`: nothing more, or a date.
fn date_tail(rest: &str) -> bool {
    rest.is_empty()
        || rest
            .strip_prefix('-')
            .is_some_and(|date| date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()))
}

/// `(opus|sonnet|haiku|fable)-(\d+)(?:[-.](\d{1,2}))?(?:-\d{8})?$` on the
/// whole of `tail`: the current naming, family first.
fn modern_name(tail: &str) -> Option<String> {
    let family = FAMILIES.iter().find(|family| {
        tail.strip_prefix(**family)
            .is_some_and(|rest| rest.starts_with('-'))
    })?;
    let rest = &tail[family.len() + 1..];
    let digits = count_digits(rest);
    if digits == 0 {
        return None;
    }
    let (major, rest) = rest.split_at(digits);
    // A minor version of two digits would swallow the day of a bare date
    // (`opus-4-20250514`): the longest that still ends the id well wins, and
    // none at all is the last resort.
    if let Some(after) = rest.strip_prefix(['-', '.']) {
        for width in [2, 1] {
            if count_digits(after) >= width {
                let (minor, tail) = after.split_at(width);
                if date_tail(tail) {
                    return Some(format!("{} {major}.{minor}", capitalize(family)));
                }
            }
        }
    }
    date_tail(rest).then(|| format!("{} {major}", capitalize(family)))
}

/// `^claude-(\d+)(?:-(\d))?-(opus|sonnet|haiku)`: the Claude 3 naming put the
/// version first.
fn legacy_name(id: &str) -> Option<String> {
    let rest = id.strip_prefix("claude-")?;
    let digits = count_digits(rest);
    if digits == 0 {
        return None;
    }
    let (major, rest) = rest.split_at(digits);
    let family_of = |text: &str| {
        let text = text.strip_prefix('-')?;
        ["opus", "sonnet", "haiku"]
            .into_iter()
            .find(|family| text.starts_with(family))
    };
    if let Some(after) = rest.strip_prefix('-')
        && count_digits(after) >= 1
        && let Some(family) = family_of(&after[1..])
    {
        return Some(format!("{} {major}.{}", capitalize(family), &after[..1]));
    }
    family_of(rest).map(|family| format!("{} {major}", capitalize(family)))
}

/// "claude-sonnet-4-5-20250929" → "Sonnet 4.5" (`prettyModel`). Anything not
/// recognised as a Claude model comes back untouched rather than guessed at.
pub fn pretty_model(model: &str) -> String {
    let lower = model.trim().to_lowercase();
    // `[1m]` and the like: a suffix of the CLI's, not of the model.
    let id = match lower.find('[') {
        Some(open) if lower.ends_with(']') => &lower[..open],
        _ => lower.as_str(),
    };
    // The regex is not anchored at the start, so a "claude-" anywhere
    // (`us.anthropic.claude-opus-4-5`) opens a match too.
    let starts = std::iter::once(0).chain(
        id.match_indices("claude-")
            .map(|(at, hit)| at + hit.len()),
    );
    for start in starts {
        if let Some(label) = modern_name(&id[start..]) {
            return label;
        }
    }
    if let Some(label) = legacy_name(id) {
        return label;
    }
    if FAMILIES.contains(&id) {
        return capitalize(id);
    }
    model.trim().to_owned()
}

/// A model's name as people say it: "Opus 5.5" for a Claude id (see
/// [`pretty_model`]), "GPT-5.6 Sol" for an OpenAI one, and anything else
/// exactly as the transcript wrote it.
pub fn model_label(id: &str) -> String {
    // `^gpt-(\d[\d.]*)((?:-[a-z0-9]+)*)$`, case-insensitive.
    let trimmed = id.trim();
    if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("gpt-") {
        let rest = &trimmed[4..];
        let version_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'.')
            .count();
        if version_len > 0 && rest.as_bytes()[0].is_ascii_digit() {
            let (version, tail) = rest.split_at(version_len);
            let words_ok = tail.is_empty()
                || (tail.starts_with('-')
                    && tail
                        .split('-')
                        .skip(1)
                        .all(|word| !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric())));
            if words_ok {
                let mut parts = vec![format!("GPT-{version}")];
                parts.extend(tail.split('-').filter(|word| !word.is_empty()).map(capitalize));
                return parts.join(" ");
            }
        }
    }
    pretty_model(id)
}

/// The first hour with anything in it.
pub fn first_activity(stats: &UsageStats) -> Option<i64> {
    stats.buckets.iter().map(|row| row.at).min()
}

/// A map that keeps the order its keys came in, as a JavaScript `Map` does:
/// a tie in a ranking falls to whichever was seen first.
struct Ordered<K, V> {
    index: HashMap<K, usize>,
    rows: Vec<V>,
}

impl<K: Hash + Eq + Clone, V> Ordered<K, V> {
    fn new() -> Self {
        Ordered {
            index: HashMap::new(),
            rows: Vec::new(),
        }
    }

    fn entry(&mut self, key: &K, make: impl FnOnce() -> V) -> &mut V {
        let at = match self.index.get(key) {
            Some(at) => *at,
            None => {
                self.rows.push(make());
                self.index.insert(key.clone(), self.rows.len() - 1);
                self.rows.len() - 1
            }
        };
        &mut self.rows[at]
    }

    fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let at = *self.index.get(key)?;
        self.rows.get_mut(at)
    }
}

fn ranked_entry<'m>(
    into: &'m mut Ordered<String, Ranked>,
    key: &str,
    label: &str,
    detail: &str,
) -> &'m mut Ranked {
    into.entry(&key.to_owned(), || Ranked {
        key: key.to_owned(),
        label: label.to_owned(),
        detail: detail.to_owned(),
        ..Ranked::default()
    })
}

fn rank(entries: Ordered<String, Ranked>, measure: Measure, total: f64) -> Vec<Ranked> {
    let mut rows = entries.rows;
    for row in &mut rows {
        row.share = if total > 0.0 {
            row.totals.measured(measure) / total
        } else {
            0.0
        };
    }
    rows.sort_by(|a, b| {
        b.totals
            .measured(measure)
            .total_cmp(&a.totals.measured(measure))
            .then_with(|| b.totals.active_ms.cmp(&a.totals.active_ms))
            .then_with(|| compare_labels(&a.label, &b.label))
    });
    rows
}

/// `localeCompare` for a label: alphabetical, case not deciding.
fn compare_labels(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn count_session<K: Hash + Eq + Clone>(into: &mut HashMap<K, HashSet<usize>>, key: &K, session: usize) {
    into.entry(key.clone()).or_default().insert(session);
}

pub fn summarize(stats: &UsageStats, options: &SummaryOptions<'_>) -> Summary {
    let first_at = first_activity(stats);
    let range = period_range(options.period, options.now, first_at);
    let granularity = if range.days > WEEKLY_AFTER_DAYS {
        Granularity::Week
    } else {
        Granularity::Day
    };
    let column_of = |at: i64| match granularity {
        Granularity::Week => start_of_week(at),
        Granularity::Day => start_of_day(at),
    };
    let step = if granularity == Granularity::Week { 7 } else { 1 };

    // Every column of the period, empty ones included: a quiet day is a gap
    // in the chart, not a day that never happened.
    let mut series: Vec<Point> = Vec::new();
    let mut column: HashMap<i64, usize> = HashMap::new();
    let mut at = column_of(range.from);
    while at < range.to {
        column.insert(at, series.len());
        series.push(Point {
            at,
            totals: Totals::default(),
        });
        at = add_days(at, step);
    }

    let in_range = |at: i64| at >= range.from && at < range.to;
    let in_previous = |at: i64| range.previous.is_some_and(|(from, to)| at >= from && at < to);
    let project_of = |session: usize| options.projects.get(session);
    let picked = |session: usize| {
        options
            .project
            .is_none_or(|key| project_of(session).is_some_and(|project| project.key == key))
    };

    let mut totals = Totals::default();
    let mut previous = range.previous.map(|_| Totals::default());
    let mut sessions_now: HashSet<usize> = HashSet::new();
    let mut sessions_before: HashSet<usize> = HashSet::new();
    let mut projects: Ordered<String, Ranked> = Ordered::new();
    let mut project_sessions: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut agents: Ordered<String, Ranked> = Ordered::new();
    let mut agent_sessions: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut models: Ordered<String, Ranked> = Ordered::new();
    let mut per_session: Ordered<usize, SessionRow> = Ordered::new();
    let mut session_models: HashMap<usize, Ordered<String, u64>> = HashMap::new();
    let mut heat = [[0i64; 24]; 7];
    let mut unpriced_tokens = 0u64;
    let mut active_days: HashSet<i64> = HashSet::new();
    let mut column_sessions: HashMap<i64, HashSet<usize>> = HashMap::new();

    for row in &stats.buckets {
        let index = row.session as usize;
        let Some(session) = stats.sessions.get(index) else {
            continue;
        };
        if in_previous(row.at)
            && picked(index)
            && let Some(before) = previous.as_mut()
        {
            before.add(row);
            sessions_before.insert(index);
        }
        if !in_range(row.at) {
            continue;
        }

        // The project ranking ignores the project filter: it is how one is
        // picked.
        let project = project_of(index);
        if let Some(project) = project {
            ranked_entry(&mut projects, &project.key, &project.name, &project.path)
                .totals
                .add(row);
            count_session(&mut project_sessions, &project.key, index);
        }
        if !picked(index) {
            continue;
        }

        totals.add(row);
        sessions_now.insert(index);
        if let Some(point) = column.get(&column_of(row.at)).map(|at| &mut series[*at]) {
            point.totals.add(row);
            count_session(&mut column_sessions, &point.at, index);
        }
        if row.active_ms > 0 {
            active_days.insert(start_of_day(row.at));
        }

        let agent_id = agent_key(session.agent);
        let label = (options.agent_label)(session.agent);
        ranked_entry(&mut agents, agent_id, &label, "").totals.add(row);
        count_session(&mut agent_sessions, &agent_id.to_owned(), index);

        let tokens = row.input + row.output + row.cache_write + row.cache_read;
        if row.model >= 0 {
            let model = stats.models.get(row.model as usize);
            let id = model.map_or("unknown", |model| model.id.as_str());
            let label = model_label(id);
            let detail = if label == id { "" } else { id };
            let entry = ranked_entry(&mut models, id, &label, detail);
            entry.totals.add(row);
            if model.is_some_and(|model| !model.priced) {
                entry.unpriced = true;
                unpriced_tokens += tokens;
            }
            *session_models
                .entry(index)
                .or_insert_with(Ordered::new)
                .entry(&id.to_owned(), || 0) += tokens;
        }

        if row.active_ms > 0 {
            heat[weekday_of(row.at)][hour_of(row.at)] += row.active_ms;
        }

        per_session
            .entry(&index, || SessionRow {
                index,
                id: session.id.clone(),
                agent: session.agent,
                title: session.title.clone(),
                project: project.cloned().unwrap_or_else(|| Project {
                    path: session.cwd.clone(),
                    ..Project::default()
                }),
                model: None,
                last_at: session.last_at,
                totals: Totals::default(),
            })
            .totals
            .add(row);
    }

    totals.sessions = sessions_now.len();
    for point in &mut series {
        point.totals.sessions = column_sessions.get(&point.at).map_or(0, HashSet::len);
    }
    if let Some(before) = previous.as_mut() {
        before.sessions = sessions_before.len();
    }
    for (key, set) in &project_sessions {
        if let Some(row) = projects.get_mut(key) {
            row.totals.sessions = set.len();
        }
    }
    for (key, set) in &agent_sessions {
        if let Some(row) = agents.get_mut(key) {
            row.totals.sessions = set.len();
        }
    }

    let project_total: f64 = projects
        .rows
        .iter()
        .map(|row| row.totals.measured(options.measure))
        .sum();
    let measured = totals.measured(options.measure);

    let mut sessions: Vec<SessionRow> = per_session
        .rows
        .into_iter()
        .filter(|row| row.totals.calls > 0 || row.totals.active_ms > 0)
        .collect();
    sessions.sort_by(|a, b| {
        b.totals
            .measured(options.measure)
            .total_cmp(&a.totals.measured(options.measure))
            .then_with(|| b.totals.active_ms.cmp(&a.totals.active_ms))
    });
    sessions.truncate(options.top_sessions);
    for row in &mut sessions {
        row.totals.sessions = 1;
        if let Some(by_model) = session_models.get(&row.index) {
            // The first of the largest wins a tie, as the stable sort did.
            let top = by_model
                .rows
                .iter()
                .zip(by_model.index.iter().fold(
                    vec![String::new(); by_model.rows.len()],
                    |mut names, (name, at)| {
                        names[*at] = name.clone();
                        names
                    },
                ))
                .fold(None::<(&u64, String)>, |best, (tokens, name)| match best {
                    Some((most, _)) if most >= tokens => best,
                    _ => Some((tokens, name)),
                });
            if let Some((_, name)) = top {
                row.model = Some(model_label(&name));
            }
        }
    }

    let heat_max = heat.iter().flatten().copied().max().unwrap_or(0).max(0);
    Summary {
        range,
        granularity,
        totals,
        previous,
        series,
        projects: rank(projects, options.measure, project_total),
        agents: rank(agents, options.measure, measured),
        models: rank(models, options.measure, measured),
        heat,
        heat_max,
        sessions,
        unpriced_tokens,
        active_days: active_days.len(),
        first_at,
    }
}

/// The heatmap's five steps above empty, by the square root of the value: a
/// few long days must not wash every other hour out.
pub fn heat_level(value: i64, max: i64) -> usize {
    if value <= 0 || max <= 0 {
        return 0;
    }
    let level = ((value as f64 / max as f64).sqrt() * 5.0).ceil() as usize;
    level.clamp(1, 5)
}

/// Relative change, or none when there was nothing to compare with.
pub fn change(now: f64, before: f64) -> Option<f64> {
    (before > 0.0).then(|| (now - before) / before)
}

#[cfg(test)]
mod tests;
