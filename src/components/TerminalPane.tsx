import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import type { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";

import {
  bridgeCancel,
  contextUsage,
  ptyAlive,
  ptyHistory,
  ptyKill,
  ptyResize,
  ptySpawn,
  ptyWrite,
  sessionExists,
  type QueuedPrompt,
} from "../lib/ipc";
import { isHandingOver } from "../lib/windows";
import { sessionName } from "../lib/paneNames";
import { subscribeBridge, type BridgeEvent } from "../lib/bridge";
import {
  clearPaneActivity,
  setConflict,
  setPaneActivity,
  setPlanLimits,
  useConflict,
  type AgentActivity,
} from "../lib/agentState";
import { Icon } from "./Icon";
import {
  computeContextGauge,
  formatTokens,
  type ContextGauge,
} from "../lib/context";
import { subscribePty } from "../lib/ptyBus";
import { registerFinder, registerTerminal } from "../lib/terminalRegistry";
import { attachWebgl, WEBGL_PARK_MS } from "../lib/webgl";
import { useShortcutLabel, useShortcutTitle } from "../lib/useShortcuts";
import { createActivityWatcher } from "../lib/activity";
import { notify } from "../lib/notify";
import { usePaneDrag } from "./PaneGrid";
import { CommandMenu } from "./CommandMenu";
import { TerminalSearch } from "./TerminalSearch";
import {
  claim,
  isResumable,
  knownSessionIds,
  launchCommand,
  sleep,
  watchForSession,
} from "../lib/agents";
import { useT } from "../i18n";
import type { CommandId } from "../lib/shortcuts";
import type { CommandScope } from "../store";
import type { TerminalTheme } from "../themes";
import {
  AGENTS,
  type AgentId,
  type Pane,
  type SavedCommand,
  type Settings,
  type ShellInfo,
} from "../types";

type Status = "starting" | "running" | "exited";

interface Props {
  pane: Pane;
  /** Owning workspace — where a click on this pane's notification has to land. */
  workspaceId: string;
  /** Goes into the name a Claude Code session runs under, never on screen. */
  workspaceName: string;
  cwd: string;
  /** Folders of the workspace that the terminal did not open in. */
  extraRoots: string[];
  settings: Settings;
  theme: TerminalTheme;
  focused: boolean;
  /**
   * False while the whole workspace is hidden. Focus is stored per workspace,
   * so `focused` alone says nothing about what the user is actually looking
   * at: every mounted workspace keeps one focused pane, on screen or not.
   */
  visible: boolean;
  index: number;
  availableAgents: string[];
  shells: ShellInfo[];
  /** Already resolved through the pane → workspace → global chain. */
  shell: ShellInfo | null;
  /** False when this is the last pane left: a workspace keeps at least one. */
  canClose: boolean;
  /** Saved commands of the workspace, listed before the global ones. */
  workspaceCommands: SavedCommand[];
  globalCommands: SavedCommand[];
  onFocus: () => void;
  onAgentChange: (agent: AgentId) => void;
  onShellChange: (shellId: string | null) => void;
  /** `null` when the conversation the pane held has nothing left to resume. */
  onSessionCaptured: (sessionId: string | null) => void;
  onRestart: () => void;
  onSplit: () => void;
  onClose: () => void;
  onOpenSessions: () => void;
  /** Opens what changed in the pane's folder; no button without it. */
  onShowDiff?: () => void;
  /** Writes the command out — to this pane, or to all of them if it says so. */
  onRunCommand: (command: SavedCommand) => void;
  onEditCommand: (command: SavedCommand, scope: CommandScope) => void;
  onRemoveCommand: (command: SavedCommand, scope: CommandScope) => void;
  onAddCommand: () => void;
}

export function TerminalPane({
  pane,
  workspaceId,
  workspaceName,
  cwd,
  extraRoots,
  settings,
  theme,
  focused,
  visible,
  index,
  availableAgents,
  shells,
  shell,
  canClose,
  workspaceCommands,
  globalCommands,
  onFocus,
  onAgentChange,
  onShellChange,
  onSessionCaptured,
  onRestart,
  onSplit,
  onClose,
  onOpenSessions,
  onShowDiff,
  onRunCommand,
  onEditCommand,
  onRemoveCommand,
  onAddCommand,
}: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const commandsRef = useRef<HTMLButtonElement>(null);
  const [status, setStatus] = useState<Status>("starting");
  /** The pane handed control back while you were looking somewhere else. */
  const [attention, setAttention] = useState(false);
  /** Where the agent stands in its context window, from its transcript. */
  const [context, setContext] = useState<ContextGauge | null>(null);
  /** Open per pane, not per workspace: two panes can have their own menu. */
  const [commandsOpen, setCommandsOpen] = useState(false);
  /** Where the agent stands — see lib/agentState. */
  const [activity, setActivity] = useState<AgentActivity>("idle");
  /**
   * The pane's Claude Code loaded the hangar-bridge mod: its turns, dialogs
   * and usage are reported rather than guessed from the output.
   */
  const [bridged, setBridged] = useState(false);
  /** The context as the mod reports it, which beats the transcript's. */
  const [bridgeContext, setBridgeContext] = useState<ContextGauge | null>(null);
  /** Prompts waiting for the agent to be free. */
  const [queue, setQueue] = useState<QueuedPrompt[]>([]);
  /** Another pane is asking whether it may edit a file this one edited. */
  const conflictFrom = useConflict(pane.id);
  /** The search bar, while open. `request` counts the shortcut's presses. */
  const [search, setSearch] = useState<{ request: number; seed: string } | null>(null);
  /** What the bar held when it last closed, for the next time it opens. */
  const lastQueryRef = useRef("");
  const drag = usePaneDrag();
  const t = useT();
  const shortcut = useShortcutLabel();
  const withKeys = useShortcutTitle();

  // Latest settings/theme without forcing the terminal to be rebuilt.
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const shellRef = useRef(shell);
  shellRef.current = shell;
  const extraRootsRef = useRef(extraRoots);
  extraRootsRef.current = extraRoots;
  // The terminal effect runs once per pane id, so whatever it reads later must
  // come through a ref rather than the closure captured on mount.
  const focusedRef = useRef(focused);
  focusedRef.current = focused;
  const visibleRef = useRef(visible);
  visibleRef.current = visible;
  const tRef = useRef(t);
  tRef.current = t;
  const activityRef = useRef(activity);
  activityRef.current = activity;
  const bridgedRef = useRef(bridged);
  bridgedRef.current = bridged;
  const sessionIdRef = useRef(pane.sessionId);
  sessionIdRef.current = pane.sessionId;
  const workspaceNameRef = useRef(workspaceName);
  workspaceNameRef.current = workspaceName;

  const jumpKeys = index < 9 ? shortcut(`pane.focus${index + 1}` as CommandId) : "";

  const paneCwd = pane.cwd || cwd;
  const agentMeta = AGENTS.find((a) => a.id === pane.agent);
  const agentMissing = pane.agent !== "shell" && !availableAgents.includes(pane.agent);

  // The terminal is bound to the pane id: changing agent or restarting
  // creates a new id upstream, which remounts this whole effect.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const controller = new AbortController();
    let unsubscribe: (() => void) | null = null;
    let disposed = false;

    const term = new Terminal({
      fontFamily: settings.fontFamily,
      fontSize: settings.fontSize,
      lineHeight: settings.lineHeight,
      letterSpacing: settings.letterSpacing,
      cursorStyle: settings.cursorStyle,
      cursorBlink: settings.cursorBlink,
      scrollback: settings.scrollback,
      theme: theme.xterm,
      allowProposedApi: true,
      macOptionIsMeta: true,
      convertEol: false,
    });

    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    // Emoji and many of the symbols agents draw their UI with are two cells
    // wide from Unicode 9 on. xterm's default table is Unicode 6, which gives
    // them one and shifts every column after them.
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.open(host);

    /**
     * Ctrl+V pastes, as it does everywhere else on the machine.
     *
     * Left alone, xterm maps every Ctrl+letter to its control code — Ctrl+V
     * becomes ^V, sent straight to the shell — and cancels the key event,
     * which also cancels the webview's own paste. Returning false hands the
     * key back untouched: the browser pastes into the helper textarea, and
     * xterm already listens for that paste event.
     *
     * The trade-off is that ^V no longer reaches the shell, so readline's
     * quoted-insert is out of reach from that key — the same bargain
     * Windows Terminal makes.
     *
     * Shift+Enter opens a line instead of sending one, in an agent pane.
     *
     * xterm decides what Enter means from the keycode alone, so Shift never
     * makes it out: both keys write a bare CR, and the agent answers a prompt
     * you were still halfway through writing. ESC+CR is the sequence the CLIs
     * already read as "newline" — it is what `claude /terminal-setup` binds
     * Shift+Enter to in VS Code and iTerm2 — so sending it here gives the key
     * the meaning it has in every other terminal the user has set up.
     *
     * A plain shell is left out, because there ESC is not a prefix waiting for
     * a second byte but a key of its own: PSReadLine reverts the line on it and
     * cmd clears it, so the shell would throw away what was typed rather than
     * run it — and readline, which does treat it as a prefix, has nothing bound
     * to M-CR and would ignore the keystroke. Shift+Enter keeps submitting
     * there, which is what it did before and what every other terminal does.
     *
     * Read straight off `pane` rather than through a ref: this effect is keyed
     * on the pane id, and changing a pane's agent mints a new one.
     */
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown" || event.altKey) return true;

      if (
        pane.agent !== "shell" &&
        event.key === "Enter" &&
        event.shiftKey &&
        !event.ctrlKey &&
        !event.metaKey
      ) {
        // xterm bows out on `false` without cancelling the event, and Enter
        // left to the webview types into the helper textarea it reads pastes
        // from.
        event.preventDefault();
        term.input("\x1b\r");
        return false;
      }

      const pasting =
        (event.ctrlKey || event.metaKey) && (event.key === "v" || event.key === "V");
      return !pasting;
    });

    termRef.current = term;
    fitRef.current = fit;
    // Shortcuts that act on the terminal itself — clear, copy, scroll — are
    // dispatched from the app and look the instance up by pane id.
    const unregister = registerTerminal(pane.id, term);

    try {
      fit.fit();
    } catch {
      /* host not measured yet; the observer below will retry */
    }

    /**
     * Set while a taken-over pane's history is replayed. That history holds
     * the queries the program sent when it started — cursor position, terminal
     * attributes — and xterm answers each one as it reads it. Those answers
     * are long overdue: let through, they reach the program as keystrokes and
     * garble whatever is typed next.
     */
    let replaying = false;

    term.onData((data) => {
      if (replaying) return;
      ptyWrite(pane.id, data).catch(() => undefined);
    });

    // A key pressed in the pane is you taking it back, or answering what it
    // was waiting on. Keys rather than data: the terminal also answers the
    // agent's own queries through onData, and those are nobody's answer.
    term.onKey(() => {
      if (activityRef.current === "yours") setActivity("idle");
      else if (activityRef.current === "waiting") setActivity("working");
    });

    term.onResize(({ cols, rows }) => {
      ptyResize(pane.id, cols, rows).catch(() => undefined);
    });

    /**
     * Everything needed to decide whether a hand-back is worth interrupting
     * you for is local to the pane: whether it is the one you are watching,
     * and whether the window is even in front. Nothing has to travel upwards.
     */
    function announce(body: string) {
      if (disposed) return;
      const windowFocused = document.hasFocus();
      // You are already looking at it: not news. All three have to hold —
      // being the focused pane of a workspace that is off screen means the
      // hand-back happened out of sight, which is the whole point of this.
      if (windowFocused && focusedRef.current && visibleRef.current) return;

      setAttention(true);

      const current = settingsRef.current;
      if (!current.notifyOnIdle) return;
      // With the window in front, the pane badge is enough of a signal.
      if (current.notifyOnlyWhenAway && windowFocused) return;

      notify(
        tRef.current("notify.title", {
          // Both safe to capture: the name never changes, and a different
          // agent means a different pane id, which remounts this effect.
          name: pane.name,
          agent: agentMeta?.label ?? pane.agent,
        }),
        body,
        // Same reasoning for the target: a pane never changes workspace, and
        // its id lives exactly as long as this effect does.
        { workspaceId, paneId: pane.id },
      );
    }

    /** The agent finished its turn: yours, unless you are already on it. */
    function handBack(body: string) {
      if (disposed) return;
      const looking = document.hasFocus() && focusedRef.current && visibleRef.current;
      setActivity(looking ? "idle" : "yours");
      announce(body);
    }

    // Everything below that reads the output is a guess, and a pane whose mod
    // reports its turns has no use for one — it would only notify twice.
    const watcher = createActivityWatcher({
      idleMs: () => settingsRef.current.notifyIdleMs,
      onSettle: () => {
        if (!bridgedRef.current) handBack(tRef.current("notify.settled"));
      },
      onBusy: () => {
        if (!bridgedRef.current) setActivity("working");
      },
    });

    // A bell is the agent asking for you outright, so it settles the pane
    // without waiting out the silence.
    term.onBell(() => {
      if (!bridgedRef.current) watcher.ring();
    });

    // OSC 9 and OSC 777 are the terminal escape codes for "raise a desktop
    // notification": the agent already wrote the message, so it is passed
    // through instead of being guessed at.
    term.parser.registerOscHandler(9, (data) => {
      // Only iTerm2's `OSC 9 ; <message>` is a notification. On Windows the
      // same code is mostly used for ConEmu sub-commands that have nothing to
      // say to you: `OSC 9 ; 4 ; <state> ; <pct>` is the taskbar progress bar
      // (PowerShell 7, winget, pip) and `OSC 9 ; 9 ; <cwd>` reports the
      // working directory (ConEmu, Windows Terminal). They are told apart by
      // their leading numeric segment — a message written for a human does not
      // start with one — and left to whoever else wants them, rather than
      // badging the pane and popping up a notification reading `4;3;0`.
      if (/^\d+(;|$)/.test(data)) return false;
      if (!bridgedRef.current) handBack(data.trim() || tRef.current("notify.settled"));
      return true;
    });
    term.parser.registerOscHandler(777, (data) => {
      const [kind, title, body] = data.split(";");
      if (kind !== "notify") return false;
      if (!bridgedRef.current) {
        handBack(body?.trim() || title?.trim() || tRef.current("notify.settled"));
      }
      return true;
    });

    /**
     * What the hangar-bridge mod reports from inside this pane's Claude Code.
     * The first report of any kind is proof the mod loaded, and from then on
     * the guesses above stand down.
     */
    let bridgeModel: string | null = null;
    /** The pane whose file this one is asking about, while it asks. */
    let conflictWith: string | null = null;
    function captureSession(sessionId: string) {
      // Typed into the shell on the next launch, after --resume: anything but
      // a plain id is refused rather than quoted.
      if (!/^[\w-]+$/.test(sessionId) || sessionId === sessionIdRef.current) return;
      claim(sessionId);
      onSessionCaptured(sessionId);
    }
    function onBridgeEvent(event: BridgeEvent) {
      if (disposed) return;
      if (event.kind !== "end") setBridged(true);
      switch (event.kind) {
        case "hello":
          bridgeModel = event.model;
          captureSession(event.sessionId);
          break;
        case "session":
          captureSession(event.sessionId);
          break;
        case "turn":
          if (event.phase === "start") {
            setActivity("working");
          } else {
            handBack(
              event.answer ||
                tRef.current(event.reason === "aborted" ? "notify.aborted" : "notify.settled"),
            );
          }
          break;
        case "permission":
          // Whichever way the question ends, the pane it was about is no
          // longer in anyone's way.
          if (conflictWith) {
            setConflict(conflictWith, null);
            conflictWith = null;
          }
          if (event.waiting && event.conflictPane) {
            conflictWith = event.conflictPane;
            setConflict(conflictWith, pane.name);
          }
          if (event.waiting) {
            setActivity("waiting");
            announce(
              event.conflict
                ? tRef.current("notify.conflict", { name: event.conflict })
                : tRef.current("notify.permission", { tool: event.tool ?? "" }),
            );
          } else if (activityRef.current === "waiting") {
            setActivity("working");
          }
          break;
        case "usage": {
          const { tokens, window: size, percent } = event.context;
          const pct =
            percent ?? (tokens !== null && size > 0 ? (tokens / size) * 100 : null);
          setBridgeContext(
            pct === null
              ? null
              : {
                  pct: Math.max(0, Math.min(100, Math.round(pct))),
                  usedTokens: tokens ?? Math.round((pct / 100) * size),
                  window: size,
                  model: bridgeModel,
                },
          );
          setPlanLimits(event.rateLimits);
          break;
        }
        case "dropped":
          announce(tRef.current("notify.dropped", { text: event.text }));
          break;
        case "end":
          // Claude Code exited; whatever runs in the pane next is a shell.
          if (conflictWith) {
            setConflict(conflictWith, null);
            conflictWith = null;
          }
          setBridged(false);
          setBridgeContext(null);
          setActivity("idle");
          break;
      }
    }
    let unsubscribeBridge: (() => void) | null = null;

    // Read and cleared by the session watcher: a silent pane cannot have
    // started a conversation, so it is not worth a look at the transcripts.
    let printedSinceLastLook = false;

    /**
     * Output held back while a running pane is being taken over, until its
     * history is on screen; then, what that history already covered. Starts
     * holding, because whether the pane is already running is only known
     * after the subscription is in place.
     */
    let held: { data: string; seq: number }[] | null = [];
    let replayedTo = -1;
    const replayed = (seq: number) => typeof seq === "number" && seq <= replayedTo;

    (async () => {
      try {
        const stop = await subscribePty(
          pane.id,
          (data, seq) => {
            if (held) {
              held.push({ data, seq });
              return;
            }
            if (replayed(seq)) return;
            term.write(data);
            watcher.push();
            printedSinceLastLook = true;
          },
          () => {
            if (disposed) return;
            setStatus("exited");
            // A Claude Code killed mid-turn never says goodbye: its last
            // report would otherwise hold the pane, and the sidebar, at
            // "working" until a restart.
            if (conflictWith) {
              setConflict(conflictWith, null);
              conflictWith = null;
            }
            setBridged(false);
            setActivity("idle");
          },
        );
        // The cleanup may have run during that await, in which case it saw
        // `unsubscribe` still null and unsubscribed nothing. Assigning now
        // would strand the handlers — and the terminal they close over — for
        // good, so the subscription is undone here instead.
        if (disposed) {
          stop();
          return;
        }
        unsubscribe = stop;

        // Before the spawn too: the mod says hello as soon as Claude Code
        // starts, and a hello nobody heard leaves the pane guessing.
        const stopBridge = await subscribeBridge(pane.id, onBridgeEvent, (pending) => {
          if (!disposed) setQueue(pending);
        });
        if (disposed) {
          stopBridge();
          return;
        }
        unsubscribeBridge = stopBridge;

        /**
         * Already running: another window let go of this pane — its workspace
         * moved to this window — or this window was reloaded. The pane is
         * taken over as it stands: no spawn and no agent launch, just what it
         * printed last, then the live output from where that left off.
         */
        const adopted = await ptyAlive(pane.id).catch(() => false);
        if (disposed) return;
        if (adopted) {
          const history = await ptyHistory(pane.id).catch(() => null);
          if (disposed) return;
          if (history) {
            replaying = true;
            // The callback runs once xterm has parsed the whole history, and
            // so once it has answered everything in it.
            term.write(history.data, () => {
              replaying = false;
            });
            replayedTo = history.end;
          }
          // The size it had in the other window is not this one's.
          ptyResize(pane.id, term.cols, term.rows).catch(() => undefined);
          setStatus("running");
        }
        const pending = held ?? [];
        held = null;
        for (const chunk of pending) {
          if (!replayed(chunk.seq)) term.write(chunk.data);
        }

        // Snapshot before launching, so a brand new transcript stands out.
        const known = isResumable(pane.agent)
          ? await knownSessionIds(pane.agent, paneCwd)
          : new Set<string>();
        if (disposed) return;

        if (!adopted) {
          await ptySpawn({
            id: pane.id,
            cwd: paneCwd,
            shell: shellRef.current,
            cols: term.cols,
            rows: term.rows,
            name: pane.name,
            bridge: settingsRef.current.claudeBridge,
          });
          // Same race as the subscription above: a cleanup that ran during the
          // spawn killed an id the backend did not know yet, so the shell it
          // just registered would outlive the pane. Killing again is free when
          // the id is already gone.
          if (disposed) {
            if (!isHandingOver(pane.id)) ptyKill(pane.id).catch(() => undefined);
            return;
          }
          setStatus("running");

          if (pane.agent === "shell") return;

          // Claude Code names a session the moment it starts but writes its
          // transcript only on the first message, so a pane reset and left
          // alone holds an id with nothing behind it, and resuming that only
          // prints "No conversation found". Asked while the shell settles.
          const saved = settingsRef.current.autoResume ? pane.sessionId : null;
          const behind =
            saved && isResumable(pane.agent)
              ? sessionExists(pane.agent, saved).catch(() => null)
              : null;

          // Let the shell profile settle before typing into it.
          await sleep(settingsRef.current.launchDelayMs, controller.signal);
          if (disposed || controller.signal.aborted) return;

          let resumeId = saved;
          if (resumeId && (await behind) === false) {
            if (disposed) return;
            resumeId = null;
            onSessionCaptured(null);
          }
          const command = launchCommand(pane.agent, resumeId, {
            extraRoots: extraRootsRef.current,
            commands: settingsRef.current.agentCommands,
            // Read when the agent starts: a workspace renamed later shows up
            // on the next launch.
            name: sessionName(pane.name, workspaceNameRef.current),
          });
          if (command) {
            if (resumeId) claim(resumeId);
            await ptyWrite(pane.id, `${command}\r`).catch(() => undefined);
          }
        } else if (pane.agent === "shell") {
          return;
        }

        // Watched for as long as the pane lives, resumed or not: /new inside
        // the agent opens another transcript, and the pane has to follow it
        // or the next launch would resume the conversation you walked away
        // from. `known` already holds the resumed id, so it is not re-read as
        // a discovery.
        if (isResumable(pane.agent)) {
          // Deliberately not awaited — it polls until the pane goes away.
          watchForSession({
            agent: pane.agent,
            cwd: paneCwd,
            known,
            signal: controller.signal,
            hadOutput: () => {
              const printed = printedSinceLastLook;
              printedSinceLastLook = false;
              return printed;
            },
            // The mod names the session outright; the transcript hunt is for
            // the agents that have none.
            onFound: (sessionId) =>
              !disposed && !bridgedRef.current && onSessionCaptured(sessionId),
          }).catch(() => undefined);
        }
      } catch (err) {
        // Covers the whole start path, event subscription included: a pane
        // that cannot start says so and offers its restart button, instead of
        // sitting on "starting" forever behind an unhandled rejection.
        if (disposed) return;
        term.writeln(
          `\r\n\x1b[31m${tRef.current("pane.spawnFailed")}: ${String(err)}\x1b[0m`,
        );
        setStatus("exited");
      }
    })();

    const observer = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        /* pane is hidden or zero-sized */
      }
    });
    observer.observe(host);

    return () => {
      disposed = true;
      controller.abort();
      observer.disconnect();
      watcher.dispose();
      unregister();
      unsubscribe?.();
      unsubscribeBridge?.();
      if (conflictWith) setConflict(conflictWith, null);
      // A pane moving to another window keeps running there.
      if (!isHandingOver(pane.id)) ptyKill(pane.id).catch(() => undefined);
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pane.id]);

  // Live restyle: theme and typography changes apply without a restart.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = theme.xterm;
    term.options.fontFamily = settings.fontFamily;
    term.options.fontSize = settings.fontSize;
    term.options.lineHeight = settings.lineHeight;
    term.options.letterSpacing = settings.letterSpacing;
    term.options.cursorStyle = settings.cursorStyle;
    term.options.cursorBlink = settings.cursorBlink;
    term.options.scrollback = settings.scrollback;
    requestAnimationFrame(() => {
      try {
        fitRef.current?.fit();
      } catch {
        /* ignore */
      }
    });
  }, [
    theme,
    settings.fontFamily,
    settings.fontSize,
    settings.lineHeight,
    settings.letterSpacing,
    settings.cursorStyle,
    settings.cursorBlink,
    settings.scrollback,
  ]);

  // Visibility is a dependency, not just a guard: switching workspaces changes
  // no pane's `focused` — it is stored per workspace — while the grid it left
  // gets `display:none`, which drops the DOM focus back onto <body>. Without
  // the re-run, keystrokes would go nowhere until a pane is clicked. A hidden
  // pane never runs this, so it can neither steal the focus nor clear a badge
  // it earned while off screen when the window comes back to the front.
  useEffect(() => {
    if (!focused || !visible) return;
    termRef.current?.focus();
    // Looking at the pane clears its badge — including when the window comes
    // back to the front with this pane already focused.
    setAttention(false);
    // Seen counts as taken back; a dialog still waiting is still waiting.
    if (activityRef.current === "yours") setActivity("idle");
    const clear = () => {
      setAttention(false);
      if (activityRef.current === "yours") setActivity("idle");
    };
    window.addEventListener("focus", clear);
    return () => window.removeEventListener("focus", clear);
  }, [focused, visible]);

  // Published for the sidebar, which counts each workspace's agents by state.
  // A plain shell is left out unless a Claude Code in it reports: its builds
  // and test runs would otherwise count as agents at work.
  const reportsActivity = pane.agent !== "shell" || bridged;
  useEffect(() => {
    if (reportsActivity) setPaneActivity(pane.id, { workspaceId, activity, bridged });
    else clearPaneActivity(pane.id);
  }, [pane.id, workspaceId, activity, bridged, reportsActivity]);
  // A pane moving to another window keeps its state: that window reports it
  // from now on, and clearing it here could land after its first report.
  useEffect(
    () => () => {
      if (!isHandingOver(pane.id)) clearPaneActivity(pane.id);
    },
    [pane.id],
  );

  // The transcript is the agent's own account of its context, polled at a
  // walking pace — a gauge needs no more — and only while the pane is on
  // screen with its shell alive. A pane that goes hidden keeps its last
  // honest reading and picks up again when shown.
  useEffect(() => {
    // The mod's figure is the live one, window included; reading the
    // transcript as well would only race it.
    if (bridged) return;
    if (!pane.sessionId || !isResumable(pane.agent)) {
      // Gemini and opencode keep no readable transcript, and before a session
      // is captured there is nothing to read: no gauge beats a made-up zero.
      setContext(null);
      return;
    }
    if (!visible || status !== "running") return;
    const sessionId = pane.sessionId;
    let cancelled = false;
    const tick = async () => {
      try {
        const usage = await contextUsage(pane.agent, sessionId, paneCwd);
        if (!cancelled) {
          setContext(usage ? computeContextGauge(pane.agent, usage) : null);
        }
      } catch {
        /* transient read failure: keep the last honest reading */
      }
    };
    tick();
    const timer = window.setInterval(tick, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [pane.id, pane.sessionId, pane.agent, status, visible, paneCwd, bridged]);

  const gauge = (bridged && bridgeContext) || context;

  // The menu is placed from the button's position on screen, and a hidden
  // workspace has none — leaving it open would put it back somewhere else
  // entirely on the way back.
  useEffect(() => {
    if (!visible) setCommandsOpen(false);
  }, [visible]);

  // Drawn on the GPU while on screen. A pane that leaves the screen keeps its
  // context for a moment — a quick look at another workspace, or a zoom
  // toggled back, should not rebuild a texture atlas — then hands it back so
  // the panes of other workspaces never add up past the browser's cap.
  const webglRef = useRef<WebglAddon | null>(null);
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    if (visible) {
      if (!webglRef.current) {
        const addon = attachWebgl(term, () => {
          if (webglRef.current === addon) webglRef.current = null;
        });
        webglRef.current = addon;
      }
      return;
    }
    const timer = window.setTimeout(() => {
      webglRef.current?.dispose();
      webglRef.current = null;
    }, WEBGL_PARK_MS);
    return () => window.clearTimeout(timer);
  }, [visible, pane.id]);

  // Opened from the app's shortcut, which reaches the bar by pane id. A
  // single-line selection is what you meant to look for, as in an editor —
  // but only on the way in: once the bar is open, the selection is the
  // current match, and reading it back would replace a regex with what it hit.
  useEffect(
    () =>
      registerFinder(pane.id, () =>
        setSearch((current) => {
          if (current) return { request: current.request + 1, seed: "" };
          const selected = termRef.current?.getSelection() ?? "";
          const seed =
            selected && !selected.includes("\n") ? selected : lastQueryRef.current;
          return { request: 1, seed };
        }),
      ),
    [pane.id],
  );

  function closeSearch(query: string) {
    lastQueryRef.current = query;
    setSearch(null);
    termRef.current?.focus();
  }

  /**
   * Focus is handed back explicitly rather than left to the effect above: the
   * pane was already the focused one in most cases, so nothing it depends on
   * changed, and the keyboard would stay with the ⚡ button — which the click
   * that opened the menu gave it, and which swallows everything typed at it.
   */
  function closeCommands() {
    setCommandsOpen(false);
    termRef.current?.focus();
  }

  function runCommand(command: SavedCommand) {
    onRunCommand(command);
    closeCommands();
  }

  return (
    <section
      className={`pane ${focused ? "pane--focused" : ""} ${attention ? "pane--attn" : ""}`}
      onMouseDown={onFocus}
      aria-label={t("pane.label", { name: pane.name })}
    >
      <header
        className="pane__bar"
        onPointerDown={(event) => drag.begin(pane.id, event)}
        title={t("pane.move")}
      >
        {/*
          Only the first nine panes get the shortcut hint: there are nine
          "focus pane N" commands, and the user is free to unbind any of them.
          The rest fall back to naming the pane rather than promising a key
          that does not exist.
        */}
        <span
          className="pane__index"
          title={
            jumpKeys
              ? t("pane.jump", { keys: jumpKeys })
              : t("pane.label", { name: pane.name })
          }
        >
          {pane.name}
        </span>

        <select
          className="pane__agent"
          value={pane.agent}
          onChange={(e) => onAgentChange(e.target.value as AgentId)}
          title={t("pane.agent")}
        >
          {AGENTS.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.label}
              {agent.id !== "shell" && !availableAgents.includes(agent.id)
                ? t("pane.missing")
                : ""}
            </option>
          ))}
        </select>

        <select
          className="pane__agent pane__agent--shell"
          value={pane.shellId ?? ""}
          onChange={(e) => onShellChange(e.target.value || null)}
          title={
            shell ? t("pane.shellTitle", { program: shell.program }) : t("pane.shell")
          }
        >
          <option value="">
            {t("pane.inherited", { name: shell ? shell.label : t("pane.shell") })}
          </option>
          {shells.map((entry) => (
            <option key={entry.id} value={entry.id}>
              {entry.label}
            </option>
          ))}
        </select>

        {agentMeta?.resumable && (
          <button
            className={`pane__session ${pane.sessionId ? "is-set" : ""}`}
            onClick={onOpenSessions}
            title={
              pane.sessionId
                ? t("pane.resuming", { id: pane.sessionId })
                : t("pane.noSession")
            }
          >
            <Icon name="history" size={12} />
            {pane.sessionId ? pane.sessionId.slice(0, 8) : t("pane.newSession")}
          </button>
        )}

        {gauge && (
          <span
            className={`pane__ctx ${
              gauge.pct >= 90 ? "pane__ctx--hot" : gauge.pct >= 70 ? "pane__ctx--warn" : ""
            }`}
            title={t("pane.context", {
              used: formatTokens(gauge.usedTokens),
              window: formatTokens(gauge.window),
              pct: gauge.pct,
              model: gauge.model ?? pane.agent,
            })}
          >
            <span className="pane__ctxbar">
              <span className="pane__ctxfill" style={{ width: `${gauge.pct}%` }} />
            </span>
            <span className="pane__ctxpct">{gauge.pct}%</span>
          </span>
        )}

        <span className="pane__spacer" />

        {queue.length > 0 && (
          <button
            className="pane__queue"
            onClick={() => bridgeCancel(pane.id).catch(() => undefined)}
            title={t("pane.queue", { n: queue.length })}
            aria-label={t("pane.queue", { n: queue.length })}
          >
            <Icon name="hourglass" size={12} />
            {queue.length}
          </button>
        )}
        {conflictFrom && (
          <span
            className="pane__warn pane__warn--conflict"
            title={t("pane.conflict", { name: conflictFrom })}
            role="img"
            aria-label={t("pane.conflict", { name: conflictFrom })}
          >
            <Icon name="alert" size={13} />
          </span>
        )}
        {agentMissing && (
          <span
            className="pane__warn"
            title={t("pane.notOnPath", { agent: pane.agent })}
            role="img"
            aria-label={t("pane.notOnPath", { agent: pane.agent })}
          >
            <Icon name="alert" size={13} />
          </span>
        )}
        {/* An agent pane says the same through its state; a shell keeps the
            badge for the build that finished while you were away. */}
        {attention && !reportsActivity && (
          <span
            className="pane__attn"
            title={t("pane.attention")}
            role="img"
            aria-label={t("pane.attention")}
          >
            <Icon name="bell" size={13} />
          </span>
        )}
        <PaneState
          status={status}
          activity={reportsActivity ? activity : null}
          bridged={bridged}
        />
        {/* A button among the other pane actions, which the header's drag
            already steps aside for — it ignores a press that lands on one. */}
        <button
          ref={commandsRef}
          className={`pane__action ${commandsOpen ? "is-open" : ""}`}
          onClick={() => (commandsOpen ? closeCommands() : setCommandsOpen(true))}
          title={t("commands.menu")}
          aria-label={t("commands.menu")}
          aria-haspopup="menu"
          aria-expanded={commandsOpen}
        >
          <Icon name="zap" />
        </button>
        {onShowDiff && (
          <button
            className="pane__action"
            onClick={onShowDiff}
            title={t("diff.open")}
            aria-label={t("diff.open")}
          >
            <Icon name="diff" />
          </button>
        )}
        <button
          className="pane__action"
          onClick={onRestart}
          title={withKeys(t("pane.restart"), "pane.restart")}
          aria-label={t("pane.restart")}
        >
          <Icon name="restart" />
        </button>
        <button
          className="pane__action"
          onClick={onSplit}
          title={withKeys(t("pane.split"), "pane.split")}
          aria-label={t("pane.split")}
        >
          <Icon name="split" />
        </button>
        <button
          className="pane__action pane__action--close"
          onClick={onClose}
          disabled={!canClose}
          title={canClose ? withKeys(t("pane.close"), "pane.close") : t("pane.closeLast")}
          aria-label={t("pane.close")}
        >
          <Icon name="close" />
        </button>
      </header>

      {/* The search bar floats over the terminal from beside its host rather
          than inside it: xterm owns every child of that element. */}
      <div className="pane__body">
        <div
          className="pane__term"
          ref={hostRef}
          style={{ padding: settings.padding, background: theme.xterm.background }}
        />
        {search && termRef.current && (
          <TerminalSearch
            term={termRef.current}
            theme={theme}
            request={search.request}
            seed={search.seed}
            onClose={closeSearch}
          />
        )}
      </div>

      {/* Outside the header on purpose: nested in it, a press anywhere in the
          menu that is not on a control would reach the bar's drag handler. */}
      {commandsOpen && (
        <CommandMenu
          anchorRef={commandsRef}
          workspaceCommands={workspaceCommands}
          globalCommands={globalCommands}
          onRun={runCommand}
          onEdit={(command, scope) => {
            setCommandsOpen(false);
            onEditCommand(command, scope);
          }}
          onRemove={onRemoveCommand}
          onAdd={() => {
            setCommandsOpen(false);
            onAddCommand();
          }}
          onClose={closeCommands}
        />
      )}
    </section>
  );
}

/**
 * The dot at the end of a pane's header. Until the shell runs it says that
 * much; after, for an agent, where the agent stands — with a word for the one
 * state that needs you to act, since a colour alone is easy to miss in a grid
 * of eight.
 */
function PaneState({
  status,
  activity,
  bridged,
}: {
  status: Status;
  /** null for a plain shell, which has no agent to report on. */
  activity: AgentActivity | null;
  bridged: boolean;
}) {
  const t = useT();
  const state = status !== "running" ? status : (activity ?? "running");
  const label = t(`pane.state.${state}` as const);
  const source =
    status === "running" && activity
      ? t(bridged ? "pane.state.reported" : "pane.state.guessed")
      : "";
  const title = source ? `${label} · ${source}` : label;
  return (
    <span
      className={`pane__state pane__state--${state}`}
      title={title}
      role="status"
      aria-label={title}
    >
      <span className="pane__state-dot" aria-hidden="true" />
      {state === "waiting" && (
        <span className="pane__state-word">{t("pane.state.waitingShort")}</span>
      )}
    </span>
  );
}
