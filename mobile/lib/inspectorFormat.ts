import type { RateLimit } from "./bridge";

/**
 * The inspector's readings the phone shares with the desktop: the tone of a
 * gauge, a model's spoken name, a plan window's usage. Pure, so every rule
 * here has a test.
 *
 * The desktop's src/lib/inspectorFormat.ts (tag v1.12.1) also worded times and
 * costs in the desktop's own dictionary; the phone words those itself
 * (src/format.ts), so only what both read the same way is kept here.
 */

export type Tone = "normal" | "warn" | "danger";

/** Amber from 70 %, red from 90 %: the same reading for context and plan. */
export function usageTone(pct: number): Tone {
  if (pct >= 90) return "danger";
  if (pct >= 70) return "warn";
  return "normal";
}

const FAMILIES = ["opus", "sonnet", "haiku", "fable"];
const capitalize = (word: string) => word.charAt(0).toUpperCase() + word.slice(1);

/**
 * "claude-sonnet-4-5-20250929" → "Sonnet 4.5", for the header's sub-line.
 * The Session section still shows the id as the agent gave it; anything not
 * recognised as a Claude model is returned untouched rather than guessed at.
 */
export function prettyModel(model: string | null): string | null {
  if (!model) return null;
  const id = model.trim().toLowerCase().replace(/\[.*\]$/, "");
  // Current naming: family first, then the version.
  const modern =
    /(?:^|claude-)(opus|sonnet|haiku|fable)-(\d+)(?:[-.](\d{1,2}))?(?:-\d{8})?$/.exec(id);
  if (modern) {
    const [, family, major, minor] = modern;
    return `${capitalize(family)} ${minor ? `${major}.${minor}` : major}`;
  }
  // The Claude 3 naming put the version first.
  const legacy = /^claude-(\d+)(?:-(\d))?-(opus|sonnet|haiku)/.exec(id);
  if (legacy) {
    const [, major, minor, family] = legacy;
    return `${capitalize(family)} ${minor ? `${major}.${minor}` : major}`;
  }
  if (FAMILIES.includes(id)) return capitalize(id);
  return model.trim();
}

/** A plan window's usage, 0–100, or null when no session reported it. */
export function planUsage(
  limits: RateLimit[] | null | undefined,
  kind: string,
): RateLimit | null {
  const hit = limits?.find((limit) => limit.kind === kind);
  if (!hit) return null;
  return { ...hit, percentUsed: Math.max(0, Math.min(100, Math.round(hit.percentUsed))) };
}
