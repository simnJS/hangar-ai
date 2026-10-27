import { invoke } from "@tauri-apps/api/core";
import type { PaneMcp } from "./paneInsight";
import type { ShellInfo, Workspace, WorkspaceMcp } from "../types";

/**
 * The MCP servers a workspace's Claude Code panes start with
 * (src-tauri/src/mcp_scope.rs).
 *
 * Every Claude Code session starts its own copy of every stdio server it is
 * configured with, so a server one project needs runs once per pane in every
 * workspace. A workspace in `custom` mode names the servers its panes load;
 * the pane then starts Claude Code with `--mcp-config <file>
 * --strict-mcp-config`, the file being written by the Rust side at each
 * launch from Claude Code's own configuration. The command typed names the
 * file through the shell's HANGAR_MCP_CONFIG (`mcpConfigArg`).
 */

export type McpScopeName = "user" | "local" | "project";
export type McpTransport = "stdio" | "http" | "sse" | "ws";
/** Whether Claude Code starts the server by itself here, and if not, why. */
export type McpStatus = "active" | "unapproved" | "rejected" | "disabled";

/** One server, as `mcp_servers` reports it. Nothing in it is a secret. */
export interface McpServer {
  name: string;
  scope: McpScopeName;
  transport: McpTransport;
  /** `npx @scope/pkg`, `mcp.example.com`: what it is, never its env or headers. */
  summary: string;
  status: McpStatus;
  /** Hangar's own server, under its current name or a former one. */
  hangar: boolean;
  /** The file the definition comes from. */
  source: string;
}

export interface McpDiscovery {
  servers: McpServer[];
  /** Files that exist but could not be read; their servers are missing. */
  unreadable: string[];
}

export interface McpScopeFile {
  path: string;
  /** The servers in the file, Hangar's included. */
  servers: string[];
  /** Enabled names nothing defines any more. */
  missing: string[];
}

export const mcpServers = (cwd: string) => invoke<McpDiscovery>("mcp_servers", { cwd });

export const mcpScopeWrite = (args: {
  workspaceId: string;
  /** Hangar's own server is told this is its workspace. */
  workspaceCwd: string;
  /** Where the pane runs, which decides what it would otherwise load. */
  cwd: string;
  enabled: string[];
}) => invoke<McpScopeFile>("mcp_scope_write", args);

export const mcpScopeClear = (workspaceId: string) =>
  invoke<void>("mcp_scope_clear", { workspaceId });

/** Absent from every workspace saved before this existed: Claude Code's own. */
export const INHERIT: WorkspaceMcp = { mode: "inherit", enabled: [] };

/**
 * A workspace's choice, read defensively: state.json is a file people edit,
 * and anything that is not a well-formed `custom` reads as today's behaviour.
 */
export function workspaceMcp(ws: Pick<Workspace, "mcp"> | null | undefined): WorkspaceMcp {
  const mcp = ws?.mcp;
  if (!mcp || mcp.mode !== "custom" || !Array.isArray(mcp.enabled)) return INHERIT;
  return {
    mode: "custom",
    enabled: mcp.enabled.filter((name): name is string => typeof name === "string"),
  };
}

/** The servers Claude Code starts by itself here, Hangar's aside. */
export const inheritedNames = (servers: McpServer[]) =>
  servers.filter((s) => s.status === "active" && !s.hangar).map((s) => s.name);

/**
 * Whether a server is loaded under a choice. Hangar's own is always loaded
 * in `custom` mode — the panes need it to reach the board, each other and
 * the memory — and loaded by Claude Code's configuration otherwise.
 */
export function isLoaded(server: McpServer, mcp: WorkspaceMcp): boolean {
  if (mcp.mode === "inherit") return server.status === "active";
  return server.hangar || mcp.enabled.includes(server.name);
}

/** The `custom` choice that loads exactly what Claude Code would. */
export const presetAll = (servers: McpServer[]): WorkspaceMcp => ({
  mode: "custom",
  enabled: inheritedNames(servers),
});

export const presetHangarOnly = (): WorkspaceMcp => ({ mode: "custom", enabled: [] });

/**
 * One server switched on or off. From `inherit`, the first toggle starts from
 * what Claude Code would have loaded, so changing one server changes only
 * that one.
 */
export function toggleServer(
  mcp: WorkspaceMcp,
  servers: McpServer[],
  name: string,
  on: boolean,
): WorkspaceMcp {
  const base = mcp.mode === "custom" ? mcp.enabled : inheritedNames(servers);
  const enabled = on
    ? base.includes(name)
      ? base
      : [...base, name]
    : base.filter((entry) => entry !== name);
  return { mode: "custom", enabled };
}

/**
 * Server processes a session starts under a choice: the stdio servers, each
 * one at least a process of its own — `npx` and `uvx` add a launcher on top.
 * Remote servers are a connection, not a process.
 */
export function stdioCount(servers: McpServer[], mcp: WorkspaceMcp): number {
  const loaded = servers.filter((s) => s.transport === "stdio" && isLoaded(s, mcp));
  // Hangar's own server is added in `custom` mode even where no file
  // registers it.
  const hangarExtra = mcp.mode === "custom" && !servers.some((s) => s.hangar) ? 1 : 0;
  return loaded.length + hangarExtra;
}

/**
 * Whether a pane's Claude Code runs with something else than the choice as
 * it stands — the panes « Relancer les agents » has to reach. A pane whose
 * launch this window never saw (`null`: another window draws it, or it has
 * not started) counts as out of date: restarting it costs nothing more than
 * a resume.
 */
export function needsRestart(
  running: PaneMcp | null,
  mcp: WorkspaceMcp,
  servers: McpServer[],
): boolean {
  if (!running) return true;
  if (running.mode !== mcp.mode) return true;
  if (running.mode === "inherit") return false;
  // What a launch now would put in the file: Hangar's own, and every chosen
  // name something still defines.
  const defined = new Set(servers.filter((s) => !s.hangar).map((s) => s.name));
  const expected = new Set(["hangar", ...mcp.enabled.filter((name) => defined.has(name))]);
  const actual = new Set(running.servers);
  return expected.size !== actual.size || [...expected].some((name) => !actual.has(name));
}

/** How a shell reads a quoted argument. */
export type ShellFamily = "powershell" | "cmd" | "posix" | "fish" | "nu" | "wsl";

const FAMILY_BY_NAME: Record<string, ShellFamily> = {
  pwsh: "powershell",
  powershell: "powershell",
  cmd: "cmd",
  "git-bash": "posix",
  msys2: "posix",
  cygwin: "posix",
  bash: "posix",
  zsh: "posix",
  sh: "posix",
  dash: "posix",
  ksh: "posix",
  fish: "fish",
  nu: "nu",
  wsl: "wsl",
};

/** By the id shells.rs gives it, else by its program's name; null when unknown. */
export function shellFamily(shell: ShellInfo | null | undefined): ShellFamily | null {
  if (!shell) return null;
  const byId = FAMILY_BY_NAME[shell.id];
  if (byId) return byId;
  const program = (shell.program.split(/[\\/]/).pop() ?? "")
    .toLowerCase()
    .replace(/\.exe$/, "");
  return FAMILY_BY_NAME[program] ?? null;
}

/**
 * A path as one argument the pane's shell types through untouched, or null
 * when that shell cannot be given it safely — the launch then goes without.
 *
 * The path is the app's own (its data folder), but the folder holds the user
 * name, and a name can hold anything a file name can.
 *
 * - PowerShell and the POSIX shells take single quotes literally: no `$`, no
 *   backtick, no backslash escape. A quote inside is doubled (PowerShell) or
 *   closed and escaped (POSIX).
 * - fish's single quotes still read `\\` and `\'`.
 * - Nushell's single quotes are raw and cannot hold a quote at all.
 * - cmd only has double quotes, and expands `%VAR%` inside them.
 * - An unknown shell gets single quotes, which every shell above but cmd reads
 *   literally, as long as there is no quote to escape.
 */
export function quoteArg(value: string, family: ShellFamily | null): string | null {
  if (/[\r\n\0]/.test(value)) return null;
  switch (family) {
    case "powershell":
      return `'${value.replace(/'/g, "''")}'`;
    case "posix":
      return `'${value.replace(/'/g, `'\\''`)}'`;
    case "fish":
      return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
    case "cmd":
      return /["%]/.test(value) ? null : `"${value}"`;
    case "nu":
    case null:
      return value.includes("'") ? null : `'${value}'`;
    // A Linux Claude Code reads its own configuration, and a Windows path
    // means nothing to it: a WSL pane keeps its own servers.
    case "wsl":
      return null;
  }
}

/**
 * Where the pane's shell holds the path of its `--mcp-config` file: pty.rs
 * sets it when the shell starts, the file being written just before.
 */
export const MCP_CONFIG_ENV = "HANGAR_MCP_CONFIG";

/**
 * The `--mcp-config` argument as the pane's shell is to read it: the variable
 * that holds the file's path rather than the path, which lies in the app's
 * data folder and so holds the user's name — and the typed command stays on
 * screen, in front of anyone watching, recording or streaming it. Every shell
 * below reads the variable as one argument, spaces and all.
 *
 * A shell whose syntax is not known gets the path itself (`quoteArg`), and a
 * WSL pane nothing, as before.
 */
export function mcpConfigArg(path: string, family: ShellFamily | null): string | null {
  switch (family) {
    case "powershell":
      return `$env:${MCP_CONFIG_ENV}`;
    // Expanded before the line is read, so quoted: a path may hold a space,
    // `&` or `^`, and never a double quote.
    case "cmd":
      return `"%${MCP_CONFIG_ENV}%"`;
    case "posix":
    case "fish":
      return `"$${MCP_CONFIG_ENV}"`;
    case "nu":
      return `$env.${MCP_CONFIG_ENV}`;
    default:
      return quoteArg(path, family);
  }
}
