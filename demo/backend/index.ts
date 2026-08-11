/**
 * Every `invoke` the app sends to Rust arrives here instead.
 *
 * The command names and the shapes they return are the ones in
 * src-tauri. The app is unmodified, so anything that drifts here shows up as
 * a broken panel in the demo rather than a type error.
 */
import * as shell from "./shell";
import * as board from "./board";
import {
  AVAILABLE_AGENTS,
  HOME,
  SESSIONS,
  SHELLS,
  appState,
  setAppState,
} from "./state";
import type { AppState } from "../../src/types";

const MCP_TARGETS = [
  { id: "claude", label: "Claude Code", path: ".mcp.json", detected: true, configured: true, project_scoped: true },
  { id: "codex", label: "Codex", path: "~/.codex/config.toml", detected: true, configured: false, project_scoped: false },
  { id: "gemini", label: "Gemini CLI", path: "~/.gemini/settings.json", detected: true, configured: false, project_scoped: false },
  { id: "cursor", label: "Cursor", path: ".cursor/mcp.json", detected: false, configured: false, project_scoped: true },
  { id: "vscode", label: "VS Code", path: ".vscode/mcp.json", detected: true, configured: false, project_scoped: true },
];

/** Flipped by mcp_install so the panel reflects what you just did. */
const configured = new Set(MCP_TARGETS.filter((t) => t.configured).map((t) => t.id));
let instructionsWritten = false;

export async function route(command: string, args: Record<string, any>): Promise<unknown> {
  switch (command) {
    /* ── terminals ── */
    case "pty_spawn":
      shell.spawn(args as any);
      return undefined;
    case "pty_write":
      shell.write(args.id, args.data);
      return undefined;
    case "pty_resize":
      shell.resize(args.id, args.cols, args.rows);
      return undefined;
    case "pty_kill":
      shell.kill(args.id);
      return undefined;
    case "pty_alive":
      return shell.alive(args.id);

    /* ── machine ── */
    case "detect_agents":
      return AVAILABLE_AGENTS;
    case "detect_shells":
      return SHELLS;
    case "home_dir":
      return HOME;
    case "dir_exists":
      return true;
    case "find_code_workspace":
      return null;
    case "read_code_workspace":
      return [];
    case "list_sessions":
      return SESSIONS[args.agent] ?? [];

    /* ── persistence ── */
    case "load_state":
      return appState;
    case "save_state":
      setAppState(args.state as AppState);
      return undefined;

    /* ── board ── */
    case "board_load":
      return board.load(args.cwd);
    case "board_create":
      return board.create(args.cwd, args.task);
    case "board_update":
      return board.update(args.cwd, args.id, args.patch);
    case "board_delete":
      return board.remove(args.cwd, args.id);
    case "board_claim":
      return board.claim(args.cwd, args.id, args.agent);
    case "board_comment":
      return board.comment(args.cwd, args.id, args.author, args.text);

    /* ── MCP registration ── */
    case "mcp_targets":
      return MCP_TARGETS.map((t) => ({ ...t, configured: configured.has(t.id) }));
    case "mcp_install":
      return (args.ids as string[]).map((id) => {
        configured.add(id);
        const target = MCP_TARGETS.find((t) => t.id === id);
        return {
          id,
          ok: true,
          message: "registered (demo, nothing was written to disk)",
          path: target?.path ?? id,
        };
      });
    case "mcp_manual_commands":
      return {
        executable: "C:\\Program Files\\Hangar.AI\\hangar-ai.exe",
        claude: 'claude mcp add hangar -- "C:\\Program Files\\Hangar.AI\\hangar-ai.exe" --mcp',
        codex: 'codex mcp add hangar -- "C:\\Program Files\\Hangar.AI\\hangar-ai.exe" --mcp',
        workspace: args.workspaceCwd ?? "C:\\dev\\hangar-ai",
      };
    case "agent_instructions_status":
      return instructionsWritten;
    case "write_agent_instructions":
      instructionsWritten = true;
      return [
        { path: "AGENTS.md", ok: true, message: "block written (demo)" },
        { path: "CLAUDE.md", ok: true, message: "block written (demo)" },
      ];

    /* ── git worktrees ──
       There is no checkout behind the demo, and no git to ask about one, so
       the panel says so instead of inventing a repository. */
    case "git_repo_info":
      throw new Error("Worktrees need the desktop app. This is the browser demo.");

    /* ── global memory ──
       Nothing behind the demo can keep a fact across sessions, and a mocked
       store pretending otherwise would teach the wrong thing. The view is
       always on screen, so it gets a sentence instead of the router's
       "no such command". */
    case "memory_load":
    case "memory_create":
    case "memory_update":
    case "memory_delete":
      throw new Error("The memory needs the desktop app. This is the browser demo.");

    /* ── Discord ── */
    case "discord_presence_set":
      return undefined;
    case "discord_presence_status":
      return { enabled: false, connected: false, error: null };

    /* ── dictation ──
       A web page has no resident speech model and should not open a
       visitor's microphone, so the settings panel renders with nothing
       installed and the button refuses rather than throwing. */
    case "voice_models":
      return [
        {
          id: "parakeet-tdt-0.6b-v3",
          label: "Parakeet TDT 0.6b v3",
          approxBytes: 660_000_000,
          languages: "25 languages",
          license: "CC-BY-4.0",
          installed: false,
        },
      ];
    case "voice_recording":
      return false;
    case "voice_start":
      throw new Error("Dictation needs the desktop app. This is the browser demo.");
    case "voice_model_download":
      throw new Error("The demo does not download models. Install the app for dictation.");
    case "voice_stop":
    case "voice_unload":
      return undefined;

    default:
      throw new Error(`the demo backend has no "${command}"`);
  }
}
