import { useState } from "react";
import type { RemoteState, Waiting, WaitingKind } from "../api";
import { AgentCounts, Link, PcChip, useNavigation } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { NotificationsView, pushPlace, useNotifications } from "../components/Notifications";
import { duration } from "../format";
import { usePolling } from "../hooks";
import { locale, t, tn } from "../i18n";
import { destinationOf, fleetOf, forYouGroups, waitingKey } from "../model";
import { useRemoteContext } from "../remote";

const HEADINGS: Record<WaitingKind, "forYou.panes" | "forYou.questions" | "forYou.lands"> = {
  pane: "forYou.panes",
  needsYou: "forYou.questions",
  readyToLand: "forYou.lands",
};

/**
 * For you: what waits for the person across every project, first and within
 * the thumb's reach; above it, how many agents are in each state; below, the
 * way into the history and the notifications switch.
 */
export function ForYou({ now }: { now: number }) {
  const { state, pc, connection, client, visit } = useRemoteContext();
  const push = useNotifications();
  const [news, setNews] = useState<number | null>(null);

  // How much happened since the last visit, for the way into the history.
  usePolling(
    async () => {
      const { entries } = await client.history(visit.since ?? Date.now() - 24 * 3_600_000);
      setNews(entries.length);
    },
    30_000,
    state !== null,
  );

  if (!state) return null;
  const groups = forYouGroups(state.waiting);
  const fleet = fleetOf(state.projects);
  const count = state.waiting.length;
  const place = pushPlace(push);

  return (
    <div className="page">
      <header>
        <div className="top">
          <h1 className="title">{t("forYou.title")}</h1>
          <PcChip pc={pc} connection={connection} />
        </div>
        <p className="lede">{count ? tn("forYou.count", count) : t("forYou.empty")}</p>
        <AgentCounts counts={fleet.counts} />
      </header>

      {place === "top" && <NotificationsView {...push} />}

      {groups.length === 0 ? (
        // The counts above already say who is at work.
        <p className="calm">{fleet.total ? t("forYou.emptyHint") : t("forYou.emptyQuiet")}</p>
      ) : (
        groups.map((group) => (
          <section className="section" key={group.kind}>
            <div className="section__head">
              <h2 className="section__title">{t(HEADINGS[group.kind])}</h2>
              <span className="section__count">{group.items.length}</span>
            </div>
            {group.kind === "readyToLand" && <p className="section__hint">{t("forYou.landHint")}</p>}
            <ul className="stack">
              {group.items.map((item) => (
                <WaitingCard key={waitingKey(item)} item={item} state={state} now={now} />
              ))}
            </ul>
          </section>
        ))
      )}

      <ul className="list section">
        <li>
          <Link to={{ name: "history" }} className="row">
            <Icon name="clock" />
            <span className="row__main">
              <span className="row__title">{t("forYou.history")}</span>
              {news !== null && (
                <span className="row__sub">{news ? tn("forYou.historyCount", news) : t("forYou.historyNone")}</span>
              )}
            </span>
            <Icon name="chevronRight" className="chevron" />
          </Link>
        </li>
        {place === "foot" && (
          <li>
            <NotificationsView {...push} />
          </li>
        )}
      </ul>
    </div>
  );
}

/** One thing that waits: where, what, for how long, and the one thing to do. */
function WaitingCard({ item, state, now }: { item: Waiting; state: RemoteState; now: number }) {
  const { navigate } = useNavigation();
  const destination = destinationOf(item, state);
  const go = () =>
    navigate(
      destination.to === "pane"
        ? { name: "pane", id: destination.pane }
        : destination.to === "delivery"
          ? { name: "delivery", task: destination.task }
          : { name: "project", id: destination.projectId },
    );
  // A pane's card is headed by its name: its section says what it waits for.
  const title = item.kind === "pane" ? (item.pane ?? "") : (item.title ?? "");
  const tone = item.kind === "readyToLand" ? "is-yours" : "is-waiting";

  return (
    <li className={`card ${tone}`} onClick={go}>
      <p className="card__project">{item.workspace}</p>
      <p className="card__title">{title}</p>
      <div className="card__foot">
        <span className="card__since">{t("forYou.since", { duration: duration(now - item.since, locale) })}</span>
        <button
          type="button"
          className={`btn ${destination.to === "project" ? "" : "btn--primary"}`}
          onClick={(event) => {
            event.stopPropagation();
            go();
          }}
        >
          {destination.to === "pane" ? t("forYou.answer") : destination.to === "delivery" ? t("forYou.seeDelivery") : t("forYou.seeProject")}
        </button>
      </div>
    </li>
  );
}
