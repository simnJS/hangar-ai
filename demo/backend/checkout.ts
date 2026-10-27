/**
 * The checkout-v2 story, in one place: the orchestrated workspace the hero of
 * the site is captured on.
 *
 * Iris was asked to ship checkout v2. She wrote her plan to the memory, put
 * five tasks on the board and started two workers: Lou on the price helpers
 * everything else builds on, Kai on the address lost after a failed payment.
 * Lou's delivery landed, which made the cart totals ready to start, and Iris
 * gave them to Jade. Kai's delivery passed its check and its review and waits
 * for the person, automatic landing being off here. The coupon fix owns the
 * same folder as Kai's task, so it waits for Kai's to land. The EU card
 * provider is a product decision, so it waits for the person too.
 *
 * The board seed (backend/board), the transcripts (backend/replies), the
 * memory (backend/memory) and the opening scene (backend/scene) all read this,
 * so the panes, the board and the Needs you list tell the same story.
 *
 * Branches and worktrees are spelled the way lib/workerControl names them
 * (`workerBranch`, `worktreeFolder`), ports come from its pool, and briefs are
 * written by its own `workerBrief`, so they read exactly as a real
 * orchestration's would.
 *
 * Nothing here calls into src/ while the module loads: the backend sits in an
 * import cycle with the app (src/lib → @tauri-apps/api/core → backend), and a
 * binding of a module still loading is not there yet. `workerBrief` is only
 * called once the demo runs.
 */
import { workerBrief } from "../../src/lib/workerControl";

export const CHECKOUT = "C:\\dev\\checkout-v2";
export const WORKSPACE = "checkout-v2";
export const ORCHESTRATOR = "Iris";
export const CHECK = "pnpm test";
export const TARGET = "main";

export interface StoryTask {
  id: string;
  title: string;
  description: string;
  owns: string[];
  depends_on: string[];
  priority: number;
  labels: string[];
}

const task = (t: Partial<StoryTask> & Pick<StoryTask, "id" | "title">): StoryTask => ({
  description: "",
  owns: [],
  depends_on: [],
  priority: 1,
  labels: [],
  ...t,
});

/** Descriptions in the order the orchestrator's brief asks for them. */
export const HELPERS = task({
  id: "29",
  title: "Price formatting helpers",
  priority: 3,
  owns: ["src/money/"],
  description: [
    "Goal: one place that formats an amount in cents with its currency, so the cart and the emails stop formatting prices by hand.",
    "Context: amounts are integer cents with their currency (memory: \"Prices are integers in cents\"). src/cart/ and src/checkout/ each call toFixed(2) today.",
    "Acceptance:",
    "- formatPrice(cents, currency, locale) in src/money/format.ts, on Intl.NumberFormat",
    "- sumByCurrency(lines) returns one total per currency and never adds two currencies",
    "- tests for EUR, USD and JPY, which has no minor unit",
    "Check: pnpm test src/money",
    "Out of scope: the callers. The cart task moves them over.",
  ].join("\n"),
});

export const CART = task({
  id: "31",
  title: "Cart: one total per currency",
  priority: 2,
  owns: ["src/cart/"],
  depends_on: ["29"],
  description: [
    "Goal: a cart holding items in several currencies shows one total per currency, instead of one wrong sum.",
    "Context: src/cart/totals.ts adds every line into one number. Use sumByCurrency and formatPrice from src/money/ (task 29, landed).",
    "Acceptance:",
    "- cartTotals() returns one total per currency, in the order the currencies first appear",
    "- CartSummary shows one line per currency, formatted with formatPrice",
    "- a cart in a single currency looks exactly as before",
    "Check: pnpm test src/cart",
    "Out of scope: the checkout and the payment; exchange rates.",
  ].join("\n"),
});

export const ADDRESS = task({
  id: "32",
  title: "Keep the address after a failed payment",
  priority: 2,
  owns: ["src/checkout/payment/"],
  description: [
    "Goal: when a payment is declined and the customer tries again, the shipping address they typed is still there.",
    "Context: usePaymentRetry() in src/checkout/payment/ resets the checkout state, and PaymentStep remounts the form on a retry.",
    "Acceptance:",
    "- a declined card keeps the address and the delivery option",
    "- the retry submits the same address",
    "- a test covers decline, then retry",
    "Check: pnpm test src/checkout",
    "Out of scope: the coupon on a retry (task 33).",
  ].join("\n"),
});

export const COUPON = task({
  id: "33",
  title: "Apply a coupon once per retry",
  owns: ["src/checkout/payment/"],
  depends_on: ["32"],
  description: [
    "Goal: a retried payment applies the coupon once.",
    "Context: usePaymentRetry() replays applyCoupon(). Same folder as task 32, so it starts once 32 lands.",
    "Acceptance:",
    "- the total after a retry equals the total before it",
    "- a test covers a coupon, a decline, then a retry",
    "Check: pnpm test src/checkout",
    "Out of scope: the address (task 32).",
  ].join("\n"),
});

export const PROVIDER = task({
  id: "30",
  title: "Pick the payment provider for EU cards",
  priority: 3,
  labels: ["needs-you"],
  description:
    "A product decision for the person. The EU card tasks are planned once it is made.",
});

/** What Iris asked on task 30: one question, the options, her recommendation. */
export const PROVIDER_QUESTION =
  "Stripe or Adyen for the EU cards? Stripe is already wired for the US, so nothing new to learn. " +
  "Adyen costs less above 50k a month, for a second integration to keep. I recommend Stripe until " +
  "EU volume gets there. I'll plan the EU card tasks once you choose.";

export const TASKS = [HELPERS, PROVIDER, CART, ADDRESS, COUPON];

/** The person's objective, as typed into Iris. */
export const OBJECTIVE =
  "Ship checkout v2: one total per currency, keep the address after a failed payment, " +
  "no double coupon on retry. EU cards: ask me.";

export const PLAN_TITLE = `Plan — ${WORKSPACE}`;

/** Iris's plan document, as she would rewrite it after the last event. */
export const PLAN = [
  "Objective: ship checkout v2 — one total per currency in the cart, the address kept after a failed payment, the coupon applied once on a retry. EU cards next.",
  "Validation criteria:",
  "1. A cart in EUR and USD shows two totals, each formatted for its currency.",
  "2. Decline a card, retry: same address, same delivery option, coupon applied once.",
  "3. pnpm test passes on main.",
  "Decisions (the person's): none yet.",
  "Assumptions (mine, reversible): amounts stay integer cents; no exchange rates in the cart.",
  "Open questions: EU card provider, Stripe or Adyen (task 30).",
  "Tasks:",
  "- 29 Price formatting helpers: landed",
  "- 31 Cart: one total per currency: Jade",
  "- 32 Keep the address after a failed payment: waits for the person to land it",
  "- 33 Apply a coupon once per retry: after 32 (same folder)",
  "- 30 EU card provider: needs you",
  "Status: Jade on the cart totals; 33 starts once 32 lands.",
].join("\n");

/** One worker per task, in the order Iris started them. */
export interface StoryWorker {
  name: string;
  task: StoryTask;
  branch: string;
  worktree: string;
  portBase: number;
}

/** `branch` as `workerBranch(task)` makes it: `hangar/<title slug>-<id>`. */
function worker(name: string, task: StoryTask, branch: string, portBase: number): StoryWorker {
  // worktreeFolder(): a sibling of the checkout, the branch's slash a dash.
  const worktree = `${CHECKOUT}-worktrees\\${branch.replace(/\//g, "-")}`;
  return { name, task, branch, worktree, portBase };
}

/** PORT_POOL_START in lib/workerControl; a worker gets PORT_COUNT (20) ports from it. */
const POOL = 21000;

/** Lou went with the landing of its delivery: its port block went back to the pool, and Jade got it. */
export const LOU = worker("Lou", HELPERS, "hangar/price-formatting-helpers-29", POOL);
export const KAI = worker("Kai", ADDRESS, "hangar/keep-the-address-after-a-failed-32", POOL + 20);
export const JADE = worker("Jade", CART, "hangar/cart-one-total-per-currency-31", POOL);

/** The first message a worker gets, exactly as lib/workerControl writes it. */
export const briefOf = (w: StoryWorker) =>
  workerBrief({
    task: w.task,
    worker: w.name,
    orchestrator: ORCHESTRATOR,
    workspace: WORKSPACE,
    branch: w.branch,
    target: TARGET,
    worktree: w.worktree,
    portBase: w.portBase,
    checkCommand: CHECK,
    attempt: 1,
    maxAttempts: 2,
  });

/** A commit id that stays the same for the same text: made up, but stable. */
export function shaOf(text: string): string {
  let hash = 2166136261;
  for (const char of text) hash = Math.imul(hash ^ char.charCodeAt(0), 16777619) >>> 0;
  return `${hash.toString(16).padStart(8, "0")}${(hash % 251).toString(16).padStart(2, "0")}`;
}

/** The merge Lou's delivery landed as, and the one Kai's was tested as (backend/index lands that one). */
export const LOU_MERGE = shaOf(LOU.branch).slice(0, 7);
export const KAI_MERGE = shaOf(KAI.branch).slice(0, 7);
