import { useEffect, useState } from "react";
import { Backdrop } from "./Backdrop";
import {
  BOARD_COLUMNS,
  boardComment,
  boardDelete,
  boardUpdate,
  columnKey,
  type BoardColumn,
  type Task,
} from "../lib/board";
import { useLocale, useT } from "../i18n";

interface Props {
  cwd: string;
  task: Task;
  allTasks: Task[];
  onClose: () => void;
  onChanged: () => void;
}

export function TaskDialog({ cwd, task, allTasks, onClose, onChanged }: Props) {
  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [comment, setComment] = useState("");
  const t = useT();
  const locale = useLocale();

  const when = (ms: number) => new Date(ms).toLocaleString(locale);

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
          <h2>{t("task.title")}</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <div className="modal__body modal__body--form">
          <section className="form-row">
            <label className="form-label" htmlFor="task-title">
              {t("task.name")}
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
              {t("task.description")}
              <span className="form-hint">{t("task.descriptionHint")}</span>
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
              <span className="form-label">{t("task.column")}</span>
              <select
                className="text-input"
                value={task.column}
                onChange={(e) => save({ column: e.target.value as BoardColumn })}
              >
                {BOARD_COLUMNS.map((column) => (
                  <option key={column} value={column}>
                    {t(columnKey(column))}
                  </option>
                ))}
              </select>
            </div>

            <div>
              <span className="form-label">{t("task.priority")}</span>
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
              <span className="form-label">{t("task.assignee")}</span>
              {task.assignee ? (
                <div className="assignee">
                  <span className="chip chip--assignee">{task.assignee}</span>
                  <button className="btn btn--ghost" onClick={() => save({ release: true })}>
                    {t("task.release")}
                  </button>
                </div>
              ) : (
                <span className="form-hint">{t("task.free")}</span>
              )}
            </div>
          </section>

          {dependencies.length > 0 && (
            <section className="form-row">
              <span className="form-label">{t("task.dependencies")}</span>
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
              {t("task.thread")}
              <span className="form-hint">{t("task.threadHint")}</span>
            </span>

            <div className="thread">
              {task.comments.length === 0 && (
                <p className="form-hint">{t("task.noComments")}</p>
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
                placeholder={t("task.writeComment")}
                value={comment}
                onChange={(e) => setComment(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") sendComment();
                }}
              />
              <button className="btn" onClick={sendComment} disabled={!comment.trim()}>
                {t("task.send")}
              </button>
            </div>
          </section>
        </div>

        <footer className="modal__foot modal__foot--actions">
          <span className="modal__note">
            {t("task.createdAt", { date: when(task.created_at) })}
          </span>
          <span className="pane__spacer" />
          <button className="btn btn--danger" onClick={remove}>
            {t("task.delete")}
          </button>
          <button className="btn btn--primary" onClick={onClose}>
            {t("task.close")}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
