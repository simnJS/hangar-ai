export type AgentId = "shell" | "claude" | "codex" | "gemini" | "opencode";

/** Where a dictation is transcribed. Mirrored in Rust as `EngineKind`. */
export type VoiceEngine = "local" | "groq";

/**
 * The catalogue id the Rust side downloads and loads for local dictation.
 *
 * A settings file written before this changed still names the old checkpoint;
 * Rust resolves an id it does not know to the one model there is, so nothing
 * has to migrate this.
 */
export const DEFAULT_VOICE_MODEL = "whisper-small-q5_1";

/**
 * Groq's transcription models, cheapest first.
 *
 * Turbo is the default for the same reason it is elsewhere: at $0.04 an hour
 * against $0.111 it is the one that makes the cloud fallback cost nothing worth
 * noticing, and dictation is too short for the accuracy gap to show.
 */
export const CLOUD_MODELS = [
  { id: "whisper-large-v3-turbo", label: "Whisper large-v3 turbo", price: "$0.04/h" },
  { id: "whisper-large-v3", label: "Whisper large-v3", price: "$0.111/h" },
];

/** Mirrors `cleanup::DEFAULT_MODEL`, and only ever shown as a placeholder. */
export const DEFAULT_CLEANUP_MODEL = "llama-3.1-8b-instant";

/**
 * Offered in the language picker. Not the ninety-nine Whisper handles — just
 * the ones worth a click, with "detect" covering the rest.
 */
export const VOICE_LANGUAGES = [
  "en",
  "fr",
  "de",
  "es",
  "it",
  "pt",
  "nl",
  "pl",
  "ru",
  "uk",
  "sv",
];

/** Starting points offered in the UI; a workspace can hold any pane count. */
export type LayoutSize = 1 | 2 | 4 | 8;

/** A shell discovered on this machine, as reported by the Rust side. */
export interface ShellInfo {
  id: string;
  label: string;
  program: string;
  args: string[];
}

export interface Pane {
  id: string;
  /**
   * Human name for this pane — unique within its workspace, and stable across
   * splits, moves and restarts, unlike a position.
   */
  name: string;
  agent: AgentId;
  /** Agent session to resume on next launch. Captured automatically. */
  sessionId: string | null;
  /** Optional per-pane directory, overriding the workspace root. */
  cwd: string | null;
  /** Overrides the workspace shell. null follows the workspace. */
  shellId: string | null;
  title: string | null;
}

/**
 * A command kept around to be run again — `pnpm tauri dev`, `cargo check`, the
 * prompt an agent gets asked the same way every morning.
 *
 * `command` is typed into a pane verbatim; `autoRun` decides whether the Enter
 * that runs it is sent along or left to the user, and `broadcast` sends it to
 * every pane of the workspace rather than to the one it was launched from.
 */
export interface SavedCommand {
  id: string;
  /** What the menu entry reads. Empty falls back to the command itself. */
  label: string;
  command: string;
  autoRun: boolean;
  broadcast: boolean;
}

export interface Workspace {
  id: string;
  name: string;
  cwd: string;
  /**
   * The other folders of a VS Code `.code-workspace`, once `cwd` has taken the
   * main one. Terminals still open in `cwd` — a shell has one directory — but
   * the agents that can read beyond it are told about these.
   */
  extraRoots: string[];
  panes: Pane[];
  /** Overrides the global theme for this workspace only. */
  themeId: string | null;
  /** Overrides the global default shell. null follows the settings. */
  shellId: string | null;
  /**
   * Nested split tree describing the pane arrangement. Unlike a CSS grid, each
   * split owns its own ratio, so resizing one boundary leaves the rest alone.
   */
  tree: SplitNode | null;
  /**
   * Saved commands belonging to this project, listed before the global ones —
   * the useful ones are usually the ones a single repository needs.
   */
  savedCommands: SavedCommand[];
  /** The sidebar folder it is filed under. null sits at the top level. */
  folderId: string | null;
}

/**
 * A sidebar group of workspaces. One level deep only: a folder holds
 * workspaces, never other folders.
 *
 * It owns nothing — deleting one sends its workspaces back to the top level —
 * and its contents are not listed here: each workspace names its folder, and
 * the order of `AppState.workspaces` is the order inside every folder.
 */
export interface WorkspaceFolder {
  id: string;
  name: string;
  collapsed: boolean;
}

export type SplitNode =
  /** Leaves point at a pane id: closing or moving a pane never renumbers. */
  | { type: "pane"; id: string }
  /** `row` places children side by side (vertical bar), `col` stacks them. */
  | { type: "split"; dir: "row" | "col"; ratio: number; a: SplitNode; b: SplitNode };

export interface Settings {
  themeId: string;
  /** null follows the OS language. */
  locale: "en" | "fr" | null;
  /** Default shell for every new pane. null means "first one detected". */
  shellId: string | null;
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  letterSpacing: number;
  cursorStyle: "block" | "bar" | "underline";
  cursorBlink: boolean;
  scrollback: number;
  padding: number;
  /**
   * What a pane types to start each agent, by agent id — only where it differs
   * from the plain command, so an agent nobody customised follows whatever a
   * later version ships.
   *
   * This is the command line, not a path: it is typed into an interactive
   * shell, so an alias or a function of your own works as well as a binary.
   * Resume arguments are appended to it, which is what keeps `--resume`
   * working on top of a custom launcher.
   */
  agentCommands: Record<string, string>;
  /** Relaunch agents with their previous session when a workspace opens. */
  autoResume: boolean;
  /**
   * Load the hangar-bridge mod into the Claude Code each pane starts (see
   * src-tauri/src/bridge.rs). On by default; the way out for a machine whose
   * Claude Code refuses to start with a plugin directory it did not install.
   */
  claudeBridge: boolean;
  /** Wait before auto-launching agents, so the shell finishes its profile. */
  launchDelayMs: number;
  /** Raise a desktop notification when a pane hands control back. */
  notifyOnIdle: boolean;
  /** Silence, in ms, after which a working pane counts as finished. */
  notifyIdleMs: number;
  /** Stay quiet for the pane you are already watching. */
  notifyOnlyWhenAway: boolean;
  /**
   * Re-bound keyboard shortcuts, by command id — only what differs from the
   * defaults in lib/shortcuts, so a default that changes in a later version
   * reaches everyone who never touched it. An empty array disables a command.
   */
  keybindings: Record<string, string[]>;
  /**
   * Saved commands every workspace offers, after its own. A command that is
   * about the tool rather than the project — `git status`, a prompt you reuse
   * — belongs here.
   */
  savedCommands: SavedCommand[];
  /**
   * Dictation. Off until asked for: it wants a microphone, and either a model
   * downloaded or a key pasted, so turning it on is a decision rather than a
   * default someone stumbles into.
   */
  voiceEnabled: boolean;
  /** `local` runs on this machine, `groq` sends the audio to Groq. */
  voiceEngine: VoiceEngine;
  /** Downloaded checkpoint, by catalogue id. */
  voiceModel: string;
  /** Model name on Groq. Kept apart from `voiceModel` so switching engines
      back and forth does not lose either choice. */
  voiceCloudModel: string;
  /** ISO code, or empty to let the model work it out. */
  voiceLanguage: string;
  /**
   * Legacy: the Groq key, in plain text, as versions before the keychain kept
   * it. load_state moves it to the system keychain and drops it; it is only
   * still here when the keychain refused it, and is never used for dictation.
   * The voice settings offer to delete it.
   */
  voiceApiKey?: string;
  /** Rewrite the transcript with a small model before it lands. */
  voiceCleanup: boolean;
  voiceCleanupModel: string;
  /** Project words the cleanup pass should spell the way you do. */
  voiceCleanupHint: string;
  /**
   * Press Enter for you once the text is in. Off by default — reading back
   * what a microphone heard before an agent acts on it is worth the keystroke.
   */
  voiceSubmit: boolean;
  /**
   * Publish what you are working on to Discord. Off by default: this is the
   * one setting that sends anything out of the machine.
   */
  discordPresence: boolean;
  /** Name the workspace on Discord. Off keeps the presence generic. */
  discordShowWorkspace: boolean;
  /** Name the agents running. Off only shows how many. */
  discordShowAgents: boolean;
  /**
   * Discord application the presence speaks for — its uploaded art is what the
   * card shows. Empty follows the one this app ships with.
   */
  discordAppId: string;
  /** The board shown as a panel beside the terminals. */
  boardDockOpen: boolean;
  /** Its width in px, as last dragged. */
  boardDockWidth: number;
  /**
   * Size of the interface text, in percent — sidebar, board, settings, pane
   * headers. The terminals keep `fontSize`: the two are read at different
   * distances and rarely want changing together.
   */
  uiScale: number;
}

export interface AppState {
  workspaces: Workspace[];
  /** In sidebar order. */
  folders: WorkspaceFolder[];
  activeWorkspaceId: string | null;
  settings: Settings;
}

/** One `folders[]` entry of a VS Code `.code-workspace`, already resolved. */
export interface WorkspaceRoot {
  name: string;
  path: string;
  /** False when the file points at a folder this machine does not have. */
  exists: boolean;
}

export interface AgentSession {
  id: string;
  label: string;
  modified_ms: number;
}

export const AGENTS: { id: AgentId; label: string; resumable: boolean }[] = [
  { id: "shell", label: "Shell", resumable: false },
  { id: "claude", label: "Claude Code", resumable: true },
  { id: "codex", label: "Codex", resumable: true },
  { id: "gemini", label: "Gemini", resumable: false },
  { id: "opencode", label: "OpenCode", resumable: false },
];

export const DEFAULT_SETTINGS: Settings = {
  themeId: "tokyo-night",
  locale: null,
  shellId: null,
  fontFamily: '"Cascadia Code", "JetBrains Mono", Consolas, "Courier New", monospace',
  fontSize: 13,
  lineHeight: 1.3,
  letterSpacing: 0,
  cursorStyle: "bar",
  cursorBlink: true,
  scrollback: 10000,
  padding: 10,
  agentCommands: {},
  autoResume: true,
  claudeBridge: true,
  launchDelayMs: 700,
  notifyOnIdle: true,
  notifyIdleMs: 3000,
  notifyOnlyWhenAway: true,
  keybindings: {},
  savedCommands: [],
  voiceEnabled: false,
  voiceEngine: "local",
  voiceModel: DEFAULT_VOICE_MODEL,
  voiceCloudModel: CLOUD_MODELS[0].id,
  voiceLanguage: "",
  voiceCleanup: false,
  voiceCleanupModel: "",
  voiceCleanupHint: "",
  voiceSubmit: false,
  discordPresence: false,
  discordShowWorkspace: true,
  discordShowAgents: true,
  discordAppId: "",
  boardDockOpen: false,
  boardDockWidth: 420,
  uiScale: 100,
};

/** Bounds of the interface size slider, in percent. */
export const UI_SCALE_MIN = 90;
export const UI_SCALE_MAX = 130;
