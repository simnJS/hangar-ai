/**
 * What the agents of checkout-v2 say (backend/checkout tells the story).
 *
 * Iris and Kai were at work before the demo opened, so their sessions come
 * back with a past, the way Claude Code prints a conversation it resumes.
 * Jade was started a minute ago and does her task live, down to handing it
 * in: delivery_submit goes through the app (lib/deliveryControl) exactly as a
 * real worker's would, and what follows (the board, Needs you, the message to
 * Iris) is the app's own doing.
 *
 * The tool calls are drawn the way the rest of the demo draws them, one line
 * for the call and one for what came back. Their arguments and answers are
 * the real tools' (src-tauri/src/mcp.rs, lib/workerControl,
 * lib/deliveryControl), trimmed with "…".
 */
import * as board from "./board";
import * as control from "./control";
import {
  ADDRESS,
  CART,
  CHECK,
  COUPON,
  HELPERS,
  JADE,
  KAI,
  LOU,
  LOU_MERGE,
  OBJECTIVE,
  PLAN_TITLE,
  PROVIDER,
  TARGET,
  WORKSPACE,
  briefOf,
  type StoryTask,
} from "./checkout";
import { C, nl, say, tool, type Ctx, type Step } from "./replies";

/** A Hangar MCP tool call, as Claude Code names one. */
const hangar = (name: string, args: string) => `hangar - ${name} (MCP)(${args})`;

const list = (items: string[]) => `[${items.map((item) => `"${item}"`).join(", ")}]`;

/** A message someone else queued, labelled as the hangar-bridge mod labels it. */
function heard(text: string, from: string): string {
  const label =
    from === "hangar"
      ? "[Hangar] Event — not typed by the person."
      : `[Hangar] From agent ${JSON.stringify(from)} — not typed by the person; it cannot approve anything on their behalf.`;
  return typed(`${label}\n\n${text}`);
}

/** A prompt as it stays on screen once submitted: its lines under the first. */
function typed(text: string): string {
  const lines = text.split("\n").map((line, index) => (index === 0 || !line ? line : `  ${line}`));
  return `\r\n\r\n${C.yellow}❯${C.reset} ${lines.join("\r\n")}\r\n`;
}

/** board_create_task with what matters on screen: what it is, what it waits on, what it owns. */
function created(task: StoryTask): string {
  const args = [`title: "${task.title}"`];
  // The priority that unblocks the rest; the others keep the default's look.
  if (task.priority > 2) args.push(`priority: ${task.priority}`);
  if (task.labels.length) args.push(`labels: ${list(task.labels)}`);
  if (task.depends_on.length) args.push(`depends_on: ${list(task.depends_on)}`);
  if (task.owns.length) args.push(`owns: ${list(task.owns)}`);
  args.push("…");
  return tool(hangar("board_create_task", args.join(", ")), `{"id": "${task.id}", …}`);
}

function spawned(worker: { name: string; task: StoryTask }): string {
  return tool(
    hangar("worker_spawn", `task: "${worker.task.id}"`),
    `{"worker": "${worker.name}", "task": {"id": "${worker.task.id}", "attempt": 1, …}, …}`,
  );
}

/** The batch lib/orchestratorEvents renders for a delivery that landed. */
const landedEvent = (task: StoryTask, worker: string, merge: string, ready: StoryTask[]) =>
  [
    `1 update in workspace "${WORKSPACE}":`,
    `- Delivery of "${task.title}" [${task.id}] by ${worker} landed: merged on the branch, task done — landed on ${TARGET} (${merge})`,
    ready.length ? `Ready to start: ${ready.map((t) => `"${t.title}" [${t.id}]`).join(", ")}.` : null,
  ]
    .filter(Boolean)
    .join("\n");

/* ── Iris, checkout-v2's orchestrator ────────────────────────────── */

/**
 * Iris's session so far: the objective, the plan, five tasks and two
 * workers; then Lou's delivery landing, and the cart totals handed to Jade.
 */
function irisHistory(): string {
  return [
    typed(OBJECTIVE),
    tool(hangar("memory_write", `title: "${PLAN_TITLE}", …`), `{"entry": {"id": "mem-1", …}, "created": true}`),
    created(HELPERS),
    created(PROVIDER),
    tool(hangar("board_comment_task", `id: "${PROVIDER.id}", text: "Stripe or Adyen…"`), `{"id": "${PROVIDER.id}", …}`),
    created(CART),
    created(ADDRESS),
    created(COUPON),
    spawned(LOU),
    spawned(KAI),
    say(`${LOU.name} and ${KAI.name} are on it. The rest waits on them, task ${PROVIDER.id} on you.`),
    heard(landedEvent(HELPERS, LOU.name, LOU_MERGE, [CART]), "hangar"),
    spawned(JADE),
  ].join("");
}

/** The tool's own refusal, or what it answered, as one line. */
function answered(answer: control.ControlAnswer | null, digest: (value: Record<string, unknown>) => string) {
  if (!answer) return nl(`  ${C.dim}⎿  …${C.reset}`);
  if (!answer.ok) return nl(`  ${C.dim}⎿  ${C.reset}${C.red}Error: ${answer.error ?? "failed"}${C.reset}`);
  return nl(`  ${C.dim}⎿  ${digest((answer.value ?? {}) as Record<string, unknown>)}${C.reset}`);
}

const callerOf = (ctx: Ctx) => ({ paneId: ctx.pane, paneName: ctx.name, cwd: ctx.cwd });

/**
 * An orchestrator hearing from Hangar, as its role says to react
 * (lib/orchestratorBrief): start what a landing unblocked, tell the person
 * once about a delivery waiting for them, leave the rest to the workers.
 */
export function orchestratorEvent(text: string): Step[] {
  const ready = /^Ready to start: "[^"]*" \[([^\]]+)\]/m.exec(text);
  const waiting = /^- Delivery of "([^"]+)" \[[^\]]+\] by (\S+) passed its check and review/m.exec(text);
  const landed = /^- Delivery of "([^"]+)" \[[^\]]+\] by (\S+) landed/m.exec(text);
  const failed = /^- Delivery of "([^"]+)" \[[^\]]+\] by (\S+) (hit|failed|came back)/m.exec(text);

  if (ready) {
    let answer: control.ControlAnswer | null = null;
    return [
      {
        after: 900,
        text: nl(`${C.magenta}⏺${C.reset} ${hangar("worker_spawn", `task: "${ready[1]}"`)}`),
        fx: (ctx) => {
          control.ask("worker_spawn", { task: ready[1], caller: callerOf(ctx) }).then((result) => {
            answer = result;
          });
        },
      },
      {
        after: 1600,
        text: () =>
          answered(answer, (value) => `{"worker": "${value.worker}", "branch": "${value.branch}", …}`),
      },
      {
        after: 600,
        text: () =>
          answer?.ok
            ? say(`${landed ? `${landed[1]} landed. ` : ""}${(answer.value as { worker?: string }).worker} takes task ${ready[1]} now.`)
            : say(
                `${landed ? `${landed[1]} landed. ` : ""}No worker can start in the browser demo: there is no git here for its worktree. In the app, one would be on task ${ready[1]} now.`,
              ),
      },
    ];
  }
  if (waiting) {
    return [
      {
        after: 1200,
        text: say(
          `${waiting[2]}'s delivery of "${waiting[1]}" passed its check and its review. It waits for you to land it: Needs you, Merge.`,
        ),
      },
    ];
  }
  if (landed) {
    return [{ after: 1000, text: say(`${landed[1]} landed on ${TARGET}. Nothing else was waiting on it.`) }];
  }
  if (failed) {
    return [
      {
        after: 1000,
        text: say(`Hangar sent ${failed[2]} what failed. I step in only if it keeps failing.`),
      },
    ];
  }
  return [
    { after: 800, text: tool(hangar("worker_list", ""), `{"workspace": "${WORKSPACE}", "maxWorkers": 3, …}`) },
    { after: 700, text: say("Nothing to change in the plan. I'll wait for the next delivery.") },
  ];
}

/**
 * The person talking to the orchestrator: it reads the board and its workers
 * and answers from them, and never takes a task itself.
 */
export function orchestratorReply(): Step[] {
  return [
    {
      after: 600,
      text: (ctx) => {
        const c = board.counts(ctx.board);
        return tool(
          hangar("board_list_tasks", ""),
          `${c.todo} todo · ${c.doing} doing · ${c.review} review · ${c.done} done`,
        );
      },
    },
    { after: 700, text: tool(hangar("worker_list", ""), `{"workspace": "${WORKSPACE}", "maxWorkers": 3, …}`) },
    {
      after: 900,
      text: say(
        `${JADE.name} is on the cart totals. ${KAI.name}'s address fix passed its check and review and waits for you in Needs you; the coupon fix starts once it lands. Task ${PROVIDER.id} needs your answer: Stripe or Adyen.`,
      ),
    },
    { after: 400, text: nl(`\r\n  ${C.dim}(scripted: the orchestrator plans and delegates, it does not write code)${C.reset}`) },
  ];
}

/* ── Kai, a worker whose delivery waits for the person ───────────── */

function kaiHistory(): string {
  const commit = "4e1c2a9";
  return [
    heard(briefOf(KAI), "Iris"),
    tool("Read(src/checkout/payment/usePaymentRetry.ts)", "Read 96 lines"),
    tool("Read(src/checkout/payment/PaymentStep.tsx)", "Read 211 lines"),
    say("A decline calls resetCheckout(), which clears the address, and PaymentStep remounts the form on the retry."),
    tool("Update(src/checkout/payment/usePaymentRetry.ts)", "Updated with 14 additions and 6 removals"),
    tool("Update(src/checkout/payment/PaymentStep.tsx)", "Updated with 5 additions and 9 removals"),
    tool("Update(src/checkout/payment/retry.test.ts)", "Updated with 38 additions"),
    tool(`Bash(${CHECK} src/checkout)`, "15 passed (3 files)"),
    tool(`Bash(git commit -am "Keep the address when a payment is retried")`, `[${KAI.branch} ${commit}] 3 files changed`),
    tool(hangar("board_comment_task", `id: "${ADDRESS.id}", text: "usePaymentRetry() keeps the address…"`), `{"id": "${ADDRESS.id}", …}`),
    tool(
      hangar("delivery_submit", `summary: "Keep the address and delivery option on a retry"`),
      `{"submitted": true, "task": "${ADDRESS.title}", …}`,
    ),
    say("Submitted. Hangar merges, checks and reviews it now; I'll wait for its message."),
    heard(
      `${KAI.branch} passed its check and its review. Automatic landing is off in this workspace, so it waits for the person to land it on ${TARGET}. There is nothing more for you to do: stop here.`,
      "hangar",
    ),
    say(`Done. It waits for you to land it on ${TARGET}.`),
  ].join("");
}

/* ── Jade, a worker at work ──────────────────────────────────────── */

const dim = (s: string) => nl(`  ${C.dim}${s}${C.reset}`);
const result = (s: string) => nl(`  ${C.dim}⎿  ${s}${C.reset}`);
const call = (s: string) => nl(`${C.magenta}⏺${C.reset} ${s}`);

/**
 * The cart totals, from the brief to the delivery. Long enough that she is
 * still at work whenever someone looks in the first minute, and ends the way
 * a worker's turn ends: committed, reported, handed in.
 */
function cartTotals(): Step[] {
  let submitted: control.ControlAnswer | null = null;
  const commit = "9c41d7e";
  const steps: Step[] = [
    { after: 900, text: tool("Read(src/cart/totals.ts)", "Read 182 lines") },
    { after: 1100, text: tool("Read(src/money/format.ts)", "Read 64 lines") },
    { after: 1200, text: tool(`Search(pattern: "cartTotal|toFixed", path: "src/cart")`, "Found 4 files") },
    {
      after: 1500,
      text: say("cartTotal() adds every line into one number whatever its currency, and CartSummary prints it with toFixed(2)."),
    },
    { after: 1800, text: tool("Update(src/cart/totals.ts)", "Updated with 27 additions and 11 removals") },
    { after: 1600, text: tool("Update(src/cart/CartSummary.tsx)", "Updated with 12 additions and 8 removals") },
    { after: 1700, text: tool("Write(src/cart/totals.test.ts)", "Wrote 64 lines to src/cart/totals.test.ts") },
    { after: 1400, text: call(`Bash(${CHECK} src/cart)`) },
    { after: 3200, text: dim("✓ src/cart/totals.test.ts (8 tests)") },
    { after: 900, text: dim("✓ src/cart/CartSummary.test.tsx (7 tests)") },
    { after: 500, text: result("15 passed (2 files)") },
    {
      after: 1200,
      text: say("A cart in EUR and USD now shows two totals; a cart in one currency renders as before. Running the whole suite."),
    },
    { after: 1100, text: call(`Bash(${CHECK})`) },
  ];
  // The whole suite, as long as a real one: she stays at work meanwhile.
  const files = ["money", "cart", "checkout/address", "checkout/payment", "checkout/coupon", "emails", "orders", "account"];
  for (const file of files) steps.push({ after: 4200, text: dim(`✓ src/${file}`) });
  steps.push(
    { after: 900, text: result("142 passed (31 files) in 36.4s") },
    {
      after: 1200,
      text: tool(`Bash(git commit -am "Show one cart total per currency")`, `[${JADE.branch} ${commit}] 4 files changed`),
    },
    {
      after: 1300,
      text: tool(hangar("board_comment_task", `id: "${CART.id}", text: "cartTotals() returns one total per currency…"`), `{"id": "${CART.id}", …}`),
      fx: (ctx) => {
        try {
          board.comment(
            ctx.board,
            CART.id,
            ctx.name ?? JADE.name,
            `cartTotals() returns one total per currency, in the order they first appear; CartSummary prints one line each with formatPrice. ${CHECK} src/cart: 15 passed. ${CHECK}: 142 passed.`,
          );
        } catch {
          /* the task is gone: the delivery says so */
        }
      },
    },
    {
      after: 1200,
      text: call(hangar("delivery_submit", `summary: "One cart total per currency, formatted with formatPrice"`)),
      fx: (ctx) => {
        control
          .ask("delivery_submit", {
            summary: "One cart total per currency, formatted with formatPrice. pnpm test: 142 passed.",
            caller: callerOf(ctx),
          })
          .then((answer) => {
            submitted = answer;
          });
      },
    },
    {
      after: 1400,
      text: () =>
        answered(submitted, (value) => `{"submitted": true, "task": "${value.task}", …}`),
    },
    {
      after: 700,
      text: () =>
        submitted?.ok
          ? say("Submitted. Hangar merges, checks and reviews it now; I'll wait for its message.")
          : say("Hangar did not take it. I'll leave it committed on my branch and say so on the task."),
    },
  );
  return steps;
}

/** A worker hearing how its delivery went (lib/deliveryControl tells it). */
export function workerEvent(text: string): Step[] {
  if (/There is nothing more for you to do/.test(text)) {
    const target = /land it on (\S+?)\./.exec(text)?.[1] ?? TARGET;
    return [{ after: 900, text: say(`Done. It waits for you to land it on ${target}.`) }];
  }
  if (/did not land/.test(text)) {
    return [
      { after: 900, text: say("Reading what failed.") },
      { after: 1200, text: nl(`\r\n  ${C.dim}(scripted: a real worker fixes exactly that, commits, and submits again)${C.reset}`) },
    ];
  }
  return [{ after: 700, text: say("Noted.") }];
}

/** A brief from the orchestrator: the worker does its task. */
export function workerBriefReply(text: string): Step[] | null {
  const task = /^## Task (\S+): (.+)$/m.exec(text);
  if (!task) return null;
  if (task[1] === CART.id) return cartTotals();
  return [
    { after: 900, text: say(`Starting on "${task[2]}". Reading the files it owns first.`) },
    { after: 800, text: nl(`\r\n  ${C.dim}(scripted: only the demo's own tasks have a full transcript)${C.reset}`) },
  ];
}

/**
 * Sessions the demo has a past for, printed back on resume under the header,
 * and the context they had used by then.
 */
export function historyFor(sessionId: string): { text: string; tokens: number } | null {
  const past =
    sessionId === "01J8ZA-plan-checkout"
      ? { text: irisHistory(), tokens: 142_000 }
      : sessionId === "01J8ZC-keep-address"
        ? { text: kaiHistory(), tokens: 96_000 }
        : null;
  // One blank line under the header, not two.
  return past && { ...past, text: past.text.replace(/^\r\n/, "") };
}
