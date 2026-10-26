/**
 * What the fake agents answer.
 *
 * These are canned transcripts, chosen by keyword. Nothing here talks to a
 * model. The demo says so in the banner each agent prints on start, and the
 * page around it says so too. Keep it that way.
 *
 * Some replies really do move the board, so switching to the Board view after
 * watching a pane work shows what the pane just did. One really calls a Hangar
 * tool: "show the model" opens the sample model in the 3D preview.
 */
import * as board from "./board";
import * as control from "./control";
import { appState } from "./state";

export interface Ctx {
  cwd: string;
  agent: string;
  /** The pane's id, which is also its PTY's. */
  pane: string;
  name: string | null;
}

/**
 * `text` is written at that point in the transcript, `fx` runs there. A
 * function for `text` is resolved when the step fires, which is how a reply
 * can print something an earlier step only learned at run time.
 */
export interface Step {
  after: number;
  text?: string | ((ctx: Ctx) => string);
  fx?: (ctx: Ctx) => void;
}

const C = {
  reset: "\x1b[0m",
  dim: "\x1b[90m",
  red: "\x1b[31m",
  green: "\x1b[32m",
  yellow: "\x1b[33m",
  blue: "\x1b[34m",
  magenta: "\x1b[35m",
  cyan: "\x1b[36m",
  bold: "\x1b[1m",
};

const nl = (s = "") => `\r\n${s}`;

/** A Claude Code style tool call: the call, then what it returned. */
const tool = (call: string, result: string) =>
  nl(`${C.magenta}⏺${C.reset} ${call}`) + nl(`  ${C.dim}⎿  ${result}${C.reset}`);

const say = (s: string) => nl(`${C.dim}⏺${C.reset} ${s}`);

const ok = (s: string) => nl(`${C.green}✓${C.reset} ${s}`);
const bad = (s: string) => nl(`${C.red}✗${C.reset} ${s}`);

/** Longest match wins, so "board" beats a bare "task" in the same sentence. */
const has = (text: string, words: string[]) =>
  words.some((w) => text.includes(w));

function tests(): Step[] {
  return [
    { after: 500, text: tool("Read(src-tauri/src/board/mod.rs)", "Read 268 lines") },
    { after: 900, text: tool("Write(src-tauri/src/board/claim_test.rs)", "Wrote 64 lines") },
    { after: 700, text: say("Running the suite") },
    { after: 1200, text: nl(`  ${C.dim}running 14 tests${C.reset}`) },
    { after: 900, text: nl(`  ${C.dim}test board::claims_are_exclusive ... ${C.green}ok${C.reset}`) },
    { after: 400, text: nl(`  ${C.dim}test board::second_claim_fails ... ${C.green}ok${C.reset}`) },
    { after: 600, text: ok("14 passed in 0.42s") },
    {
      after: 500,
      text: tool("board_update_task(#11, column: done)", "moved to done"),
      fx: (ctx) => {
        try {
          board.update(ctx.cwd, "11", { column: "done", assignee: ctx.agent });
        } catch {
          /* the demo board may not hold that id */
        }
      },
    },
    { after: 400, text: nl(`\r\n  Two agents can't both win a claim now. The second gets an error\r\n  instead of a task someone else is already holding.`) },
  ];
}

function refactor(subject: string): Step[] {
  return [
    { after: 600, text: tool("Grep(pattern: \"wslpath\")", "3 files") },
    { after: 800, text: tool("Read(src-tauri/src/pty.rs)", "Read 412 lines") },
    { after: 900, text: say(`Found it. The path goes to the shell unconverted.`) },
    { after: 800, text: tool("Edit(src-tauri/src/pty.rs)", "+18 −6") },
    { after: 700, text: tool("Bash(cargo check)", "Finished in 6.1s") },
    { after: 600, text: ok("cargo check clean") },
    { after: 500, text: nl(`\r\n  ${subject} now resolves through ${C.cyan}wslpath -w${C.reset} before the spawn,\r\n  so a WSL pane opens in the directory you actually picked.`) },
  ];
}

function i18n(): Step[] {
  return [
    { after: 600, text: tool("Read(src/i18n.ts)", "Read 604 lines") },
    { after: 900, text: say("Diffing the fr table against en") },
    { after: 1000, text: bad("missing key: settings.discord.hint") },
    { after: 400, text: bad("missing key: board.emptyColumn") },
    {
      after: 700,
      text: tool("board_create_task(\"Two fr keys are missing\")", "filed"),
      fx: (ctx) => {
        board.create(ctx.cwd, {
          title: "Two fr keys are missing in src/i18n.ts",
          description: "settings.discord.hint and board.emptyColumn",
          labels: ["i18n"],
          priority: 1,
        });
      },
    },
    { after: 600, text: nl(`\r\n  The fr table is typed against en, so this fails the build rather\r\n  than falling back at runtime. Filed it for whoever picks it up.`) },
  ];
}

function boardWork(): Step[] {
  // Filled by the first step and read by the ones after it, so the transcript
  // names the task this pane actually got.
  let picked: { id: string; title: string } | null = null;
  let lost = false;

  return [
    { after: 500, text: tool("board_list_tasks()", "reading the board") },
    {
      after: 700,
      text: (ctx) => {
        const c = board.counts(ctx.cwd);
        return nl(`  ${C.dim}${c.todo} open · ${c.doing} in progress · ${c.review} in review · ${c.done} done${C.reset}`);
      },
    },
    {
      after: 700,
      fx: (ctx) => {
        const t = board.next(ctx.cwd);
        picked = t ? { id: t.id, title: t.title } : null;
      },
      text: () =>
        picked
          ? tool("board_next_task()", `#${picked.id} ${picked.title}`)
          : tool("board_next_task()", "nothing free"),
    },
    {
      after: 800,
      fx: (ctx) => {
        if (!picked) return;
        try {
          board.claim(ctx.cwd, picked.id, ctx.agent);
        } catch {
          // Someone got there first, which is exactly what the lock is for.
          lost = true;
        }
      },
      text: () => {
        if (!picked) return nl(`  ${C.dim}Board is clear. Nothing to pick up.${C.reset}`);
        if (lost) return bad(`#${picked.id} was claimed by another pane first`);
        return tool(`board_claim_task(#${picked.id})`, "claimed");
      },
    },
    {
      after: 600,
      text: () =>
        picked && !lost
          ? nl(`\r\n  Holding ${C.bold}#${picked.id}${C.reset} now. Open the ${C.cyan}Board${C.reset} tab. It moved\r\n  to Doing while you were reading this.`)
          : nl(`\r\n  Open the ${C.cyan}Board${C.reset} tab to see where things stand.`),
    },
  ];
}

/** The procedural sample shipped with the demo, relative to the demo
    workspace, which plays the Hangar repository itself. */
const SAMPLE_MODEL = "demo/models/sample.glb";

/**
 * Opens the sample in the 3D preview through model_show, the way an agent
 * would: the app carries the call out (lib/control), shows the model, says
 * who opened it, and the transcript prints what the viewer found.
 */
function showModel(french: boolean): Step[] {
  let answer: control.ControlAnswer | null = null;

  const found = () => {
    if (!answer) return french ? "ouverture en cours…" : "opening…";
    if (!answer.ok) return null;
    const value = answer.value as {
      title?: string;
      status?: string;
      info?: { triangles: number; materials: number; animations: string[] } | null;
    };
    const title = value.title ?? "sample.glb";
    if (!value.info) return french ? `${title} ouvert, encore en chargement` : `${title} opened, still loading`;
    const n = (count: number) => count.toLocaleString(french ? "fr-FR" : "en-US");
    const { triangles, materials, animations } = value.info;
    return french
      ? `${title} · ${n(triangles)} triangles · ${n(materials)} matériaux · ${n(animations.length)} animation(s)`
      : `${title} · ${n(triangles)} triangles · ${n(materials)} materials · ${n(animations.length)} animation(s)`;
  };

  return [
    {
      after: 500,
      text: say(french ? "Je te l'ouvre dans l'aperçu 3D de Hangar." : "Opening it in Hangar's 3D preview."),
    },
    {
      after: 600,
      text: nl(`${C.magenta}⏺${C.reset} hangar - model_show (MCP)(path: "${SAMPLE_MODEL}")`),
      fx: (ctx) => {
        // The name the pane has now, as the app would read it from the
        // environment of a real agent.
        const paneName =
          appState.workspaces.flatMap((ws) => ws.panes).find((pane) => pane.id === ctx.pane)?.name ??
          ctx.name;
        control
          .ask("model_show", {
            path: SAMPLE_MODEL,
            caller: { paneId: ctx.pane, paneName, cwd: ctx.cwd },
          })
          .then((result) => {
            answer = result;
          });
      },
    },
    {
      // The viewer loads three.js on demand the first time: give it a moment.
      after: 2400,
      text: () => {
        const line = found();
        return line === null
          ? nl(`  ${C.dim}⎿  ${C.reset}${C.red}${answer?.error ?? "failed"}${C.reset}`)
          : nl(`  ${C.dim}⎿  ${line}${C.reset}`);
      },
    },
    {
      after: 500,
      text: () =>
        answer && !answer.ok
          ? nl(
              french
                ? `\r\n  L'aperçu n'a pas pu s'ouvrir. Le reste de la démo marche quand même.`
                : `\r\n  The preview did not open. The rest of the demo still works.`,
            )
          : nl(
              french
                ? `\r\n  Le voilà, dans l'aperçu 3D à côté des terminaux. Fais-le tourner à la\r\n  souris, molette pour zoomer.`
                : `\r\n  There it is, in the 3D preview beside the terminals. Drag to turn it,\r\n  scroll to zoom.`,
            ),
    },
  ];
}

function build(): Step[] {
  return [
    { after: 500, text: tool("Bash(pnpm tauri build)", "started") },
    { after: 900, text: nl(`  ${C.dim}vite v7.0.4 building for production...${C.reset}`) },
    { after: 800, text: nl(`  ${C.dim}✓ 214 modules transformed${C.reset}`) },
    { after: 700, text: nl(`  ${C.dim}   Compiling hangar-ai v0.4.1${C.reset}`) },
    { after: 1100, text: nl(`  ${C.dim}   Compiling tauri-plugin-updater v2.10.1${C.reset}`) },
    { after: 900, text: ok("Finished `release` profile in 48.2s") },
    { after: 500, text: nl(`\r\n  Installers are in ${C.cyan}src-tauri/target/release/bundle${C.reset}.`) },
  ];
}

function themes(): Step[] {
  return [
    { after: 600, text: tool("Read(src/themes.ts)", "Read 1020 lines") },
    { after: 800, text: say("32 palettes, 24 dark and 8 light.") },
    { after: 700, text: nl(`  ${C.dim}Each one carries an accent the window chrome uses,${C.reset}`) },
    { after: 400, text: nl(`  ${C.dim}not just the sixteen ANSI colours.${C.reset}`) },
    { after: 600, text: nl(`\r\n  Settings → Appearance switches them live. Try it. The whole\r\n  window follows, this pane included.`) },
  ];
}

function capabilities(agent: string): Step[] {
  return [
    { after: 500, text: nl(`  A stand-in for ${C.bold}${agent}${C.reset}. The window around it is the real app;`) },
    { after: 400, text: nl(`  only the agent and the shell are faked.`) },
    { after: 600, text: nl(`\r\n  ${C.dim}Try:${C.reset}`) },
    { after: 300, text: nl(`    ${C.cyan}take the next task${C.reset}    claims from the board`) },
    { after: 200, text: nl(`    ${C.cyan}write the tests${C.reset}       runs a suite, closes its task`) },
    { after: 200, text: nl(`    ${C.cyan}check the fr table${C.reset}    files a new task`) },
    { after: 200, text: nl(`    ${C.cyan}show the model${C.reset}        opens it in the 3D preview`) },
    { after: 200, text: nl(`    ${C.cyan}/exit${C.reset}                 back to the shell`) },
  ];
}

function generic(prompt: string): Step[] {
  const short = prompt.length > 46 ? prompt.slice(0, 46) + "…" : prompt;
  return [
    { after: 600, text: tool("Grep(pattern: \"" + short.replace(/"/g, "") + "\")", "4 files") },
    { after: 800, text: tool("Read(src/App.tsx)", "Read 517 lines") },
    { after: 900, text: say("Here's the shape of it:") },
    { after: 500, text: nl(`    1. ${C.dim}find where it is decided today${C.reset}`) },
    { after: 300, text: nl(`    2. ${C.dim}change it in one place${C.reset}`) },
    { after: 300, text: nl(`    3. ${C.dim}check the build still types${C.reset}`) },
    { after: 700, text: tool("Edit(src/App.tsx)", "+11 −3") },
    { after: 700, text: tool("Bash(pnpm build)", "tsc clean, built in 3.4s") },
    { after: 500, text: ok("done") },
    {
      after: 400,
      text: nl(`\r\n  ${C.dim}(canned, nothing was read or written)${C.reset}`),
    },
  ];
}

export function replyFor(agent: string, prompt: string): Step[] {
  if (!prompt.trim()) return [];
  // Padded so a word test can bound both sides without missing the first or
  // last word of the line.
  const p = ` ${prompt.toLowerCase()} `;

  // Ahead of the rest: "montre-moi le modèle" would otherwise read as a
  // French greeting or a translation job.
  const french = has(p, ["montre", "modèle", "modele", "aperçu", "apercu"]);
  if (
    has(p, [
      "montre le mod",
      "montre-moi le mod",
      "montre moi le mod",
      "modèle 3d",
      "modele 3d",
      "aperçu 3d",
      "apercu 3d",
      "show the model",
      "show me the model",
      "3d model",
      "3d preview",
      "model_show",
    ])
  )
    return showModel(french);
  if (has(p, [" hello", " hi ", " hey", " salut", " bonjour", "who are you", "what can you", "help me", "qui es"]))
    return capabilities(agent);
  if (has(p, ["board", "next task", "take the next", "claim", "todo", "tâche", "tache"]))
    return boardWork();
  if (has(p, ["test", "spec", "suite"])) return tests();
  if (has(p, ["translat", "i18n", "locale", "french", "français", "francais", " fr "])) return i18n();
  if (has(p, ["build", "release", "installer", "ship", "compile"])) return build();
  if (has(p, ["theme", "palette", "colour", "color", "couleur"])) return themes();
  if (has(p, ["refactor", "fix", "bug", "wsl", "pty", "bridge", "path", "crash", "error"]))
    return refactor(prompt.trim());

  return generic(prompt.trim());
}

export { C, nl, tool, say, ok, bad };
