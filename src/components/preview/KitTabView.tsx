import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../Icon";
import { StageMessage, Tool } from "./parts";
import { useT, type Translator } from "../../i18n";
import { getPaneActivity, useAgentsIn } from "../../lib/agentState";
import { modelRoots } from "../../lib/control";
import { gitWorktreeList } from "../../lib/git";
import { createKitHost, kitPanes, type KitHost, type KitHostDeps, type KitLoopbackAnswer } from "../../lib/kitApi";
import { askRootGrant, kitTabState, useInstalledKits } from "../../lib/kitTabs";
import { MAX_MODEL_BYTES, modelFormatOf } from "../../lib/modelFiles";
import { grantModelRoots } from "../../lib/modelHost";
import { reloadKitTab, showPreview, type OpenKitTab } from "../../lib/modelStore";
import { notify } from "../../lib/notify";
import { checkPreviewFile, previewInspect } from "../../lib/preview";
import { bridgeEnqueue } from "../../lib/ipc";
import { isMainWindow } from "../../lib/windows";
import { useStore } from "../../store";
import { kitLoopbackPort, type KitNamedPermission, type KitPermission } from "../../lib/kits";
import type { Workspace } from "../../types";

/** What a tab was granted, for its bar's hint. */
function permissionSaid(t: Translator, permission: KitPermission): string {
  const port = kitLoopbackPort(permission);
  // As text: `t` writes a number the way the language does, and a port has no
  // thousands.
  return port === null
    ? t(`kitsTab.permission.${permission as KitNamedPermission}`)
    : t("kitsTab.permission.loopback", { port: String(port) });
}

/**
 * A kit's tab (docs/kits.md): a page of the kit, served by the `hangar-kit`
 * scheme (src-tauri/src/kit_scheme.rs) into a frame sandboxed to scripts
 * alone — no `allow-same-origin`, no navigation of Hangar's window, no
 * popups, no forms — and answered by its own host (lib/kitApi), with what
 * the workspace granted the kit and the tab declares.
 *
 * ModelPanel keeps every kit tab of the window mounted, hidden when it is not
 * the one on screen, so a page keeps its state while another tab or
 * workspace is in front; a workspace put to sleep lets its pages go.
 *
 * A tab speaks to Hangar from the page it opened on and no other: the frame
 * loading a second document — a link followed, a page sent elsewhere — ends
 * its session, and the frame is taken down until the person reloads it.
 */
export function KitTabView({ entry, shown }: { entry: OpenKitTab; shown: boolean }) {
  const t = useT();
  const { state } = useStore();
  const installed = useInstalledKits();
  const ws = state.workspaces.find((candidate) => candidate.id === entry.workspaceId) ?? null;
  const tab = kitTabState(ws, installed, entry.kitId, entry.tabId);
  const kitName = tab.kind === "ready" ? tab.choice.kitName : entry.kitId;
  const permissions = tab.kind === "ready" ? tab.choice.permissions : [];
  const said = permissions.map((permission) => permissionSaid(t, permission)).join(", ");

  return (
    <div
      className="preview-file"
      hidden={!shown}
      // `.preview-file` is a flex box, which the attribute alone would not hide.
      style={shown ? undefined : { display: "none" }}
    >
      <div className="model-panel__bar" role="toolbar" aria-label={t("kitsTab.tools", { title: entry.title })}>
        <Tool label={t("kitsTab.reload")} onClick={() => reloadKitTab(entry.id)}>
          <Icon name="restart" size={14} />
        </Tool>
        <span className="model-panel__spacer" />
        <span
          className="preview-isolated"
          title={
            said
              ? t("kitsTab.isolatedHint", { kit: kitName, permissions: said })
              : t("kitsTab.isolatedNothing", { kit: kitName })
          }
        >
          <Icon name="lock" size={12} />
          {t("kitsTab.isolated")}
        </span>
      </div>
      {tab.kind === "ready" && ws ? (
        <KitFrame
          // A reload is a new frame, and a new host: nothing of the last page carries over.
          key={`${entry.revision}:${tab.url}`}
          entry={entry}
          workspace={ws}
          url={tab.url}
          kitName={kitName}
          permissions={permissions}
        />
      ) : (
        <div className="model-panel__stage preview-stage">
          {tab.kind === "loading" ? (
            <StageMessage icon="window" title={t("kitsTab.loading")} />
          ) : tab.kind === "disabled" ? (
            <StageMessage icon="lock" title={t("kitsTab.disabled")} detail={t("kitsTab.disabledHint")} />
          ) : tab.kind === "missing" ? (
            <StageMessage icon="alert" tone="warning" title={t("kitsTab.missing")} detail={t("kitsTab.missingHint")} />
          ) : (
            <StageMessage
              icon="alert"
              tone="danger"
              title={t("kitsTab.broken")}
              detail={tab.kind === "broken" ? tab.error : undefined}
            />
          )}
        </div>
      )}
    </div>
  );
}

/**
 * The preview's folders granted for the workspace — the same an agent's
 * preview_show grants — by the main window, which keeps the app's one list.
 */
async function grantRoots(ws: Workspace, workspaces: () => Workspace[]): Promise<void> {
  if (!isMainWindow) return askRootGrant(ws.id);
  const roots = await modelRoots(ws, { worktrees: gitWorktreeList });
  await grantModelRoots(
    ws.id,
    roots,
    workspaces().map((entry) => entry.id),
  );
}

/** What the kit API reaches of the app, for real. */
function liveDeps(workspaces: () => Workspace[]): KitHostDeps {
  return {
    readFile: (root, path) => invoke<string>("kit_file_read", { root, path }),
    resolveFile: (root, path) => invoke<string>("kit_workspace_path", { root, path }),
    showFile: async (ws, path, openedBy) => {
      await grantRoots(ws, workspaces);
      const file = checkPreviewFile(await previewInspect(path));
      if (file.kind === "model") {
        const format = modelFormatOf(file.path);
        if (!format) throw new Error(`${file.name} is not a model the preview opens`);
        if (file.size > MAX_MODEL_BYTES) throw new Error(`${file.name} is too large for the preview`);
        showPreview({
          kind: "model",
          path: file.path,
          format,
          workspaceId: ws.id,
          openedBy,
          modifiedMs: file.modifiedMs,
        });
        return;
      }
      showPreview({
        kind: file.kind,
        path: file.path,
        title: file.name,
        mime: file.mime,
        size: file.size,
        workspaceId: ws.id,
        openedBy,
        modifiedMs: file.modifiedMs,
      });
    },
    enqueue: bridgeEnqueue,
    activity: getPaneActivity,
    notify: (title, body) => notify(title, body),
    readData: (kit, path) => invoke<string>("kit_data_read", { kit, path }),
    writeData: (kit, path, text) => invoke<void>("kit_data_write", { kit, path, text }),
    loopbackFetch: (call) => invoke<KitLoopbackAnswer>("kit_loopback_fetch", { ...call }),
    now: () => Date.now(),
  };
}

function KitFrame({
  entry,
  workspace,
  url,
  kitName,
  permissions,
}: {
  entry: OpenKitTab;
  workspace: Workspace;
  url: string;
  kitName: string;
  permissions: KitPermission[];
}) {
  const t = useT();
  const { snapshot } = useStore();
  const frameRef = useRef<HTMLIFrameElement>(null);
  const hostRef = useRef<KitHost | null>(null);
  const loads = useRef(0);
  const [left, setLeft] = useState(false);

  // What the host reads at each call: the tab's grants and kit as they are now.
  const now = useRef({ permissions, kitName });
  now.current = { permissions, kitName };

  useEffect(() => {
    const host = createKitHost(
      () => frameRef.current?.contentWindow ?? null,
      {
        kitId: entry.kitId,
        kitName: () => now.current.kitName,
        workspace: () => snapshot().workspaces.find((ws) => ws.id === entry.workspaceId) ?? null,
        permissions: () => now.current.permissions,
      },
      liveDeps(() => snapshot().workspaces),
    );
    hostRef.current = host;
    const listener = (event: MessageEvent) => host.receive(event);
    window.addEventListener("message", listener);
    return () => {
      window.removeEventListener("message", listener);
      host.close();
      hostRef.current = null;
    };
    // One host per frame: the frame is remounted (its key) for anything else.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // `panes.changed`: the panes, their names, roles or states, changed.
  useAgentsIn([workspace.id]);
  const panes = JSON.stringify(kitPanes(workspace, getPaneActivity));
  const lastPanes = useRef(panes);
  useEffect(() => {
    if (panes === lastPanes.current) return;
    lastPanes.current = panes;
    hostRef.current?.emit("panes.changed", {});
  }, [panes]);

  const onLoad = () => {
    loads.current += 1;
    // The first document is the tab's page; any other is somewhere it went.
    if (loads.current > 1) {
      hostRef.current?.close();
      setLeft(true);
    }
  };

  if (left) {
    return (
      <div className="model-panel__stage preview-stage">
        <StageMessage
          icon="alert"
          tone="warning"
          title={t("kitsTab.left")}
          detail={t("kitsTab.leftHint")}
          action={t("kitsTab.reload")}
          onAction={() => reloadKitTab(entry.id)}
        />
      </div>
    );
  }

  return (
    <div className="model-panel__stage preview-stage">
      <iframe
        ref={frameRef}
        className="preview-frame preview-frame--page"
        src={url}
        title={t("kitsTab.tabHint", { title: entry.title, kit: kitName })}
        // Scripts, and nothing else: an opaque origin of its own, no way to
        // Hangar's window, no popups, no forms, no downloads, no dialogs.
        sandbox="allow-scripts"
        allow=""
        referrerPolicy="no-referrer"
        onLoad={onLoad}
      />
    </div>
  );
}
