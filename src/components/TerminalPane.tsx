import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";

import { ptyKill, ptyResize, ptySpawn, ptyWrite } from "../lib/ipc";
import { subscribePty } from "../lib/ptyBus";
import { registerTerminal } from "../lib/terminalRegistry";
import { useShortcutLabel, useShortcutTitle } from "../lib/useShortcuts";
import { createActivityWatcher } from "../lib/activity";
import { notify } from "../lib/notify";
import { usePaneDrag } from "./PaneGrid";
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
import type { TerminalTheme } from "../themes";
import {
  AGENTS,
  type AgentId,
  type Pane,
  type Settings,
  type ShellInfo,
} from "../types";

type Status = "starting" | "running" | "exited";

interface Props {
  pane: Pane;
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
  onFocus: () => void;
  onAgentChange: (agent: AgentId) => void;
  onShellChange: (shellId: string | null) => void;
  onSessionCaptured: (sessionId: string) => void;
  onRestart: () => void;
  onSplit: () => void;
  onClose: () => void;
  onOpenSessions: () => void;
}

export function TerminalPane({
  pane,
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
  onFocus,
  onAgentChange,
  onShellChange,
  onSessionCaptured,
  onRestart,
  onSplit,
  onClose,
  onOpenSessions,
}: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [status, setStatus] = useState<Status>("starting");
  /** The pane handed control back while you were looking somewhere else. */
  const [attention, setAttention] = useState(false);
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
     */
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown" || event.altKey) return true;
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

    term.onData((data) => {
      ptyWrite(pane.id, data).catch(() => undefined);
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
      );
    }

    const watcher = createActivityWatcher({
      idleMs: () => settingsRef.current.notifyIdleMs,
      onSettle: () => announce(tRef.current("notify.settled")),
    });

    // A bell is the agent asking for you outright, so it settles the pane
    // without waiting out the silence.
    term.onBell(() => watcher.ring());

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
      announce(data.trim() || tRef.current("notify.settled"));
      return true;
    });
    term.parser.registerOscHandler(777, (data) => {
      const [kind, title, body] = data.split(";");
      if (kind !== "notify") return false;
      announce(body?.trim() || title?.trim() || tRef.current("notify.settled"));
      return true;
    });

    // Read and cleared by the session watcher: a silent pane cannot have
    // started a conversation, so it is not worth a look at the transcripts.
    let printedSinceLastLook = false;

    (async () => {
      try {
        const stop = await subscribePty(
          pane.id,
          (data) => {
            term.write(data);
            watcher.push();
            printedSinceLastLook = true;
          },
          () => !disposed && setStatus("exited"),
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

        // Snapshot before launching, so a brand new transcript stands out.
        const known = isResumable(pane.agent)
          ? await knownSessionIds(pane.agent, paneCwd)
          : new Set<string>();
        if (disposed) return;

        await ptySpawn({
          id: pane.id,
          cwd: paneCwd,
          shell: shellRef.current,
          cols: term.cols,
          rows: term.rows,
        });
        // Same race as the subscription above: a cleanup that ran during the
        // spawn killed an id the backend did not know yet, so the shell it
        // just registered would outlive the pane. Killing again is free when
        // the id is already gone.
        if (disposed) {
          ptyKill(pane.id).catch(() => undefined);
          return;
        }
        setStatus("running");

        if (pane.agent === "shell") return;

        // Let the shell profile settle before typing into it.
        await sleep(settingsRef.current.launchDelayMs, controller.signal);
        if (disposed || controller.signal.aborted) return;

        const resumeId = settingsRef.current.autoResume ? pane.sessionId : null;
        const command = launchCommand(pane.agent, resumeId, extraRootsRef.current);
        if (command) {
          if (resumeId) claim(resumeId);
          await ptyWrite(pane.id, `${command}\r`).catch(() => undefined);
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
            onFound: (sessionId) => !disposed && onSessionCaptured(sessionId),
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
      ptyKill(pane.id).catch(() => undefined);
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
    const clear = () => setAttention(false);
    window.addEventListener("focus", clear);
    return () => window.removeEventListener("focus", clear);
  }, [focused, visible]);

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
            {pane.sessionId ? `⟲ ${pane.sessionId.slice(0, 8)}` : t("pane.newSession")}
          </button>
        )}

        <span className="pane__spacer" />

        {agentMissing && (
          <span className="pane__warn" title={t("pane.notOnPath", { agent: pane.agent })}>
            !
          </span>
        )}
        {attention && (
          <span className="pane__attn" title={t("pane.attention")}>
            ✳
          </span>
        )}
        <span className={`pane__status pane__status--${status}`} title={status} />
        <button
          className="pane__action"
          onClick={onRestart}
          title={withKeys(t("pane.restart"), "pane.restart")}
        >
          ↻
        </button>
        <button
          className="pane__action"
          onClick={onSplit}
          title={withKeys(t("pane.split"), "pane.split")}
        >
          ⊞
        </button>
        <button
          className="pane__action pane__action--close"
          onClick={onClose}
          disabled={!canClose}
          title={canClose ? withKeys(t("pane.close"), "pane.close") : t("pane.closeLast")}
        >
          ✕
        </button>
      </header>

      <div
        className="pane__term"
        ref={hostRef}
        style={{ padding: settings.padding, background: theme.xterm.background }}
      />
    </section>
  );
}
