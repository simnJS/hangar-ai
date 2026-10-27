/**
 * The world the demo starts in: a few workspaces, some filed in folders, the
 * shells and agents a machine would have reported, and the board the agents
 * work.
 *
 * Everything lives in memory. Reloading the page resets the demo, which is
 * what a visitor expects from one.
 */
import { ORCHESTRATION_DEFAULTS } from "../../src/lib/orchestration";
import { DEFAULT_SETTINGS, type AppState, type Pane, type ShellInfo } from "../../src/types";

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
  /** An orchestrator's or a worker's part (lib/orchestration). */
  orchestration: Pick<Pane, "role" | "taskId" | "model" | "portBase"> & { cwd?: string } = {},
): Pane => ({ id, name, agent, sessionId, cwd: null, shellId: null, title: null, ...orchestration });

/** `tree: null` lets lib/layout build the preset arrangement for the count. */
const workspace = (
  id: string,
  name: string,
  cwd: string,
  folderId: string | null,
  panes: Pane[],
) => ({
  id,
  name,
  cwd,
  extraRoots: [],
  themeId: null,
  shellId: "pwsh",
  tree: null,
  savedCommands: [],
  folderId,
  panes,
});

/**
 * A machine that has been in use for a while: projects filed in folders, a
 * name too long for the navigator, and agents busy in workspaces off screen
 * (backend/scene says what they are doing when the demo opens).
 */
export const SEED_STATE: AppState = {
  activeWorkspaceId: "ws-hangar",
  folders: [
    { id: "f-clients", name: "Clients", collapsed: false },
    { id: "f-oss", name: "Open source", collapsed: false },
    { id: "f-archive", name: "Archive", collapsed: true },
  ],
  workspaces: [
    workspace("ws-hangar", "hangar-ai", "C:\\dev\\hangar-ai", null, [
      pane("pane-1", "Ava", "claude", "01J8Z4-pty-bridge"),
      pane("pane-2", "Ben", "codex", "01J8Z6-board-tests"),
      pane("pane-3", "Cleo", "gemini"),
      pane("pane-4", "Dan", "shell"),
    ]),
    workspace("ws-site", "marketing-site-redesign", "C:\\dev\\marketing-site-redesign", null, [
      pane("pane-7", "Leo", "claude"),
    ]),
    // An orchestrator at work: Iris plans, Jade and Kai each do one task in a
    // worktree of their own. Its board (backend/board) holds a question for
    // you and a delivery waiting to be merged, which « Besoin de toi » lists.
    {
      ...workspace("ws-checkout", "checkout-v2", "C:\\dev\\checkout-v2", null, [
        pane("pane-20", "Iris", "claude", "01J8ZA-plan-checkout", {
          role: "orchestrator",
          model: "opus",
        }),
        pane("pane-21", "Jade", "claude", "01J8ZB-cart-currency", {
          role: "worker",
          taskId: "31",
          model: "sonnet",
          portBase: 41000,
          cwd: "C:\\dev\\checkout-v2-worktrees\\cart-currency",
        }),
        pane("pane-22", "Kai", "claude", "01J8ZC-keep-address", {
          role: "worker",
          taskId: "32",
          model: "sonnet",
          portBase: 41020,
          cwd: "C:\\dev\\checkout-v2-worktrees\\keep-address",
        }),
      ]),
      // Deliveries wait for you here, and are checked before they do.
      orchestration: { ...ORCHESTRATION_DEFAULTS, autoMerge: false, checkCommand: "pnpm test" },
    },
    workspace("ws-store", "storefront", "C:\\dev\\storefront", "f-clients", [
      pane("pane-5", "Eve", "claude"),
      pane("pane-6", "Finn", "shell"),
    ]),
    workspace("ws-billing", "billing-api", "C:\\dev\\billing-api", "f-clients", [
      pane("pane-8", "Max", "claude"),
      pane("pane-9", "Mia", "codex"),
    ]),
    workspace("ws-docs", "docs", "C:\\dev\\docs", "f-oss", [
      pane("pane-10", "Ivy", "claude"),
      pane("pane-11", "Theo", "shell"),
    ]),
    workspace("ws-themes", "terminal-themes", "C:\\dev\\terminal-themes", "f-oss", [
      pane("pane-12", "Nora", "gemini"),
    ]),
    workspace("ws-legacy", "legacy-dashboard", "C:\\dev\\legacy-dashboard", "f-archive", [
      pane("pane-13", "Sam", "claude"),
    ]),
  ],
  settings: {
    ...DEFAULT_SETTINGS,
    themeId: "graphite",
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
    { id: "01J8ZA-plan-checkout", label: "Plan checkout v2 and hand it out", modified_ms: Date.now() - 2 * 60_000 },
    { id: "01J8ZB-cart-currency", label: "Cart: one total per currency", modified_ms: Date.now() - 4 * 60_000 },
    { id: "01J8ZC-keep-address", label: "Keep the address after a failed payment", modified_ms: Date.now() - 6 * 60_000 },
  ],
  codex: [
    { id: "01J8Z6-board-tests", label: "Tests for atomic board claims", modified_ms: Date.now() - 9 * 60_000 },
    { id: "01J8X2-ci-matrix", label: "Build every platform in one job", modified_ms: Date.now() - 31 * 3600_000 },
  ],
};
