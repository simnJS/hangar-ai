import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  BOARD_COLUMNS,
  COLUMN_LABELS,
  boardCreate,
  boardLoad,
  boardUpdate,
  type BoardColumn,
  type Task,
} from "../lib/board";
import { TaskDialog } from "./TaskDialog";

interface Props {
  cwd: string;
  onOpenMcp: () => void;
}

export function BoardView({ cwd, onOpenMcp }: Props) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [openTaskId, setOpenTaskId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [dragOver, setDragOver] = useState<BoardColumn | null>(null);

  const refresh = useCallback(() => {
    boardLoad(cwd)
      .then((board) => setTasks(board.tasks ?? []))
      .catch(() => setTasks([]));
  }, [cwd]);

  useEffect(refresh, [refresh]);

  // Agents mutate the board through the HTTP API; the backend emits this so
  // the window reflects their work without polling.
  useEffect(() => {
    const unlisten = listen<string>("board:changed", (event) => {
      if (event.payload === cwd) refresh();
    });
    return () => {
      unlisten.then((off) => off()).catch(() => undefined);
    };
  }, [cwd, refresh]);

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

  const openTask = tasks.find((t) => t.id === openTaskId) ?? null;

  return (
    <div className="board">
      <div className="board__bar">
        <span className="board__count">
          {tasks.length} tâche{tasks.length > 1 ? "s" : ""}
        </span>
        <span className="board__hint">
          Les agents lisent et modifient ce tableau via MCP
        </span>
        <button className="btn btn--ghost" onClick={onOpenMcp}>
          ⚙ Connexion MCP
        </button>
      </div>

      <div className="board__columns">
        {BOARD_COLUMNS.map((column) => {
          const items = tasks
            .filter((task) => task.column === column)
            .sort((a, b) => b.priority - a.priority || a.order - b.order);

          return (
            <section
              key={column}
              className={`column ${dragOver === column ? "column--over" : ""}`}
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
                <span className="column__name">{COLUMN_LABELS[column]}</span>
                <span className="column__count">{items.length}</span>
              </header>

              <div className="column__list">
                {items.map((task) => (
                  <article
                    key={task.id}
                    className={`card ${task.assignee ? "card--claimed" : ""}`}
                    draggable
                    onDragStart={(e) => e.dataTransfer.setData("text/plain", task.id)}
                    onClick={() => setOpenTaskId(task.id)}
                  >
                    <p className="card__title">{task.title}</p>

                    <div className="card__meta">
                      {task.priority > 1 && (
                        <span className="chip chip--priority">P{task.priority}</span>
                      )}
                      {task.assignee && (
                        <span className="chip chip--assignee" title="Agent en charge">
                          {task.assignee}
                        </span>
                      )}
                      {task.depends_on.length > 0 && (
                        <span className="chip" title="Dépend d'autres tâches">
                          ⛓ {task.depends_on.length}
                        </span>
                      )}
                      {task.comments.length > 0 && (
                        <span className="chip">💬 {task.comments.length}</span>
                      )}
                      {task.labels.map((label) => (
                        <span key={label} className="chip">
                          {label}
                        </span>
                      ))}
                    </div>
                  </article>
                ))}

                <input
                  className="column__add"
                  placeholder="+ Ajouter…"
                  value={draft[column] ?? ""}
                  onChange={(e) => setDraft((prev) => ({ ...prev, [column]: e.target.value }))}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") addTask(column);
                  }}
                  onBlur={() => addTask(column)}
                />
              </div>
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
