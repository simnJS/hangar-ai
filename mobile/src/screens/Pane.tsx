import { workerApps } from "../apps";
import { isClaude } from "../chat";
import { AppButton } from "../components/Apps";
import { EmptyState, PcChip } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { WakeUp } from "../components/Wake";
import { t } from "../i18n";
import { locatePane } from "../model";
import { useRemoteContext } from "../remote";
import { ChatPane } from "./Chat";
import { Console } from "./Console";

/**
 * The two screens of one pane: a pane of a project, and the project
 * manager's tab. Claude Code opens on its chat, its terminal behind a
 * button; any other agent, or a shell, on its terminal.
 */

/** A pane of a project, opened from the project or from a card. */
export function PaneScreen({ id }: { id: string }) {
  const { state, client, noteWaking, apps } = useRemoteContext();
  const located = state ? locatePane(state, id) : null;
  // A worker's dev server, beside its name.
  const running = located?.pane.role === "worker" ? workerApps(apps, located.project.id, located.pane.name) : [];
  const paneId = located?.pane.id ?? id;
  const name = located?.pane.name ?? id;
  // Claude Code gets its chat; another agent or a shell, its terminal. A pane
  // not found yet tries the chat, which says when there is none.
  const Screen = located && !isClaude(located.pane.agent) ? Console : ChatPane;
  return (
    <Screen
      key={paneId}
      paneId={paneId}
      name={name}
      where={located?.project.name ?? ""}
      activity={located?.pane.activity ?? null}
      send={async (text) => {
        const sent = await client.send({ pane: paneId, text, submit: true });
        // Sending to a project asleep wakes it on the PC.
        if (located?.project.asleep) noteWaking(located.project.id);
        return sent;
      }}
      placeholder={t("composer.placeholder", { name })}
      workspaceId={located?.project.id}
      back={located ? { name: "project", id: located.project.id } : { name: "projects" }}
      notice={located ? <WakeUp project={located.project} /> : undefined}
      app={located && running.length > 0 ? <AppButton apps={running} projectId={located.project.id} variant="head" /> : undefined}
    />
  );
}

/** The project manager's tab: its pane, and a composer that talks to it. */
export function ManagerScreen() {
  const { state, client, pc, connection } = useRemoteContext();
  if (!state) return null;
  const manager = state.manager;
  if (!manager) {
    return (
      <div className="page">
        <div className="top">
          <h1 className="title">{t("manager.title")}</h1>
          <PcChip pc={pc} connection={connection} />
        </div>
        <EmptyState title={t("manager.none")} text={t("manager.noneHint")} mark={<Icon name="orchestrator" size={40} />} />
      </div>
    );
  }
  return (
    <ChatPane
      key={manager.paneId}
      paneId={manager.paneId}
      name={manager.name}
      where={t("manager.title")}
      activity={manager.activity}
      send={(text) => client.manager(text)}
      placeholder={t("manager.placeholder")}
      tabbed
    />
  );
}
