import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useT, type Translator } from "../../i18n";
import { kitName, sourceLine, updateOutcome } from "../../lib/kitActivation";
import {
  kitAddFolder,
  kitAddGit,
  kitCheckUpdate,
  kitRemove,
  kitUpdate,
  kitsList,
  onKitsChanged,
  type KitInfo,
} from "../../lib/kits";
import { Icon } from "../Icon";
import { WorkspaceKitsDialog } from "../WorkspaceDialog";
import { useStore } from "../../store";
import { workspaceKits } from "../../lib/kits";
import "./KitsSettings.css";

/**
 * Settings › Kits: the kits installed on this machine, adding one from a
 * folder or a GitHub link, checking and applying updates, removing.
 *
 * `KitsView` is the markup, from props alone, so every state can be drawn
 * without Rust; `KitsSettings` feeds it from the Tauri commands.
 *
 * A kit's own icon (`manifest.icon`, a file in the kit's folder) is not drawn:
 * the webview has no asset scope for arbitrary folders and `lib/kits` has no
 * reader for it. A plug glyph stands in until one exists.
 */

/** What a kit's row says about its update, once asked. */
export type UpdateNote =
  | { kind: "checking" }
  | { kind: "none"; version: string }
  | { kind: "available"; current: string; latest: string }
  | { kind: "updating" }
  | { kind: "done"; from: string; to: string; newPermissions: number }
  | { kind: "folder" }
  | { kind: "reloaded"; version: string }
  | { kind: "error"; detail: string };

export interface KitsViewProps {
  t: Translator;
  /** Null until Rust has answered. */
  kits: KitInfo[] | null;
  error: string | null;
  /** A confirmation after adding a kit. */
  flash: string | null;
  notes: Record<string, UpdateNote>;
  /** The kit whose removal waits for its second click. */
  removing: string | null;
  adding: "folder" | "git" | null;
  folderError: string | null;
  gitUrl: string;
  gitError: string | null;
  onGitUrl: (url: string) => void;
  onAddFolder: () => void;
  onAddGit: () => void;
  onCheck: (kit: KitInfo) => void;
  onUpdate: (kit: KitInfo) => void;
  onAskRemove: (id: string | null) => void;
  onRemove: (kit: KitInfo) => void;
  /** The workspaces a kit can be enabled on, with how many kits each has. */
  workspaces: { id: string; name: string; kits: number }[];
  onManageWorkspace: (id: string) => void;
}

function Block({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section className="set-section">
      <header className="set-section__head">
        <h3 className="set-section__title">{title}</h3>
        {hint && <p className="set-section__hint">{hint}</p>}
      </header>
      <div className="set-group">{children}</div>
    </section>
  );
}

function noteText(t: Translator, note: UpdateNote): string {
  switch (note.kind) {
    case "checking":
      return t("kitsManage.checking");
    case "none":
      return t("kitsManage.upToDate", { version: note.version });
    case "available":
      return t("kitsManage.updateAvailable", { current: note.current, latest: note.latest });
    case "updating":
      return t("kitsManage.updating");
    case "done":
      return t("kitsManage.updated", { from: note.from, to: note.to });
    case "folder":
      return t("kitsManage.folderNoUpdate");
    case "reloaded":
      return t("kitsManage.reloaded", { version: note.version });
    case "error":
      return t("kitsManage.errorPrefix", { detail: note.detail });
  }
}

function KitRow({ kit, props }: { kit: KitInfo; props: KitsViewProps }) {
  const { t, notes, removing } = props;
  const note = notes[kit.id];
  const source = sourceLine(kit.source);
  const isFolder = kit.source.kind === "folder";
  const working = note?.kind === "checking" || note?.kind === "updating";
  const available = note?.kind === "available" || (kit.update?.available && !note);
  const latest = note?.kind === "available" ? note.latest : kit.update?.latest;
  const manifest = kit.manifest;

  return (
    <div className="set-row set-row--block kit-row">
      <div className="kit-row__top">
        <span className="kit-row__icon" aria-hidden="true">
          <Icon name="plug" size={16} />
        </span>
        <div className="kit-row__text">
          <span className="set-row__label">
            {manifest ? manifest.name : t("kitsManage.brokenName", { id: kit.id })}
            {manifest && <span className="kit-row__ver">{t("kitsManage.version", { version: kit.installedVersion })}</span>}
          </span>
          {manifest?.description && <span className="set-row__hint">{manifest.description}</span>}
          <span className="kit-row__source" title={source.text}>
            {source.kind === "folder"
              ? t("kitsManage.sourceFolder", { path: source.text })
              : t("kitsManage.sourceGit", { url: source.text })}
            {isFolder && ` · ${t("kitsManage.linked")}`}
          </span>
        </div>
      </div>

      {kit.error && (
        <p className="notice notice--error kit-row__error" role="alert">
          <span className="notice__text">
            <strong>{t("kitsManage.broken")}</strong>
            <br />
            {kit.error}
          </span>
        </p>
      )}

      {note && note.kind !== "available" && (
        <p className="kit-row__note" role="status">
          {noteText(t, note)}
          {note.kind === "done" && note.newPermissions > 0 && (
            <>
              <br />
              {t("kitsManage.updatedPerms")}
            </>
          )}
        </p>
      )}
      {available && latest && (
        <p className="kit-row__note" role="status">
          {t("kitsManage.updateAvailable", { current: kit.installedVersion, latest })}
        </p>
      )}

      {removing === kit.id ? (
        <div className="kit-row__confirm" role="group" aria-label={t("kitsManage.removeAsk", { name: kitName(kit) })}>
          <p className="kit-row__note">
            <strong>{t("kitsManage.removeAsk", { name: kitName(kit) })}</strong>
            <br />
            {isFolder ? t("kitsManage.removeFolder") : t("kitsManage.removeGit")}
          </p>
          <div className="kit-row__actions">
            <button type="button" className="btn btn--ghost" onClick={() => props.onAskRemove(null)}>
              {t("kitsManage.cancel")}
            </button>
            <button type="button" className="btn btn--danger" onClick={() => props.onRemove(kit)}>
              {t("kitsManage.removeConfirm")}
            </button>
          </div>
        </div>
      ) : (
        <div className="kit-row__actions">
          {isFolder ? (
            <button type="button" className="btn" disabled={working || (!!kit.error && !manifest)} onClick={() => props.onUpdate(kit)}>
              {t("kitsManage.reload")}
            </button>
          ) : available ? (
            <button type="button" className="btn btn--primary" disabled={working} onClick={() => props.onUpdate(kit)}>
              {working ? t("kitsManage.updating") : t("kitsManage.update")}
            </button>
          ) : (
            <button type="button" className="btn" disabled={working} onClick={() => props.onCheck(kit)}>
              {note?.kind === "checking" ? t("kitsManage.checking") : t("kitsManage.checkUpdate")}
            </button>
          )}
          <button type="button" className="btn btn--danger" onClick={() => props.onAskRemove(kit.id)}>
            {t("kitsManage.remove")}
          </button>
        </div>
      )}
    </div>
  );
}

export function KitsView(props: KitsViewProps) {
  const { t, kits, error, flash, adding, folderError, gitUrl, gitError } = props;

  return (
    <div className="kits">
      <p className="kits__intro">{t("kitsManage.intro")}</p>

      {error && (
        <p className="notice notice--error" role="alert">
          <span className="notice__text">{t("kitsManage.errorPrefix", { detail: error })}</span>
        </p>
      )}

      <Block title={t("kitsManage.installed")}>
        {kits === null ? (
          <div className="set-row">
            <span className="set-row__hint">{t("kitsManage.loading")}</span>
          </div>
        ) : kits.length === 0 ? (
          <div className="set-row">
            <span className="set-row__hint">{t("kitsManage.empty")}</span>
          </div>
        ) : (
          kits.map((kit) => <KitRow key={kit.id} kit={kit} props={props} />)
        )}
      </Block>

      {props.workspaces.length > 0 && (
        <Block title={t("kitsManage.byWorkspace")} hint={t("kitsManage.byWorkspaceHint")}>
          {props.workspaces.map((ws) => (
            <div key={ws.id} className="set-row">
              <span className="set-row__text">
                <span className="set-row__label">{ws.name}</span>
                <span className="set-row__hint">{t("kitsManage.enabledCount", { n: ws.kits })}</span>
              </span>
              <button type="button" className="btn" onClick={() => props.onManageWorkspace(ws.id)}>
                {t("kitsManage.manage")}
              </button>
            </div>
          ))}
        </Block>
      )}

      <Block title={t("kitsManage.add")}>
        <div className="set-row set-row--block kit-add">
          <div className="kit-add__line">
            <button type="button" className="btn" disabled={adding !== null} onClick={props.onAddFolder}>
              {adding === "folder" ? t("kitsManage.adding") : t("kitsManage.addFolder")}
            </button>
            <span className="set-row__hint">{t("kitsManage.addFolderHint")}</span>
          </div>
          {folderError && (
            <p className="notice notice--error" role="alert">
              <span className="notice__text">{folderError}</span>
            </p>
          )}
        </div>

        <form
          className="set-row set-row--block kit-add"
          onSubmit={(event) => {
            event.preventDefault();
            props.onAddGit();
          }}
        >
          <label className="set-row__label" htmlFor="kit-git-url">
            {t("kitsManage.gitLabel")}
          </label>
          <div className="kit-add__line">
            <input
              id="kit-git-url"
              type="text"
              className="set-input kit-add__url"
              value={gitUrl}
              placeholder="https://github.com/owner/repo"
              spellCheck={false}
              autoComplete="off"
              aria-invalid={gitError ? true : undefined}
              aria-describedby={gitError ? "kit-git-error" : "kit-git-hint"}
              onChange={(event) => props.onGitUrl(event.target.value)}
            />
            <button type="submit" className="btn" disabled={adding !== null || gitUrl.trim() === ""}>
              {adding === "git" ? t("kitsManage.adding") : t("kitsManage.addGit")}
            </button>
          </div>
          <span className="set-row__hint" id="kit-git-hint">
            {t("kitsManage.gitHint", { example: "https://github.com/owner/repo/tree/main/my-kit" })}
          </span>
          {gitError && (
            <p className="notice notice--error" id="kit-git-error" role="alert">
              <span className="notice__text">{gitError}</span>
            </p>
          )}
        </form>
        {flash && (
          <div className="set-row">
            <span className="kit-row__note" role="status">
              {flash}
            </span>
          </div>
        )}
      </Block>
    </div>
  );
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/** The page itself: reads the store of kits, and refreshes when it changes. */
export function KitsSettings() {
  const t = useT();
  const [kits, setKits] = useState<KitInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [flash, setFlash] = useState<string | null>(null);
  const [notes, setNotes] = useState<Record<string, UpdateNote>>({});
  const [removing, setRemoving] = useState<string | null>(null);
  const [adding, setAdding] = useState<"folder" | "git" | null>(null);
  const [folderError, setFolderError] = useState<string | null>(null);
  const [gitUrl, setGitUrl] = useState("");
  const [gitError, setGitError] = useState<string | null>(null);
  const { state } = useStore();
  const [managing, setManaging] = useState<string | null>(null);
  // Read live from the store, so the dialog sees the grants it just saved.
  const managed = state.workspaces.find((ws) => ws.id === managing) ?? null;
  const live = useRef(true);
  const kitsRef = useRef<KitInfo[] | null>(null);
  kitsRef.current = kits;

  const refresh = useCallback(async () => {
    try {
      const list = await kitsList();
      if (!live.current) return;
      setKits(list);
      setError(null);
    } catch (err) {
      if (live.current) setError(message(err));
    }
  }, []);

  useEffect(() => {
    live.current = true;
    void refresh();
    // The store changed — here, in another window, or a linked folder edited.
    const stop = onKitsChanged(() => void refresh());
    return () => {
      live.current = false;
      void stop.then((off) => off()).catch(() => undefined);
    };
  }, [refresh]);

  const setNote = (id: string, note: UpdateNote | null) =>
    setNotes(({ [id]: _was, ...rest }) => (note ? { ...rest, [id]: note } : rest));

  async function addFolder() {
    setFolderError(null);
    setFlash(null);
    let picked: string | string[] | null;
    try {
      picked = await open({ directory: true, multiple: false, title: t("kitsManage.addFolderTitle") });
    } catch (err) {
      setFolderError(message(err));
      return;
    }
    if (typeof picked !== "string") return;
    setAdding("folder");
    try {
      const added = await kitAddFolder(picked);
      setFlash(t("kitsManage.added", { name: kitName(added) }));
      await refresh();
    } catch (err) {
      setFolderError(message(err));
    } finally {
      setAdding(null);
    }
  }

  async function addGit() {
    setGitError(null);
    setFlash(null);
    setAdding("git");
    try {
      const added = await kitAddGit(gitUrl.trim());
      setGitUrl("");
      setFlash(t("kitsManage.added", { name: kitName(added) }));
      await refresh();
    } catch (err) {
      setGitError(message(err));
    } finally {
      setAdding(null);
    }
  }

  async function check(kit: KitInfo) {
    setNote(kit.id, { kind: "checking" });
    try {
      const update = await kitCheckUpdate(kit.id);
      if (!update) setNote(kit.id, { kind: "folder" });
      else if (update.available) setNote(kit.id, { kind: "available", current: update.current, latest: update.latest });
      else setNote(kit.id, { kind: "none", version: update.current });
    } catch (err) {
      setNote(kit.id, { kind: "error", detail: message(err) });
    }
  }

  /** A git kit moves to the fetched commit; a linked folder just re-reads its manifest. */
  async function update(kit: KitInfo) {
    setNote(kit.id, { kind: "updating" });
    try {
      const after = await kitUpdate(kit.id);
      const outcome = updateOutcome(kit, after);
      setNote(
        kit.id,
        kit.source.kind === "folder"
          ? { kind: "reloaded", version: after.installedVersion }
          : { kind: "done", from: outcome.from, to: outcome.to, newPermissions: outcome.newPermissions.length },
      );
      await refresh();
    } catch (err) {
      setNote(kit.id, { kind: "error", detail: message(err) });
    }
  }

  async function remove(kit: KitInfo) {
    setRemoving(null);
    try {
      await kitRemove(kit.id);
      setNote(kit.id, null);
      await refresh();
    } catch (err) {
      setNote(kit.id, { kind: "error", detail: message(err) });
    }
  }

  return (
    <>
    <KitsView
      workspaces={state.workspaces
        .filter((ws) => !ws.hq)
        .map((ws) => ({ id: ws.id, name: ws.name, kits: workspaceKits(ws).length }))}
      onManageWorkspace={setManaging}
      t={t}
      kits={kits}
      error={error}
      flash={flash}
      notes={notes}
      removing={removing}
      adding={adding}
      folderError={folderError}
      gitUrl={gitUrl}
      gitError={gitError}
      onGitUrl={(url) => {
        setGitUrl(url);
        setGitError(null);
      }}
      onAddFolder={() => void addFolder()}
      onAddGit={() => void addGit()}
      onCheck={(kit) => void check(kit)}
      onUpdate={(kit) => void update(kit)}
      onAskRemove={setRemoving}
      onRemove={(kit) => void remove(kit)}
    />
    {managed && <WorkspaceKitsDialog workspace={managed} onClose={() => setManaging(null)} />}
    </>
  );
}
