import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AgentId, ShellInfo } from "../types";

/**
 * Files dropped on a pane, typed into it as paths.
 *
 * Only the native drop of the Tauri window carries real paths: an HTML5 drop
 * hands the page a File object and never says where it lives on disk. The
 * native events are listened to directly rather than through
 * `getCurrentWebview()`, which needs the Tauri runtime — the browser demo
 * stands in for `listen` and nothing else.
 */

export interface Point {
  x: number;
  y: number;
}

interface NativeDrag {
  paths?: string[];
  /** From the top left of the webview — see `toCss` for the unit. */
  position: Point;
}

const WINDOWS =
  typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);

/**
 * CSS pixels, the unit elementFromPoint and the rest of the page speak.
 *
 * Tauri calls the position physical everywhere, but only WebView2 measures it
 * that way (ScreenToClient). WKWebView reports AppKit points and WebKitGTK
 * widget coordinates, both already logical: dividing those by the pixel ratio
 * would aim at a pane halfway up the window on a Retina screen.
 */
const toCss = ({ x, y }: Point): Point => {
  const ratio = WINDOWS ? window.devicePixelRatio || 1 : 1;
  return { x: x / ratio, y: y / ratio };
};

/**
 * Reports a file drag as it moves over the window, and the drop that ends it.
 * `over` receives null once the drag leaves or lands.
 */
export function watchFileDrops(handlers: {
  over: (point: Point | null, count: number) => void;
  drop: (paths: string[], point: Point) => void;
}): () => void {
  // Only the enter event lists the files; the moves that follow do not.
  let count = 0;
  let stopped = false;
  let unlisten: UnlistenFn[] = [];

  Promise.all([
    listen<NativeDrag>("tauri://drag-enter", ({ payload }) => {
      count = payload.paths?.length ?? 0;
      handlers.over(toCss(payload.position), count);
    }),
    listen<NativeDrag>("tauri://drag-over", ({ payload }) =>
      handlers.over(toCss(payload.position), count),
    ),
    listen<NativeDrag>("tauri://drag-drop", ({ payload }) => {
      handlers.over(null, 0);
      if (payload.paths?.length) handlers.drop(payload.paths, toCss(payload.position));
    }),
    listen("tauri://drag-leave", () => handlers.over(null, 0)),
  ])
    .then((fns) => {
      // Stopped while the listeners were still being registered.
      if (stopped) fns.forEach((fn) => fn());
      else unlisten = fns;
    })
    .catch(() => undefined);

  return () => {
    stopped = true;
    unlisten.forEach((fn) => fn());
  };
}

/** How the program in the pane spells a path the host handed over. */
export type PathStyle = "native" | "wsl" | "msys" | "cygwin";

/** How a path that needs it gets quoted for whoever reads it. */
export type QuoteStyle =
  /** An agent's prompt: double quotes, as Windows Terminal pastes a drop. */
  | "agent"
  /** sh, bash, zsh, fish: single quotes, nothing inside them is special. */
  | "posix"
  /** PowerShell and Nushell: single quotes, a quote doubled inside. */
  | "single"
  | "cmd";

const programName = (shell: ShellInfo | null) =>
  (shell?.program.split(/[\\/]/).pop() ?? "").toLowerCase();

export function pathStyleOf(shell: ShellInfo | null): PathStyle {
  const id = shell?.id ?? "";
  if (id === "wsl" || programName(shell) === "wsl.exe") return "wsl";
  if (id === "git-bash" || id === "msys2") return "msys";
  if (id === "cygwin") return "cygwin";
  return "native";
}

/**
 * An agent reads the paste, not the shell it was started from — but the path
 * itself still has to be one the agent's side of WSL can open.
 */
export function quoteStyleOf(agent: AgentId, shell: ShellInfo | null): QuoteStyle {
  if (agent !== "shell") return "agent";
  const id = shell?.id ?? "";
  const program = programName(shell);
  if (id === "cmd" || program === "cmd.exe") return "cmd";
  if (
    id === "pwsh" ||
    id === "powershell" ||
    id === "nu" ||
    /^(pwsh|powershell|nu)(\.exe)?$/.test(program)
  ) {
    return "single";
  }
  return "posix";
}

/**
 * A Windows path as the pane's program would write it. Anything that is not a
 * drive or WSL path — a POSIX host, a network share — goes through untouched.
 */
export function translatePath(path: string, style: PathStyle): string {
  if (style === "native") return path;

  // A file dragged out of the WSL file system, as Explorer shows it.
  const inWsl = /^\\\\wsl(?:\.localhost|\$)\\[^\\]+(\\.*)?$/i.exec(path);
  if (inWsl) return style === "wsl" ? (inWsl[1] ?? "\\").replace(/\\/g, "/") : path;

  const drive = /^([A-Za-z]):(?:[\\/](.*))?$/.exec(path);
  if (!drive) return path;
  const letter = drive[1].toLowerCase();
  const rest = (drive[2] ?? "").replace(/\\/g, "/");
  const root =
    style === "wsl" ? `/mnt/${letter}` : style === "cygwin" ? `/cygdrive/${letter}` : `/${letter}`;
  return rest ? `${root}/${rest}` : root;
}

/** Characters each reader takes literally outside quotes. */
const BARE: Record<QuoteStyle, RegExp> = {
  agent: /^\S+$/,
  posix: /^[\w\-./:@%+=,]+$/,
  single: /^[\w\-./:\\%+=~]+$/,
  cmd: /^[\w\-./:\\@+=,~#]+$/,
};

export function quotePath(path: string, style: QuoteStyle): string {
  if (BARE[style].test(path)) return path;
  switch (style) {
    case "agent":
      return `"${path.replace(/"/g, '\\"')}"`;
    case "posix":
      return `'${path.replace(/'/g, "'\\''")}'`;
    case "single":
      return `'${path.replace(/'/g, "''")}'`;
    case "cmd":
      // A Windows file name cannot hold a double quote.
      return `"${path}"`;
  }
}

/**
 * What a drop types into a pane: every path, translated and quoted, with a
 * trailing space so the next word can follow — and no Enter.
 */
export function dropText(paths: string[], agent: AgentId, shell: ShellInfo | null): string {
  const pathStyle = pathStyleOf(shell);
  const quoteStyle = quoteStyleOf(agent, shell);
  return `${paths.map((path) => quotePath(translatePath(path, pathStyle), quoteStyle)).join(" ")} `;
}
