//! P11: Usage statistics.
//!
//! What the agents did, in tokens, time and the money it would have cost at
//! API prices, across every project or one: a port of `StatsView.tsx`,
//! `StatsCharts.tsx` and `lib/{stats,statsModel}.ts`, with the charts drawn on
//! iced's `canvas`.
//!
//! - [`model`]: the pure part: periods, projects, `summarize` (P20's
//!   `remote_stats` uses it too).
//! - [`cache`]: the usage statistics, fresh for a minute and shared with Hangar
//!   Mobile; [`usage`] is the function P20 calls.
//! - [`view`], [`ranked`], [`sessions`], [`tables`] and [`charts`]: the page.
//! - [`format`] and [`time`]: numbers, dates and local days in English and
//!   French.
//!
//! The page keeps what the person picked (period, measure, project) for as
//! long as the app runs, like the web page did for the life of its window;
//! it is not a setting.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::time::Instant;

use hangar_core::stats::UsageStats;
use iced::keyboard::{self, Key as KeyboardKey, key::Named};
use iced::{Element, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Region, View};
use crate::state::Model;

pub mod cache;
pub mod charts;
pub mod format;
pub mod model;
mod ranked;
mod sessions;
mod tables;
pub mod time;
mod ui;
mod view;

// P20 (mobile) calls it; nothing in the window does.
#[allow(unused_imports)]
pub use cache::{usage, worktrees_of};
pub use charts::ChartId;
use model::{Measure, Period, Project, ProjectRoot, Summary, SummaryOptions, TOP_SESSIONS};

/// What the page shows once the statistics are in, kept between frames: the
/// summary is the costly part, worked out off the UI thread (see
/// [`State::recompute`]) whenever what it is made from changes (the data, the
/// choices, the workspaces' folders, the day).
#[derive(Debug)]
pub struct Computed {
    /// The statistics it was made from: the page shows these with it, never
    /// a newer answer beside an older summary.
    pub data: Arc<UsageStats>,
    pub summary: Arc<Summary>,
    /// The project of each session, by index.
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedFor {
    generation: u64,
    period: Period,
    measure: Measure,
    project: Option<String>,
    /// Bumped when the workspaces' roots are rebuilt.
    roots_rev: u64,
    day: i64,
}

/// The package's UI state; lives in `App.pkg.stats`.
#[derive(Debug, Default)]
pub struct State {
    period: Period,
    measure: Measure,
    /// A project key, or none for all of them.
    project: Option<String>,
    daily_table: bool,
    heat_table: bool,
    all_projects: bool,
    menu_open: bool,
    /// The worktrees of each workspace's repository: their sessions are the
    /// workspace's.
    worktrees: HashMap<String, Vec<String>>,
    /// The workspaces' folders when the worktrees were last asked for.
    repos: String,
    worktrees_seq: u64,
    /// The chart that has the arrow keys.
    focus: Option<ChartId>,
    /// The column and the cell the readouts are about.
    columns: Option<usize>,
    heat: Option<(usize, usize)>,
    /// The readouts follow the keys, not the pointer.
    keyboard: bool,
    /// The last summary and what it was made for; stays on screen while the
    /// next one is worked out.
    computed: Option<(ComputedFor, Arc<Computed>)>,
    /// What the summary being worked out is for, and its request number.
    pending: Option<ComputedFor>,
    compute_seq: u64,
    /// The workspaces' roots, rebuilt only when `fingerprint` of the
    /// workspaces moves, so a frame (or a `cd` in a shell) costs nothing.
    roots: Arc<Vec<ProjectRoot>>,
    roots_rev: u64,
    fingerprint: u64,
}

impl State {
    /// The summary of the page: the last one worked out, which may be a
    /// moment behind the choices while the next one is under way.
    fn computed(&self) -> Option<Arc<Computed>> {
        self.computed.as_ref().map(|(_, computed)| computed.clone())
    }

    /// Asks for the summary of the data and choices the page has now, unless
    /// that is the one shown or already under way. The work runs on a
    /// blocking thread; its answer is dropped if a newer ask went out.
    fn recompute(&mut self, m: &Model) -> Task<Message> {
        let live = &m.live.stats;
        let Some(data) = live.data.clone() else {
            return Task::none();
        };
        let now = time::now_ms();
        let key = ComputedFor {
            generation: live.generation,
            period: self.period,
            measure: self.measure,
            project: self.project.clone(),
            roots_rev: self.roots_rev,
            day: time::start_of_day(now),
        };
        if self.pending.as_ref() == Some(&key)
            || (self.pending.is_none()
                && self
                    .computed
                    .as_ref()
                    .is_some_and(|(known, _)| *known == key))
        {
            return Task::none();
        }
        self.compute_seq += 1;
        let seq = self.compute_seq;
        self.pending = Some(key.clone());
        let roots = self.roots.clone();
        let (period, measure, project) = (self.period, self.measure, self.project.clone());
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let projects: Vec<Project> = data
                        .sessions
                        .iter()
                        .map(|session| model::project_for(&session.cwd, &roots))
                        .collect();
                    let summary = model::summarize(
                        &data,
                        &SummaryOptions {
                            period,
                            project: project.as_deref(),
                            measure,
                            now,
                            projects: &projects,
                            top_sessions: TOP_SESSIONS,
                        },
                    );
                    Arc::new(Computed {
                        data,
                        summary: Arc::new(summary),
                        projects,
                    })
                })
                .await
                .ok()
            },
            move |computed| say(Msg::Computed(seq, key.clone(), computed)),
        )
    }

    /// A summary came back. Only the latest ask counts: an older answer is
    /// for choices the page has moved on from.
    fn land(&mut self, seq: u64, key: ComputedFor, computed: Option<Arc<Computed>>) {
        if seq != self.compute_seq {
            return;
        }
        self.pending = None;
        if let Some(computed) = computed {
            self.computed = Some((key, computed));
        }
    }

    /// Rebuilds the roots when the workspaces they come from changed; true
    /// then. The pane folders in them are the ones at that moment: a `cd` in
    /// a shell is no reason to work the whole summary out again.
    fn refresh_roots(&mut self, m: &Model) -> bool {
        let workspaces = &m.state().workspaces;
        let fingerprint = workspaces_fingerprint(workspaces);
        if fingerprint == self.fingerprint {
            return false;
        }
        self.fingerprint = fingerprint;
        self.rebuild_roots(m);
        true
    }

    fn rebuild_roots(&mut self, m: &Model) {
        self.roots = Arc::new(model::workspace_roots(
            &m.state().workspaces,
            &self.worktrees,
        ));
        self.roots_rev += 1;
    }

    /// The last summary worked out, however stale: what the keys walk over.
    fn last_computed(&self) -> Option<Arc<Computed>> {
        self.computed()
    }

    fn clear_readouts(&mut self) {
        self.columns = None;
        self.heat = None;
    }
}

/// A hash of what the roots are built from, without building them: the
/// workspaces' ids, names and folders. Pane folders are left out on purpose
/// (see [`State::refresh_roots`]).
fn workspaces_fingerprint(workspaces: &[crate::state::types::Workspace]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for ws in workspaces {
        ws.id.hash(&mut hasher);
        ws.name.hash(&mut hasher);
        ws.cwd.hash(&mut hasher);
        ws.extra_roots.hash(&mut hasher);
    }
    hasher.finish()
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.stats`: the statistics the page shows and where the refresh
/// stands. The cache that is fresh for 60 s and shared with P20's
/// `remote_stats` is [`cache`]'s, process-wide.
#[derive(Debug, Default)]
pub struct Live {
    /// The last answer: the cache's on first sight, then the scan's.
    pub data: Option<Arc<UsageStats>>,
    /// Bumped whenever `data` is replaced, so the summary knows to redo.
    pub generation: u64,
    /// Reading the transcripts: the page shows it without hiding what it has.
    pub refreshing: bool,
    pub refresh_started: Option<Instant>,
    pub error: Option<String>,
    /// When the last full scan finished, ms.
    pub checked_at: Option<i64>,
}

impl Live {
    fn set_data(&mut self, data: Arc<UsageStats>) {
        self.data = Some(data);
        self.generation += 1;
    }

    /// Whether a scan this recent is still the truth.
    fn is_fresh(&self) -> bool {
        self.checked_at
            .is_some_and(|at| time::now_ms() - at < cache::FRESH_FOR_MS)
    }
}

/// An arrow key, Home, End or Escape, for the chart that has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Escape,
}

/// What a key press means to the focused chart, if anything. A key the chart
/// has no use for goes on to the rest of the window; Escape is the chart's
/// only while it shows a readout.
fn chart_key(
    focus: Option<ChartId>,
    reading: bool,
    key: &KeyboardKey,
    modifiers: keyboard::Modifiers,
) -> Option<ChartKey> {
    let chart = focus?;
    if !modifiers.is_empty() {
        return None;
    }
    let KeyboardKey::Named(named) = key else {
        return None;
    };
    match (named, chart) {
        (Named::ArrowLeft, _) => Some(ChartKey::Left),
        (Named::ArrowRight, _) => Some(ChartKey::Right),
        (Named::ArrowUp, ChartId::Heat) => Some(ChartKey::Up),
        (Named::ArrowDown, ChartId::Heat) => Some(ChartKey::Down),
        (Named::Home, _) => Some(ChartKey::Home),
        (Named::End, _) => Some(ChartKey::End),
        (Named::Escape, _) if reading => Some(ChartKey::Escape),
        _ => None,
    }
}

/// The package's messages, routed by the shell as `Message::Stats(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The page came on screen (a subscription that lives while it does).
    Mounted,
    SetPeriod(Period),
    SetMeasure(Measure),
    SetProject(Option<String>),
    /// A click on a project's ranking row: filter on it, or off if it is.
    TogglePick(String),
    ToggleDailyTable,
    ToggleHeatTable,
    ToggleAllProjects,
    ProjectMenu(bool),
    Refresh,
    /// The cache's answer, which is shown while the scan runs.
    Cached(Result<Arc<UsageStats>, String>),
    /// The scan's.
    Loaded(Result<Arc<UsageStats>, String>),
    /// The worktrees of each workspace, for the request numbered so.
    Worktrees(u64, HashMap<String, Vec<String>>),
    /// A summary worked out off the UI thread, for the request numbered so
    /// and the choices it was made for (`None`: the thread failed).
    Computed(u64, ComputedFor, Option<Arc<Computed>>),
    Chart(ChartId, charts::Event),
    /// F6 landed on the page: the first chart takes the keys.
    FocusChart(ChartId),
    Key(ChartKey),
    /// The spinner turns.
    Tick,
}

fn say(msg: Msg) -> Message {
    Message::Stats(msg)
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let mut tasks = Vec::new();
    // What moves only the readouts or the spinner changes no summary.
    let moves_summary = !matches!(
        msg,
        Msg::Tick | Msg::Chart(..) | Msg::Key(_) | Msg::FocusChart(_)
    );
    match msg {
        Msg::Mounted => {
            st.refresh_roots(m);
            // The worktrees are asked again at every visit, like the web
            // page's effect did at every mount.
            tasks.push(ask_worktrees(st, m, true));
            let host = sv.core().host();
            let shared = cache::shared(host);
            let live = &mut m.live.stats;
            // A scan Hangar Mobile or an earlier visit made is as good as
            // one of ours.
            if let Some(last) = shared.fresh_last()
                && live.checked_at.is_none_or(|at| at < last.at)
            {
                live.set_data(last.data.clone());
                live.checked_at = Some(last.at);
                live.error = None;
            }
            if !live.is_fresh() {
                tasks.push(refresh(m, sv));
            }
        }
        Msg::Refresh => tasks.push(refresh(m, sv)),
        Msg::Cached(answer) => {
            let live = &mut m.live.stats;
            // An empty cache is no answer yet: the scan's is the first one.
            if let Ok(data) = answer
                && live.data.is_none()
                && !data.buckets.is_empty()
            {
                live.set_data(data);
            }
            tasks.push(scan(sv));
        }
        Msg::Loaded(answer) => {
            let live = &mut m.live.stats;
            live.refreshing = false;
            live.refresh_started = None;
            match answer {
                Ok(data) => {
                    live.set_data(data);
                    live.error = None;
                    live.checked_at = Some(time::now_ms());
                }
                Err(error) => live.error = Some(error),
            }
        }
        Msg::Worktrees(seq, found) => {
            if seq == st.worktrees_seq {
                st.worktrees = found;
                st.rebuild_roots(m);
            }
        }
        Msg::Computed(seq, key, computed) => st.land(seq, key, computed),
        Msg::SetPeriod(period) => {
            st.period = period;
            st.clear_readouts();
        }
        Msg::SetMeasure(measure) => st.measure = measure,
        Msg::SetProject(project) => {
            st.project = project;
            st.menu_open = false;
            st.clear_readouts();
        }
        Msg::TogglePick(key) => {
            st.project = if st.project.as_deref() == Some(key.as_str()) {
                None
            } else {
                Some(key)
            };
            st.clear_readouts();
        }
        Msg::ToggleDailyTable => st.daily_table = !st.daily_table,
        Msg::ToggleHeatTable => st.heat_table = !st.heat_table,
        Msg::ToggleAllProjects => st.all_projects = !st.all_projects,
        Msg::ProjectMenu(open) => st.menu_open = open,
        Msg::Chart(chart, event) => chart_event(st, chart, event),
        Msg::FocusChart(chart) => {
            st.focus = Some(chart);
            st.keyboard = true;
            match chart {
                ChartId::Columns => {
                    let last = st
                        .last_computed()
                        .map(|computed| computed.summary.series.len())
                        .filter(|len| *len > 0)
                        .map(|len| len - 1);
                    st.columns = st.columns.or(last);
                }
                ChartId::Heat => st.heat = st.heat.or_else(today_cell),
            }
        }
        Msg::Key(key) => walk(st, key),
        Msg::Tick => {}
    }
    if moves_summary {
        tasks.push(st.recompute(m));
    }
    Task::batch(tasks)
}

fn today_cell() -> Option<(usize, usize)> {
    let now = time::now_ms();
    Some((time::weekday_of(now), time::hour_of(now)))
}

fn chart_event(st: &mut State, chart: ChartId, event: charts::Event) {
    match event {
        charts::Event::Hover(Some((first, second))) => {
            // The pointer takes the readout back from the keys.
            st.keyboard = false;
            match chart {
                ChartId::Columns => st.columns = Some(first),
                ChartId::Heat => st.heat = Some((first, second)),
            }
        }
        charts::Event::Hover(None) => {
            if !st.keyboard {
                match chart {
                    ChartId::Columns => st.columns = None,
                    ChartId::Heat => st.heat = None,
                }
            }
        }
        charts::Event::Focus => st.focus = Some(chart),
        charts::Event::Blur => {
            if st.focus == Some(chart) {
                st.focus = None;
                // The keys show the readout; without them it goes.
                if st.keyboard {
                    st.clear_readouts();
                    st.keyboard = false;
                }
            }
        }
    }
}

/// The arrow keys walking a chart: the readout follows them.
fn walk(st: &mut State, key: ChartKey) {
    let Some(chart) = st.focus else {
        return;
    };
    if key == ChartKey::Escape {
        match chart {
            ChartId::Columns => st.columns = None,
            ChartId::Heat => st.heat = None,
        }
        return;
    }
    match chart {
        ChartId::Columns => {
            let Some(len) = st
                .last_computed()
                .map(|computed| computed.summary.series.len())
                .filter(|len| *len > 0)
            else {
                return;
            };
            let last = len - 1;
            let from = st.columns.unwrap_or(last).min(last);
            let next = match key {
                ChartKey::Left => from.saturating_sub(1),
                ChartKey::Right => (from + 1).min(last),
                ChartKey::Home => 0,
                ChartKey::End => last,
                _ => return,
            };
            st.keyboard = true;
            st.columns = Some(next);
        }
        ChartId::Heat => {
            let (day, hour) = st.heat.or_else(today_cell).unwrap_or((0, 0));
            let next = match key {
                ChartKey::Up => (day.saturating_sub(1), hour),
                ChartKey::Down => ((day + 1).min(6), hour),
                ChartKey::Left => (day, hour.saturating_sub(1)),
                ChartKey::Right => (day, (hour + 1).min(23)),
                ChartKey::Home => (day, 0),
                ChartKey::End => (day, 23),
                ChartKey::Escape => return,
            };
            st.keyboard = true;
            st.heat = Some(next);
        }
    }
}

/// Reads the transcripts: the cache first (instant, even right after a
/// launch, from the copy the last run left on disk) when the page has nothing
/// yet, then a scan for whatever changed since. Asking while one runs joins it.
fn refresh(m: &mut Model, sv: &Services) -> Task<Message> {
    let live = &mut m.live.stats;
    if live.refreshing {
        return Task::none();
    }
    live.refreshing = true;
    live.refresh_started = Some(Instant::now());
    if live.data.is_some() {
        scan(sv)
    } else {
        let shared = cache::shared(sv.core().host()).clone();
        Task::perform(async move { shared.cached_only().await }, |answer| {
            say(Msg::Cached(answer))
        })
    }
}

fn scan(sv: &Services) -> Task<Message> {
    let shared = cache::shared(sv.core().host()).clone();
    Task::perform(async move { shared.scan().await }, |answer| {
        say(Msg::Loaded(answer))
    })
}

/// Worktrees live beside their repository, under names of their own: git
/// knows which belong to which workspace. Asked at a visit, and again only
/// when a workspace's folder changes.
fn ask_worktrees(st: &mut State, m: &Model, force: bool) -> Task<Message> {
    let workspaces: Vec<(String, String)> = m
        .state()
        .workspaces
        .iter()
        .map(|ws| (ws.id.clone(), ws.cwd.clone()))
        .collect();
    let repos = workspaces
        .iter()
        .map(|(id, cwd)| format!("{id}\n{cwd}"))
        .collect::<Vec<_>>()
        .join("\n");
    if !force && repos == st.repos {
        return Task::none();
    }
    st.repos = repos;
    st.worktrees_seq += 1;
    let seq = st.worktrees_seq;
    Task::perform(
        async move {
            let asked = workspaces.into_iter().map(|(id, cwd)| async move {
                let paths = hangar_core::git::git_worktree_list(cwd)
                    .await
                    .map(|list| list.into_iter().map(|tree| tree.path).collect())
                    .unwrap_or_default();
                (id, paths)
            });
            iced::futures::future::join_all(asked)
                .await
                .into_iter()
                .collect::<HashMap<String, Vec<String>>>()
        },
        move |found| say(Msg::Worktrees(seq, found)),
    )
}

pub fn subscription(_st: &State, m: &Model) -> Subscription<Message> {
    if m.ui.view != View::Stats {
        return Subscription::none();
    }
    // Lives while the page is on screen, so coming back starts it again: the
    // page's "mounted".
    let mounted = Subscription::run(|| iced::futures::stream::once(async { say(Msg::Mounted) }));
    if m.live.stats.refreshing {
        Subscription::batch([
            mounted,
            iced::time::every(std::time::Duration::from_millis(80)).map(|_| say(Msg::Tick)),
        ])
    } else {
        mounted
    }
}

/// What takes the focus when F6 reaches region `Main`, which this package
/// owns: the period's chart, which then answers the arrow keys.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    Some(Task::done(say(Msg::FocusChart(ChartId::Columns))))
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it. Only a change of the workspaces concerns the page.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    _sv: &Services,
    _intent: &Intent,
) -> Option<Task<Message>> {
    // Only the page on screen follows the workspaces; a visit catches up.
    if m.ui.view != View::Stats || !st.refresh_roots(m) {
        return None;
    }
    // A change of the workspaces' folders is worth asking git again.
    Some(Task::batch([ask_worktrees(st, m, false), st.recompute(m)]))
}

/// Every core event, after the shell's own handling of it. The statistics
/// read the transcripts when asked; no event changes them.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// Region `main` in the stats view.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let reading = st.columns.is_some() || st.heat.is_some();
    let focus = st.focus;
    iced_keys(view::view(st, ctx), focus, reading)
}

/// The page, with the arrow keys given to the chart that has them.
fn iced_keys<'a>(
    page: Element<'a, Message>,
    focus: Option<ChartId>,
    reading: bool,
) -> Element<'a, Message> {
    hangar_ui::keys::on_key(page, move |key, modifiers| {
        chart_key(focus, reading, key, modifiers).map(|key| say(Msg::Key(key)))
    })
}

#[cfg(test)]
mod tests;
