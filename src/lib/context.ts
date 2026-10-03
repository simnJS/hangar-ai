import type { AgentId } from "../types";

/** What the Rust side read out of the agent's own transcript. */
export interface ContextUsage {
  usedTokens: number;
  model: string | null;
  contextWindow: number | null;
}

/** What the pane header draws: a fill percentage and its tooltip material. */
export interface ContextGauge {
  /** 0–100, toward the auto-compaction boundary. */
  pct: number;
  usedTokens: number;
  window: number;
  model: string | null;
}

/**
 * Context-window sizes and compaction boundaries, as configuration rather
 * than constants scattered through the code: they move with agent and model
 * releases, and this table is the one place to correct when they do.
 *
 * Sources, read on 2026-08-11:
 * - Windows: platform.claude.com/docs/en/about-claude/models/overview
 * - Claude Code compacts at the model's limit unless the user lowers it
 *   (`autoCompactWindow`): code.claude.com/docs/en/context-window
 * - Codex compacts at unpublished per-model defaults, and its transcript
 *   carries the window itself (`model_context_window`):
 *   learn.chatgpt.com/docs/config-file/config-reference
 *
 * What this table cannot see, and the gauge therefore ignores: a user-lowered
 * auto-compact window, and deployments pinned to 200k (Bedrock,
 * CLAUDE_CODE_DISABLE_1M_CONTEXT) — the transcript does not say.
 */
export const CONTEXT_GAUGE_CONFIG = {
  /** Claude model id fragment → window tokens; first match wins. */
  claudeWindows: [
    ["fable-5", 1_000_000],
    ["opus-5", 1_000_000],
    ["sonnet-5", 1_000_000],
    ["haiku-4-5", 200_000],
    ["opus-4-8", 1_000_000],
    ["opus-4-7", 1_000_000],
    ["opus-4-6", 1_000_000],
    ["sonnet-4-6", 1_000_000],
    ["sonnet-4-5", 200_000],
    ["opus-4-5", 200_000],
  ] as [string, number][],
  /**
   * Fraction of the window where the agent compacts, per agent. 1 for both
   * today: Claude Code's default boundary is the window itself, and Codex's
   * true value is model-tuned and unpublished — promising an earlier
   * compaction point the agent does not honour would make the gauge lie.
   */
  compactFraction: { claude: 1, codex: 1 } as Record<string, number>,
};

function claudeWindow(model: string | null): number | null {
  if (!model) return null;
  const hit = CONTEXT_GAUGE_CONFIG.claudeWindows.find(([fragment]) =>
    model.includes(fragment),
  );
  return hit ? hit[1] : null;
}

/**
 * Turns a transcript reading into what the gauge shows, or nothing when the
 * window is unknown — an unrecognised model gets no bar rather than a made-up
 * percentage.
 */
export function computeContextGauge(
  agent: AgentId,
  usage: ContextUsage,
): ContextGauge | null {
  const window =
    usage.contextWindow ?? (agent === "claude" ? claudeWindow(usage.model) : null);
  if (!window || usage.usedTokens <= 0) return null;
  const fraction = CONTEXT_GAUGE_CONFIG.compactFraction[agent] ?? 1;
  const limit = Math.max(1, Math.round(window * fraction));
  // Clamped: right after a compaction the numbers fall, and a mid-write
  // transcript must never push the bar past its box or below zero.
  const pct = Math.min(100, Math.max(0, Math.round((usage.usedTokens / limit) * 100)));
  return { pct, usedTokens: usage.usedTokens, window, model: usage.model };
}

/** 166234 → "166k", 1000000 → "1M": the pane header has no room for digits. */
export function formatTokens(count: number): string {
  if (count >= 1_000_000) {
    const millions = count / 1_000_000;
    return `${millions >= 10 || Number.isInteger(millions) ? Math.round(millions) : millions.toFixed(1)}M`;
  }
  if (count >= 1000) return `${Math.round(count / 1000)}k`;
  return `${count}`;
}
