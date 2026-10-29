import { typeset } from "../lib/typography";
import type {
  Activity,
  Chat,
  ChatAnswer,
  ChatItem,
  HistoryEntry,
  Pane,
  Project,
  RemoteState,
  Screen,
  SendArgs,
  Share,
  Waiting,
} from "./api";
import { locale as appLocale, type Locale } from "./i18n";
import { chatPage, conversations, type FakeChat } from "./mockChat";

/**
 * A fake PC, for working on the app without one: the dev server always uses
 * it, a build with `?mock` in the address. It answers the same routes as the
 * real mobile server, through a `fetch` of its own, so every call the app
 * makes goes through the real client.
 *
 * Its projects live: a message gets an answer, a permission granted or a
 * question answered sets the agent back to work, and a few seconds later it
 * hands back. billing-api is asleep: woken, or written to, it wakes a
 * moment later, its message answered then. Its Claude Code panes have their
 * conversations too (mockChat.ts), which the chat reads page by page.
 *
 * `?mock=<scenario>` shows a state that is hard to come by otherwise:
 * - empty: nothing waits for the person;
 * - unpaired: the phone has to pair (any code but 000000 works);
 * - offline: the PC stopped answering 47 min ago;
 * - nomanager: no project manager is open.
 *
 * Like the real PC, it writes its history in its own language, taken here to
 * be the phone's; task titles stay as they were written.
 */

export type Scenario = "default" | "empty" | "unpaired" | "offline" | "nomanager";

export function scenarioOf(value: string | null): Scenario {
  return value === "empty" || value === "unpaired" || value === "offline" || value === "nomanager"
    ? value
    : "default";
}

const MIN = 60_000;

/** How long the fake PC takes to wake a project: long enough to see « Réveil en cours… ». */
export const WAKE_MS = 2_500;

/**
 * The fake PC's VAPID key: a real P-256 point (RFC 8291's example browser
 * key), so a browser subscribes with it all the same; nothing is ever pushed.
 */
export const PUSH_KEY =
  "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";

interface FakePane extends Pane {
  workspaceId: string;
  /** The screen above Claude Code's input box. */
  body: string[];
  /** Its conversation, for a Claude Code pane with a session. */
  chat?: FakeChat;
  waitingSince?: number;
  timer?: ReturnType<typeof setTimeout>;
}

/** The history's words, as the window writes them (src/lib/activityLog.ts, src/i18n.ts). */
const LINES = {
  en: {
    paneWaiting: "{pane} is waiting for you",
    needsYou: "Needs you: {title}",
    boardDone: "Every task is done ({n})",
    readyToLand: "Ready to merge: {title}",
    shared: "{pane} shared {name}",
    landed: "Landed: {title}",
    awake: "{name} woke up",
    asleep: "{name} fell asleep",
    taskDone: "Done: {title}",
  },
  fr: {
    paneWaiting: "{pane} vous attend",
    needsYou: "Vous attend : {title}",
    boardDone: "Toutes les tâches sont faites ({n})",
    readyToLand: "Prête à fusionner : {title}",
    shared: "{pane} a partagé {name}",
    landed: "Intégrée : {title}",
    awake: "{name} s'est réveillé",
    asleep: "{name} s'est endormi",
    taskDone: "Terminée : {title}",
  },
} satisfies Record<Locale, Record<string, string>>;

type LineKind = keyof (typeof LINES)["en"];

const CLAUDE_PROMPT = ["", "────────────────────────────────────────────────────────", "> ", "────────────────────────────────────────────────────────", "  ⏵⏵ accept edits on (shift+tab to cycle)"];

function scenery(now: number, locale: Locale) {
  const entry = (minutesAgo: number, workspace: string, kind: LineKind, vars: Record<string, string | number>): HistoryEntry => ({
    at: now - minutesAgo * MIN,
    workspace,
    kind,
    text: typeset(LINES[locale][kind], locale).replace(/\{(\w+)\}/g, (all, name: string) => String(vars[name] ?? all)),
  });

  const pane = (
    workspaceId: string,
    id: string,
    name: string,
    role: string | null,
    activity: Activity | null,
    body: string[],
    agent = "claude",
  ): FakePane => ({ workspaceId, id, name, role, activity, agent, body });

  const panes: FakePane[] = [
    pane("ws-iabench", "p-jack", "Jack", "orchestrator", "working", [
      "> Plan the Hangar Mobile work and hand it out",
      "",
      "⏺ I split it into seven tasks on the board: the server (M0a), the window's",
      "  ops (M1), the phone app (M2), files both ways (M0b), settings (M3), push",
      "  (M4) and the week away (M5). M0a, M1 and M2 run side by side.",
      "",
      "⏺ hangar - worker_spawn (MCP)(task: \"81070ac8\", name: \"Ada\")",
      "  ⎿  Ada started on « Hangar Mobile — serveur distant »",
      "",
      "⏺ hangar - worker_spawn (MCP)(task: \"f804a999\", name: \"Ben\")",
      "  ⎿  Ben started on « ce que la fenêtre répond au téléphone »",
      "",
      "⏺ hangar - worker_spawn (MCP)(task: \"26e29387\", name: \"Cora\")",
      "  ⎿  Cora started on « l'app sur le téléphone (PWA) »",
      "",
      "✻ Watching the three workers… (esc to interrupt)",
    ]),
    pane("ws-iabench", "p-ada", "Ada", "worker", "working", [
      "⏺ Update(src-tauri/src/remote/pair.rs)",
      "  ⎿  Updated src-tauri/src/remote/pair.rs with 38 additions and 4 removals",
      "",
      "⏺ The pairing code is now single use, valid five minutes, and dies after",
      "  five wrong tries. Next: the cookie flags and the 401 on every route.",
      "",
      "⏺ Bash(cargo test --manifest-path src-tauri/Cargo.toml remote::)",
      "  ⎿  running 14 tests",
      "     test remote::pair::a_code_is_single_use ... ok",
      "     test remote::pair::a_code_expires_after_five_minutes ... ok",
      "     test remote::pair::five_wrong_tries_kill_the_code ... ok",
      "     test result: ok. 14 passed; 0 failed",
      "",
      "✻ Writing the cookie tests… (2m 14s · ↓ 3.1k tokens · esc to interrupt)",
    ]),
    pane("ws-iabench", "p-ben", "Ben", "worker", "waiting", [
      "⏺ remote_permission answers Claude Code's prompt with the key of its",
      "  first or last choice. Let me check it against the real prompt.",
      "",
      "╭──────────────────────────────────────────────────────────────────────╮",
      "│ Bash command                                                         │",
      "│                                                                      │",
      "│   pnpm vitest run src/lib/remoteControl.test.ts                      │",
      "│   Run the window ops' tests                                          │",
      "│                                                                      │",
      "│ Do you want to proceed?                                              │",
      "│ ❯ 1. Yes                                                             │",
      "│   2. Yes, and don't ask again for pnpm vitest commands               │",
      "│   3. No, and tell Claude what to do differently (esc)                │",
      "╰──────────────────────────────────────────────────────────────────────╯",
    ]),
    pane("ws-iabench", "p-cora", "Cora", "worker", "waiting", [
      "⏺ The phone app builds into dist-mobile/ and every screen works in mock",
      "  mode, light and dark, in French and English.",
      "",
      "  Checks:",
      "  - pnpm exec tsc --noEmit: clean",
      "  - pnpm test: 61 files, 1 024 tests passed",
      "  - pnpm mobile:build: dist-mobile/ in 1.9 s",
      "",
      "────────────────────────────────────────────────────────────────────────",
      " ☐ Delivery  ☐ Report  ☐ Next  ✔ Submit  →",
      "",
      "Shall I submit the delivery now?",
      "",
      "❯ 1. Submit now",
      "     Hangar merges it in a scratch worktree, runs the check and has it reviewed",
      "  2. Wait for the screenshots",
      "     I take the 390×844 shots, light and dark, then submit",
      "  3. Type something.",
      "",
      "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
    ]),
    pane("ws-storefront", "p-mia", "Mia", "orchestrator", "yours", [
      "⏺ All twelve tasks are done and landed on main. The storefront passes",
      "  Lighthouse at 98 on mobile; the report is in docs/lighthouse.html.",
      "",
      "  Anything else for this sprint?",
    ]),
    pane("ws-storefront", "p-shell", "shell", null, null, [
      "PS C:\\Users\\simnJS\\Documents\\storefront> pnpm preview",
      "",
      "  ➜  Local:   http://localhost:4173/",
      "  ➜  Network: use --host to expose",
    ], "shell"),
    pane("ws-billing", "p-noe", "Noé", "orchestrator", null, [], "claude"),
    pane("ws-recipes", "p-leo", "Leo", null, "working", [
      "codex",
      "",
      "▌ Add the shopping list export",
      "",
      "• Reading src/lists/export.ts",
      "• Running pnpm test lists",
      "",
      "  ✓ 23 tests passed",
      "",
      "• Writing the CSV and PDF exporters…",
    ], "codex"),
    pane("hq", "p-max", "Max", "manager", "idle", [
      "> Where does everything stand?",
      "",
      "⏺ hangar - project_list (MCP)",
      "",
      "⏺ Four projects:",
      "  - IaBench: Jack orchestrates three workers on Hangar Mobile. Ben waits",
      "    for a permission; one delivery is ready to merge.",
      "  - storefront: all twelve tasks done, Mia waits for what's next.",
      "  - billing-api: asleep since yesterday evening, two tasks to do.",
      "  - recipes-app: Leo (Codex) is adding the shopping list export.",
    ]),
  ];
  panes.find((candidate) => candidate.id === "p-ben")!.waitingSince = now - 4 * MIN;
  panes.find((candidate) => candidate.id === "p-cora")!.waitingSince = now - 2 * MIN;
  const chats = conversations(now);
  for (const candidate of panes) candidate.chat = chats[candidate.id];

  const projects: Omit<Project, "panes" | "orchestrator">[] = [
    {
      id: "ws-iabench",
      name: "IaBench",
      open: true,
      asleep: false,
      git: true,
      board: { todo: 4, doing: 3, review: 1, done: 22 },
      planStatus: "Hangar Mobile: 3 of 7 tasks under way, the server in review",
      needsYou: [{ id: "t-funnel", title: "Should Tailscale Funnel stay out of scope, or be offered as an option?" }],
      readyToLand: [{ id: "t-settings", title: "Mobile settings: pair a phone by QR code" }],
    },
    {
      id: "ws-storefront",
      name: "storefront",
      open: true,
      asleep: false,
      git: true,
      board: { todo: 0, doing: 0, review: 0, done: 12 },
      planStatus: "Done: the twelve tasks landed",
      needsYou: [],
      readyToLand: [],
    },
    {
      id: "ws-billing",
      name: "billing-api",
      open: false,
      asleep: true,
      git: true,
      board: { todo: 2, doing: 0, review: 0, done: 9 },
      planStatus: null,
      needsYou: [],
      readyToLand: [],
    },
    {
      id: "ws-recipes",
      name: "recipes-app",
      open: true,
      asleep: false,
      git: false,
      board: { todo: 0, doing: 0, review: 0, done: 0 },
      planStatus: null,
      needsYou: [],
      readyToLand: [],
    },
  ];

  const history: HistoryEntry[] = [
    entry(4, "IaBench", "paneWaiting", { pane: "Ben" }),
    entry(25, "IaBench", "needsYou", { title: "Should Tailscale Funnel stay out of scope, or be offered as an option?" }),
    entry(52, "storefront", "boardDone", { n: 12 }),
    entry(70, "IaBench", "readyToLand", { title: "Mobile settings: pair a phone by QR code" }),
    entry(95, "IaBench", "shared", { pane: "Cora", name: "mobile-for-you.png" }),
    entry(3 * 60, "storefront", "landed", { title: "Product pages load their images lazily" }),
    entry(5 * 60, "recipes-app", "awake", { name: "recipes-app" }),
    entry(19 * 60, "billing-api", "asleep", { name: "billing-api" }),
    entry(20 * 60, "billing-api", "landed", { title: "Invoices carry the VAT number" }),
    entry(26 * 60, "IaBench", "taskDone", { title: "The project manager opens in one click" }),
  ];

  const shares: Share[] = [
    { id: "s-render", at: now - 95 * MIN, workspace: "IaBench", pane: "Cora", name: "mobile-for-you.png", mime: "image/png", size: 182_340, caption: "The For you screen, in dark mode" },
    { id: "s-lighthouse", at: now - 3 * 60 * MIN, workspace: "storefront", pane: "Mia", name: "lighthouse.html", mime: "text/html", size: 412_903, caption: "Lighthouse report, mobile" },
    { id: "s-chart", at: now - 22 * 60 * MIN, workspace: "billing-api", pane: "Noé", name: "latency-p95.png", mime: "image/png", size: 96_120, caption: "p95 latency before and after the cache" },
    { id: "s-csv", at: now - 30 * 60 * MIN, workspace: "billing-api", pane: "Noé", name: "invoices-october.csv", mime: "text/csv", size: 18_400, caption: null },
  ];

  return { panes, projects, history, shares };
}

/** The shared images, drawn rather than shipped. */
function picture(id: string): string {
  const svg =
    id === "s-chart"
      ? `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 400"><rect width="640" height="400" fill="#121316"/><g stroke="#2c2d30">${[80, 160, 240, 320].map((y) => `<path d="M60 ${y}H610"/>`).join("")}</g><path d="M60 300 L130 280 L200 310 L270 250 L340 120 L410 140" fill="none" stroke="#8c8d91" stroke-width="3"/><path d="M410 140 L480 300 L550 310 L610 305" fill="none" stroke="#3e8bff" stroke-width="3"/><text x="60" y="44" fill="#ecedf0" font-family="sans-serif" font-size="22">p95 latency, ms</text><text x="420" y="130" fill="#68a5ff" font-family="sans-serif" font-size="16">cache on</text></svg>`
      : `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 390 600"><rect width="390" height="600" fill="#121316"/><text x="20" y="70" fill="#ecedf0" font-family="sans-serif" font-size="30" font-weight="600">Pour vous</text><text x="20" y="100" fill="#9c9da2" font-family="sans-serif" font-size="16">3 choses vous attendent</text>${[
          ["#e7c06b", 20, 70],
          ["#6fd4df", 118, 72],
          ["#5fd27f", 218, 56],
        ].map(([fill, x, width]) => `<rect x="${x}" y="122" width="8" height="8" rx="2" fill="${fill}"/><rect x="${Number(x) + 14}" y="122" width="${width}" height="8" rx="4" fill="#3a3b3f"/>`).join("")}${[0, 1, 2].map((i) => `<rect x="20" y="${160 + i * 136}" width="350" height="120" rx="10" fill="#1b1c1f"/><rect x="20" y="${160 + i * 136}" width="3" height="120" fill="${i === 2 ? "#6fd4df" : "#e7c06b"}"/><rect x="38" y="${180 + i * 136}" width="64" height="8" rx="4" fill="#3a3b3f"/><rect x="38" y="${200 + i * 136}" width="${[210, 270, 230][i]}" height="12" rx="6" fill="#8c8d91"/><rect x="38" y="${248 + i * 136}" width="70" height="8" rx="4" fill="#3a3b3f"/><rect x="${i === 2 ? 238 : 262}" y="${234 + i * 136}" width="${i === 2 ? 116 : 92}" height="36" rx="8" fill="${i === 2 ? "#252629" : "#3e8bff"}"/>`).join("")}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** A tool call a pane makes while it answers. */
interface Step {
  tool: string;
  title: string;
  detail: string;
  result: string;
}

const LOOK: Step = { tool: "Read", title: "Read CHANGELOG.md", detail: "CHANGELOG.md", result: "214 lines" };

export interface Mock {
  fetch: typeof fetch;
  fileUrl: (id: string) => string;
  /** The offline scenario's last answer and the state it gave. */
  seed: { lastOk: number; state: RemoteState } | null;
}

export function createMock(scenario: Scenario, now = Date.now(), locale: Locale = appLocale): Mock {
  const world = scenery(now, locale);
  let paired = scenario !== "unpaired";
  const latency = () => pause(90 + Math.random() * 140);

  if (scenario === "empty") {
    for (const pane of world.panes) {
      if (pane.activity === "waiting") pane.activity = "working";
      delete pane.waitingSince;
      if (pane.chat) pane.chat.pending = null;
    }
    for (const project of world.projects) {
      project.needsYou = [];
      project.readyToLand = [];
    }
  }

  const paneById = (idOrName: string) =>
    world.panes.find((pane) => pane.id === idOrName) ?? world.panes.find((pane) => pane.name === idOrName);

  const manager = () => (scenario === "nomanager" ? undefined : world.panes.find((pane) => pane.role === "manager"));

  /** What waits for each project waking up, run once it is awake. */
  const wakers = new Map<string, (() => void)[]>();

  /** Wakes a project asleep as the PC does, a moment later; false when it is not asleep. */
  function wake(project: (typeof world.projects)[number], then?: () => void): boolean {
    if (!project.asleep) return false;
    const waiting = wakers.get(project.id);
    if (waiting) {
      if (then) waiting.push(then);
      return true;
    }
    wakers.set(project.id, then ? [then] : []);
    setTimeout(() => {
      project.asleep = false;
      project.open = true;
      for (const pane of world.panes) {
        if (pane.workspaceId === project.id && pane.agent !== "shell" && pane.activity === null) pane.activity = "idle";
      }
      for (const run of wakers.get(project.id) ?? []) run();
      wakers.delete(project.id);
    }, WAKE_MS);
    return true;
  }

  function state(): RemoteState {
    const projects: Project[] = world.projects.map((project) => {
      const panes = world.panes.filter((pane) => pane.workspaceId === project.id);
      const orchestrator = panes.find((pane) => pane.role === "orchestrator");
      return {
        ...project,
        orchestrator: orchestrator ? { name: orchestrator.name, activity: orchestrator.activity } : null,
        panes: panes.map(({ id, name, agent, role, activity }) => ({ id, name, agent, role, activity })),
      };
    });
    const waiting: Waiting[] = [];
    for (const pane of world.panes) {
      if (pane.activity !== "waiting" || pane.workspaceId === "hq") continue;
      const project = world.projects.find((candidate) => candidate.id === pane.workspaceId)!;
      waiting.push({
        kind: "pane",
        workspaceId: project.id,
        workspace: project.name,
        paneId: pane.id,
        pane: pane.name,
        since: pane.waitingSince ?? Date.now(),
      });
    }
    for (const project of world.projects) {
      for (const task of project.needsYou) {
        waiting.push({ kind: "needsYou", workspaceId: project.id, workspace: project.name, taskId: task.id, title: task.title, since: now - 25 * MIN });
      }
      for (const task of project.readyToLand) {
        waiting.push({ kind: "readyToLand", workspaceId: project.id, workspace: project.name, taskId: task.id, title: task.title, since: now - 70 * MIN });
      }
    }
    const boss = manager();
    return {
      at: Date.now(),
      manager: boss ? { paneId: boss.id, name: boss.name, activity: boss.activity } : null,
      waiting,
      projects,
    };
  }

  /** Claude Code's input box under its transcript, unless a prompt of its own replaced it. */
  const lines = (pane: FakePane) =>
    pane.agent === "claude" && pane.activity !== "waiting" && pane.body.length ? [...pane.body, ...CLAUDE_PROMPT] : pane.body;

  function screen(pane: FakePane): Screen {
    const project = world.projects.find((candidate) => candidate.id === pane.workspaceId);
    return {
      paneId: pane.id,
      pane: pane.name,
      workspace: project?.name ?? "HQ",
      agent: pane.agent,
      activity: pane.activity,
      text: lines(pane).join("\n"),
    };
  }

  let written = 0;
  /** Writes into a pane's conversation, as Claude Code does into its transcript. */
  function say(pane: FakePane, item: DistributiveOmit<ChatItem, "id" | "at">): ChatItem | null {
    if (!pane.chat) return null;
    const said = { ...item, id: `${pane.id}-live-${++written}`, at: Date.now() } as ChatItem;
    pane.chat.items.push(said);
    return said;
  }

  /**
   * The agent's turn: it works a moment, says `reply`, and hands back (or
   * keeps working). Its conversation gets the same: the tool call it makes,
   * running then done, and its answer.
   */
  function respond(pane: FakePane, said: string[], reply: string[], then: Activity = "yours", step: Step = LOOK) {
    const before = [...pane.body, ...(said.length ? ["", ...said] : [])];
    pane.activity = "working";
    delete pane.waitingSince;
    pane.body = [...before, "", "✻ Thinking… (esc to interrupt)"];
    clearTimeout(pane.timer);
    pane.timer = setTimeout(() => {
      const call = say(pane, { kind: "action", tool: step.tool, title: step.title, detail: step.detail, state: "running" });
      pane.timer = setTimeout(() => {
        if (call) Object.assign(call, { state: "done", result: { ok: true, text: step.result } });
        pane.body = [...before, "", ...reply];
        say(pane, { kind: "assistant", text: reply.map((line) => line.replace(/^⏺ /, "")).join("\n") });
        pane.activity = then;
      }, 1500);
    }, 900);
  }

  function send(args: SendArgs): Response {
    const pane = paneById(args.pane);
    if (!pane) return json(400, { error: `No pane named ${args.pane}` });
    const project = world.projects.find((candidate) => candidate.id === pane.workspaceId);
    // As the PC does: it wakes the project, and a message waits for its agent to be back; keys cannot.
    if (project?.asleep) {
      if (!args.text || !args.submit) {
        wake(project);
        return json(400, { error: `"${project.name}" was asleep: it is waking up now` });
      }
      const text = args.text;
      wake(project, () => {
        say(pane, { kind: "user", text, from: "person", images: 0 });
        respond(pane, [`> ${text}`], ["⏺ Back. On it."]);
      });
      return json(200, { sent: true, queued: true });
    }
    const interrupt = args.keys?.some((key) => key === "escape" || key === "ctrl+c");
    if (interrupt && pane.activity === "working") {
      clearTimeout(pane.timer);
      pane.body = [...pane.body.filter((line) => !line.startsWith("✻")), "  ⎿  Interrupted by user"];
      pane.activity = "yours";
      for (const item of pane.chat?.items ?? []) {
        if (item.kind === "action" && item.state === "running") {
          Object.assign(item, { state: "error", result: { ok: false, text: "Interrupted by the person" } });
        }
      }
      say(pane, { kind: "notice", text: "Turn interrupted" });
    }
    if (args.text && args.submit) {
      say(pane, { kind: "user", text: args.text, from: "person", images: 0 });
      respond(pane, [`> ${args.text}`], ["⏺ On it. I'll write here when it's done."]);
    }
    return json(200, { sent: true });
  }

  /** A page of a pane's conversation; a pane without one says why, as the PC does. */
  function chat(pane: FakePane, args: Record<string, unknown>): Response {
    const project = world.projects.find((candidate) => candidate.id === pane.workspaceId);
    const head = { paneId: pane.id, pane: pane.name, workspace: project?.name ?? "HQ", agent: pane.agent, activity: pane.activity };
    if (!pane.chat) {
      const reason = pane.agent === "claude" ? "noSession" : "notClaude";
      return json(200, { ...head, sessionId: null, items: [], older: false, cursor: null, pending: null, reason } satisfies Chat);
    }
    const page = chatPage(pane.chat.items, typeof args.before === "string" ? args.before : undefined, Number(args.limit ?? 60));
    if (!page) return json(400, { error: "bad-cursor" });
    return json(200, { ...head, sessionId: pane.chat.sessionId, ...page, pending: pane.chat.pending } satisfies Chat);
  }

  /** The answers to a pane's question, checked as Claude Code would: one per question, from its options. */
  function answer(pane: FakePane, args: Record<string, unknown>): Response {
    const pending = pane.chat?.pending;
    if (pending?.kind !== "question" || pending.toolUseId !== args.toolUseId) {
      return json(409, { error: "That pane is not asking this question" });
    }
    const answers = args.answers as ChatAnswer[];
    const fits =
      Array.isArray(answers) &&
      answers.length === pending.questions.length &&
      answers.every(({ selected, other }, index) => {
        const question = pending.questions[index];
        const count = selected.length + (other ? 1 : 0);
        return (
          selected.every((label) => question.options.some((option) => option.label === label)) &&
          count > 0 &&
          (question.multiSelect || count === 1)
        );
      });
    if (!fits) return json(400, { error: "bad-answers" });
    const asked = pane.chat!.items.find((item) => item.kind === "question" && item.toolUseId === pending.toolUseId);
    if (asked?.kind === "question") asked.answer = answers;
    pane.chat!.pending = null;
    const chosen = answers.map(({ selected, other }) => [...selected, ...(other ? [other] : [])].join(", "));
    pane.body = pane.body.slice(0, pane.body.findIndex((line) => line.startsWith("────")));
    respond(pane, [`⏺ You answered: ${chosen.join(" / ")}`], [`⏺ Thanks. ${chosen.join("; ")}: on it.`], "working", {
      tool: "Bash",
      title: "Ran git status --short",
      detail: "git status --short",
      result: " M src/mobile/styles.css",
    });
    return json(200, { answered: true });
  }

  async function route(path: string, method: string, body: unknown): Promise<Response> {
    if (scenario === "offline") throw new TypeError("Failed to fetch");
    await latency();
    if (path === "/m/api/health") return json(200, { ok: true, pc: "PC-SIMON", version: "1.1.0", paired });
    if (path === "/m/api/pair" && method === "POST") {
      const { code } = body as { code: string };
      if (code === "000000" || !/^\d{6}$/.test(code)) return json(401, { error: "bad-code" });
      paired = true;
      return json(200, { device: { id: "d-phone", name: "iPhone (Safari)" } });
    }
    if (!paired) return json(401, { error: "unpaired" });

    if (path.startsWith("/m/api/shares")) {
      const since = Number(new URL(path, "http://pc").searchParams.get("since") ?? 0);
      return json(200, { shares: world.shares.filter((share) => share.at > since) });
    }
    if (path === "/m/api/push/key") return json(200, { key: PUSH_KEY });
    if (path === "/m/api/push/subscribe") {
      if (method === "DELETE") return json(200, { subscribed: false });
      const endpoint = (body as { subscription?: { endpoint?: unknown } } | undefined)?.subscription?.endpoint;
      if (typeof endpoint !== "string" || !endpoint.startsWith("https://")) {
        return json(400, { error: "bad-subscription", message: "the endpoint is not an https address" });
      }
      return json(200, { subscribed: true });
    }
    if (path.startsWith("/m/api/upload")) {
      const name = new URL(path, "http://pc").searchParams.get("name") ?? "file";
      const size = body instanceof Blob ? body.size : 0;
      await pause(Math.min(4000, 400 + size / 2000));
      const stamp = new Date().toISOString().slice(0, 19).replace("T", "-").replace(/:/g, "");
      return json(200, { path: `C:\\Users\\simnJS\\Documents\\IaBench\\.hangar\\uploads\\${stamp}-${name}` });
    }

    const op = /^\/m\/api\/call\/(\w+)$/.exec(path)?.[1];
    const args = (body ?? {}) as Record<string, unknown>;
    switch (op) {
      case "state":
        return json(200, state());
      case "screen": {
        const pane = paneById(String(args.pane));
        return pane ? json(200, screen(pane)) : json(400, { error: `No pane named ${String(args.pane)}` });
      }
      case "send":
        return send(args as unknown as SendArgs);
      case "chat": {
        const pane = paneById(String(args.pane));
        return pane ? chat(pane, args) : json(400, { error: `No pane named ${String(args.pane)}` });
      }
      case "answer": {
        const pane = paneById(String(args.pane));
        return pane ? answer(pane, args) : json(400, { error: `No pane named ${String(args.pane)}` });
      }
      case "permission": {
        const pane = paneById(String(args.pane));
        // A question is answered with `answer`, not Allow / Deny.
        if (!pane || pane.activity !== "waiting" || pane.chat?.pending?.kind === "question") {
          return json(409, { error: "That pane is not waiting for a permission" });
        }
        const allowed = Boolean(args.allow);
        pane.body = pane.body.slice(0, pane.body.findIndex((line) => line.startsWith("╭")));
        // The tool call that waited runs, or is turned down.
        const waited = pane.chat?.items[pane.chat.items.length - 1];
        if (pane.chat) pane.chat.pending = null;
        if (waited?.kind === "action" && waited.state === "running") {
          Object.assign(
            waited,
            allowed
              ? { state: "done", result: { ok: true, text: "✓ src/lib/remoteControl.test.ts (31 tests) 412ms" } }
              : { state: "error", result: { ok: false, text: "The person denied this command" } },
          );
        }
        respond(
          pane,
          ["⏺ Bash(pnpm vitest run src/lib/remoteControl.test.ts)", allowed ? "  ⎿  ✓ src/lib/remoteControl.test.ts (31 tests) 412ms" : "  ⎿  User rejected the command"],
          [allowed ? "⏺ Every op is covered and passes. Committing, then I'll submit the delivery." : "⏺ Understood, I won't run it. What would you rather I check?"],
          allowed ? "working" : "yours",
        );
        return json(200, { answered: true });
      }
      case "manager": {
        const boss = manager();
        if (!boss) return json(409, { error: "Open the project manager on the PC" });
        say(boss, { kind: "user", text: String(args.text), from: "person", images: 0 });
        respond(boss, [`> ${String(args.text)}`], ["⏺ Noted. I'll pass it on and tell you here how it goes."], "yours", {
          tool: "mcp__hangar__project_list",
          title: "Used project_list",
          detail: "{}",
          result: "4 projects",
        });
        return json(200, { sent: true });
      }
      case "history": {
        const since = typeof args.since === "number" ? args.since : 0;
        return json(200, { entries: world.history.filter((entry) => entry.at > since) });
      }
      case "wake": {
        const project = world.projects.find((candidate) => candidate.id === args.workspace || candidate.name === args.workspace);
        if (!project) return json(400, { error: `No project "${String(args.workspace)}" in Hangar.` });
        return json(200, { waking: wake(project) });
      }
      default:
        return json(404, { error: "unknown-op" });
    }
  }

  const mockFetch: typeof fetch = async (input, init) => {
    const path = typeof input === "string" ? input : input instanceof URL ? `${input.pathname}${input.search}` : input.url;
    const method = init?.method ?? "GET";
    let body: unknown = init?.body;
    if (typeof body === "string") body = JSON.parse(body);
    return route(path, method, body);
  };

  return {
    fetch: mockFetch,
    fileUrl: (id) => {
      const share = world.shares.find((candidate) => candidate.id === id);
      if (share && share.mime.startsWith("image/")) return picture(id);
      return `data:${share?.mime ?? "text/plain"};charset=utf-8,${encodeURIComponent(`${share?.name ?? id}\nA file shared by an agent, in the fake PC.`)}`;
    },
    seed: scenario === "offline" ? { lastOk: now - 47 * MIN, state: (() => { paired = true; return state(); })() } : null,
  };
}
