import { useEffect, useId, useMemo, useState } from "react";
import { Backdrop, ModalHead } from "./Backdrop";
import { Icon } from "./Icon";
import { useT, type TranslateKey } from "../i18n";
import { useStore } from "../store";
import { getPaneActivity } from "../lib/agentState";
import { getPaneInsight } from "../lib/paneInsight";
import {
  INHERIT,
  isLoaded,
  mcpScopeClear,
  mcpServers,
  needsRestart,
  presetAll,
  presetHangarOnly,
  stdioCount,
  toggleServer,
  workspaceMcp,
  type McpDiscovery,
  type McpServer,
} from "../lib/mcpScope";
import type { Workspace, WorkspaceMcp } from "../types";

interface Props {
  workspace: Workspace;
  /** Restarts these panes under fresh ids; each resumes its session. */
  onRestart: (paneIds: string[]) => void;
  onClose: () => void;
}

const STATUS_KEYS: Record<Exclude<McpServer["status"], "active">, TranslateKey> = {
  unapproved: "mcpScope.status.unapproved",
  rejected: "mcpScope.status.rejected",
  disabled: "mcpScope.status.disabled",
};

const STATUS_HINTS: Record<Exclude<McpServer["status"], "active">, TranslateKey> = {
  unapproved: "mcpScope.statusHint.unapproved",
  rejected: "mcpScope.statusHint.rejected",
  disabled: "mcpScope.statusHint.disabled",
};

const sameNames = (a: string[], b: string[]) =>
  a.length === b.length && a.every((name) => b.includes(name));

/**
 * « Serveurs MCP »: which MCP servers a workspace's Claude Code panes start
 * with (lib/mcpScope).
 *
 * Changes are saved as they are made, and reach the panes on their next
 * launch: restarting a running agent interrupts it, so that stays a button
 * of its own — and it only restarts the panes that run with something else.
 */
export function McpServersPanel({ workspace, onRestart, onClose }: Props) {
  const t = useT();
  const { updateWorkspace } = useStore();
  const [discovery, setDiscovery] = useState<McpDiscovery | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const titleId = useId();
  const listTitleId = useId();

  const mcp = workspaceMcp(workspace);
  const cwd = workspace.cwd;

  useEffect(() => {
    let live = true;
    setDiscovery(null);
    setFailure(null);
    mcpServers(cwd)
      .then((found) => live && setDiscovery(found))
      .catch((err) => live && setFailure(String(err)));
    return () => {
      live = false;
    };
  }, [cwd]);

  const servers = useMemo(() => discovery?.servers ?? [], [discovery]);
  const hangar = servers.find((server) => server.hangar) ?? null;
  const others = servers.filter((server) => !server.hangar);

  const save = (next: WorkspaceMcp) => {
    updateWorkspace(workspace.id, { mcp: next.mode === "inherit" ? INHERIT : next });
    // Back to Claude Code's own configuration, the copies Hangar made of the
    // servers' entries have nothing left to serve.
    if (next.mode === "inherit") mcpScopeClear(workspace.id).catch(() => undefined);
  };

  const all = presetAll(servers);
  const isAll = mcp.mode === "custom" && sameNames(mcp.enabled, all.enabled);
  const isNone = mcp.mode === "custom" && mcp.enabled.length === 0;

  const claudePanes = workspace.panes.filter((pane) => pane.agent === "claude");
  const stale = discovery
    ? claudePanes.filter((pane) => needsRestart(getPaneInsight(pane.id).mcp, mcp, servers))
    : [];
  const busy = stale.filter((pane) => getPaneActivity(pane.id)?.activity === "working").length;

  const perSession = stdioCount(servers, mcp);
  const sessions = claudePanes.length;
  const total = perSession * sessions;
  /** What Claude Code's own configuration would start, to compare with. */
  const inherited = stdioCount(servers, INHERIT) * sessions;

  function row(server: McpServer | null) {
    const isHangar = server === null || server.hangar;
    const name = server?.name ?? "hangar";
    const loaded = server ? isLoaded(server, mcp) : mcp.mode === "custom";
    const id = `mcps-${name}`;
    const status = server && server.status !== "active" ? server.status : null;
    const scopeHint = server
      ? t(`mcpScope.scopeHint.${server.scope}` as TranslateKey, { path: server.source })
      : undefined;
    return (
      <li
        key={name}
        className={["mcps__row", loaded ? "" : "is-off", isHangar ? "mcps__row--hangar" : ""].join(
          " ",
        )}
      >
        <label className="mcps__main" htmlFor={id}>
          <span className="mcps__name">
            <span className="mcps__name-text">{name}</span>
            {isHangar && <em className="mcps__badge mcps__badge--app">{t("mcpScope.hangar")}</em>}
            {status && (
              <em className="mcps__badge" title={t(STATUS_HINTS[status])}>
                {t(STATUS_KEYS[status])}
              </em>
            )}
          </span>
          <span className="mcps__meta">
            {server ? (
              <>
                <span className="mcps__scope" title={scopeHint}>
                  {t(`mcpScope.scope.${server.scope}` as TranslateKey)}
                </span>
                <span className="mcps__transport">{server.transport}</span>
                <span className="mcps__summary" title={server.summary}>
                  {server.summary}
                </span>
              </>
            ) : (
              <span className="mcps__summary mcps__summary--prose">
                {t("mcpScope.hangarMissing")}
              </span>
            )}
          </span>
        </label>
        <span className="switch" title={isHangar ? t("mcpScope.hangarHint") : undefined}>
          <input
            id={id}
            type="checkbox"
            role="switch"
            checked={loaded}
            // Hangar's own server is not the user's to drop: the panes reach
            // the board, each other and the memory through it.
            disabled={isHangar}
            aria-label={t("mcpScope.toggle", { name })}
            onChange={(event) =>
              server && save(toggleServer(mcp, servers, server.name, event.target.checked))
            }
          />
          <span className="switch__track" aria-hidden="true">
            <span className="switch__knob" />
          </span>
        </span>
      </li>
    );
  }

  return (
    <Backdrop onClose={onClose}>
      <div
        className="modal modal--wide modal--mcps"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <ModalHead
          id={titleId}
          title={t("mcpScope.title")}
          sub={`${workspace.name} · ${cwd}`}
          onClose={onClose}
        />

        <div className="modal__body mcps" data-modal-body>
          <p className="modal__intro">{t("mcpScope.intro")}</p>

          <div className="mcps__mode">
            <div className="seg" role="group" aria-label={t("mcpScope.modeLabel")}>
              <button
                type="button"
                className={`seg__btn ${mcp.mode === "inherit" ? "is-active" : ""}`}
                aria-pressed={mcp.mode === "inherit"}
                onClick={() => save(INHERIT)}
              >
                {t("mcpScope.inherit")}
              </button>
              <button
                type="button"
                className={`seg__btn ${mcp.mode === "custom" ? "is-active" : ""}`}
                aria-pressed={mcp.mode === "custom"}
                // Starts from what Claude Code loads, so switching changes
                // nothing until a server is switched off.
                onClick={() => mcp.mode === "inherit" && save(presetAll(servers))}
                disabled={!discovery}
              >
                {t("mcpScope.custom")}
              </button>
            </div>
            <p className="mcps__mode-hint" role="status">
              {t(mcp.mode === "inherit" ? "mcpScope.inheritHint" : "mcpScope.customHint")}
            </p>
          </div>

          <section className="mcps__group" aria-labelledby={listTitleId}>
            <header className="mcps__group-head">
              <h3 className="mcps__group-title" id={listTitleId}>
                {t("mcpScope.listTitle")}
              </h3>
              <div className="mcps__presets" role="group" aria-label={t("mcpScope.presets")}>
                <button
                  type="button"
                  className="btn btn--tiny mcps__preset"
                  aria-pressed={isAll}
                  title={t("mcpScope.allHint")}
                  disabled={!discovery}
                  onClick={() => save(all)}
                >
                  {t("mcpScope.all")}
                </button>
                <button
                  type="button"
                  className="btn btn--tiny mcps__preset"
                  aria-pressed={isNone}
                  title={t("mcpScope.noneHint")}
                  disabled={!discovery}
                  onClick={() => save(presetHangarOnly())}
                >
                  {t("mcpScope.none")}
                </button>
              </div>
            </header>

            {failure ? (
              <p className="mcps__alert" role="alert">
                <Icon name="alert" size={12} />
                {t("mcpScope.loadFailed", { error: failure })}
              </p>
            ) : !discovery ? (
              <p className="modal__empty" role="status">
                {t("mcpScope.loading")}
              </p>
            ) : (
              <ul className="mcps__list">
                {row(hangar)}
                {others.map(row)}
                {others.length === 0 && <li className="mcps__empty">{t("mcpScope.empty")}</li>}
              </ul>
            )}

            {/* --strict-mcp-config drops every source but the file, and the
                list above only covers the three the file is built from. */}
            {discovery && mcp.mode === "custom" && (
              <p className="mcps__note">
                <Icon name="info" size={12} />
                {t("mcpScope.strictNote")}
              </p>
            )}

            {discovery?.unreadable.map((path) => (
              <p key={path} className="mcps__alert" role="alert">
                <Icon name="alert" size={12} />
                {t("mcpScope.unreadable", { path })}
              </p>
            ))}
          </section>

          {discovery && (
            <ul className="mcps__stats" aria-label={t("mcpScope.statsLabel")}>
              <li className="mcps__stat">
                <span className="mcps__stat-n">{perSession}</span>
                <span className="mcps__stat-label">
                  {t("mcpScope.statPerSession", { n: perSession })}
                </span>
              </li>
              <li className="mcps__stat">
                <span className="mcps__stat-n">{sessions}</span>
                <span className="mcps__stat-label">
                  {t("mcpScope.statSessions", { n: sessions })}
                </span>
              </li>
              <li className="mcps__stat" title={t("mcpScope.statProcessesHint")}>
                <span className="mcps__stat-figures">
                  <span className="mcps__stat-n">{total}</span>
                  {mcp.mode === "custom" && inherited !== total && (
                    <span
                      className={`mcps__stat-was ${inherited > total ? "mcps__stat-was--less" : ""}`}
                      title={t("mcpScope.statWasHint")}
                    >
                      {t("mcpScope.statWas", { n: inherited })}
                    </span>
                  )}
                </span>
                <span className="mcps__stat-label">
                  {t("mcpScope.statProcesses", { n: total })}
                </span>
              </li>
            </ul>
          )}
        </div>

        <footer className="modal__foot">
          <p className="modal__note">
            {discovery && claudePanes.length > 0 && stale.length === 0
              ? t("mcpScope.upToDate")
              : t("mcpScope.applies")}
            {busy > 0 && <span className="mcps__busy"> {t("mcpScope.busy", { n: busy })}</span>}
          </p>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            {t("mcpScope.close")}
          </button>
          <button
            type="button"
            className="btn btn--primary"
            title={t("mcpScope.restartHint")}
            disabled={stale.length === 0}
            onClick={() => {
              onRestart(stale.map((pane) => pane.id));
              onClose();
            }}
          >
            <Icon name="restart" size={12} />
            {stale.length
              ? t("mcpScope.restart", { n: stale.length })
              : t("mcpScope.restartNone")}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
