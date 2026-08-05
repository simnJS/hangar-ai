import { useEffect, useState } from "react";
import { Backdrop } from "./Backdrop";
import {
  BOARD_COLUMNS,
  COLUMN_LABELS,
  boardComment,
  boardDelete,
  boardUpdate,
  type BoardColumn,
  type Task,
} from "../lib/board";

interface Props {
  cwd: string;
  task: Task;
  allTasks: Task[];
  onClose: () => void;
  onChanged: () => void;
}

const when = (ms: number) => new Date(ms).toLocaleString();

export function TaskDialog({ cwd, task, allTasks, onClose, onChanged }: Props) {
  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [comment, setComment] = useState("");

  // Follow external edits (an agent moving or renaming this very task).
  useEffect(() => {
    setTitle(task.title);
    setDescription(task.description);
  }, [task.title, task.description]);

  async function save(patch: Parameters<typeof boardUpdate>[2]) {
    await boardUpdate(cwd, task.id, patch).catch(() => undefined);
    onChanged();
  }

  async function sendComment() {
    const text = comment.trim();
    if (!text) return;
    setComment("");
    await boardComment(cwd, task.id, "user", text).catch(() => undefined);
    onChanged();
  }

  async function remove() {
    await boardDelete(cwd, task.id).catch(() => undefined);
    onChanged();
    onClose();
  }

  const dependencies = task.depends_on
    .map((id) => allTasks.find((t) => t.id === id))
    .filter((t): t is Task => Boolean(t));

  return (
    <Backdrop onClose={onClose}>
      <div className="modal modal--wide">
        <header className="modal__head">
          <h2>Tâche</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <div className="modal__body modal__body--form">
          <section className="form-row">
            <label className="form-label" htmlFor="task-title">
              Titre
            </label>
            <input
              id="task-title"
              className="text-input"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              onBlur={() => title !== task.title && save({ title })}
            />
          </section>

          <section className="form-row">
            <label className="form-label" htmlFor="task-desc">
              Description
              <span className="form-hint">visible par les agents</span>
            </label>
            <textarea
              id="task-desc"
              className="text-input text-area"
              rows={5}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              onBlur={() => description !== task.description && save({ description })}
            />
          </section>

          <section className="form-row form-row--split">
            <div>
              <span className="form-label">Colonne</span>
              <select
                className="text-input"
                value={task.column}
                onChange={(e) => save({ column: e.target.value as BoardColumn })}
              >
                {BOARD_COLUMNS.map((column) => (
                  <option key={column} value={column}>
                    {COLUMN_LABELS[column]}
                  </option>
                ))}
              </select>
            </div>

            <div>
              <span className="form-label">Priorité</span>
              <input
                className="text-input"
                type="number"
                min={0}
                max={9}
                value={task.priority}
                onChange={(e) => save({ priority: Number(e.target.value) })}
              />
            </div>

            <div>
              <span className="form-label">Assigné à</span>
              {task.assignee ? (
                <div className="assignee">
                  <span className="chip chip--assignee">{task.assignee}</span>
                  <button className="btn btn--ghost" onClick={() => save({ release: true })}>
                    Libérer
                  </button>
                </div>
              ) : (
                <span className="form-hint">libre — un agent peut la prendre</span>
              )}
            </div>
          </section>

          {dependencies.length > 0 && (
            <section className="form-row">
              <span className="form-label">Dépend de</span>
              <ul className="deps">
                {dependencies.map((dep) => (
                  <li key={dep.id} className={dep.column === "done" ? "deps--done" : ""}>
                    {dep.column === "done" ? "✓" : "○"} {dep.title}
                  </li>
                ))}
              </ul>
            </section>
          )}

          <section className="form-row">
            <span className="form-label">
              Échanges
              <span className="form-hint">c'est ici que les agents se parlent</span>
            </span>

            <div className="thread">
              {task.comments.length === 0 && (
                <p className="form-hint">Aucun message pour l'instant.</p>
              )}
              {task.comments.map((entry) => (
                <div key={entry.id} className="thread__item">
                  <div className="thread__head">
                    <span className="thread__author">{entry.author}</span>
                    <span className="thread__time">{when(entry.created_at)}</span>
                  </div>
                  <p className="thread__text">{entry.text}</p>
                </div>
              ))}
            </div>

            <div className="picker">
              <input
                className="picker__path"
                placeholder="Écrire un message…"
                value={comment}
                onChange={(e) => setComment(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") sendComment();
                }}
              />
              <button className="btn" onClick={sendComment} disabled={!comment.trim()}>
                Envoyer
              </button>
            </div>
          </section>
        </div>

        <footer className="modal__foot modal__foot--actions">
          <span className="modal__note">Créée le {when(task.created_at)}</span>
          <span className="pane__spacer" />
          <button className="btn btn--danger" onClick={remove}>
            Supprimer
          </button>
          <button className="btn btn--primary" onClick={onClose}>
            Fermer
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
