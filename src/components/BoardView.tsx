import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import { findAgentPane } from "../lib/agentPane";
import { elapsedLabel, shortTaskId } from "../lib/boardFormat";
import { useLocale, useT } from "../i18n";
import { useStore } from "../store";
import { AGENTS } from "../types";
import { Icon } from "./Icon";
import { BoardIcon } from "./BoardIcons";
import { BoardFilter, type FilterOption } from "./BoardFilter";
import { BoardAssignee, type AssigneePane } from "./BoardAssignee";
import { BoardPriority } from "./BoardPriority";
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

/**
 * Beside the terminals, the work under way matters most: what is in progress
 * and what waits for a review come first, the rest folds away below.
 */
const DOCK_ORDER: BoardColumn[] = ["doing", "review", "todo", "done"];

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
  /** null until the first read answers, so a slow read is never shown as an
      empty board. */
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [openTaskId, setOpenTaskId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [dragOver, setDragOver] = useState<BoardColumn | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [labelFilter, setLabelFilter] = useState(ANY);
  const [assigneeFilter, setAssigneeFilter] = useState(ANY);
  // Finished work, and work nobody started, are the least useful things to
  // keep in view next to the agents.
  const [folded, setFolded] = useState<BoardColumn[]>(() => (dock ? ["todo", "done"] : []));
  /** Dock only: the filter row is one click away rather than always there. */
  const [showFilters, setShowFilters] = useState(false);
  /** Asked for a first task from the empty board: show the columns anyway. */
  const [composing, setComposing] = useState(false);
  /** The column whose add field takes the focus once it is on screen. */
  const [focusAdd, setFocusAdd] = useState<BoardColumn | null>(null);
  const addRefs = useRef<Partial<Record<BoardColumn, HTMLInputElement | null>>>({});
  const t = useT();
  const locale = useLocale();
  const { state } = useStore();

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

  useEffect(() => {
    if (!focusAdd) return;
    const field = addRefs.current[focusAdd];
    field?.focus();
    field?.scrollIntoView({ block: "nearest" });
    setFocusAdd(null);
  }, [focusAdd]);

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

  /** Puts the cursor in a column's add field, unfolding it if need be. */
  function startAdding(column: BoardColumn) {
    setComposing(true);
    setFolded((current) => current.filter((c) => c !== column));
    setFocusAdd(column);
  }

  const all = useMemo(() => tasks ?? [], [tasks]);
  const byId = useMemo(() => new Map(all.map((task) => [task.id, task])), [all]);

  // The pane each assignee leads to, worked out once per board rather than
  // once per card: the same few agents hold most of the tasks.
  const paneFor = useMemo(() => {
    const cache = new Map<string, AssigneePane | null>();
    return (assignee: string): AssigneePane | null => {
      if (cache.has(assignee)) return cache.get(assignee) ?? null;
      const target = findAgentPane(assignee, state.workspaces, state.activeWorkspaceId);
      const pane = target
        ? state.workspaces
            .find((ws) => ws.id === target.workspaceId)
            ?.panes.find((p) => p.id === target.paneId)
        : undefined;
      const found = pane
        ? {
            id: pane.id,
            name: pane.name,
            agent: AGENTS.find((a) => a.id === pane.agent)?.label ?? pane.agent,
          }
        : null;
      cache.set(assignee, found);
      return found;
    };
  }, [state.workspaces, state.activeWorkspaceId]);

  // A filter whose value left the board stays listed, so the filter never
  // shows a value it has no option for.
  const labels = useMemo(
    () => sortedUnique([...all.flatMap((task) => task.labels), labelFilter].filter(Boolean)),
    [all, labelFilter],
  );
  const assignees = useMemo(
    () =>
      sortedUnique(
        [...all.map((task) => task.assignee ?? ""), assigneeFilter].filter(
          (name) => name && name !== NOBODY,
        ),
      ),
    [all, assigneeFilter],
  );

  const labelOptions: FilterOption[] = [
    { value: ANY, label: t("board.allLabels"), short: t("board.filterAll") },
    ...labels.map((label) => ({ value: label, label })),
  ];
  const assigneeOptions: FilterOption[] = [
    { value: ANY, label: t("board.allAssignees"), short: t("board.filterAll") },
    { value: NOBODY, label: t("board.unassigned") },
    ...assignees.map((name) => {
      const pane = paneFor(name);
      return { value: name, label: name, hint: pane && pane.name !== name ? pane.name : undefined };
    }),
  ];

  const filtering = labelFilter !== ANY || assigneeFilter !== ANY;
  const shown = useMemo(
    () =>
      all.filter(
        (task) =>
          (labelFilter === ANY || task.labels.includes(labelFilter)) &&
          (assigneeFilter === ANY ||
            (assigneeFilter === NOBODY ? !task.assignee : task.assignee === assigneeFilter)),
      ),
    [all, labelFilter, assigneeFilter],
  );

  const now = useMinuteClock(all.some((task) => task.column === "doing" && task.doing_since));

  const openTask = all.find((task) => task.id === openTaskId) ?? null;
  const loaded = tasks !== null;
  const empty = loaded && all.length === 0 && !composing;

  function clearFilters() {
    setLabelFilter(ANY);
    setAssigneeFilter(ANY);
  }

  function renderCard(task: Task) {
    const done = task.column === "done";
    const blockers = done ? [] : blockersOf(task, byId);
    const blockerNames = blockers.map((dep) =>
      typeof dep === "string" ? t("board.missingTask") : `#${shortTaskId(dep.id)} · ${dep.title}`,
    );
    const focusAgent = task.assignee ? (findAgent?.(task.assignee) ?? null) : null;
    const since = task.column === "doing" ? task.doing_since : null;
    const dependsDone = task.depends_on.length > 0 && blockers.length === 0;

    return (
      <article
        key={task.id}
        className={`card ${done ? "card--done" : ""} ${blockers.length ? "card--blocked" : ""} ${
          dragging === task.id ? "card--dragging" : ""
        }`}
        draggable
        tabIndex={0}
        aria-label={t("board.openTask", { title: task.title })}
        onDragStart={(e) => {
          e.dataTransfer.setData("text/plain", task.id);
          e.dataTransfer.effectAllowed = "move";
          setDragging(task.id);
        }}
        onDragEnd={() => setDragging(null)}
        onClick={() => setOpenTaskId(task.id)}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            setOpenTaskId(task.id);
          }
        }}
      >
        <div className="card__top">
          <span className="card__id" title={task.id}>
            #{shortTaskId(task.id)}
          </span>
          <BoardPriority priority={task.priority} />
        </div>

        <p className="card__title">{task.title}</p>

        {task.labels.length > 0 && (
          <div className="card__labels">
            {task.labels.map((label) => (
              <span key={label} className="chip">
                {label}
              </span>
            ))}
          </div>
        )}

        {blockers.length > 0 && (
          <p className="card__blocked" title={blockerNames.join("\n")}>
            <BoardIcon name="lock" size={11} />
            <span>{t("board.blockedBy", { tasks: blockerNames.join(", ") })}</span>
          </p>
        )}

        <footer className="card__foot">
          {task.assignee ? (
            <BoardAssignee
              assignee={task.assignee}
              pane={paneFor(task.assignee)}
              done={done}
              onFocus={focusAgent}
            />
          ) : (
            <span className="card__nobody">{t("board.nobody")}</span>
          )}
          <span className="card__meta">
            {since ? (
              <span
                className="card__stat"
                title={t("board.doingSince", { date: new Date(since).toLocaleString(locale) })}
              >
                <BoardIcon name="clock" size={11} />
                {elapsedLabel(now - since, t)}
              </span>
            ) : null}
            {dependsDone && (
              <span className="card__stat" title={t("board.dependsOn")}>
                <BoardIcon name="link" size={11} />
                {task.depends_on.length}
              </span>
            )}
            {task.comments.length > 0 && (
              <span
                className="card__stat"
                title={t("board.comments", { n: task.comments.length })}
              >
                <BoardIcon name="bubble" size={11} />
                {task.comments.length}
              </span>
            )}
          </span>
        </footer>
      </article>
    );
  }

  function renderColumn(column: BoardColumn) {
    const items = shown
      .filter((task) => task.column === column)
      .sort((a, b) => b.priority - a.priority || a.order - b.order);
    const isFolded = dock && folded.includes(column);
    const name = t(columnKey(column));

    return (
      <section
        key={column}
        className={`column column--${column} ${dragOver === column ? "column--over" : ""} ${
          isFolded ? "column--folded" : ""
        } ${items.length === 0 ? "column--empty" : ""}`}
        aria-label={name}
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
        <header className="column__head">
          {dock ? (
            <button
              type="button"
              className="column__toggle"
              aria-expanded={!isFolded}
              onClick={() =>
                setFolded((current) =>
                  current.includes(column)
                    ? current.filter((c) => c !== column)
                    : [...current, column],
                )
              }
            >
              <span className="column__caret" aria-hidden>
                <BoardIcon name="chevron" size={10} />
              </span>
              <span className="column__mark" aria-hidden />
              <span className="column__name">{name}</span>
              <span className="column__count">{items.length}</span>
            </button>
          ) : (
            <>
              <span className="column__mark" aria-hidden />
              <h3 className="column__name">{name}</h3>
              <span className="column__count">{items.length}</span>
            </>
          )}
          <button
            type="button"
            className="column__plus"
            title={t("board.addTo", { column: name })}
            aria-label={t("board.addTo", { column: name })}
            onClick={() => startAdding(column)}
          >
            <Icon name="plus" size={12} />
          </button>
        </header>

        {!isFolded && (
          <div className="column__list">
            {items.map(renderCard)}

            <input
              ref={(field) => {
                addRefs.current[column] = field;
              }}
              className="column__add"
              placeholder={t("board.add")}
              aria-label={t("board.addTo", { column: name })}
              value={draft[column] ?? ""}
              onChange={(e) => setDraft((prev) => ({ ...prev, [column]: e.target.value }))}
              onKeyDown={(e) => {
                if (e.key === "Enter") addTask(column);
                else if (e.key === "Escape" && draft[column]) {
                  e.stopPropagation();
                  setDraft((prev) => ({ ...prev, [column]: "" }));
                }
              }}
              onBlur={() => addTask(column)}
            />
          </div>
        )}
      </section>
    );
  }

  const filters = (
    <>
      <BoardFilter
        name={t("board.filterName.label")}
        title={t("board.filterLabel")}
        value={labelFilter}
        options={labelOptions}
        active={labelFilter !== ANY}
        onChange={setLabelFilter}
      />
      <BoardFilter
        name={t("board.filterName.agent")}
        title={t("board.filterAssignee")}
        value={assigneeFilter}
        options={assigneeOptions}
        active={assigneeFilter !== ANY}
        onChange={setAssigneeFilter}
      />
      {filtering && (
        <button
          type="button"
          className="board__clear"
          onClick={clearFilters}
          title={t("board.clearFilters")}
        >
          <Icon name="close" size={10} />
          {t("board.clearFiltersShort")}
        </button>
      )}
    </>
  );

  const count = filtering
    ? t("board.shown", { shown: shown.length, n: all.length })
    : t("board.tasks", { n: all.length });
  const active = shown.filter((task) => task.column === "doing" || task.column === "review");

  return (
    <div className={`board ${dock ? "board--dock" : ""}`}>
      {dock ? (
        <>
          <div className="board__bar">
            <h2 className="board__title">{t("board.title")}</h2>
            {loaded && (
              <span className="board__sub">
                {filtering ? count : t("board.dockActive", { n: active.length })}
              </span>
            )}
            <span className="board__spacer" />
            <button
              type="button"
              className={`board__tool ${filtering ? "is-active" : ""}`}
              aria-pressed={showFilters}
              title={t("board.filters")}
              aria-label={t("board.filters")}
              onClick={() => setShowFilters((on) => !on)}
            >
              <BoardIcon name="filter" size={13} />
            </button>
            <button
              type="button"
              className="board__tool"
              onClick={onExpand}
              title={t("board.dockExpand")}
              aria-label={t("board.dockExpand")}
            >
              <BoardIcon name="expand" size={12} />
            </button>
            <button
              type="button"
              className="board__tool"
              onClick={onClose}
              title={t("board.dockHide")}
              aria-label={t("board.dockHide")}
            >
              <Icon name="close" size={13} />
            </button>
          </div>
          {showFilters && (
            <div className="board__filterbar" role="group" aria-label={t("board.filters")}>
              {filters}
            </div>
          )}
        </>
      ) : (
        <div className="board__bar">
          <h2 className="board__count">{loaded ? count : t("board.title")}</h2>
          {/* Nothing to filter before the first read, or on an empty board. */}
          {(filtering || all.length > 0) && (
            <div className="board__filters" role="group" aria-label={t("board.filters")}>
              {filters}
            </div>
          )}
          <button
            type="button"
            className="board__mcp"
            onClick={onOpenMcp}
            title={t("board.mcp")}
          >
            <BoardIcon name="plug" size={12} />
            <span className="board__mcp-text">{t("board.hint")}</span>
          </button>
          <button type="button" className="btn board__add" onClick={() => startAdding("todo")}>
            <Icon name="plus" size={12} />
            {t("board.addTask")}
          </button>
        </div>
      )}

      {!loaded ? (
        <BoardSkeleton dock={dock} label={t("board.loading")} />
      ) : empty ? (
        <div className="board__empty">
          <span className="board__empty-icon" aria-hidden>
            <BoardIcon name="board" size={dock ? 18 : 22} />
          </span>
          <h3>{t("board.emptyTitle")}</h3>
          <p>{t("board.emptyBody")}</p>
          <div className="board__empty-actions">
            <button type="button" className="btn btn--primary" onClick={() => startAdding("todo")}>
              <Icon name="plus" size={12} />
              {t("board.emptyAdd")}
            </button>
            <button type="button" className="btn" onClick={onOpenMcp}>
              <BoardIcon name="plug" size={12} />
              {t("board.emptyMcp")}
            </button>
          </div>
        </div>
      ) : (
        <div className="board__columns">
          {(dock ? DOCK_ORDER : BOARD_COLUMNS).map(renderColumn)}
        </div>
      )}

      {openTask && (
        <TaskDialog
          cwd={cwd}
          task={openTask}
          allTasks={all}
          onClose={() => setOpenTaskId(null)}
          onChanged={refresh}
          assigneePane={openTask.assignee ? paneFor(openTask.assignee) : null}
          onFocusAgent={openTask.assignee ? (findAgent?.(openTask.assignee) ?? null) : null}
        />
      )}
    </div>
  );
}

/**
 * The columns' outline while the first read is out. It fades in only after a
 * beat, so the usual instant read never flashes it.
 */
function BoardSkeleton({ dock, label }: { dock: boolean; label: string }) {
  const shape = dock ? [2, 1] : [3, 2, 1, 2];
  return (
    <div className="board__columns board__skeleton" role="status" aria-label={label}>
      {shape.map((cards, column) => (
        <div key={column} className="column">
          <div className="column__head">
            <span className="skeleton skeleton--label" />
          </div>
          <div className="column__list">
            {Array.from({ length: cards }, (_, card) => (
              <div key={card} className="skeleton skeleton--card" />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
