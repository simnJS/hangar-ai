//! The context gauge of a pane header (`lib/context.ts`): which window an
//! agent's model has, and how full it is.

use hangar_core::context::ContextUsage;

use crate::state::live::ContextGauge;
use crate::state::types::AgentId;

/// Context-window sizes and compaction boundaries, as configuration rather
/// than constants scattered through the code: they move with agent and model
/// releases, and this table is the one place to correct when they do.
///
/// Sources, read on 2026-08-11 (by the TS this is ported from):
/// - Windows: platform.claude.com/docs/en/about-claude/models/overview
/// - Claude Code compacts at the model's limit unless the user lowers it
///   (`autoCompactWindow`): code.claude.com/docs/en/context-window
/// - Codex compacts at unpublished per-model defaults, and its transcript
///   carries the window itself (`model_context_window`):
///   learn.chatgpt.com/docs/config-file/config-reference
///
/// What this table cannot see, and the gauge therefore ignores: a
/// user-lowered auto-compact window, and deployments pinned to 200k
/// (Bedrock, `CLAUDE_CODE_DISABLE_1M_CONTEXT`); the transcript does not say.
pub struct ContextGaugeConfig {
    /// Claude model id fragment and its window in tokens; the first match
    /// wins.
    pub claude_windows: &'static [(&'static str, u64)],
    /// Fraction of the window where the agent compacts, per agent. 1 for both
    /// today: Claude Code's default boundary is the window itself, and
    /// Codex's true value is model-tuned and unpublished. Promising an
    /// earlier compaction point the agent does not honour would make the
    /// gauge lie.
    pub compact_fraction: &'static [(AgentId, f64)],
}

pub const CONTEXT_GAUGE_CONFIG: ContextGaugeConfig = ContextGaugeConfig {
    claude_windows: &[
        ("fable-5", 1_000_000),
        ("opus-5", 1_000_000),
        ("sonnet-5", 1_000_000),
        ("haiku-4-5", 200_000),
        ("opus-4-8", 1_000_000),
        ("opus-4-7", 1_000_000),
        ("opus-4-6", 1_000_000),
        ("sonnet-4-6", 1_000_000),
        ("sonnet-4-5", 200_000),
        ("opus-4-5", 200_000),
    ],
    compact_fraction: &[(AgentId::Claude, 1.0), (AgentId::Codex, 1.0)],
};

fn claude_window(model: Option<&str>) -> Option<u64> {
    let model = model?;
    CONTEXT_GAUGE_CONFIG
        .claude_windows
        .iter()
        .find(|(fragment, _)| model.contains(fragment))
        .map(|(_, window)| *window)
}

/// Turns a transcript reading into what the gauge shows, or nothing when the
/// window is unknown: an unrecognised model gets no bar rather than a
/// made-up percentage.
pub fn compute_context_gauge(agent: AgentId, usage: &ContextUsage) -> Option<ContextGauge> {
    // A declared window of zero is "no window", as `??` then `!window` read
    // it in the TS: it does not fall back on the table.
    let window = usage
        .context_window
        .or_else(|| {
            if agent == AgentId::Claude {
                claude_window(usage.model.as_deref())
            } else {
                None
            }
        })
        .filter(|window| *window > 0)?;
    if usage.used_tokens == 0 {
        return None;
    }
    let fraction = CONTEXT_GAUGE_CONFIG
        .compact_fraction
        .iter()
        .find(|(id, _)| *id == agent)
        .map_or(1.0, |(_, fraction)| *fraction);
    let limit = ((window as f64 * fraction).round() as u64).max(1);
    // Clamped: right after a compaction the numbers fall, and a mid-write
    // transcript must never push the bar past its box or below zero.
    let pct = (usage.used_tokens as f64 / limit as f64 * 100.0)
        .round()
        .clamp(0.0, 100.0);
    Some(ContextGauge {
        pct,
        used_tokens: usage.used_tokens,
        window,
        model: usage.model.clone(),
    })
}

/// 166234 becomes "166k", 1000000 becomes "1M": the pane header has no room
/// for digits.
pub fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        let millions = count as f64 / 1_000_000.0;
        return if millions >= 10.0 || millions.fract() == 0.0 {
            format!("{}M", millions.round() as u64)
        } else {
            format!("{millions:.1}M")
        };
    }
    if count >= 1000 {
        return format!("{}k", (count as f64 / 1000.0).round() as u64);
    }
    count.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(used: u64, model: Option<&str>, window: Option<u64>) -> ContextUsage {
        ContextUsage {
            used_tokens: used,
            model: model.map(str::to_string),
            context_window: window,
        }
    }

    #[test]
    fn reads_a_claude_window_off_the_model_name() {
        let gauge = compute_context_gauge(
            AgentId::Claude,
            &usage(166_234, Some("claude-sonnet-4-5"), None),
        )
        .unwrap();
        assert_eq!(gauge.window, 200_000);
        assert_eq!(gauge.pct, 83.0);
        assert_eq!(gauge.used_tokens, 166_234);
        assert_eq!(gauge.model.as_deref(), Some("claude-sonnet-4-5"));

        let big = compute_context_gauge(
            AgentId::Claude,
            &usage(500_000, Some("claude-opus-4-8[1m]"), None),
        )
        .unwrap();
        assert_eq!((big.window, big.pct), (1_000_000, 50.0));
    }

    #[test]
    fn shows_no_bar_for_an_unknown_window_or_an_empty_context() {
        assert!(
            compute_context_gauge(AgentId::Claude, &usage(1000, Some("mystery-1"), None)).is_none()
        );
        assert!(compute_context_gauge(AgentId::Claude, &usage(1000, None, None)).is_none());
        assert!(
            compute_context_gauge(AgentId::Claude, &usage(0, Some("claude-opus-5"), None))
                .is_none()
        );
        // Another agent's transcript has to name its window itself.
        assert!(
            compute_context_gauge(AgentId::Gemini, &usage(1000, Some("claude-opus-5"), None))
                .is_none()
        );
    }

    #[test]
    fn trusts_a_window_the_transcript_declares() {
        let gauge = compute_context_gauge(
            AgentId::Codex,
            &usage(98_823, Some("gpt-5.6"), Some(353_400)),
        )
        .unwrap();
        assert_eq!((gauge.window, gauge.pct), (353_400, 28.0));
        // It beats the table, even for Claude.
        let declared = compute_context_gauge(
            AgentId::Claude,
            &usage(50_000, Some("claude-opus-5"), Some(100_000)),
        )
        .unwrap();
        assert_eq!((declared.window, declared.pct), (100_000, 50.0));
    }

    #[test]
    fn clamps_the_percentage_into_the_box() {
        let over = compute_context_gauge(
            AgentId::Claude,
            &usage(300_000, Some("claude-haiku-4-5"), None),
        )
        .unwrap();
        assert_eq!(over.pct, 100.0);
    }

    #[test]
    fn writes_token_counts_short() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1k");
        assert_eq!(format_tokens(166_234), "166k");
        assert_eq!(format_tokens(999_999), "1000k");
        assert_eq!(format_tokens(1_000_000), "1M");
        assert_eq!(format_tokens(1_500_000), "1.5M");
        assert_eq!(format_tokens(12_400_000), "12M");
    }
}
