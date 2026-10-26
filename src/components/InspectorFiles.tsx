import { memo, useEffect, useMemo, useState } from "react";
import { Icon } from "./Icon";
import { InspectorSection } from "./InspectorSection";
import { useT, type Translator } from "../i18n";
import { gitChanges, type Changes } from "../lib/git";
import { openDiff } from "../lib/terminalRegistry";
import {
  diffSegments,
  diffTotals,
  editedRows,
  folderRows,
  type FileRow,
} from "../lib/inspectorFiles";

/** Below this, a refresh asked for twice in a row is asked for once. */
const DEBOUNCE_MS = 400;
/** The slowest refresh that still catches what an agent did by hand. */
const POLL_MS = 15_000;
/** Edited files listed before "+N more": the diff view has the rest. */
const EDITED_SHOWN = 10;
const FOLDER_SHOWN = 8;

/**
 * git's view of the pane's working tree, kept fresh while it is on screen:
 * fetched shortly after the pane, its edits or its turn change, then every
 * POLL_MS while the window is visible. A folder git cannot read — not a
 * repository, or the browser demo — is asked again only when something
 * changes, never on the timer: polling a failure would only spawn processes.
 */
function useGitChanges(folder: string, enabled: boolean, trigger: unknown): Changes | null {
  const [state, setState] = useState<{ folder: string; changes: Changes | null }>({
    folder,
    changes: null,
  });

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    let timer: number | null = null;

    const schedule = (delay: number) => {
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(tick, delay);
    };

    // The next look is booked once this one answered, and only if it did:
    // reads never overlap, and a failure ends the polling.
    function tick() {
      timer = null;
      if (document.visibilityState !== "visible") return;
      gitChanges(folder, null)
        .then((changes) => {
          if (cancelled) return;
          setState({ folder, changes });
          schedule(POLL_MS);
        })
        .catch(() => {
          if (!cancelled) setState({ folder, changes: null });
        });
    }

    // Coming back to the window is a change worth one look, whatever the
    // last attempt gave.
    const onVisible = () => {
      if (document.visibilityState === "visible") schedule(DEBOUNCE_MS);
    };

    schedule(DEBOUNCE_MS);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [folder, enabled, trigger]);

  // What was read for another pane's folder is not this pane's.
  return state.folder === folder ? state.changes : null;
}

/** What the row's tooltip says about the file's state; null when nothing is known. */
function statusName(t: Translator, row: FileRow): string | null {
  switch (row.status) {
    case "M":
    case "A":
    case "D":
    case "R":
    case "C":
    case "U":
      return t(`inspector.status.${row.status}`);
    default:
      return row.settled ? t("inspector.fileGone") : null;
  }
}

function DiffBar({ additions, deletions }: { additions: number; deletions: number }) {
  return (
    <span className="insp-diffbar" aria-hidden="true">
      {diffSegments(additions, deletions).map((segment, i) => (
        <i key={i} className={`insp-diffbar__seg insp-diffbar__seg--${segment}`} />
      ))}
    </span>
  );
}

function FileLine({ row, onOpen }: { row: FileRow; onOpen: () => void }) {
  const t = useT();
  const counted = row.additions !== null && row.deletions !== null;
  const status = statusName(t, row);
  return (
    // Every row opens the same diff view, so the rows answer the mouse and
    // the section's "View diff" button is the keyboard's way in: fifty tab
    // stops that all do one thing would be in the way.
    <li
      className={`insp-file ${row.settled ? "insp-file--settled" : ""}`}
      title={status ? t("inspector.fileRow", { path: row.path, status }) : row.path}
      onClick={onOpen}
    >
      <span className={`insp-file__status insp-file__status--${row.status ?? "none"}`}>
        {row.status ?? (row.settled ? "·" : "")}
      </span>
      <span className="insp-file__path">
        {row.dir && <span className="insp-file__dir">{`\u200e${row.dir}\u200e`}</span>}
        <span className="insp-file__name">{row.name}</span>
      </span>
      {counted ? (
        <>
          <span className="insp-file__counts insp-num">
            <span className={`insp-add ${row.additions ? "" : "is-zero"}`}>+{row.additions}</span>
            <span className={`insp-del ${row.deletions ? "" : "is-zero"}`}>−{row.deletions}</span>
          </span>
          <DiffBar additions={row.additions ?? 0} deletions={row.deletions ?? 0} />
        </>
      ) : row.binary ? (
        <span className="insp-file__counts insp-file__counts--faint">
          {t("inspector.fileBinary")}
        </span>
      ) : null}
    </li>
  );
}

export const FilesSection = memo(function FilesSection({
  workspaceId,
  paneId,
  folder,
  edited,
  turnStartedAt,
  collapsed,
  onToggle,
}: {
  workspaceId: string;
  paneId: string;
  folder: string;
  edited: string[];
  /** A turn ending is when files settle: worth a fresh look. */
  turnStartedAt: number | null;
  collapsed: boolean;
  onToggle: (id: string) => void;
}) {
  const t = useT();
  const trigger = useMemo(() => [paneId, edited, turnStartedAt], [paneId, edited, turnStartedAt]);
  // Folded, the section asks git nothing: it shows nothing that needs it.
  const changes = useGitChanges(folder, !collapsed, trigger);

  const view = useMemo(() => {
    if (edited.length) {
      const rows = editedRows(edited, changes, folder);
      return {
        mode: "edited" as const,
        rows: rows.slice(0, EDITED_SHOWN),
        more: Math.max(0, rows.length - EDITED_SHOWN),
        totals: diffTotals(rows),
        counted: rows.some((row) => row.additions !== null),
        count: rows.length,
      };
    }
    if (changes) {
      const { rows, more } = folderRows(changes, folder, FOLDER_SHOWN);
      return { mode: "folder" as const, rows, more, totals: null, counted: false, count: 0 };
    }
    return { mode: "none" as const, rows: [], more: 0, totals: null, counted: false, count: 0 };
  }, [edited, changes, folder]);

  const open = () => openDiff(workspaceId, paneId);

  const meta =
    view.mode !== "edited" ? null : view.counted && view.totals ? (
      <span
        className="insp-num"
        title={t("inspector.filesTotals", {
          add: view.totals.additions,
          del: view.totals.deletions,
        })}
      >
        <span className={`insp-add ${view.totals.additions ? "" : "is-zero"}`}>
          +{view.totals.additions}
        </span>{" "}
        <span className={`insp-del ${view.totals.deletions ? "" : "is-zero"}`}>
          −{view.totals.deletions}
        </span>
      </span>
    ) : (
      <span className="insp-num">{view.count}</span>
    );

  return (
    <InspectorSection
      id="files"
      title={t("inspector.section.files")}
      meta={meta}
      action={
        view.rows.length > 0 ? (
          <button
            type="button"
            className="insp-link"
            aria-label={t("inspector.filesDiff")}
            title={t("inspector.filesDiffHint")}
            onClick={open}
          >
            <Icon name="diff" size={11} />
            <span className="insp-link__label--optional">{t("inspector.filesDiff")}</span>
          </button>
        ) : null
      }
      collapsed={collapsed}
      onToggle={onToggle}
    >
      {view.mode === "folder" && view.rows.length > 0 && (
        <p className="insp-subhead">{t("inspector.filesFolder")}</p>
      )}
      {view.rows.length > 0 ? (
        <ul className="insp-files">
          {view.rows.map((row) => (
            <FileLine key={row.key} row={row} onOpen={open} />
          ))}
        </ul>
      ) : (
        <p className="insp-empty">
          {t(view.mode === "folder" ? "inspector.filesClean" : "inspector.filesEmpty")}
        </p>
      )}
      {view.more > 0 && (
        <p className="insp-more">{t("inspector.filesMore", { n: view.more })}</p>
      )}
    </InspectorSection>
  );
});
