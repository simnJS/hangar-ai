/**
 * The pseudo-terminal, faked.
 *
 * The real one is a PTY per pane in Rust. Here each pane gets a small state
 * machine: a shell that knows a handful of commands, and an "agent" mode it
 * drops into when you run `claude`, `codex`, `gemini` or `opencode`.
 *
 * Everything a pane prints goes out as a `pty:output` event, exactly as the
 * Rust side does it, so TerminalPane and xterm are none the wiser.
 */
import { dispatch } from "../mock/event";
import { replyFor, C, type Ctx, type Step } from "./replies";
import { SESSIONS } from "./state";
import type { ShellInfo } from "../../src/types";

type Mode = "shell" | "claude" | "codex" | "gemini" | "opencode";

interface Session {
  id: string;
  cwd: string;
  shell: ShellInfo | null;
  cols: number;
  rows: number;
  mode: Mode;
  /** What has been typed since the last Enter. */
  line: string;
  /** True while a transcript is still printing; keystrokes are held. */
  busy: boolean;
  timers: ReturnType<typeof setTimeout>[];
  history: string[];
  historyAt: number;
}

const AGENTS: Record<string, { label: string; glyph: string; color: string; version: string }> = {
  claude: { label: "Claude Code", glyph: "✻", color: C.yellow, version: "2.0.14" },
  codex: { label: "Codex", glyph: "◆", color: C.green, version: "0.9.3" },
  gemini: { label: "Gemini CLI", glyph: "✧", color: C.magenta, version: "0.4.1" },
  opencode: { label: "opencode", glyph: "●", color: C.cyan, version: "0.3.8" },
};

const sessions = new Map<string, Session>();

const out = (s: Session, data: string) => dispatch("pty:output", { id: s.id, data });

/** The shell's own prompt, in the dialect of whichever shell the pane picked. */
function prompt(s: Session): string {
  if (s.mode !== "shell") {
    const a = AGENTS[s.mode];
    return `\r\n${a.color}❯${C.reset} `;
  }
  const id = s.shell?.id ?? "pwsh";
  const posix = "/mnt/c" + s.cwd.replace(/^[A-Za-z]:/, "").replace(/\\/g, "/");
  switch (id) {
    case "cmd":
      return `\r\n${s.cwd}> `;
    case "git-bash":
    case "wsl":
      return `\r\n${C.green}you@hangar${C.reset}:${C.blue}${posix}${C.reset}$ `;
    case "nu":
      return `\r\n${C.green}${posix}${C.reset}> `;
    default:
      return `\r\n${C.dim}PS${C.reset} ${C.cyan}${s.cwd}${C.reset}${C.dim}>${C.reset} `;
  }
}

function banner(s: Session): string {
  const id = s.shell?.id ?? "pwsh";
  if (id === "cmd") return "Microsoft Windows [Version 10.0.26200.1234]\r\n";
  if (id === "wsl") return `${C.dim}Ubuntu 24.04.1 LTS${C.reset}\r\n`;
  if (id === "git-bash" || id === "nu") return "";
  return `${C.dim}PowerShell 7.4.6${C.reset}\r\n`;
}

function clearTimers(s: Session) {
  for (const t of s.timers) clearTimeout(t);
  s.timers = [];
}

/** Plays a transcript out over time, then hands the prompt back. */
function run(s: Session, steps: Step[], ctx: Ctx) {
  s.busy = true;
  let at = 0;
  for (const step of steps) {
    at += step.after;
    s.timers.push(
      setTimeout(() => {
        step.fx?.(ctx);
        if (step.text) {
          out(s, typeof step.text === "function" ? step.text(ctx) : step.text);
        }
      }, at),
    );
  }
  s.timers.push(
    setTimeout(() => {
      s.busy = false;
      s.timers = [];
      out(s, "\r\n" + prompt(s));
    }, at + 320),
  );
}

function enterAgent(s: Session, mode: Exclude<Mode, "shell">, resumeId: string | null) {
  s.mode = mode;
  const a = AGENTS[mode];
  const resumed = resumeId
    ? (SESSIONS[mode] ?? []).find((entry) => entry.id === resumeId)
    : undefined;

  const lines = [
    "",
    `${a.color}${a.glyph}${C.reset} ${C.bold}${a.label}${C.reset} ${C.dim}v${a.version}${C.reset}`,
    `  ${C.dim}${s.cwd}${C.reset}`,
    `  ${C.yellow}demo build${C.reset} ${C.dim}· replies are canned, no model runs here${C.reset}`,
  ];

  if (resumed) {
    lines.push(
      "",
      `  ${C.dim}Resumed ${resumed.id}${C.reset}`,
      `  ${C.dim}└ ${resumed.label}${C.reset}`,
    );
  }

  lines.push("", `  ${C.dim}Type anything. ${C.reset}${C.cyan}/help${C.reset}${C.dim} lists what it knows.${C.reset}`);

  s.busy = true;
  s.timers.push(
    setTimeout(() => {
      out(s, lines.join("\r\n"));
      s.busy = false;
      s.timers = [];
      out(s, "\r\n" + prompt(s));
    }, 260),
  );
}

/* ── shell commands ─────────────────────────────────────────── */

function shellCommand(s: Session, line: string): Step[] | "handled" {
  const [head, ...rest] = line.trim().split(/\s+/);
  const arg = rest.join(" ");

  if (head in AGENTS) {
    // `claude --resume <id>` / `codex resume <id>`. The app appends these
    // itself when a pane carries a session.
    const match = /(?:--resume|resume)\s+(\S+)/.exec(arg);
    enterAgent(s, head as Exclude<Mode, "shell">, match ? match[1] : null);
    return "handled";
  }

  switch (head) {
    case "":
      return [];

    case "clear":
    case "cls":
      out(s, "\x1b[2J\x1b[H");
      return [];

    case "exit":
      dispatch("pty:exit", { id: s.id });
      sessions.delete(s.id);
      return "handled";

    case "pwd":
      return [{ after: 60, text: `\r\n${s.cwd}` }];

    case "cd":
      if (arg) s.cwd = arg.startsWith("C:") ? arg : `${s.cwd}\\${arg}`;
      return [];

    case "ls":
    case "dir":
      return [
        {
          after: 80,
          text:
            "\r\n" +
            [
              `${C.blue}assets${C.reset}      ${C.blue}scripts${C.reset}     ${C.blue}site${C.reset}        ${C.blue}src${C.reset}`,
              `${C.blue}src-tauri${C.reset}   LICENSE     README.md   package.json`,
              `tsconfig.json           vite.config.ts`,
            ].join("\r\n"),
        },
      ];

    case "cat":
    case "type":
      return [
        { after: 90, text: `\r\n${C.dim}(the demo filesystem is a prop, nothing to read)${C.reset}` },
      ];

    case "git":
      if (arg.startsWith("status")) {
        return [
          { after: 120, text: `\r\nOn branch ${C.green}main${C.reset}` },
          { after: 120, text: `\r\nYour branch is up to date with 'origin/main'.` },
          { after: 200, text: `\r\n\r\nChanges not staged for commit:` },
          { after: 100, text: `\r\n  ${C.red}modified:   src-tauri/src/pty.rs${C.reset}` },
          { after: 80, text: `\r\n  ${C.red}modified:   src/i18n.ts${C.reset}` },
        ];
      }
      if (arg.startsWith("log")) {
        return [
          { after: 120, text: `\r\n${C.yellow}a1d49c9${C.reset} Release 0.4.1` },
          { after: 80, text: `\r\n${C.yellow}9403b63${C.reset} Keep Shift+Enter submitting in a plain shell` },
          { after: 80, text: `\r\n${C.yellow}a088c90${C.reset} Send a newline on Shift+Enter` },
        ];
      }
      return [{ after: 100, text: `\r\n${C.dim}git ${arg}: not wired up in the demo${C.reset}` }];

    case "pnpm":
    case "npm":
    case "cargo":
      return [
        { after: 200, text: `\r\n${C.dim}   Compiling hangar-ai v0.4.1${C.reset}` },
        { after: 900, text: `\r\n${C.dim}   Compiling tauri-plugin-updater v2.10.1${C.reset}` },
        { after: 800, text: `\r\n${C.green}    Finished${C.reset} in 48.2s` },
      ];

    case "help":
      return [
        { after: 80, text: `\r\n${C.dim}This shell answers to:${C.reset}` },
        { after: 60, text: `\r\n  ${C.cyan}claude${C.reset} · ${C.cyan}codex${C.reset} · ${C.cyan}gemini${C.reset} · ${C.cyan}opencode${C.reset}   start an agent` },
        { after: 60, text: `\r\n  ${C.cyan}ls${C.reset} · ${C.cyan}pwd${C.reset} · ${C.cyan}git status${C.reset} · ${C.cyan}git log${C.reset}` },
        { after: 60, text: `\r\n  ${C.cyan}pnpm tauri build${C.reset} · ${C.cyan}clear${C.reset} · ${C.cyan}exit${C.reset}` },
      ];

    default:
      return [
        {
          after: 90,
          text:
            `\r\n${C.red}${head}${C.reset}: command not found. ` +
            `${C.dim}Try ${C.reset}${C.cyan}help${C.reset}${C.dim}, or start an agent with ${C.reset}${C.cyan}claude${C.reset}${C.dim}.${C.reset}`,
        },
      ];
  }
}

/* ── agent commands ─────────────────────────────────────────── */

function agentCommand(s: Session, line: string): Step[] | "handled" {
  const text = line.trim();
  const a = AGENTS[s.mode];

  if (text === "/exit" || text === "/quit") {
    s.mode = "shell";
    return [{ after: 60, text: `\r\n${C.dim}left ${a.label}${C.reset}` }];
  }

  if (text === "/clear" || text === "/new") {
    out(s, "\x1b[2J\x1b[H");
    return [
      { after: 60, text: `${a.color}${a.glyph}${C.reset} ${C.dim}new conversation${C.reset}` },
    ];
  }

  if (text === "/help") {
    return [
      { after: 80, text: `\r\n  ${C.cyan}/exit${C.reset}   back to the shell` },
      { after: 60, text: `\r\n  ${C.cyan}/new${C.reset}    start a fresh conversation` },
      { after: 60, text: `\r\n  ${C.cyan}/board${C.reset}  what's on the board right now` },
      { after: 120, text: `\r\n\r\n  ${C.dim}Or just describe a job. Try "take the next task".${C.reset}` },
    ];
  }

  if (text === "/board" || text === "/tasks") {
    return replyFor(s.mode, "board");
  }

  return replyFor(s.mode, text);
}

/* ── the PTY surface the app calls ──────────────────────────── */

export function spawn(args: {
  id: string;
  cwd: string;
  shell?: ShellInfo | null;
  cols: number;
  rows: number;
}): void {
  const s: Session = {
    id: args.id,
    cwd: args.cwd,
    shell: args.shell ?? null,
    cols: args.cols,
    rows: args.rows,
    mode: "shell",
    line: "",
    busy: false,
    timers: [],
    history: [],
    historyAt: 0,
  };
  sessions.set(s.id, s);
  s.timers.push(
    setTimeout(() => {
      out(s, banner(s));
      out(s, prompt(s).replace(/^\r\n/, ""));
      s.timers = [];
    }, 90),
  );
}

export function write(id: string, data: string): void {
  const s = sessions.get(id);
  if (!s) return;

  for (let i = 0; i < data.length; i++) {
    const ch = data[i];

    // Ctrl+C cuts a running transcript short, busy or not.
    if (ch === "\x03") {
      clearTimers(s);
      s.busy = false;
      s.line = "";
      out(s, `${C.dim}^C${C.reset}\r\n` + prompt(s).replace(/^\r\n/, ""));
      continue;
    }

    if (s.busy) continue;

    // Arrow keys walk the history; everything else in an escape sequence is
    // swallowed rather than echoed as garbage.
    if (ch === "\x1b" && data[i + 1] === "[") {
      const final = data[i + 2];
      if (final === "A" || final === "B") {
        const step = final === "A" ? -1 : 1;
        const at = Math.min(Math.max(s.historyAt + step, 0), s.history.length);
        s.historyAt = at;
        const wanted = s.history[at] ?? "";
        out(s, "\x1b[2K\r" + prompt(s).replace(/^\r\n/, "") + wanted);
        s.line = wanted;
      }
      i += 2;
      continue;
    }

    if (ch === "\r" || ch === "\n") {
      const line = s.line;
      s.line = "";
      if (line.trim()) {
        s.history.push(line);
        s.historyAt = s.history.length;
      }
      out(s, "\r\n");

      const ctx: Ctx = { cwd: s.cwd, agent: s.mode === "shell" ? "shell" : s.mode };
      const steps = s.mode === "shell" ? shellCommand(s, line) : agentCommand(s, line);
      if (steps === "handled") continue;
      if (!steps.length) {
        out(s, prompt(s).replace(/^\r\n/, ""));
        continue;
      }
      run(s, steps, ctx);
      continue;
    }

    if (ch === "\x7f" || ch === "\b") {
      if (s.line.length) {
        s.line = s.line.slice(0, -1);
        out(s, "\b \b");
      }
      continue;
    }

    if (ch >= " ") {
      s.line += ch;
      out(s, ch);
    }
  }
}

export function resize(id: string, cols: number, rows: number): void {
  const s = sessions.get(id);
  if (!s) return;
  s.cols = cols;
  s.rows = rows;
}

export function kill(id: string): void {
  const s = sessions.get(id);
  if (!s) return;
  clearTimers(s);
  sessions.delete(id);
  dispatch("pty:exit", { id });
}

export const alive = (id: string) => sessions.has(id);
