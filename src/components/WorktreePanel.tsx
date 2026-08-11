import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Backdrop } from "./Backdrop";
import { useT, type Translator } from "../i18n";
import {
  gitBranchDelete,
  gitBranches,
  gitCopyUntracked,
  gitFetch,
  gitRepoInfo,
  gitWorktreeAdd,
  gitWorktreeList,
  gitWorktreeLock,
  gitWorktreePrune,
  gitWorktreeRemove,
  gitWorktreeUnlock,
  type Branch,
  type RepoInfo,
  type Worktree,
} from "../lib/git";
import { dirExists } from "../lib/ipc";
import { useStore } from "../store";

interface Props {
  workspaceId: string;
  cwd: string;
  onClose: () => void;
}

/**
 * Two paths are the same folder. Windows is the target here: the separators go
 * both ways, the case carries no meaning, and a trailing one is decoration.
 */
const normalize = (path: string) =>
  path.replace(/[\\/]+$/, "").replace(/\\/g, "/").toLowerCase();

const samePath = (a: string, b: string) => normalize(a) === normalize(b);

const lastSegment = (path: string) =>
  path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;

/** A branch name is not a folder name: `feat/login` would nest a level. */
const folderName = (branch: string) => branch.replace(/[\\/:*?"<>|\s]+/g, "-");

/** Cuts from the head: the tail is what tells two worktrees of a repo apart. */
const shortenPath = (path: string, max = 54) => {
  if (path.length <= max) return path;
  const tail = path.slice(-max);
  const cut = tail.search(/[\\/]/);
  return `…${cut === -1 ? tail : tail.slice(cut)}`;
};

/**
 * What a checkout of a remote branch is called locally: the remote is a
 * coordinate, not part of the name — "origin/feat/x" is checked out as
 * "feat/x". Only the first segment goes.
 */
const localNameOf = (remote: string) => {
  const cut = remote.indexOf("/");
  return cut === -1 ? remote : remote.slice(cut + 1);
};

/** Locals and remotes can share a name, so the select keys on both. */
const refKey = (entry: Branch) => `${entry.kind}:${entry.name}`;

/**
 * Ported from microsoft/vscode, `sanitizeBranchName` in
 * extensions/git/src/commands.ts:650 (MIT). Everything git would refuse in a
 * ref name — and everything a shell would fight over — becomes a dash.
 *
 * The pass can itself leave a leading dash (".env" → "-env"), which git
 * refuses as an option lookalike, so dashes are stripped again after it. An
 * input with nothing else in it comes out empty, and an empty name is what
 * keeps the Create button off.
 */
const sanitizeBranchName = (name: string) =>
  name
    .trim()
    .replace(/^-+/, "")
    .replace(
      /^\.|\/\.|\.\.|~|\^|:|\/$|\.lock$|\.lock\/|\\|\*|\s|^\s*$|\.$|\[|\]$/g,
      "-",
    )
    .replace(/^-+/, "");

/**
 * The files a checkout leaves behind because git ignores them. Defaults taken
 * from Git Worktree Manager's copy-on-create list, trimmed to what actually
 * stops a project from starting: secrets and local editor settings.
 */
const COPY_PATTERNS = [".env", ".env.*", "*.local", ".vscode/**"];

/** VS Code gives up after 20 suffixes too (repository.ts:1962). */
const FOLDER_TRIES = 20;

/**
 * A hand-typed relative folder, resolved where git will resolve it: against
 * the workspace `git -C` runs in. `dirExists` would otherwise probe the app
 * process's own cwd, and the collision check would answer for the wrong
 * directory.
 */
const resolveFolder = (cwd: string, path: string) => {
  const absolute = /^([a-zA-Z]:[\\/]|[\\/])/.test(path);
  if (absolute) return path;
  const separator = cwd.includes("\\") ? "\\" : "/";
  return `${cwd.replace(/[\\/]+$/, "")}${separator}${path}`;
};

/** How long a "copied, suffixed, …" line stays before it stops being news. */
const NOTICE_MS = 7000;

/**
 * The first free `<path>`, `<path>-1`, `<path>-2`… or null when even those are
 * taken — VS Code's loop when the folder of a new worktree already exists.
 */
async function freeFolder(base: string): Promise<string | null> {
  if (!(await dirExists(base))) return base;
  for (let index = 1; index <= FOLDER_TRIES; index++) {
    const candidate = `${base}-${index}`;
    if (!(await dirExists(candidate))) return candidate;
  }
  return null;
}

/** git's own wording, turned into something a badge can hold. */
function parseTrack(track: string | null) {
  if (!track) return null;
  if (/gone/i.test(track)) return { gone: true, ahead: 0, behind: 0 };
  const ahead = Number(/ahead (\d+)/.exec(track)?.[1] ?? 0);
  const behind = Number(/behind (\d+)/.exec(track)?.[1] ?? 0);
  if (!ahead && !behind) return null;
  return { gone: false, ahead, behind };
}

/** Compact ages in the panel's own abbreviations — no locale data to ship. */
function relativeAge(t: Translator, seconds: number): string {
  const elapsed = Math.max(0, Math.floor(Date.now() / 1000) - seconds);
  if (elapsed < 60) return t("worktrees.ageSec", { n: elapsed });
  const minutes = Math.floor(elapsed / 60);
  if (minutes < 60) return t("worktrees.ageMin", { n: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("worktrees.ageHour", { n: hours });
  const days = Math.floor(hours / 24);
  if (days < 7) return t("worktrees.ageDay", { n: days });
  return t("worktrees.ageWeek", { n: Math.floor(days / 7) });
}

/** What `create` is about to ask git for, once the selection is resolved. */
interface Plan {
  branch: string;
  createBranch: boolean;
  baseRef: string | null;
}

export function WorktreePanel({ workspaceId, cwd, onClose }: Props) {
  const { state, addPane, addWorkspace, setActiveWorkspace } = useStore();

  const [info, setInfo] = useState<RepoInfo | null>(null);
  const [worktrees, setWorktrees] = useState<Worktree[]>([]);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  /** Which write is in flight — and, through that, what is disabled. */
  const [busy, setBusy] = useState<string | null>(null);
  /** The worktree whose delete button is armed; only ever one at a time. */
  const [armed, setArmed] = useState<{ path: string; branch: string | null } | null>(
    null,
  );
  /** A removal git turned down, kept per worktree so the row can offer force. */
  const [blocked, setBlocked] = useState<{ path: string; message: string | null } | null>(
    null,
  );
  /** A branch left with no worktree by the removal that just went through. */
  const [orphan, setOrphan] = useState<{
    name: string;
    needsForce: boolean;
    message: string | null;
  } | null>(null);
  /** Says what a step did without stopping the flow; clears itself. */
  const [notice, setNotice] = useState<{ tone: "info" | "warn"; text: string } | null>(
    null,
  );

  const [mode, setMode] = useState<"new" | "existing">("new");
  const [newBranch, setNewBranch] = useState("");
  const [newBase, setNewBase] = useState("");
  const [existingRef, setExistingRef] = useState("");
  const [folder, setFolder] = useState("");
  /** Until this flips, the folder field follows the branch name. */
  const [folderTouched, setFolderTouched] = useState(false);
  const [openAfter, setOpenAfter] = useState(true);
  const [copyUntracked, setCopyUntracked] = useState(true);
  const t = useT();

  const timer = useRef<number | null>(null);
  const flash = useCallback((tone: "info" | "warn", text: string) => {
    setNotice({ tone, text });
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setNotice(null), NOTICE_MS);
  }, []);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  /** Returns what it read: callers act on the fresh lists, not on the state. */
  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    let repo: RepoInfo;
    try {
      repo = await gitRepoInfo(cwd);
    } catch (err) {
      // A missing git and a plain folder are states of the report, so anything
      // that lands here is the command itself having failed.
      setInfo(null);
      setError(String(err));
      setLoading(false);
      return null;
    }
    setInfo(repo);
    if (!repo.installed || !repo.isRepo) {
      setWorktrees([]);
      setBranches([]);
      setLoading(false);
      return null;
    }
    // Settled, not all-or-nothing: `for-each-ref %(worktreepath)` needs a git
    // newer (2.23) than the worktree commands themselves, and a panel that can
    // list and remove worktrees is worth having even where the branch listing
    // dies — it degrades to rows without commit metadata and a create form
    // with no branch picker, alongside the error saying why.
    const [list, refs] = await Promise.allSettled([gitWorktreeList(cwd), gitBranches(cwd)]);
    if (list.status === "rejected") {
      setError(String(list.reason));
      setLoading(false);
      return null;
    }
    setWorktrees(list.value);
    const branches = refs.status === "fulfilled" ? refs.value : [];
    setBranches(branches);
    if (refs.status === "rejected") setError(String(refs.reason));
    setLoading(false);
    return { worktrees: list.value, branches };
  }, [cwd]);

  useEffect(() => {
    void load();
  }, [load]);

  const locals = useMemo(
    () => branches.filter((entry) => entry.kind === "local"),
    [branches],
  );
  const remotes = useMemo(
    () => branches.filter((entry) => entry.kind === "remote"),
    [branches],
  );
  const byKey = useMemo(
    () => new Map(branches.map((entry) => [refKey(entry), entry] as const)),
    [branches],
  );
  /** Locals indexed by name — how a worktree row finds its commit metadata. */
  const byName = useMemo(
    () => new Map(locals.map((entry) => [entry.name, entry] as const)),
    [locals],
  );
  /**
   * A remote whose tracking branch sits in a worktree cannot be checked out
   * again: git would refuse the second one. Better said before the click.
   */
  const remoteHeld = useMemo(() => {
    const map = new Map<string, string>();
    for (const entry of locals) {
      if (entry.upstream && entry.worktreePath)
        map.set(entry.upstream, entry.worktreePath);
    }
    return map;
  }, [locals]);

  const sanitized = sanitizeBranchName(newBranch);

  /**
   * Turns the selection into the three arguments git needs. The remote case is
   * Git Worktree Manager's `checkoutBranch`: reuse the branch already tracking
   * it, otherwise create the local name it would get, based on the remote.
   */
  const resolved = useMemo<{ plan: Plan | null; error: string | null }>(() => {
    if (mode === "new") {
      if (!sanitized) return { plan: null, error: null };
      if (byName.has(sanitized))
        return { plan: null, error: t("worktrees.nameTaken", { name: sanitized }) };
      const base = newBase ? (byKey.get(newBase)?.name ?? null) : null;
      return {
        plan: { branch: sanitized, createBranch: true, baseRef: base },
        error: null,
      };
    }

    const picked = byKey.get(existingRef);
    if (!picked) return { plan: null, error: null };

    if (picked.kind === "local") {
      if (picked.worktreePath !== null)
        return {
          plan: null,
          error: t("worktrees.takenBy", { path: picked.worktreePath }),
        };
      return {
        plan: { branch: picked.name, createBranch: false, baseRef: null },
        error: null,
      };
    }

    const tracking = locals.find((entry) => entry.upstream === picked.name);
    if (tracking) {
      if (tracking.worktreePath !== null)
        return {
          plan: null,
          error: t("worktrees.trackingTaken", {
            name: tracking.name,
            path: tracking.worktreePath,
          }),
        };
      return {
        plan: { branch: tracking.name, createBranch: false, baseRef: null },
        error: null,
      };
    }

    const derived = localNameOf(picked.name);
    if (byName.has(derived))
      return { plan: null, error: t("worktrees.localExists", { name: derived }) };
    // git wires the upstream up on its own when the base is a remote branch.
    return {
      plan: { branch: derived, createBranch: true, baseRef: picked.name },
      error: null,
    };
  }, [mode, sanitized, newBase, existingRef, byKey, byName, locals, t]);

  const branch = resolved.plan?.branch ?? "";

  /**
   * `<parent of the main repo>/<repo>-worktrees/<branch>`: siblings of the
   * checkout rather than children of it, so git never sees them as content.
   */
  const suggested = useMemo(() => {
    const root = info?.mainRoot;
    if (!root || !branch) return "";
    const separator = root.includes("\\") ? "\\" : "/";
    const segments = root.replace(/[\\/]+$/, "").split(/[\\/]/);
    const repo = segments.pop() ?? "";
    return [...segments, `${repo}-worktrees`, folderName(branch)].join(separator);
  }, [info?.mainRoot, branch]);

  const path = folderTouched ? folder : suggested;
  const freeLocals = locals.filter((entry) => entry.worktreePath === null);
  const nothingToPick = freeLocals.length === 0 && remotes.length === 0;
  const canCreate = Boolean(branch) && path.trim() !== "" && busy === null;

  /**
   * Reuses the workspace already sitting on that folder — two workspaces on one
   * worktree would fight over the same files for no gain.
   */
  function activateWorkspace(target: string, label: string | null) {
    const existing = state.workspaces.find((ws) => samePath(ws.cwd, target));
    if (existing) {
      setActiveWorkspace(existing.id);
    } else {
      // A worktree is the same project on another branch: it opens with the
      // agent and the shell of the workspace it was listed from.
      const origin = state.workspaces.find((ws) => ws.id === workspaceId);
      addWorkspace({
        name: label ?? lastSegment(target),
        cwd: target,
        layout: 1,
        agents: [origin?.panes[0]?.agent ?? "shell"],
        shellId: origin?.shellId ?? null,
      });
    }
  }

  function openAsWorkspace(target: string, label: string | null) {
    activateWorkspace(target, label);
    onClose();
  }

  /**
   * Where each worktree is already open — a workspace on it, or a pane spawned
   * into it — keyed the way paths compare. Removing a checkout that live
   * terminals are standing in deletes the ground from under them (and on
   * Windows their cwd handles fail the deletion halfway), so the Remove button
   * asks for that workspace to go first.
   */
  const openWorktrees = useMemo(() => {
    const map = new Map<string, string>();
    for (const ws of state.workspaces) {
      for (const pane of ws.panes) {
        if (pane.cwd) map.set(normalize(pane.cwd), ws.name);
      }
      // Last, so the workspace's own name wins over a pane's when both point
      // at the same folder.
      map.set(normalize(ws.cwd), ws.name);
    }
    return map;
  }, [state.workspaces]);

  function openInPane(target: string) {
    if (addPane(workspaceId, { cwd: target }) === null) {
      setError(t("worktrees.paneFull"));
      return;
    }
    onClose();
  }

  async function remove(worktree: Worktree, force: boolean) {
    // Read before the deletion: afterwards git no longer ties the branch to a
    // worktree, and the offer to delete it would name the wrong one.
    const removedBranch =
      armed?.path === worktree.path ? armed.branch : worktree.branch;
    setBusy(`remove:${worktree.path}`);
    setError(null);
    try {
      const result = await gitWorktreeRemove(cwd, worktree.path, force);
      setArmed(null);
      if (result.removed) {
        setBlocked(null);
        setBusy(null);
        const fresh = await load();
        if (removedBranch && fresh) {
          const left = fresh.branches.find(
            (entry) => entry.kind === "local" && entry.name === removedBranch,
          );
          if (left && left.worktreePath === null)
            setOrphan({ name: removedBranch, needsForce: false, message: null });
        }
        return;
      }
      // Dirty or locked. Throwing the work away is the user's call, so the row
      // keeps git's reason and offers the forced run instead of deciding.
      if (result.needsForce) setBlocked({ path: worktree.path, message: result.message });
      else setError(result.message);
    } catch (err) {
      setArmed(null);
      setError(String(err));
    }
    setBusy(null);
  }

  async function deleteOrphan(force: boolean) {
    if (!orphan) return;
    setBusy(`branch:${orphan.name}`);
    setError(null);
    try {
      const result = await gitBranchDelete(cwd, orphan.name, force);
      if (result.deleted) {
        setOrphan(null);
        setBusy(null);
        await load();
        return;
      }
      // "not fully merged" is a question, not a failure: git's own sentence
      // stays on screen, next to the button that answers it.
      if (result.needsForce)
        setOrphan({ ...orphan, needsForce: true, message: result.message });
      else {
        setError(result.message);
        setOrphan(null);
      }
    } catch (err) {
      setError(String(err));
      setOrphan(null);
    }
    setBusy(null);
  }

  async function toggleLock(worktree: Worktree) {
    setBusy(`lock:${worktree.path}`);
    setError(null);
    try {
      if (worktree.locked !== null) await gitWorktreeUnlock(cwd, worktree.path);
      else await gitWorktreeLock(cwd, worktree.path);
    } catch (err) {
      setError(String(err));
    }
    setBusy(null);
    await load();
  }

  async function prune() {
    setBusy("prune");
    setError(null);
    try {
      await gitWorktreePrune(cwd);
    } catch (err) {
      setError(String(err));
    }
    setBusy(null);
    await load();
  }

  async function fetchAll() {
    setBusy("fetch");
    setError(null);
    try {
      await gitFetch(cwd);
    } catch (err) {
      setError(String(err));
    }
    setBusy(null);
    await load();
  }

  /**
   * The copy is a convenience: it reports what it did and never undoes the
   * worktree, which exists by then whatever happens here.
   */
  async function copyReport(target: string): Promise<{ text: string; warn: boolean }> {
    try {
      const result = await gitCopyUntracked(info?.root ?? cwd, target, COPY_PATTERNS);
      const copied = t("worktrees.copied", { n: result.copied });
      if (result.failed === 0) return { text: copied, warn: false };
      return {
        text: `${copied} ${t("worktrees.copyFailed", { n: result.failed })}`,
        warn: true,
      };
    } catch (err) {
      return { text: t("worktrees.copyError", { error: String(err) }), warn: true };
    }
  }

  async function create() {
    const plan = resolved.plan;
    const wanted = path.trim() && resolveFolder(cwd, path.trim());
    if (!plan || !wanted) return;
    setBusy("create");
    setError(null);

    let target: string;
    try {
      const free = await freeFolder(wanted);
      if (free === null) {
        setError(t("worktrees.folderTaken", { path: wanted }));
        setBusy(null);
        return;
      }
      target = free;
    } catch (err) {
      setError(String(err));
      setBusy(null);
      return;
    }

    try {
      await gitWorktreeAdd({
        path: cwd,
        worktreePath: target,
        branch: plan.branch,
        createBranch: plan.createBranch,
        baseRef: plan.baseRef,
      });
    } catch (err) {
      setError(String(err));
      setBusy(null);
      return;
    }

    // One line for the whole aftermath: where it landed, what came with it.
    const notes: string[] = [];
    let tone: "info" | "warn" = "info";
    if (target !== wanted) notes.push(t("worktrees.folderSuffixed", { path: target }));
    if (copyUntracked) {
      const report = await copyReport(target);
      notes.push(report.text);
      if (report.warn) tone = "warn";
    }
    if (notes.length > 0) flash(tone, notes.join(" "));

    setBusy(null);
    setNewBranch("");
    setNewBase("");
    setExistingRef("");
    setFolder("");
    setFolderTouched(false);
    if (openAfter) {
      activateWorkspace(target, plan.branch);
      // Closing would unmount the report with the panel: a failed .env copy
      // or a suffixed folder is exactly what the user must not miss. The
      // workspace is already switched behind the dialog; when there is
      // nothing to say, the panel goes with it as before.
      if (notes.length === 0) {
        onClose();
        return;
      }
    }
    await load();
  }

  /** Both selects list the same refs; only the disabling differs. */
  const localOptions = (disableTaken: boolean) =>
    locals.map((entry) => (
      <option
        key={refKey(entry)}
        value={refKey(entry)}
        // Greyed rather than hidden: knowing where a branch is
        // already checked out is half the answer.
        disabled={disableTaken && entry.worktreePath !== null}
        title={entry.worktreePath ?? undefined}
      >
        {disableTaken && entry.worktreePath !== null
          ? `${entry.name} — ${t("worktrees.taken")}`
          : entry.name}
      </option>
    ));

  const remoteOptions = (disableTaken: boolean) =>
    remotes.map((entry) => {
      const held = disableTaken ? (remoteHeld.get(entry.name) ?? null) : null;
      return (
        <option
          key={refKey(entry)}
          value={refKey(entry)}
          disabled={held !== null}
          title={held ?? undefined}
        >
          {held === null ? entry.name : `${entry.name} — ${t("worktrees.taken")}`}
        </option>
      );
    });

  return (
    <Backdrop onClose={onClose}>
      <div className="modal modal--wide">
        <header className="modal__head">
          <h2>{t("worktrees.title")}</h2>
          <button className="icon-btn" onClick={onClose}>
            ×
          </button>
        </header>

        <p className="modal__sub" title={cwd}>
          {cwd}
        </p>

        <div className="modal__body modal__body--form">
          {loading && <p className="modal__empty">{t("worktrees.loading")}</p>}

          {!loading && info && !info.installed && (
            <p className="notice notice--error">
              <span className="notice__text">{t("worktrees.noGit")}</span>
            </p>
          )}

          {!loading && info && info.installed && !info.isRepo && (
            <p className="notice">
              <span className="notice__text">{t("worktrees.notRepo")}</span>
            </p>
          )}

          {!loading && info && info.installed && info.isRepo && (
            <>
              <section className="form-row">
                {info.isLinkedWorktree && info.mainRoot && (
                  <span className="form-hint form-hint--block">
                    {t("worktrees.linked", { root: info.mainRoot })}
                  </span>
                )}

                {worktrees.length === 0 && (
                  <p className="modal__empty">{t("worktrees.none")}</p>
                )}

                <div className="wt-list">
                  {worktrees.map((worktree) => {
                    const meta = worktree.branch
                      ? byName.get(worktree.branch)
                      : undefined;
                    const track = parseTrack(meta?.track ?? null);
                    // The panel's own workspace holds the current row, which
                    // already has no Remove; this catches every other one.
                    const heldBy = worktree.isCurrent
                      ? null
                      : (openWorktrees.get(normalize(worktree.path)) ?? null);
                    return (
                      <div key={worktree.path} className="wt">
                        <div className="wt__meta">
                          <span className="wt__label">
                            <span className="wt__branch">
                              {worktree.branch ?? worktree.head ?? "—"}
                            </span>
                            {worktree.detached && (
                              <em className="target__badge target__badge--muted">
                                {t("worktrees.detached")}
                              </em>
                            )}
                            {worktree.bare && (
                              <em className="target__badge target__badge--muted">
                                {t("worktrees.bare")}
                              </em>
                            )}
                            {worktree.isMain && (
                              <em className="target__badge">{t("worktrees.main")}</em>
                            )}
                            {worktree.isCurrent && (
                              <em className="target__badge">{t("worktrees.current")}</em>
                            )}
                            {track?.gone && (
                              <em
                                className="target__badge target__badge--alert"
                                title={meta?.track ?? undefined}
                              >
                                {t("worktrees.trackGone")}
                              </em>
                            )}
                            {track && !track.gone && (
                              <em
                                className="target__badge target__badge--muted"
                                title={meta?.track ?? undefined}
                              >
                                {[
                                  track.ahead > 0 ? `${track.ahead}↑` : null,
                                  track.behind > 0 ? `${track.behind}↓` : null,
                                ]
                                  .filter(Boolean)
                                  .join(" ")}
                              </em>
                            )}
                            {worktree.locked !== null && (
                              <em
                                className="target__badge target__badge--alert"
                                title={worktree.locked || t("worktrees.lockedNoReason")}
                              >
                                {t("worktrees.locked")}
                              </em>
                            )}
                            {worktree.prunable !== null && (
                              <em
                                className="target__badge target__badge--alert"
                                title={worktree.prunable || undefined}
                              >
                                {t("worktrees.prunable")}
                              </em>
                            )}
                          </span>
                          <span className="target__path" title={worktree.path}>
                            {shortenPath(worktree.path)}
                          </span>
                          {meta && (
                            <span className="wt__stats" title={meta.subject ?? undefined}>
                              {meta.head && <code className="wt__sha">{meta.head}</code>}
                              {meta.authorName && <span>{meta.authorName}</span>}
                              {meta.committerDate !== null && (
                                <span>{relativeAge(t, meta.committerDate)}</span>
                              )}
                            </span>
                          )}
                        </div>

                        <div className="wt__actions">
                          <button
                            className="btn btn--tiny"
                            title={t("worktrees.openWorkspaceHint")}
                            disabled={busy !== null}
                            onClick={() =>
                              openAsWorkspace(worktree.path, worktree.branch)
                            }
                          >
                            {t("worktrees.openWorkspace")}
                          </button>
                          <button
                            className="btn btn--tiny"
                            title={
                              worktree.isCurrent
                                ? t("worktrees.openPaneCurrent")
                                : t("worktrees.openPaneHint")
                            }
                            disabled={worktree.isCurrent || busy !== null}
                            onClick={() => openInPane(worktree.path)}
                          >
                            {t("worktrees.openPane")}
                          </button>
                          {/* git refuses to lock the main worktree, so the
                              button only exists where it can work. */}
                          {!worktree.isMain && (
                            <button
                              className="btn btn--tiny"
                              title={
                                worktree.locked !== null
                                  ? t("worktrees.unlockHint")
                                  : t("worktrees.lockHint")
                              }
                              disabled={busy !== null}
                              onClick={() => void toggleLock(worktree)}
                            >
                              {worktree.locked !== null
                                ? t("worktrees.unlock")
                                : t("worktrees.lock")}
                            </button>
                          )}
                          {!worktree.isMain && !worktree.isCurrent && (
                            <button
                              className="btn btn--tiny btn--danger"
                              title={
                                heldBy !== null
                                  ? t("worktrees.inUse", { name: heldBy })
                                  : t("worktrees.removeHint")
                              }
                              disabled={busy !== null || heldBy !== null}
                              onClick={() => {
                                // Inline, because a native dialog would block the
                                // event loop the terminals run on.
                                if (armed?.path === worktree.path)
                                  void remove(worktree, false);
                                else
                                  setArmed({
                                    path: worktree.path,
                                    branch: worktree.branch,
                                  });
                              }}
                            >
                              {armed?.path === worktree.path
                                ? t("worktrees.removeConfirm")
                                : t("worktrees.remove")}
                            </button>
                          )}
                        </div>

                        {blocked?.path === worktree.path && (
                          <div className="notice notice--error wt__blocked">
                            <span className="notice__text">
                              {blocked.message || t("worktrees.removeHint")}
                            </span>
                            <button
                              className="btn btn--tiny btn--danger"
                              title={
                                heldBy !== null
                                  ? t("worktrees.inUse", { name: heldBy })
                                  : undefined
                              }
                              disabled={busy !== null || heldBy !== null}
                              onClick={() => void remove(worktree, true)}
                            >
                              {t("worktrees.force")}
                            </button>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>

                {/* The branch outlives the worktree that held it; removing it
                    too is one click, but never the default. */}
                {orphan && (
                  <div
                    className={`notice wt__orphan ${
                      orphan.needsForce ? "notice--error" : ""
                    }`}
                  >
                    <span className="notice__text">
                      {orphan.message || t("worktrees.orphanAsk", { name: orphan.name })}
                    </span>
                    <button
                      className="btn btn--tiny btn--danger"
                      disabled={busy !== null}
                      onClick={() => void deleteOrphan(orphan.needsForce)}
                    >
                      {orphan.needsForce
                        ? t("worktrees.orphanForce")
                        : t("worktrees.orphanDelete")}
                    </button>
                    <button
                      className="btn btn--tiny"
                      disabled={busy !== null}
                      onClick={() => setOrphan(null)}
                    >
                      {t("worktrees.orphanKeep")}
                    </button>
                  </div>
                )}
              </section>

              <section className="form-row">
                <span className="form-label">{t("worktrees.create")}</span>

                <div className="seg" role="group" aria-label={t("worktrees.create")}>
                  <button
                    className={`seg__btn ${mode === "new" ? "is-active" : ""}`}
                    aria-pressed={mode === "new"}
                    onClick={() => setMode("new")}
                  >
                    {t("worktrees.modeNew")}
                  </button>
                  <button
                    className={`seg__btn ${mode === "existing" ? "is-active" : ""}`}
                    aria-pressed={mode === "existing"}
                    onClick={() => setMode("existing")}
                  >
                    {t("worktrees.modeExisting")}
                  </button>
                </div>

                <label className="form-label" htmlFor="wt-branch">
                  {t("worktrees.branch")}
                  {mode === "existing" && nothingToPick && (
                    <span className="form-hint">{t("worktrees.noFreeBranch")}</span>
                  )}
                </label>

                {mode === "new" ? (
                  <>
                    <input
                      id="wt-branch"
                      className="text-input"
                      value={newBranch}
                      spellCheck={false}
                      placeholder={t("worktrees.branchPlaceholder")}
                      onChange={(e) => setNewBranch(e.target.value)}
                    />
                    {sanitized !== "" && sanitized !== newBranch.trim() && (
                      <span className="form-hint wt__hint">
                        {t("worktrees.sanitized", { name: sanitized })}
                      </span>
                    )}

                    <label className="form-label" htmlFor="wt-base">
                      {t("worktrees.baseLabel")}
                    </label>
                    <select
                      id="wt-base"
                      className="text-input"
                      value={newBase}
                      onChange={(e) => setNewBase(e.target.value)}
                    >
                      <option value="">
                        {t("worktrees.base", {
                          branch: info.branch ?? info.head ?? "HEAD",
                        })}
                      </option>
                      {locals.length > 0 && (
                        <optgroup label={t("worktrees.groupLocal")}>
                          {localOptions(false)}
                        </optgroup>
                      )}
                      {remotes.length > 0 && (
                        <optgroup label={t("worktrees.groupRemote")}>
                          {remoteOptions(false)}
                        </optgroup>
                      )}
                    </select>
                  </>
                ) : (
                  <select
                    id="wt-branch"
                    className="text-input"
                    value={existingRef}
                    onChange={(e) => setExistingRef(e.target.value)}
                  >
                    <option value="">{t("worktrees.pickBranch")}</option>
                    {locals.length > 0 && (
                      <optgroup label={t("worktrees.groupLocal")}>
                        {localOptions(true)}
                      </optgroup>
                    )}
                    {remotes.length > 0 && (
                      <optgroup label={t("worktrees.groupRemote")}>
                        {remoteOptions(true)}
                      </optgroup>
                    )}
                  </select>
                )}

                {resolved.error && (
                  <span className="form-hint wt__hint wt__hint--error">
                    {resolved.error}
                  </span>
                )}

                {mode === "existing" && resolved.plan?.createBranch && (
                  <span className="form-hint wt__hint">
                    {t("worktrees.willTrack", {
                      name: resolved.plan.branch,
                      remote: resolved.plan.baseRef ?? "",
                    })}
                  </span>
                )}

                <label className="form-label" htmlFor="wt-path">
                  {t("worktrees.path")}
                  <span className="form-hint">{t("worktrees.pathHint")}</span>
                </label>
                <input
                  id="wt-path"
                  className="picker__path"
                  value={path}
                  spellCheck={false}
                  onChange={(e) => {
                    setFolderTouched(true);
                    setFolder(e.target.value);
                  }}
                />

                <label className="wt__check" title={t("worktrees.copyHint")}>
                  <input
                    type="checkbox"
                    checked={copyUntracked}
                    onChange={(e) => setCopyUntracked(e.target.checked)}
                  />
                  <span>{t("worktrees.copy")}</span>
                </label>

                <label className="wt__check">
                  <input
                    type="checkbox"
                    checked={openAfter}
                    onChange={(e) => setOpenAfter(e.target.checked)}
                  />
                  <span>{t("worktrees.openAfter")}</span>
                </label>

                <button
                  className="btn btn--primary wt__create"
                  disabled={!canCreate}
                  onClick={() => void create()}
                >
                  {busy === "create"
                    ? t("worktrees.creating")
                    : t("worktrees.createSubmit")}
                </button>
              </section>
            </>
          )}

          {notice && (
            <p className={`notice ${notice.tone === "warn" ? "wt__notice--warn" : ""}`}>
              <span className="notice__text">{notice.text}</span>
            </p>
          )}

          {error && (
            <p className="notice notice--error">
              <span className="notice__text">{error}</span>
            </p>
          )}
        </div>

        <footer className="modal__foot modal__foot--actions wt__foot">
          <div className="wt__foot-group">
            <button
              className="btn btn--ghost"
              title={t("worktrees.fetchHint")}
              disabled={busy !== null || loading}
              onClick={() => void fetchAll()}
            >
              {busy === "fetch" ? t("worktrees.fetching") : t("worktrees.fetch")}
            </button>
            <button
              className="btn btn--ghost"
              title={t("worktrees.pruneHint")}
              disabled={busy !== null || loading}
              onClick={() => void prune()}
            >
              {t("worktrees.prune")}
            </button>
            <button
              className="btn btn--ghost"
              disabled={busy !== null || loading}
              onClick={() => void load()}
            >
              {t("worktrees.refresh")}
            </button>
          </div>
          <button className="btn btn--ghost" onClick={onClose}>
            {t("worktrees.close")}
          </button>
        </footer>
      </div>
    </Backdrop>
  );
}
