//! `remote_stats` (lib/remoteControl.ts and `src/remote/stats.ts`): what the
//! statistics page shows, summed up the page's way (P11's `summarize`) over
//! every project, cut down to what a phone draws: never the hourly rows.
//!
//! The statistics come from P11's cache (`stats::usage`), fresh for a minute
//! and shared with the page; a scan that takes more than 6 s answers from the
//! cache, within the 10 s the mobile server gives the window.

use std::collections::HashMap;

use serde_json::{Value, json};

use hangar_core::stats::Agent;

use crate::control::port::Port;
use crate::state::types::{AGENTS, AgentId};
use crate::stats::model::{
    Granularity, Measure, PERIODS, Period, Ranked, Summary, SummaryOptions, Totals, project_for,
    summarize, workspace_roots,
};

use super::chat_live::Plan;
use super::contract::{
    RemoteStats, StatsOther, StatsPlan, StatsPoint, StatsRange, StatsRanking, StatsRow,
    StatsTotals, StatsWindow,
};
use super::engine::Mobile;

/// Rows of a ranking the phone gets; past them, the last one stands for the
/// rest.
pub const RANKED_ROWS: usize = 6;

const MEASURES: [(&str, Measure); 2] = [("tokens", Measure::Tokens), ("cost", Measure::Cost)];

fn totals_of(totals: &Totals) -> StatsTotals {
    StatsTotals {
        tokens: totals.tokens,
        cache_read: totals.cache_read,
        cost: totals.cost,
        active_ms: totals.active_ms,
        sessions: totals.sessions,
        turns: totals.turns,
    }
}

/// The first rows as they are; past `RANKED_ROWS`, the rest folded into one
/// (`ranking`).
pub fn ranking(rows: &[Ranked]) -> StatsRanking {
    let kept = if rows.len() > RANKED_ROWS {
        &rows[..RANKED_ROWS - 1]
    } else {
        rows
    };
    let rest = &rows[kept.len()..];
    StatsRanking {
        rows: kept
            .iter()
            .map(|row| StatsRow {
                key: row.key.clone(),
                label: row.label.clone(),
                tokens: row.totals.tokens,
                cost: row.totals.cost,
                active_ms: row.totals.active_ms,
                sessions: row.totals.sessions,
                share: row.share,
                unpriced: row.unpriced.then_some(true),
            })
            .collect(),
        other: (!rest.is_empty()).then(|| StatsOther {
            count: rest.len(),
            tokens: rest.iter().map(|row| row.totals.tokens).sum(),
            cost: rest.iter().map(|row| row.totals.cost).sum(),
            active_ms: rest.iter().map(|row| row.totals.active_ms).sum(),
            share: rest.iter().map(|row| row.share).sum(),
        }),
    }
}

/// `Math.round`: halves go up, as JavaScript rounds them.
fn round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// The plan's windows, whole percents (as `planUsage` reads them) and reset
/// times in ms (`planWindows`).
pub fn plan_windows(plan: &Plan) -> Vec<StatsWindow> {
    plan.limits
        .iter()
        .map(|limit| StatsWindow {
            kind: limit.kind.clone(),
            percent_used: round(limit.percent_used).clamp(0.0, 100.0) as u32,
            resets_at: limit
                .resets_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.timestamp_millis()),
        })
        .collect()
}

/// What the measure reads of a column: whole tokens, or dollars.
fn measured(totals: &Totals, measure: Measure) -> Value {
    match measure {
        Measure::Tokens => json!(totals.tokens),
        Measure::Cost => json!(totals.cost),
    }
}

pub struct StatsOf {
    pub period: Period,
    pub measure: Measure,
    pub read_at: i64,
    pub cached_only: bool,
    pub plan: Option<Plan>,
}

/// What `remote_stats` answers, from the statistics page's own summary
/// (`remoteStatsOf`).
pub fn remote_stats_of(summary: &Summary, of: StatsOf) -> RemoteStats {
    let measure_id = MEASURES
        .iter()
        .find(|(_, measure)| *measure == of.measure)
        .map_or("tokens", |(id, _)| id);
    RemoteStats {
        period: of.period.id(),
        measure: measure_id,
        read_at: of.read_at,
        cached_only: of.cached_only,
        range: StatsRange {
            from: summary.range.from,
            to: summary.range.to,
            days: summary.range.days,
        },
        granularity: match summary.granularity {
            Granularity::Day => "day",
            Granularity::Week => "week",
        },
        first_at: summary.first_at,
        totals: totals_of(&summary.totals),
        previous: summary.previous.as_ref().map(totals_of),
        active_days: summary.active_days,
        series: summary
            .series
            .iter()
            .map(|point| StatsPoint {
                at: point.at,
                value: measured(&point.totals, of.measure),
            })
            .collect(),
        projects: ranking(&summary.projects),
        agents: ranking(&summary.agents),
        models: ranking(&summary.models),
        unpriced_tokens: summary.unpriced_tokens,
        plan: of.plan.as_ref().map(|plan| StatsPlan {
            at: plan.at,
            windows: plan_windows(plan),
        }),
    }
}

/// A choice among `options`, the default when none is given (`choice`).
fn choice<T: Copy>(
    value: Option<&Value>,
    options: &[(&str, T)],
    fallback: T,
    name: &str,
) -> Result<T, String> {
    let given = match value {
        None | Some(Value::Null) => return Ok(fallback),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    };
    options
        .iter()
        .find(|(id, _)| *id == given)
        .map(|(_, option)| *option)
        .ok_or_else(|| {
            let ids: Vec<&str> = options.iter().map(|(id, _)| *id).collect();
            format!("Unknown {name} \"{given}\". One of: {}.", ids.join(", "))
        })
}

fn agent_label(agent: Agent) -> String {
    let id = match agent {
        Agent::Claude => AgentId::Claude,
        Agent::Codex => AgentId::Codex,
    };
    AGENTS
        .iter()
        .find(|entry| entry.id == id)
        .map_or_else(|| id.as_str().to_string(), |entry| entry.label.to_string())
}

/// `remote_stats {period?, measure?}`.
pub async fn remote_stats(
    port: &Port,
    mobile: &Mobile,
    args: &Value,
) -> Result<RemoteStats, String> {
    let periods: Vec<(&str, Period)> = PERIODS
        .iter()
        .map(|period| (period.id(), *period))
        .collect();
    let period = choice(args.get("period"), &periods, Period::D30, "period")?;
    let measure = choice(args.get("measure"), &MEASURES, Measure::Tokens, "measure")?;
    let snapshot = port.snapshot();
    let workspaces = &snapshot.state.workspaces;
    let sources = mobile.sources();
    let trees = iced::futures::future::join_all(workspaces.iter().map(|ws| async move {
        // Not a repository, or no git: the workspace's own folders say enough.
        let paths = sources.worktrees(&ws.cwd).await.unwrap_or_default();
        (ws.id.clone(), paths)
    }));
    let (data, trees) = tokio::join!(sources.usage(), trees);
    let data = data?;
    let worktrees: HashMap<String, Vec<String>> = trees.into_iter().collect();
    let roots = workspace_roots(workspaces, &worktrees);
    let projects: Vec<_> = data
        .sessions
        .iter()
        .map(|session| project_for(&session.cwd, &roots))
        .collect();
    let summary = summarize(
        &data,
        &SummaryOptions {
            period,
            project: None,
            measure,
            now: sources.stats_now(),
            projects: &projects,
            agent_label: &agent_label,
            top_sessions: 0,
        },
    );
    Ok(remote_stats_of(
        &summary,
        StatsOf {
            period,
            measure,
            read_at: data.generated_at,
            cached_only: data.scan.cached_only,
            plan: mobile.plan(),
        },
    ))
}
