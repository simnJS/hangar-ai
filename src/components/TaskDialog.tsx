import { useEffect, useId, useRef, useState } from "react";
import { Backdrop } from "./Backdrop";
import { Icon } from "./Icon";
import { BoardPriority } from "./BoardPriority";
import { BoardAssignee, type AssigneePane } from "./BoardAssignee";
import {
  BOARD_COLUMNS,
  boardComment,
  boardDelete,
  boardUpdate,
  columnKey,
  type BoardColumn,
  type Task,
} from "../lib/board";
import { shortTaskId } from "../lib/boardFormat";
import { useLocale, useT } from "../i18n";

interface Props {
  cwd: string;
  task: Task;
  allTasks: Task[];
  onClose: () => void;
  onChanged: () => void;
  /** The pane the assignee resolved to, as the card shows it. */
  assigneePane?: AssigneePane | null;
  /** Brings the assignee's pane forward, as the card's name does. */
  onFocusAgent?: (() => void) | null;
}

export function TaskDialog({
  cwd,
  task,
  allTasks,
  onClose,
  onChanged,
  assigneePane = null,
  onFocusAgent = null,
}: Props) {
  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [comment, setComment] = useState("");
  const dialogRef = useRef<HTMLDivElement>(null);
  const threadRef = useRef<HTMLOListElement>(null);
  const id = useId();
  const t = useT();
  const locale = useLocale();

  const when = (ms: number) =>
    new Date(ms).toLocaleString(locale, { dateStyle: "short", timeStyle: "short" });

  // Follow external edits (an agent moving or renaming this very task).
  useEffect(() => {
    setTitle(task.title);
    setDescription(task.description);
  }, [task.title, task.description]);

  // The keyboard lands in the dialog, and goes back to the card that opened
  // it when it closes, so the board can be walked task after task.
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    return () => opener?.focus?.();
  }, []);

  // The newest message is the one worth reading: the thread opens on it, and
  // follows the ones that arrive while it is open.
  useEffect(() => {
    const thread = threadRef.current;
    if (thread) thread.scrollTop = thread.scrollHeight;
  }, [task.comments.length]);

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
    .map((dep) => allTasks.find((other) => other.id === dep))
    .filter((dep): dep is Task => Boolean(dep));

  return (
    <Backdrop onClose={onClose}>
      <div
        ref={dialogRef}
        className="modal modal--task"
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${id}-heading`}
        tabIndex={-1}
      >
        <header className="task__head">
          <span className={`task__mark task__mark--${task.column}`} aria-hidden />
          <h2 id={`${id}-heading`} className="task__heading">
            {t("task.title")}
            <span className="task__id" title={task.id}>
              #{shortTaskId(task.id)}
            </span>
          </h2>
          <span className="task__where">{t(columnKey(task.column))}</span>
          <button
            type="button"
            className="task__close"
            onClick={onClose}
            title={t("task.close")}
            aria-label={t("task.close")}
          >
            <Icon name="close" size={14} />
          </button>
        </header>

        <div className="task__body">
          <div className="task__field">
            <label className="task__label" htmlFor={`${id}-title`}>
              {t("task.name")}
            </label>
            <input
              id={`${id}-title`}
              className="task__input task__input--title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              onBlur={() => title !== task.title && save({ title })}
            />
          </div>

          <div className="task__field">
            <label className="task__label" htmlFor={`${id}-desc`}>
              {t("task.description")}
              <span className="task__hint">{t("task.descriptionHint")}</span>
            </label>
            <textarea
              id={`${id}-desc`}
              className="task__input task__input--area"
              rows={5}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              onBlur={() => description !== task.description && save({ description })}
            />
          </div>

          <div className="task__grid">
            <div className="task__field">
              <label className="task__label" htmlFor={`${id}-column`}>
                {t("task.column")}
              </label>
              <select
                id={`${id}-column`}
                className="task__input"
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

            <div className="task__field">
              <label className="task__label" htmlFor={`${id}-priority`}>
                {t("task.priority")}
              </label>
              <div className="task__priority">
                <input
                  id={`${id}-priority`}
                  className="task__input task__input--number"
                  type="number"
                  min={0}
                  max={9}
                  value={task.priority}
                  onChange={(e) => save({ priority: Number(e.target.value) })}
                />
                <BoardPriority priority={task.priority} />
              </div>
            </div>

            <div className="task__field">
              <span className="task__label">{t("task.assignee")}</span>
              {task.assignee ? (
                <div className="task__assignee">
                  <BoardAssignee
                    assignee={task.assignee}
                    pane={assigneePane}
                    done={task.column === "done"}
                    onFocus={onFocusAgent}
                  />
                  <button
                    type="button"
                    className="btn task__small"
                    onClick={() => save({ release: true })}
                  >
                    {t("task.release")}
                  </button>
                </div>
              ) : (
                <span className="task__free">{t("task.free")}</span>
              )}
            </div>
          </div>

          {task.labels.length > 0 && (
            <div className="task__labels" aria-label={t("task.labels")}>
              {task.labels.map((label) => (
                <span key={label} className="chip">
                  {label}
                </span>
              ))}
            </div>
          )}

          {dependencies.length > 0 && (
            <div className="task__field">
              <span className="task__label">{t("task.dependencies")}</span>
              <ul className="task__deps">
                {dependencies.map((dep) => {
                  const done = dep.column === "done";
                  return (
                    <li key={dep.id} className={done ? "task__dep task__dep--done" : "task__dep"}>
                      <span className="task__dep-mark" aria-hidden>
                        {done ? (
                          <Icon name="check" size={11} />
                        ) : (
                          <Icon name="lock" size={11} />
                        )}
                      </span>
                      <span className="task__dep-id">#{shortTaskId(dep.id)}</span>
                      <span className="task__dep-title">{dep.title}</span>
                      <span className="task__dep-col">{t(columnKey(dep.column))}</span>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}

          <section className="task__thread" aria-labelledby={`${id}-thread`}>
            <h3 id={`${id}-thread`} className="task__label">
              {t("task.thread")}
              <span className="task__hint">{t("task.threadHint")}</span>
            </h3>

            {task.comments.length === 0 ? (
              <p className="task__empty">{t("task.noComments")}</p>
            ) : (
              <ol className="task__messages" ref={threadRef}>
                {task.comments.map((entry) => (
                  <li key={entry.id} className="task__message">
                    <div className="task__message-head">
                      <span className="task__author">{entry.author}</span>
                      <time
                        className="task__time"
                        dateTime={new Date(entry.created_at).toISOString()}
                      >
                        {when(entry.created_at)}
                      </time>
                    </div>
                    <p className="task__text">{entry.text}</p>
                  </li>
                ))}
              </ol>
            )}

            <div className="task__compose">
              <input
                className="task__input"
                placeholder={t("task.writeComment")}
                aria-label={t("task.writeComment")}
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

        <footer className="task__foot">
          <span className="task__note">{t("task.createdAt", { date: when(task.created_at) })}</span>
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
