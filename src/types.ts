export type AgentId = "shell" | "claude" | "codex" | "gemini" | "opencode";

/** Where a dictation is transcribed. Mirrored in Rust as `EngineKind`. */
export type VoiceEngine = "local" | "groq";

/** The catalogue id the Rust side downloads and loads for local dictation. */
export const DEFAULT_VOICE_MODEL = "parakeet-tdt-0.6b-v3";

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
 * Offered in the language picker. Not the full list either engine handles —
 * Parakeet has 25 and Whisper 99 — just the ones worth a click, with the field
 * left free-form underneath for anything else.
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
  /** Also used by the cleanup pass, which talks to the same API. */
  voiceApiKey: string;
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
}

export interface AppState {
  workspaces: Workspace[];
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
  launchDelayMs: 700,
  notifyOnIdle: true,
  notifyIdleMs: 3000,
  notifyOnlyWhenAway: true,
  keybindings: {},
  voiceEnabled: false,
  voiceEngine: "local",
  voiceModel: DEFAULT_VOICE_MODEL,
  voiceCloudModel: CLOUD_MODELS[0].id,
  voiceLanguage: "",
  voiceApiKey: "",
  voiceCleanup: false,
  voiceCleanupModel: "",
  voiceCleanupHint: "",
  voiceSubmit: false,
  discordPresence: false,
  discordShowWorkspace: true,
  discordShowAgents: true,
  discordAppId: "",
};
