/**
 * The usage statistics, made up.
 *
 * The real ones come from the transcripts Claude Code and Codex leave on
 * disk (src-tauri/src/stats.rs). A browser has none, so this draws four
 * months of plausible work instead: weekdays busier than weekends, mornings
 * and late afternoons busier than lunch, most of it on hangar-ai, a share on
 * storefront, and a few folders no workspace claims. A seeded generator
 * keeps the page the same from one reload to the next, and it is drawn
 * relative to today so the last days are never empty.
 */
import type { StatsBucket, StatsSession, UsageStats } from "../../src/lib/stats";

/** mulberry32: small, seeded, good enough to look like life. */
function generator(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let x = state;
    x = Math.imul(x ^ (x >>> 15), x | 1);
    x ^= x + Math.imul(x ^ (x >>> 7), x | 61);
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}

/** Per million tokens: input, output, cache write, cache read — as in pricing.rs. */
const PRICES: Record<string, [number, number, number, number]> = {
  "claude-opus-5-5": [4, 20, 5, 0.2],
  "claude-sonnet-5-5": [2, 10, 2.5, 0.1],
  "claude-haiku-5-5": [0.1, 0.5, 0.125, 0.01],
  "gpt-5.5": [5, 30, 5, 0.5],
  "gpt-6-astra": [10, 50, 10, 1],
};
const MODELS = [...Object.keys(PRICES), "gpt-5.5-codex"];

interface Project {
  cwd: string;
  weight: number;
  titles: string[];
}

const PROJECTS: Project[] = [
  {
    cwd: "C:\\dev\\hangar-ai",
    weight: 0.52,
    titles: [
      "Pane zoom keeps the PTY alive",
      "Board drag and drop between columns",
      "Usage statistics page",
      "Codex rollout reader",
      "Inspector context gauge",
      "Fix flaky worktree test",
      "Voice dictation hotkey",
      "Command palette fuzzy match",
      "Detach a workspace to its own window",
      "MCP pane_send retries",
    ],
  },
  {
    cwd: "C:\\dev\\hangar-ai-stats",
    weight: 0.08,
    titles: ["Heatmap keyboard navigation", "Stats cache on disk"],
  },
  {
    cwd: "C:\\dev\\storefront",
    weight: 0.24,
    titles: [
      "Checkout rounding in cents",
      "Product grid lazy images",
      "Stripe webhook retries",
      "Cart badge animation",
      "Search facets",
    ],
  },
  {
    cwd: "C:\\dev\\notes-api",
    weight: 0.1,
    titles: ["Rate limiter", "Migrate to Postgres 17", "OpenAPI schema"],
  },
  {
    cwd: "C:\\Users\\demo\\scratch",
    weight: 0.06,
    titles: ["Quick regex question", "Explain this stack trace"],
  },
];

const HOUR = 3_600_000;

/** How busy an hour of the day is, 0–1: mornings, late afternoons, a little at night. */
function hourWeight(hour: number): number {
  const curve = [
    0.02, 0.01, 0, 0, 0, 0, 0.02, 0.1, 0.35, 0.8, 1, 0.9, 0.45, 0.4, 0.75, 0.95, 1, 0.85, 0.55,
    0.35, 0.3, 0.4, 0.3, 0.1,
  ];
  return curve[hour];
}

let cached: UsageStats | null = null;

export function usageStats(args: { from?: number; to?: number; cachedOnly?: boolean }): UsageStats {
  if (!cached) cached = generate();
  const { from, to } = args;
  const keep = (at: number) => (from === undefined || at >= from) && (to === undefined || at < to);
  return {
    ...cached,
    // The cached answer is the same work, said to come from the last run.
    scan: { ...cached.scan, cachedOnly: Boolean(args.cachedOnly) },
    buckets: from === undefined && to === undefined ? cached.buckets : cached.buckets.filter((row) => keep(row.at)),
  };
}

function generate(): UsageStats {
  const random = generator(20261026);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const sessions: StatsSession[] = [];
  const buckets: StatsBucket[] = [];
  const rows = new Map<string, StatsBucket>();
  const row = (session: number, at: number, model: number): StatsBucket => {
    const key = `${session}:${at}:${model}`;
    let found = rows.get(key);
    if (!found) {
      found = {
        session,
        at,
        model,
        input: 0,
        output: 0,
        cacheWrite: 0,
        cacheRead: 0,
        cost: 0,
        costInput: 0,
        costOutput: 0,
        costCacheWrite: 0,
        costCacheRead: 0,
        calls: 0,
        turns: 0,
        activeMs: 0,
      };
      rows.set(key, found);
      buckets.push(found);
    }
    return found;
  };
  const pickProject = () => {
    let roll = random();
    for (const project of PROJECTS) {
      roll -= project.weight;
      if (roll <= 0) return project;
    }
    return PROJECTS[0];
  };

  // 118 days back, ramping up: the app got more use as it got better.
  for (let back = 118; back >= 0; back--) {
    const day = new Date(today);
    day.setDate(day.getDate() - back);
    const weekday = (day.getDay() + 6) % 7;
    const ramp = 0.45 + 0.55 * (1 - back / 118);
    const busy = (weekday >= 5 ? 0.3 : 1) * ramp;
    const count = Math.round(busy * (1.5 + random() * 3.5));
    for (let n = 0; n < count; n++) {
      // A start hour drawn from the day's rhythm.
      let hour = 9;
      for (let tries = 0; tries < 30; tries++) {
        const candidate = Math.floor(random() * 24);
        if (random() < hourWeight(candidate)) {
          hour = candidate;
          break;
        }
      }
      const project = pickProject();
      const codex = random() < 0.22;
      const index = sessions.length;
      const start = day.getTime() + hour * HOUR + Math.floor(random() * 50) * 60_000;
      const lengthHours = 0.3 + random() * random() * 4;
      const end = start + lengthHours * HOUR;
      const mainModel = codex ? (random() < 0.6 ? 3 : 4) : random() < 0.8 ? 0 : 1;
      sessions.push({
        agent: codex ? "codex" : "claude",
        id: `demo-${index.toString(16).padStart(4, "0")}`,
        cwd: project.cwd,
        title: project.titles[Math.floor(random() * project.titles.length)],
        firstAt: start,
        lastAt: end,
      });

      // Walk the session hour by hour: calls, prompts and active time.
      for (let at = start; at < end; at += HOUR) {
        const hourStart = new Date(at);
        hourStart.setMinutes(0, 0, 0);
        const slot = hourStart.getTime();
        const minutes = Math.min(60, (end - at) / 60_000);
        const activity = row(index, slot, -1);
        activity.activeMs += Math.round(minutes * (0.55 + random() * 0.4)) * 60_000;
        activity.turns += 1 + Math.floor(random() * 5 * (minutes / 60));
        const calls = Math.max(1, Math.round((minutes / 60) * (18 + random() * 40)));
        for (let call = 0; call < calls; call++) {
          // A subagent on Haiku now and then, on Claude's side.
          const model = !codex && random() < 0.12 ? 2 : random() < 0.04 && codex ? 5 : mainModel;
          const cell = row(index, slot, model);
          const input = Math.round(3 + random() * 40);
          const output = Math.round(200 + random() * random() * 4000);
          const cacheWrite = Math.round(random() < 0.3 ? 2000 + random() * 18000 : random() * 1200);
          const cacheRead = Math.round(40000 + random() * 220000);
          cell.input += input;
          cell.output += output;
          cell.cacheWrite += cacheWrite;
          cell.cacheRead += cacheRead;
          cell.calls += 1;
          const price = PRICES[MODELS[model]];
          if (price) {
            const costs = [input, output, cacheWrite, cacheRead].map((count, k) => (count * price[k]) / 1e6);
            cell.costInput += costs[0];
            cell.costOutput += costs[1];
            cell.costCacheWrite += costs[2];
            cell.costCacheRead += costs[3];
            cell.cost += costs[0] + costs[1] + costs[2] + costs[3];
          }
        }
      }
    }
  }

  // Never in the future: today stops at this hour.
  const now = Date.now();
  const kept = buckets.filter((entry) => entry.at <= now);
  kept.sort((a, b) => a.at - b.at || a.session - b.session || a.model - b.model);
  return {
    generatedAt: now,
    scan: { files: 412, parsed: 3, reused: 409, failed: 0, bytes: 1_840_000, ms: 64, cachedOnly: false },
    sources: [
      { agent: "claude", root: "C:\\Users\\demo\\.claude\\projects", found: true, files: 351 },
      { agent: "codex", root: "C:\\Users\\demo\\.codex\\sessions", found: true, files: 61 },
    ],
    activeGapCapMs: 5 * 60_000,
    sessions,
    models: MODELS.map((id) => ({ id, priced: id in PRICES })),
    buckets: kept,
  };
}
