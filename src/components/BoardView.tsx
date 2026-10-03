import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  BOARD_COLUMNS,
  blockersOf,
  boardCreate,
  boardLoad,
  boardUpdate,
  columnKey,
  type BoardColumn,
  type Task,
} from "../lib/board";
import { useLocale, useT, type Translator } from "../i18n";
import { TaskDialog } from "./TaskDialog";

interface Props {
  cwd: string;
  onOpenMcp: () => void;
  /** A way to bring an assignee's pane forward, or null when no pane is
      clearly theirs. */
  findAgent?: (assignee: string) => (() => void) | null;
  /**
   * `dock` is the narrow panel beside the terminals: columns stack instead of
   * sitting side by side, and each one folds away.
   */
  variant?: "full" | "dock";
  /** Dock only: hide the panel. */
  onClose?: () => void;
  /** Dock only: trade the panel for the full board view. */
  onExpand?: () => void;
}

/** Select values that cannot collide with a label or an agent name. */
const ANY = "";
const NOBODY = "\u0000";

/** How long a card has been in `doing`, at a glance rather than to the second. */
function elapsed(ms: number, t: Translator): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return t("board.elapsedNow");
  if (minutes < 60) return t("board.elapsedMinutes", { m: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return t("board.elapsedHours", { h: hours, m: String(minutes % 60).padStart(2, "0") });
  }
  return t("board.elapsedDays", { d: Math.floor(hours / 24), h: hours % 24 });
}

/** Ticks once a minute, and only while something on the board has a clock. */
function useMinuteClock(running: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [running]);
  return now;
}

const sortedUnique = (values: Iterable<string>) =>
  [...new Set(values)].sort((a, b) => a.localeCompare(b));

export function BoardView({
  cwd,
  onOpenMcp,
  findAgent,
  variant = "full",
  onClose,
  onExpand,
}: Props) {
  const dock = variant === "dock";
  const [tasks, setTasks] = useState<Task[]>([]);
  const [openTaskId, setOpenTaskId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [dragOver, setDragOver] = useState<BoardColumn | null>(null);
  const [labelFilter, setLabelFilter] = useState(ANY);
  const [assigneeFilter, setAssigneeFilter] = useState(ANY);
  // Finished work is the least useful thing to keep in view next to the agents.
  const [folded, setFolded] = useState<BoardColumn[]>(() => (dock ? ["done"] : []));
  const t = useT();
  const locale = useLocale();

  const refresh = useCallback(() => {
    boardLoad(cwd)
      .then((board) => setTasks(board.tasks ?? []))
      .catch(() => setTasks([]));
  }, [cwd]);

  useEffect(refresh, [refresh]);

  // Agents mutate the board through the HTTP API; the backend emits this so
  // the window reflects their work without polling. The payload names the
  // caller's cwd, which is NOT compared against ours: worktree sharing makes
  // several paths alias one board, so an agent writing from a linked worktree
  // carries a path this view has never heard of. Only the active workspace
  // mounts a BoardView and a reload is one small file read, so refreshing on
  // every change is cheaper than resolving path identity here.
  useEffect(() => {
    const unlisten = listen<string>("board:changed", () => refresh());
    return () => {
      unlisten.then((off) => off()).catch(() => undefined);
    };
  }, [refresh]);

  async function addTask(column: BoardColumn) {
    const title = (draft[column] ?? "").trim();
    if (!title) return;
    setDraft((prev) => ({ ...prev, [column]: "" }));
    await boardCreate(cwd, { title, column }).catch(() => undefined);
    refresh();
  }

  async function moveTask(id: string, column: BoardColumn) {
    await boardUpdate(cwd, id, { column }).catch(() => undefined);
    refresh();
  }

  const byId = useMemo(() => new Map(tasks.map((task) => [task.id, task])), [tasks]);

  // A filter whose value left the board stays listed, so the select never
  // shows a value it has no option for.
  const labels = useMemo(
    () => sortedUnique([...tasks.flatMap((task) => task.labels), labelFilter].filter(Boolean)),
    [tasks, labelFilter],
  );
  const assignees = useMemo(
    () =>
      sortedUnique(
        [...tasks.map((task) => task.assignee ?? ""), assigneeFilter].filter(
          (name) => name && name !== NOBODY,
        ),
      ),
    [tasks, assigneeFilter],
  );

  const filtering = labelFilter !== ANY || assigneeFilter !== ANY;
  const shown = useMemo(
    () =>
      tasks.filter(
        (task) =>
          (labelFilter === ANY || task.labels.includes(labelFilter)) &&
          (assigneeFilter === ANY ||
            (assigneeFilter === NOBODY ? !task.assignee : task.assignee === assigneeFilter)),
      ),
    [tasks, labelFilter, assigneeFilter],
  );

  const now = useMinuteClock(tasks.some((task) => task.column === "doing" && task.doing_since));

  const openTask = tasks.find((t) => t.id === openTaskId) ?? null;

  function renderCard(task: Task) {
    const blockers = task.column === "done" ? [] : blockersOf(task, byId);
    const blockerNames = blockers.map((dep) =>
      typeof dep === "string" ? t("board.missingTask") : dep.title,
    );
    const focusAgent = task.assignee ? (findAgent?.(task.assignee) ?? null) : null;
    const since = task.column === "doing" ? task.doing_since : null;

    return (
      <article
        key={task.id}
        className={`card ${task.assignee ? "card--claimed" : ""} ${blockers.length ? "card--blocked" : ""}`}
        draggable
        onDragStart={(e) => e.dataTransfer.setData("text/plain", task.id)}
        onClick={() => setOpenTaskId(task.id)}
      >
        <p className="card__title">{task.title}</p>

        {blockers.length > 0 && (
          <p className="card__blocked" title={blockerNames.join("\n")}>
            {t("board.blockedBy", { tasks: blockerNames.join(", ") })}
          </p>
        )}

        <div className="card__meta">
          {task.priority > 1 && <span className="chip chip--priority">P{task.priority}</span>}
          {task.assignee &&
            (focusAgent ? (
              <button
                type="button"
                className="chip chip--assignee chip--link"
                title={t("board.focusAgent", { name: task.assignee })}
                onClick={(e) => {
                  e.stopPropagation();
                  focusAgent();
                }}
              >
                {task.assignee}
              </button>
            ) : (
              <span className="chip chip--assignee" title={t("board.assignee")}>
                {task.assignee}
              </span>
            ))}
          {since ? (
            <span
              className="chip"
              title={t("board.doingSince", { date: new Date(since).toLocaleString(locale) })}
            >
              ⏱ {elapsed(Math.max(0, now - since), t)}
            </span>
          ) : null}
          {task.depends_on.length > 0 && blockers.length === 0 && (
            <span className="chip" title={t("board.dependsOn")}>
              ⛓ {task.depends_on.length} ✓
            </span>
          )}
          {task.comments.length > 0 && (
            <span className="chip" title={t("board.comments", { n: task.comments.length })}>
              💬 {task.comments.length}
            </span>
          )}
          {task.labels.map((label) => (
            <span key={label} className="chip">
              {label}
            </span>
          ))}
        </div>
      </article>
    );
  }

  return (
    <div className={`board ${dock ? "board--dock" : ""}`}>
      <div className="board__bar">
        <span className="board__count">
          {filtering
            ? t("board.shown", { shown: shown.length, n: tasks.length })
            : t("board.tasks", { n: tasks.length })}
        </span>
        {dock ? (
          <span className="pane__spacer" />
        ) : (
          <span className="board__hint">{t("board.hint")}</span>
        )}

        <div className="board__filters" role="group" aria-label={t("board.filters")}>
          <select
            className={`board__filter ${labelFilter !== ANY ? "is-active" : ""}`}
            value={labelFilter}
            onChange={(e) => setLabelFilter(e.target.value)}
            aria-label={t("board.filterLabel")}
          >
            <option value={ANY}>{t("board.allLabels")}</option>
            {labels.map((label) => (
              <option key={label} value={label}>
                {label}
              </option>
            ))}
          </select>
          <select
            className={`board__filter ${assigneeFilter !== ANY ? "is-active" : ""}`}
            value={assigneeFilter}
            onChange={(e) => setAssigneeFilter(e.target.value)}
            aria-label={t("board.filterAssignee")}
          >
            <option value={ANY}>{t("board.allAssignees")}</option>
            <option value={NOBODY}>{t("board.unassigned")}</option>
            {assignees.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
          {filtering && (
            <button
              type="button"
              className="btn btn--ghost"
              onClick={() => {
                setLabelFilter(ANY);
                setAssigneeFilter(ANY);
              }}
            >
              {t("board.clearFilters")}
            </button>
          )}
        </div>

        {dock ? (
          <div className="board__dock-actions">
            <button
              type="button"
              className="icon-btn"
              onClick={onExpand}
              title={t("board.dockExpand")}
              aria-label={t("board.dockExpand")}
            >
              ⤢
            </button>
            <button
              type="button"
              className="icon-btn"
              onClick={onClose}
              title={t("board.dockHide")}
              aria-label={t("board.dockHide")}
            >
              ×
            </button>
          </div>
        ) : (
          <button className="btn btn--ghost" onClick={onOpenMcp}>
            {t("board.mcp")}
          </button>
        )}
      </div>

      <div className="board__columns">
        {BOARD_COLUMNS.map((column) => {
          const items = shown
            .filter((task) => task.column === column)
            .sort((a, b) => b.priority - a.priority || a.order - b.order);
          const isFolded = dock && folded.includes(column);

          return (
            <section
              key={column}
              className={`column ${dragOver === column ? "column--over" : ""} ${isFolded ? "column--folded" : ""}`}
              onDragOver={(e) => {
                e.preventDefault();
                setDragOver(column);
              }}
              onDragLeave={() => setDragOver((c) => (c === column ? null : c))}
              onDrop={(e) => {
                e.preventDefault();
                setDragOver(null);
                const id = e.dataTransfer.getData("text/plain");
                if (id) moveTask(id, column);
              }}
            >
              {dock ? (
                <button
                  type="button"
                  className="column__head column__head--toggle"
                  aria-expanded={!isFolded}
                  onClick={() =>
                    setFolded((current) =>
                      current.includes(column)
                        ? current.filter((c) => c !== column)
                        : [...current, column],
                    )
                  }
                >
                  <span className="column__name">
                    <span className="column__caret" aria-hidden>
                      {isFolded ? "▸" : "▾"}
                    </span>
                    {t(columnKey(column))}
                  </span>
                  <span className="column__count">{items.length}</span>
                </button>
              ) : (
                <header className="column__head">
                  <span className="column__name">{t(columnKey(column))}</span>
                  <span className="column__count">{items.length}</span>
                </header>
              )}

              {!isFolded && (
                <div className="column__list">
                  {items.map(renderCard)}

                  <input
                    className="column__add"
                    placeholder={t("board.add")}
                    value={draft[column] ?? ""}
                    onChange={(e) => setDraft((prev) => ({ ...prev, [column]: e.target.value }))}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") addTask(column);
                    }}
                    onBlur={() => addTask(column)}
                  />
                </div>
              )}
            </section>
          );
        })}
      </div>

      {openTask && (
        <TaskDialog
          cwd={cwd}
          task={openTask}
          allTasks={tasks}
          onClose={() => setOpenTaskId(null)}
          onChanged={refresh}
        />
      )}
    </div>
  );
}
