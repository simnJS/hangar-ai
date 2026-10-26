import { describe, expect, it } from "vitest";
import type { StatsBucket, UsageStats } from "./stats";
import {
  addDays,
  formatCompact,
  formatDuration,
  formatShare,
  formatUsd,
  heatLevel,
  modelLabel,
  normalizePath,
  periodRange,
  projectFor,
  startOfDay,
  summarize,
  workspaceRoots,
  type Project,
} from "./statsModel";
import type { Translator } from "../i18n";
import type { Pane, Workspace } from "../types";

// Local times throughout, so the tests hold in any time zone.
const at = (day: number, hour = 10) => new Date(2026, 9, day, hour).getTime();
const NOW = at(26, 15);

const t: Translator = (key, params) => {
  const templates: Record<string, string> = {
    "stats.durationMinutes": "{m} min",
    "stats.durationHours": "{h} h {m}",
    "stats.durationHoursOnly": "{h} h",
    "stats.durationUnderMinute": "< 1 min",
  };
  return (templates[key] ?? key).replace(/\{(\w+)\}/g, (_, name) => String(params?.[name] ?? ""));
};

function workspace(id: string, cwd: string, extra: Partial<Workspace> = {}): Workspace {
  return {
    id,
    name: id,
    cwd,
    extraRoots: [],
    panes: [],
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
    ...extra,
  };
}

function bucket(session: number, when: number, patch: Partial<StatsBucket> = {}): StatsBucket {
  return {
    session,
    at: when,
    model: 0,
    input: 10,
    output: 20,
    cacheWrite: 30,
    cacheRead: 940,
    cost: 1,
    costInput: 0.1,
    costOutput: 0.5,
    costCacheWrite: 0.2,
    costCacheRead: 0.2,
    calls: 2,
    turns: 0,
    activeMs: 0,
    ...patch,
  };
}

function stats(buckets: StatsBucket[]): UsageStats {
  return {
    generatedAt: NOW,
    scan: { files: 2, parsed: 2, reused: 0, failed: 0, bytes: 0, ms: 1, cachedOnly: false },
    sources: [],
    activeGapCapMs: 300_000,
    sessions: [
      { agent: "claude", id: "a", cwd: "C:\\dev\\app", title: "Auth", firstAt: at(20), lastAt: at(25) },
      { agent: "codex", id: "b", cwd: "C:\\dev\\shop", title: "", firstAt: at(24), lastAt: at(24) },
    ],
    models: [
      { id: "claude-opus-5-5", priced: true },
      { id: "house-model", priced: false },
    ],
    buckets,
  };
}

const PROJECTS: Project[] = [
  { key: "ws:app", name: "app", path: "c:/dev/app", workspaceId: "app" },
  { key: "dir:c:/dev/shop", name: "shop", path: "C:\\dev\\shop", workspaceId: null },
];

const options = {
  period: "7d" as const,
  project: null,
  measure: "tokens" as const,
  now: NOW,
  projects: PROJECTS,
  agentLabel: (agent: string) => (agent === "claude" ? "Claude Code" : "Codex"),
};

describe("periodRange", () => {
  it("covers whole days up to today, and the same length before for the trend", () => {
    const range = periodRange("7d", NOW, null);
    expect(range.to).toBe(startOfDay(at(27)));
    expect(range.from).toBe(startOfDay(at(20)));
    expect(range.days).toBe(7);
    expect(range.previous).toEqual({ from: startOfDay(at(13)), to: startOfDay(at(20)) });
  });

  it("starts 'all' on the first day with data, with nothing to compare", () => {
    const range = periodRange("all", NOW, at(3, 22));
    expect(range.from).toBe(startOfDay(at(3)));
    expect(range.days).toBe(24);
    expect(range.previous).toBeNull();
  });
});

describe("projects", () => {
  it("spells a Windows path one way, case and slashes aside", () => {
    expect(normalizePath("C:\\Dev\\App\\")).toBe("c:/dev/app");
    expect(normalizePath("/home/Me/app/")).toBe("/home/Me/app");
    expect(normalizePath("C:\\")).toBe("c:/");
  });

  it("finds the workspace with the deepest root above a folder", () => {
    const roots = workspaceRoots(
      [
        workspace("mono", "C:\\dev\\mono"),
        workspace("web", "C:\\dev\\mono\\web"),
        workspace("app", "C:\\dev\\app", { extraRoots: ["D:\\shared\\lib"] }),
      ],
      { app: ["C:\\dev\\app-stats"] },
    );
    expect(projectFor("c:/DEV/mono/web/src", roots).key).toBe("ws:web");
    expect(projectFor("C:\\dev\\mono\\api", roots).key).toBe("ws:mono");
    expect(projectFor("C:\\dev\\app-stats", roots).key).toBe("ws:app");
    expect(projectFor("D:\\shared\\lib\\x", roots).name).toBe("app");
    // A sibling whose name merely starts the same is not inside.
    expect(projectFor("C:\\dev\\application", roots).key).toBe("dir:c:/dev/application");
    expect(projectFor("C:\\dev\\application", roots).name).toBe("application");
  });

  it("counts a pane's folder, unless it sits above the workspace", () => {
    const pane = (cwd: string) => ({ id: cwd, cwd }) as unknown as Pane;
    const roots = workspaceRoots([
      workspace("app", "C:\\Users\\me\\dev\\app", {
        panes: [pane("C:\\Users\\me"), pane("C:\\Users\\me\\dev\\app-wt")],
      }),
    ]);
    expect(projectFor("C:\\Users\\me\\dev\\app-wt", roots).key).toBe("ws:app");
    expect(projectFor("C:\\Users\\me\\other", roots).key).toBe("dir:c:/users/me/other");
  });
});

describe("summarize", () => {
  const rows = [
    bucket(0, at(22, 9), { activeMs: 600_000, turns: 2 }),
    bucket(0, at(25, 14), { output: 1000, cost: 3, costOutput: 2.5 }),
    bucket(1, at(24, 9), { model: 1, cost: 0, costInput: 0, costOutput: 0, costCacheWrite: 0, costCacheRead: 0 }),
    // The week before: the trend's material.
    bucket(0, at(15, 11)),
    // Out of every period but "all".
    bucket(0, at(2, 11)),
  ];

  it("sums the period, fills the quiet days and compares with the one before", () => {
    const summary = summarize(stats(rows), options);
    expect(summary.series).toHaveLength(7);
    expect(summary.series.map((point) => point.tokens)).toEqual([0, 0, 1000, 0, 1000, 1980, 0]);
    expect(summary.totals.tokens).toBe(3980);
    expect(summary.totals.sessions).toBe(2);
    expect(summary.totals.turns).toBe(2);
    expect(summary.previous?.tokens).toBe(1000);
    expect(summary.previous?.sessions).toBe(1);
    expect(summary.activeDays).toBe(1);
    // Thursday 22 October, 9 h.
    expect(summary.heat[3][9]).toBe(600_000);
    expect(summary.heatMax).toBe(600_000);
  });

  it("ranks by the measure and keeps every project pickable under a filter", () => {
    const filtered = summarize(stats(rows), { ...options, project: "ws:app", measure: "cost" });
    expect(filtered.totals.sessions).toBe(1);
    expect(filtered.projects.map((row) => row.key)).toEqual(["ws:app", "dir:c:/dev/shop"]);
    expect(filtered.projects[0].share).toBe(1);
    expect(filtered.agents.map((row) => row.label)).toEqual(["Claude Code"]);
    expect(filtered.sessions[0].model).toBe("Opus 5.5");
  });

  it("puts the tokens of unpriced models aside and names them", () => {
    const summary = summarize(stats(rows), options);
    expect(summary.unpricedTokens).toBe(1000);
    const odd = summary.models.find((row) => row.key === "house-model");
    expect(odd?.unpriced).toBe(true);
    expect(odd?.label).toBe("house-model");
  });

  it("orders the heaviest sessions by the measure", () => {
    const byTokens = summarize(stats(rows), options).sessions.map((row) => row.id);
    expect(byTokens).toEqual(["a", "b"]);
    const byCost = summarize(stats(rows), { ...options, measure: "cost" }).sessions;
    expect(byCost[0].id).toBe("a");
    expect(byCost[0].cost).toBe(4);
  });

  it("counts weeks rather than days past 120 days", () => {
    const old = [bucket(0, addDays(at(26), -200)), bucket(0, at(26))];
    const summary = summarize(stats(old), { ...options, period: "all" });
    expect(summary.granularity).toBe("week");
    expect(summary.series.length).toBeGreaterThan(28);
    expect(summary.series.length).toBeLessThan(32);
    expect(new Date(summary.series[0].at).getDay()).toBe(1);
    expect(summary.totals.tokens).toBe(2000);
  });
});

describe("formats", () => {
  it("compacts token counts the way each language does", () => {
    expect(formatCompact(1_234_000, "en")).toBe("1.2M");
    expect(formatCompact(124_500_000, "en")).toBe("125M");
    expect(formatCompact(845_000, "en")).toBe("845K");
    expect(formatCompact(1_234_000, "fr")).toMatch(/^1,2\s?M$/);
    expect(formatCompact(30_080_000_000, "fr")).toMatch(/^30,1\s?Md$/);
  });

  it("writes dollars with cents only where they matter", () => {
    expect(formatUsd(0.84, "en")).toBe("$0.84");
    expect(formatUsd(1284.4, "en")).toBe("$1,284");
    expect(formatUsd(312_000, "en")).toBe("$312K");
  });

  it("writes durations in minutes, hours and minutes, or hours", () => {
    expect(formatDuration(45 * 60_000, t)).toBe("45 min");
    expect(formatDuration((3 * 60 + 5) * 60_000, t)).toBe("3 h 05");
    expect(formatDuration(142 * 3_600_000, t)).toBe("142 h");
    expect(formatDuration(20_000, t)).toBe("< 1 min");
  });

  it("says a sliver of a share is under one percent", () => {
    expect(formatShare(0.004, "en")).toBe("<\u00a01%");
    expect(formatShare(0.34, "en")).toBe("34%");
  });

  it("names models the way people say them", () => {
    expect(modelLabel("claude-opus-5-5")).toBe("Opus 5.5");
    expect(modelLabel("gpt-5.6-sol")).toBe("GPT-5.6 Sol");
    expect(modelLabel("gpt-5.5")).toBe("GPT-5.5");
    expect(modelLabel("house-model")).toBe("house-model");
  });

  it("steps the heatmap by the square root of the value", () => {
    expect(heatLevel(0, 100)).toBe(0);
    expect(heatLevel(1, 100)).toBe(1);
    expect(heatLevel(25, 100)).toBe(3);
    expect(heatLevel(100, 100)).toBe(5);
  });
});
