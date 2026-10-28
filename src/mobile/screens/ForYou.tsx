import { useState } from "react";
import type { RemoteState, Waiting, WaitingKind } from "../api";
import { FleetStrip, Link, PcChip, useNavigation } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { Notifications } from "../components/Notifications";
import { isIos, isStandalone } from "../device";
import { duration } from "../format";
import { usePolling } from "../hooks";
import { locale, t, tn } from "../i18n";
import { destinationOf, fleetOf, forYouGroups, waitingKey } from "../model";
import { useRemoteContext } from "../remote";
import { load, save } from "../storage";

const HEADINGS: Record<WaitingKind, "forYou.panes" | "forYou.questions" | "forYou.lands"> = {
  pane: "forYou.panes",
  needsYou: "forYou.questions",
  readyToLand: "forYou.lands",
};

/** For you: what waits for the person across every project, and nothing else. */
export function ForYou({ now }: { now: number }) {
  const { state, pc, connection, client, visit } = useRemoteContext();
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

  return (
    <div className="page">
      <header>
        <div className="top">
          <h1 className="title">{t("forYou.title")}</h1>
          <PcChip pc={pc} connection={connection} />
        </div>
        <p className="lede">{count ? tn("forYou.count", count) : t("forYou.empty")}</p>
        <FleetStrip fleet={fleet} />
      </header>

      <InstallNotice />

      <Notifications />

      <Link to={{ name: "history" }} className="row row--history">
        <Icon name="clock" />
        <span className="row__main">
          <span className="row__title">{t("forYou.history")}</span>
          {news !== null && (
            <span className="row__sub">{news ? tn("forYou.historyCount", news) : t("forYou.historyNone")}</span>
          )}
        </span>
        <Icon name="chevronRight" className="chevron" />
      </Link>

      {groups.length === 0 ? (
        <p className="calm">
          {fleet.counts.working ? tn("forYou.emptyWorking", fleet.counts.working) : t("forYou.emptyQuiet")}
        </p>
      ) : (
        groups.map((group) => (
          <section className="section" key={group.kind}>
            <div className="section__head">
              <h2 className="section__title">{t(HEADINGS[group.kind])}</h2>
              <span className="section__count">{group.items.length}</span>
            </div>
            <ul className="stack">
              {group.items.map((item) => (
                <WaitingCard key={waitingKey(item)} item={item} state={state} now={now} />
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  );
}

/** One thing that waits: where, what, for how long, and the one thing to do. */
function WaitingCard({ item, state, now }: { item: Waiting; state: RemoteState; now: number }) {
  const { navigate } = useNavigation();
  const destination = destinationOf(item, state);
  const go = () =>
    navigate(destination.to === "pane" ? { name: "pane", id: destination.pane } : { name: "project", id: destination.projectId });
  const title = item.kind === "pane" ? t("forYou.paneWaits", { pane: item.pane ?? "" }) : (item.title ?? "");
  const tone = item.kind === "readyToLand" ? "is-yours" : "is-waiting";

  return (
    <li className={`card ${tone}`} onClick={go}>
      <div className="card__meta">
        <span className="card__project">{item.workspace}</span>
        <span className="card__since">{t("forYou.since", { duration: duration(now - item.since, locale) })}</span>
      </div>
      <p className="card__title">{title}</p>
      <div className="card__foot">
        {item.kind === "readyToLand" && <span className="card__note">{t("forYou.landOnPc")}</span>}
        <button
          type="button"
          className={`btn ${destination.to === "pane" ? "btn--primary" : ""}`}
          onClick={(event) => {
            event.stopPropagation();
            go();
          }}
        >
          {destination.to === "pane" ? t("forYou.answer") : t("forYou.seeProject")}
        </button>
      </div>
    </li>
  );
}

/** On an iPhone in Safari: how to put the app on the Home Screen, once. */
function InstallNotice() {
  const [dismissed, setDismissed] = useState(() => load("installNoticeDismissed") === "1");
  if (dismissed || !isIos(navigator.userAgent, navigator.maxTouchPoints) || isStandalone()) return null;
  return (
    <aside className="notice">
      <Icon name="share" />
      <p className="notice__title">{t("install.title")}</p>
      <p className="notice__body">{t("install.body")}</p>
      <button
        type="button"
        className="btn btn--quiet"
        onClick={() => {
          save("installNoticeDismissed", "1");
          setDismissed(true);
        }}
      >
        {t("install.dismiss")}
      </button>
    </aside>
  );
}
