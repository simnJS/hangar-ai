import { createContext, useContext, type MouseEvent, type ReactNode } from "react";
import type { Activity, Connection } from "../api";
import { moment } from "../format";
import { locale, t, tn } from "../i18n";
import type { Fleet } from "../model";
import { routeHash, type Route, type Tab } from "../route";
import { Icon, type IconName } from "./Icon";

/** Moving between screens, for any component that leads somewhere. */
export interface Navigation {
  navigate: (route: Route, options?: { replace?: boolean }) => void;
  back: (fallback: Route) => void;
}

export const NavigationContext = createContext<Navigation>({ navigate: () => {}, back: () => {} });

export const useNavigation = () => useContext(NavigationContext);

/** A link to a screen: a real address, followed without reloading. */
export function Link({ to, className, children, label }: { to: Route; className?: string; children: ReactNode; label?: string }) {
  const { navigate } = useNavigation();
  const follow = (event: MouseEvent) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey) return;
    event.preventDefault();
    navigate(to);
  };
  return (
    <a href={routeHash(to)} className={className} onClick={follow} aria-label={label}>
      {children}
    </a>
  );
}

export function BackBar({ to, label }: { to: Route; label: string }) {
  const { back } = useNavigation();
  return (
    <div className="backbar">
      <button type="button" className="back" onClick={() => back(to)}>
        <Icon name="chevronLeft" size={22} />
        {label}
      </button>
    </div>
  );
}

const STATE_CLASS: Record<Activity, string> = {
  working: "is-working",
  waiting: "is-waiting",
  yours: "is-yours",
  idle: "is-idle",
};

export const stateClass = (activity: Activity | null) => (activity ? STATE_CLASS[activity] : "is-none");

export function activityText(activity: Activity | null): string {
  return activity ? t(`activity.${activity}`) : t("activity.none");
}

/** An agent's state: its colour and its word. */
export function StateLabel({ activity }: { activity: Activity | null }) {
  return (
    <span className={`state ${stateClass(activity)}`}>
      <span className="dot" />
      {activityText(activity)}
    </span>
  );
}

/** The PC this phone talks to, and whether it answers. */
export function PcChip({ pc, connection }: { pc: string | null; connection: Connection }) {
  if (!pc) return null;
  const away = connection.unreachable;
  return (
    <span className={`pc ${away ? "pc--away" : ""}`} title={t(away ? "pc.unreachable" : "pc.reachable", { pc })}>
      <span className="dot" />
      <span className="pc__name">{pc}</span>
      <span className="sr-only">{t(away ? "pc.unreachable" : "pc.reachable", { pc })}</span>
    </span>
  );
}

export function Banner({ connection, now, onRetry }: { connection: Connection; now: number; onRetry: () => void }) {
  const { lastOk } = connection;
  return (
    <div className="banner" role="status">
      <Icon name="alert" />
      <span className="banner__text">
        <span className="banner__title">
          {lastOk === null ? t("banner.unreachable") : t("banner.unreachableSince", { time: moment(lastOk, now, locale) })}
        </span>
        <span className="banner__hint">{t("banner.hint")}</span>
      </span>
      <button type="button" className="btn" onClick={onRetry}>
        {t("error.retry")}
      </button>
    </div>
  );
}

/** Every agent as a tile in its state's colour, each project's held together. */
export function FleetStrip({ fleet }: { fleet: Fleet }) {
  if (!fleet.total) return null;
  const states: Activity[] = ["working", "waiting", "yours", "idle"];
  return (
    <div className="fleet">
      <div className="fleet__tiles" role="img" aria-label={t("fleet.label")}>
        {fleet.projects.map((project) => (
          <span className="fleet__project" key={project.id}>
            <span className="fleet__panes">
              {project.tiles.map((tile) => (
                <span key={tile.id} className={`tile ${stateClass(tile.activity)}`} />
              ))}
            </span>
            <span className="fleet__name">{project.name}</span>
          </span>
        ))}
      </div>
      <ul className="legend">
        {states
          .filter((state) => fleet.counts[state] > 0)
          .map((state) => (
            <li key={state} className={stateClass(state)}>
              <span className="dot" />
              {tn(`fleet.${state}`, fleet.counts[state])}
            </li>
          ))}
      </ul>
    </div>
  );
}

/** A project's agents in small, beside its name. */
export function Mosaic({ activities }: { activities: (Activity | null)[] }) {
  const tiles = activities.filter((activity): activity is Activity => activity !== null);
  if (!tiles.length) return null;
  return (
    <span className="mosaic" aria-hidden="true">
      {tiles.map((activity, index) => (
        <span key={index} className={`tile ${stateClass(activity)}`} />
      ))}
    </span>
  );
}

const TABS: { tab: Tab; route: Route; icon: IconName; label: () => string }[] = [
  { tab: "forYou", route: { name: "forYou" }, icon: "tray", label: () => t("tab.forYou") },
  { tab: "projects", route: { name: "projects" }, icon: "panes", label: () => t("tab.projects") },
  { tab: "shares", route: { name: "shares" }, icon: "image", label: () => t("tab.shares") },
  { tab: "manager", route: { name: "manager" }, icon: "orchestrator", label: () => t("tab.manager") },
];

export function TabBar({ current, badges }: { current: Tab; badges: Partial<Record<Tab, { count: number; tone: "waiting" | "accent" }>> }) {
  const { navigate } = useNavigation();
  return (
    <nav className="tabs" aria-label={t("tab.label")}>
      {TABS.map(({ tab, route, icon, label }) => {
        const badge = badges[tab];
        return (
          <a
            key={tab}
            href={routeHash(route)}
            className="tab"
            aria-current={tab === current ? "page" : undefined}
            onClick={(event) => {
              event.preventDefault();
              // A tab tapped again goes back to its top screen.
              navigate(route, { replace: tab === current });
            }}
          >
            <Icon name={icon} />
            <span className="tab__label">{label()}</span>
            {badge && badge.count > 0 && (
              <span className={`badge ${badge.tone === "accent" ? "badge--accent" : ""}`}>
                {badge.count > 99 ? "99+" : badge.count}
              </span>
            )}
          </a>
        );
      })}
    </nav>
  );
}

/** An empty screen: what it will hold, and how it gets there. */
export function EmptyState({ title, text, mark, children }: { title: string; text?: string; mark?: ReactNode; children?: ReactNode }) {
  return (
    <div className="empty">
      {mark && <div className="empty__mark">{mark}</div>}
      <h2 className="empty__title">{title}</h2>
      {text && <p className="empty__text">{text}</p>}
      {children}
    </div>
  );
}
