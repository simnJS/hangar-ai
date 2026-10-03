import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Backdrop } from "./Backdrop";
import { useT } from "../i18n";
import {
  gitChanges,
  gitFileDiff,
  gitRepoInfo,
  gitWorktreeList,
  type ChangedFile,
  type Changes,
  type FileDiff,
  type RepoInfo,
} from "../lib/git";
import { parseUnifiedDiff, splitPath } from "../lib/diff";

interface Props {
  /** Any folder inside the working tree: a pane's, a worktree's. */
  cwd: string;
  /** Who the changes belong to — a pane or a branch name — shown in the title. */
  label?: string | null;
  onClose: () => void;
}

/**
 * What an agent changed in a folder, read-only: the files on the left, the
 * unified diff of the selected one on the right.
 *
 * A linked worktree is compared with where its branch left the main checkout's
 * branch by default — agents commit, and their commits are the work as much
 * as what is still uncommitted. Anywhere else, HEAD.
 */

type Setup =
  | { state: "loading" }
  /** No backend to ask, which is the browser demo. */
  | { state: "unavailable" }
  | { state: "ready"; info: RepoInfo; mainBranch: string | null };

const STATUS_KEYS = ["M", "A", "D", "R", "C", "T", "U", "?"] as const;

const fileKey = (file: ChangedFile) => `${file.oldPath ?? ""}\u0000${file.path}`;

/** The letter in the list. Untracked reads U, as in VS Code; git's own U — a
    conflict — is shown as "!" so the two never share a letter. */
const statusMark = (status: string) => (status === "?" ? "U" : status === "U" ? "!" : status);

export function DiffView({ cwd, label, onClose }: Props) {
  const t = useT();
  const [setup, setSetup] = useState<Setup>({ state: "loading" });
  /** null compares with HEAD; a branch name, with where this branch forked. */
  const [base, setBase] = useState<string | null>(null);
  const [changes, setChanges] = useState<Changes | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [diffError, setDiffError] = useState<string | null>(null);
  /**
   * Diffs already read for this listing, keyed by base and file. A reload
   * swaps in a new map rather than clearing this one, so a request still in
   * flight from before lands in the map nobody reads any more.
   */
  const cache = useRef(new Map<string, FileDiff>());
  /** Which `load` is the latest: an older answer arriving late is dropped. */
  const loadSeq = useRef(0);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      let info: RepoInfo;
      try {
        info = await gitRepoInfo(cwd);
      } catch {
        // git_repo_info never fails on the desktop: a rejection means there
        // is no backend at all.
        if (!cancelled) setSetup({ state: "unavailable" });
        return;
      }
      let mainBranch: string | null = null;
      if (info.isRepo && info.isLinkedWorktree) {
        const worktrees = await gitWorktreeList(cwd).catch(() => []);
        mainBranch = worktrees.find((worktree) => worktree.isMain)?.branch ?? null;
      }
      if (cancelled) return;
      const comparable = mainBranch && mainBranch !== info.branch ? mainBranch : null;
      setSetup({ state: "ready", info, mainBranch: comparable });
      setBase(comparable);
    })();
    return () => {
      cancelled = true;
    };
  }, [cwd]);

  const ready = setup.state === "ready" && setup.info.installed && setup.info.isRepo;

  const load = useCallback(async () => {
    const seq = ++loadSeq.current;
    setLoading(true);
    setError(null);
    cache.current = new Map();
    try {
      const next = await gitChanges(cwd, base);
      // Toggling the base twice quickly races two listings; only the last one
      // asked for may describe what the toggle shows.
      if (seq !== loadSeq.current) return;
      setChanges(next);
      setSelected((current) =>
        current && next.files.some((file) => fileKey(file) === current)
          ? current
          : next.files[0]
            ? fileKey(next.files[0])
            : null,
      );
    } catch (err) {
      if (seq !== loadSeq.current) return;
      setChanges(null);
      setError(String(err));
    }
    setLoading(false);
  }, [cwd, base]);

  useEffect(() => {
    if (ready) void load();
  }, [ready, load]);

  const files = useMemo(() => changes?.files ?? [], [changes]);
  const current = files.find((file) => fileKey(file) === selected) ?? null;

  useEffect(() => {
    if (!current) {
      setDiff(null);
      return;
    }
    const key = `${base ?? ""}\u0001${fileKey(current)}`;
    const store = cache.current;
    const known = store.get(key);
    if (known) {
      setDiff(known);
      setDiffError(null);
      return;
    }
    let cancelled = false;
    setDiff(null);
    setDiffError(null);
    gitFileDiff(cwd, current, base)
      .then((result) => {
        if (cancelled) return;
        store.set(key, result);
        setDiff(result);
      })
      .catch((err) => {
        if (!cancelled) setDiffError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [cwd, base, current]);

  const rows = useMemo(() => (diff ? parseUnifiedDiff(diff.text) : []), [diff]);
  const hasLines = rows.some((row) => row.kind !== "meta");

  const totals = useMemo(
    () =>
      files.reduce(
        (sum, file) => ({
          add: sum.add + (file.additions ?? 0),
          del: sum.del + (file.deletions ?? 0),
        }),
        { add: 0, del: 0 },
      ),
    [files],
  );

  /** Up and down walk the list, the way a file list in an editor does. */
  function onListKey(event: React.KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const at = files.findIndex((file) => fileKey(file) === selected);
    const index = Math.min(files.length - 1, Math.max(0, at + (event.key === "ArrowDown" ? 1 : -1)));
    const next = files[index];
    if (!next) return;
    setSelected(fileKey(next));
    listRef.current?.querySelectorAll<HTMLElement>(".diff__file")[index]?.focus();
  }

  const statusLabel = (status: string) =>
    (STATUS_KEYS as readonly string[]).includes(status)
      ? t(`diff.status.${status as (typeof STATUS_KEYS)[number]}`)
      : status;

  function renderState() {
    if (setup.state === "loading") return <p className="diff__empty">{t("diff.loading")}</p>;
    if (setup.state === "unavailable") return <p className="diff__empty">{t("diff.unavailable")}</p>;
    if (!setup.info.installed) return <p className="diff__empty">{t("diff.noGit")}</p>;
    if (!setup.info.isRepo) return <p className="diff__empty">{t("diff.notRepo")}</p>;
    if (error) {
      return (
        <p className="notice notice--error diff__error">
          <span className="notice__text">{error}</span>
        </p>
      );
    }
    if (!changes) return <p className="diff__empty">{t("diff.loading")}</p>;
    if (files.length === 0) {
      return (
        <p className="diff__empty">
          {changes.base ? t("diff.clean", { base: changes.base }) : t("diff.cleanUnborn")}
        </p>
      );
    }
    return null;
  }

  const state = renderState();
  const info = setup.state === "ready" ? setup.info : null;
  const mainBranch = setup.state === "ready" ? setup.mainBranch : null;

  return (
    <Backdrop onClose={onClose}>
      <div className="modal modal--diff" role="dialog" aria-label={t("diff.title")}>
        <header className="modal__head">
          <h2>
            {t("diff.title")}
            {label && <span className="diff__label">{label}</span>}
          </h2>
          <button className="icon-btn" onClick={onClose} aria-label={t("diff.close")}>
            ×
          </button>
        </header>

        <div className="diff__bar">
          <span className="diff__root" title={changes?.root ?? info?.root ?? cwd}>
            {changes?.root ?? info?.root ?? cwd}
          </span>
          {info?.branch && <em className="target__badge">{info.branch}</em>}

          {mainBranch && (
            <div className="seg diff__seg" role="group" aria-label={t("diff.title")}>
              <button
                className={`seg__btn ${base === null ? "is-active" : ""}`}
                aria-pressed={base === null}
                title={t("diff.uncommittedHint")}
                onClick={() => setBase(null)}
              >
                {t("diff.uncommitted")}
              </button>
              <button
                className={`seg__btn ${base !== null ? "is-active" : ""}`}
                aria-pressed={base !== null}
                title={t("diff.sinceBranchHint", { branch: mainBranch })}
                onClick={() => setBase(mainBranch)}
              >
                {t("diff.sinceBranch", { branch: mainBranch })}
              </button>
            </div>
          )}

          <span className="pane__spacer" />

          {changes && files.length > 0 && (
            <span className="diff__summary">
              {t("diff.files", { n: files.length })}
              <span className="diff__add">+{totals.add}</span>
              <span className="diff__del">−{totals.del}</span>
              {changes.base && (
                <span className="diff__base">{t("diff.against", { base: changes.base })}</span>
              )}
            </span>
          )}

          {ready && (
            <button className="btn btn--ghost" disabled={loading} onClick={() => void load()}>
              {t("diff.refresh")}
            </button>
          )}
        </div>

        {state ?? (
          <div className="diff__body">
            <div
              className="diff__files"
              ref={listRef}
              role="listbox"
              aria-label={t("diff.files", { n: files.length })}
              onKeyDown={onListKey}
            >
              {files.map((file) => {
                const key = fileKey(file);
                const { dir, name } = splitPath(file.path);
                return (
                  <button
                    key={key}
                    role="option"
                    aria-selected={key === selected}
                    className={`diff__file ${key === selected ? "is-active" : ""}`}
                    title={
                      file.oldPath
                        ? `${file.path} — ${t("diff.renamedFrom", { path: file.oldPath })}`
                        : file.path
                    }
                    onClick={() => setSelected(key)}
                  >
                    <span
                      className={`diff__status diff__status--${file.status === "?" ? "new" : file.status}`}
                      title={statusLabel(file.status)}
                    >
                      {statusMark(file.status)}
                    </span>
                    <span className="diff__path">
                      <span className="diff__dir">{dir}</span>
                      {name}
                    </span>
                    {!file.binary && (file.additions !== null || file.deletions !== null) && (
                      <span className="diff__counts">
                        {file.additions ? <span className="diff__add">+{file.additions}</span> : null}
                        {file.deletions ? <span className="diff__del">−{file.deletions}</span> : null}
                      </span>
                    )}
                  </button>
                );
              })}
              {changes?.truncated && (
                <p className="diff__note">{t("diff.truncated", { n: files.length })}</p>
              )}
            </div>

            <div className="diff__view">
              {!current && <p className="diff__empty">{t("diff.pick")}</p>}
              {current && diffError && (
                <p className="notice notice--error diff__error">
                  <span className="notice__text">{diffError}</span>
                </p>
              )}
              {current && !diff && !diffError && <p className="diff__empty">{t("diff.loading")}</p>}
              {diff && (
                <>
                  {diff.truncated && <p className="diff__note">{t("diff.diffTruncated")}</p>}
                  {diff.binary ? (
                    <p className="diff__empty">{t("diff.binary")}</p>
                  ) : !hasLines ? (
                    <p className="diff__empty">{t("diff.noText")}</p>
                  ) : (
                    <table className="diff__table">
                      <tbody>
                        {rows.map((row, index) =>
                          row.kind === "meta" ? null : (
                            <tr key={index} className={`diff__row diff__row--${row.kind}`}>
                              <td className="diff__num">{row.oldLine ?? ""}</td>
                              <td className="diff__num">{row.newLine ?? ""}</td>
                              <td className="diff__sign">
                                {row.kind === "add" ? "+" : row.kind === "del" ? "−" : ""}
                              </td>
                              <td className="diff__code">{row.text}</td>
                            </tr>
                          ),
                        )}
                      </tbody>
                    </table>
                  )}
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </Backdrop>
  );
}
