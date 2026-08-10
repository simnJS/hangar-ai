/**
 * The world the demo starts in: two workspaces, the shells and agents a
 * machine would have reported, and the board the agents work.
 *
 * Everything lives in memory. Reloading the page resets the demo, which is
 * what a visitor expects from one.
 */
import { DEFAULT_SETTINGS, type AppState, type ShellInfo } from "../../src/types";

export const HOME = "C:\\Users\\you";

export const SHELLS: ShellInfo[] = [
  { id: "pwsh", label: "PowerShell 7", program: "pwsh.exe", args: ["-NoLogo"] },
  { id: "powershell", label: "Windows PowerShell", program: "powershell.exe", args: ["-NoLogo"] },
  { id: "cmd", label: "Command Prompt", program: "cmd.exe", args: [] },
  { id: "git-bash", label: "Git Bash", program: "bash.exe", args: ["-l"] },
  { id: "wsl", label: "WSL (Ubuntu)", program: "wsl.exe", args: ["-d", "Ubuntu"] },
  { id: "nu", label: "Nushell", program: "nu.exe", args: [] },
];

export const AVAILABLE_AGENTS = ["claude", "codex", "gemini", "opencode"];

const pane = (
  id: string,
  name: string,
  agent: "shell" | "claude" | "codex" | "gemini" | "opencode",
  sessionId: string | null = null,
) => ({ id, name, agent, sessionId, cwd: null, shellId: null, title: null });

/** `tree: null` lets lib/layout build the preset arrangement for the count. */
export const SEED_STATE: AppState = {
  activeWorkspaceId: "ws-hangar",
  workspaces: [
    {
      id: "ws-hangar",
      name: "hangar-ai",
      cwd: "C:\\dev\\hangar-ai",
      extraRoots: [],
      themeId: null,
      shellId: "pwsh",
      tree: null,
      panes: [
        pane("pane-1", "one", "claude", "01J8Z4-pty-bridge"),
        pane("pane-2", "two", "codex", "01J8Z6-board-tests"),
        pane("pane-3", "three", "gemini"),
        pane("pane-4", "four", "shell"),
      ],
    },
    {
      id: "ws-store",
      name: "storefront",
      cwd: "C:\\dev\\storefront",
      extraRoots: [],
      themeId: null,
      shellId: "pwsh",
      tree: null,
      panes: [pane("pane-5", "one", "claude"), pane("pane-6", "two", "shell")],
    },
  ],
  settings: {
    ...DEFAULT_SETTINGS,
    themeId: "tokyo-night",
    shellId: "pwsh",
    // The demo should be alive within a second of loading, so the pause that
    // waits for a real shell profile is cut down here.
    launchDelayMs: 350,
    autoResume: true,
    notifyOnIdle: false,
    discordPresence: false,
  },
};

/** Mutated by save_state so the app's own persistence round-trips. */
export let appState: AppState = structuredClone(SEED_STATE);

export function setAppState(next: AppState) {
  appState = next;
}

export const SESSIONS: Record<string, { id: string; label: string; modified_ms: number }[]> = {
  claude: [
    { id: "01J8Z4-pty-bridge", label: "Refactor the pty bridge for WSL paths", modified_ms: Date.now() - 5 * 60_000 },
    { id: "01J8Y1-theme-port", label: "Port four more VS Code themes", modified_ms: Date.now() - 3 * 3600_000 },
    { id: "01J8W7-installer", label: "Installer bitmaps and the new mark", modified_ms: Date.now() - 26 * 3600_000 },
  ],
  codex: [
    { id: "01J8Z6-board-tests", label: "Tests for atomic board claims", modified_ms: Date.now() - 9 * 60_000 },
    { id: "01J8X2-ci-matrix", label: "Build every platform in one job", modified_ms: Date.now() - 31 * 3600_000 },
  ],
};
