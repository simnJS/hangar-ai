import { useCallback, useMemo, useState } from "react";
import { Banner, NavigationContext, TabBar } from "./components/Chrome";
import { Logo } from "./components/Icon";
import { useNotificationOpens, useNow, useRoute, useVisibleViewport } from "./hooks";
import { t } from "./i18n";
import { unseenShares } from "./model";
import { RemoteContext, useRemote, type Boot } from "./remote";
import { routeHash, tabOf, type Route } from "./route";
import { ManagerScreen, PaneScreen } from "./screens/Pane";
import { ForYou } from "./screens/ForYou";
import { History } from "./screens/History";
import { Pairing } from "./screens/Pairing";
import { ProjectScreen, Projects } from "./screens/Projects";
import { Shares } from "./screens/Shares";
import { Stats } from "./screens/Stats";
import { loadNumber, save } from "./storage";

export function App({ boot }: { boot: Boot }) {
  const remote = useRemote(boot);
  const { route, navigate, back } = useRoute();
  const navigation = useMemo(() => ({ navigate, back }), [navigate, back]);
  const now = useNow();
  useVisibleViewport();
  useNotificationOpens();

  const [sharesSeen, setSharesSeen] = useState(() => loadNumber("sharesSeen"));
  const onSharesSeen = useCallback((at: number) => {
    setSharesSeen(at);
    save("sharesSeen", String(at));
  }, []);

  const { paired, state, connection } = remote;
  const banner = connection.unreachable && paired !== null;
  // A console scrolls its own text, under its header and over its composer.
  const fixed = paired !== false && state !== null && (route.name === "pane" || (route.name === "manager" && state.manager !== null));

  return (
    <RemoteContext.Provider value={remote}>
      <NavigationContext.Provider value={navigation}>
        <div className={`app ${banner ? "app--banner" : ""}`}>
          {banner && <Banner connection={connection} now={now} onRetry={() => void remote.refresh().catch(() => {})} />}
          <main key={paired === false ? "pairing" : routeHash(route)} className={`app__main ${fixed ? "app__main--fixed" : ""}`}>
            {paired === false ? (
              <Pairing />
            ) : state === null ? (
              <div className="splash" role="status">
                <Logo size={36} />
                <span>{t("forYou.loading")}</span>
              </div>
            ) : (
              <Screen route={route} now={now} onSharesSeen={onSharesSeen} />
            )}
          </main>
          {paired !== false && state !== null && route.name !== "pane" && (
            <TabBar
              current={tabOf(route)}
              badges={{
                forYou: { count: state.waiting.length, tone: "waiting" },
                shares: route.name === "shares" ? undefined : { count: unseenShares(remote.shares, sharesSeen), tone: "accent" },
              }}
            />
          )}
        </div>
      </NavigationContext.Provider>
    </RemoteContext.Provider>
  );
}

function Screen({ route, now, onSharesSeen }: { route: Route; now: number; onSharesSeen: (at: number) => void }) {
  switch (route.name) {
    case "forYou":
      return <ForYou now={now} />;
    case "projects":
      return <Projects />;
    case "project":
      return <ProjectScreen id={route.id} />;
    case "pane":
      return <PaneScreen id={route.id} />;
    case "shares":
      return <Shares now={now} onSeen={onSharesSeen} />;
    case "manager":
      return <ManagerScreen />;
    case "history":
      return <History now={now} />;
    case "stats":
      return <Stats now={now} />;
  }
}
