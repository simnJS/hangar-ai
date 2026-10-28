import { useEffect, useState } from "react";
import type { HistoryEntry } from "../api";
import { BackBar, EmptyState } from "../components/Chrome";
import { clock, moment } from "../format";
import { errorText } from "../errors";
import { locale, t } from "../i18n";
import { byDay } from "../model";
import { useRemoteContext } from "../remote";

const DAY = 24 * 3_600_000;

/** Since your last visit: what happened in every project while the phone was away. */
export function History({ now }: { now: number }) {
  const { client, visit } = useRemoteContext();
  // Since the last visit; on a first one, or when asked, the last day.
  const [dayFrom, setDayFrom] = useState<number | null>(() => (visit.since === null ? Date.now() - DAY : null));
  const since = dayFrom ?? visit.since ?? now - DAY;
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    setEntries(null);
    client.history(since).then(
      (answer) => current && (setEntries(answer.entries), setError(null)),
      (failure) => current && setError(errorText(failure)),
    );
    return () => {
      current = false;
    };
  }, [client, since]);

  return (
    <div className="page">
      <BackBar to={{ name: "forYou" }} label={t("forYou.title")} />
      <h1 className="title">{t("history.title")}</h1>
      <p className="subtitle">{dayFrom !== null ? t("history.lastDay") : t("history.since", { time: moment(since, now, locale) })}</p>

      {error ? (
        <EmptyState title={error} />
      ) : entries === null ? (
        <p className="calm">{t("history.loading")}</p>
      ) : entries.length === 0 ? (
        <EmptyState title={t("history.empty")} text={t("history.emptyHint")}>
          {dayFrom === null && (
            <button type="button" className="btn" onClick={() => setDayFrom(Date.now() - DAY)}>
              {t("history.showDay")}
            </button>
          )}
        </EmptyState>
      ) : (
        byDay(entries, now, locale).map(({ day, entries }) => (
          <section className="section" key={day}>
            <div className="section__head">
              <h2 className="section__title">{day}</h2>
            </div>
            <ul className="list events">
              {entries.map((entry, index) => (
                <li key={`${entry.at}-${index}`} className="event">
                  <span className="event__time">{clock(entry.at, locale)}</span>
                  <p className="event__text">
                    {entry.text}
                    <span className="event__where">{entry.workspace}</span>
                  </p>
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  );
}
