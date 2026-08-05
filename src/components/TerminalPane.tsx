import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";

import { ptyKill, ptyResize, ptySpawn, ptyWrite } from "../lib/ipc";
import { subscribePty } from "../lib/ptyBus";
import {
  claim,
  isResumable,
  knownSessionIds,
  launchCommand,
  sleep,
  watchForSession,
} from "../lib/agents";
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
  settings: Settings;
  theme: TerminalTheme;
  focused: boolean;
  index: number;
  availableAgents: string[];
  shells: ShellInfo[];
  /** Already resolved through the pane → workspace → global chain. */
  shell: ShellInfo | null;
  onFocus: () => void;
  onAgentChange: (agent: AgentId) => void;
  onShellChange: (shellId: string | null) => void;
  onSessionCaptured: (sessionId: string) => void;
  onRestart: () => void;
  onOpenSessions: () => void;
  /** Grid placement supplied by PaneGrid. */
  style?: React.CSSProperties;
}

export function TerminalPane({
  pane,
  cwd,
  settings,
  theme,
  focused,
  index,
  availableAgents,
  shells,
  shell,
  onFocus,
  onAgentChange,
  onShellChange,
  onSessionCaptured,
  onRestart,
  onOpenSessions,
  style,
}: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [status, setStatus] = useState<Status>("starting");

  // Latest settings/theme without forcing the terminal to be rebuilt.
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const shellRef = useRef(shell);
  shellRef.current = shell;

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

    termRef.current = term;
    fitRef.current = fit;

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

    (async () => {
      unsubscribe = await subscribePty(
        pane.id,
        (data) => term.write(data),
        () => !disposed && setStatus("exited"),
      );
      if (disposed) return;

      // Snapshot before launching, so a brand new transcript stands out.
      const known = isResumable(pane.agent)
        ? await knownSessionIds(pane.agent, paneCwd)
        : new Set<string>();
      if (disposed) return;

      try {
        await ptySpawn({
          id: pane.id,
          cwd: paneCwd,
          shell: shellRef.current,
          cols: term.cols,
          rows: term.rows,
        });
      } catch (err) {
        term.writeln(`\r\n\x1b[31mFailed to start shell: ${String(err)}\x1b[0m`);
        setStatus("exited");
        return;
      }
      if (disposed) return;
      setStatus("running");

      if (pane.agent === "shell") return;

      // Let the shell profile settle before typing into it.
      await sleep(settingsRef.current.launchDelayMs, controller.signal);
      if (disposed || controller.signal.aborted) return;

      const resumeId = settingsRef.current.autoResume ? pane.sessionId : null;
      const command = launchCommand(pane.agent, resumeId);
      if (command) {
        if (resumeId) claim(resumeId);
        await ptyWrite(pane.id, `${command}\r`).catch(() => undefined);
      }

      // Only hunt for an id when we did not already resume one.
      if (isResumable(pane.agent) && !resumeId) {
        watchForSession({
          agent: pane.agent,
          cwd: paneCwd,
          known,
          signal: controller.signal,
          onFound: (sessionId) => !disposed && onSessionCaptured(sessionId),
        });
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

  useEffect(() => {
    if (focused) termRef.current?.focus();
  }, [focused]);

  return (
    <section
      className={`pane ${focused ? "pane--focused" : ""}`}
      style={style}
      onMouseDown={onFocus}
      aria-label={`Panneau ${index + 1}`}
    >
      <header className="pane__bar">
        <span className="pane__index">{index + 1}</span>

        <select
          className="pane__agent"
          value={pane.agent}
          onChange={(e) => onAgentChange(e.target.value as AgentId)}
          title="Agent lancé dans ce panneau"
        >
          {AGENTS.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.label}
              {agent.id !== "shell" && !availableAgents.includes(agent.id) ? " (absent)" : ""}
            </option>
          ))}
        </select>

        <select
          className="pane__agent pane__agent--shell"
          value={pane.shellId ?? ""}
          onChange={(e) => onShellChange(e.target.value || null)}
          title={shell ? `Shell : ${shell.program}` : "Shell du panneau"}
        >
          <option value="">{shell ? shell.label : "Shell"} (hérité)</option>
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
                ? `Session reprise : ${pane.sessionId}`
                : "Aucune session mémorisée — cliquer pour en choisir une"
            }
          >
            {pane.sessionId ? `⟲ ${pane.sessionId.slice(0, 8)}` : "⟲ nouvelle"}
          </button>
        )}

        <span className="pane__spacer" />

        {agentMissing && (
          <span className="pane__warn" title={`${pane.agent} introuvable dans le PATH`}>
            !
          </span>
        )}
        <span className={`pane__status pane__status--${status}`} title={status} />
        <button className="pane__action" onClick={onRestart} title="Relancer le panneau">
          ↻
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
