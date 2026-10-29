import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { ApiError, createClient, type App, type Client, type Connection, type RemoteState, type Share } from "./api";
import { deviceName } from "./device";
import { usePolling } from "./hooks";
import { mergeShares } from "./model";
import { keepMockApart, load, loadJson, loadNumber, save } from "./storage";
import { parseVisit, touchVisit, type Visit } from "./visit";
import { failWake, settleWakes, startWake, type Wakes } from "./wake";

/**
 * The app's link to the PC: the client, whether this phone is paired, the
 * PC's state polled every 2 s, what agents shared, the projects being woken,
 * the apps the agents build while a screen shows them, and since when the PC
 * has not answered. The last state and the time of the last answer are kept
 * on the phone, so the app opens on what it last knew even with the PC away.
 */

/** The apps listening on the PC are asked for this often: each asks it to knock on every worker's ports. */
const APPS_POLL_MS = 10_000;

export interface Boot {
  client: Client;
  /** A pairing code from the QR code's link (`?code=`). */
  code: string | null;
}

/** Reads the address: the fake PC or the real one, and a code to pair with. */
export async function bootstrap(): Promise<Boot> {
  const params = new URLSearchParams(location.search);
  const code = params.get("code");
  if (!params.has("mock") && !import.meta.env.DEV) {
    return { client: createClient({ lastOk: loadNumber("lastOk"), ...answers() }), code };
  }
  const { createMock, scenarioOf } = await import("./mock");
  const scenario = scenarioOf(params.get("mock"));
  keepMockApart(scenario);
  const fake = createMock(scenario);
  // Offline, the fake PC never answers: the phone remembers what it said last.
  if (fake.seed) {
    save("lastOk", String(fake.seed.lastOk));
    save("state", JSON.stringify(fake.seed.state));
    save("paired", "1");
  }
  const client = createClient({ fetch: fake.fetch, fileUrl: fake.fileUrl, lastOk: loadNumber("lastOk"), ...answers() });
  return { client, code };
}

/** The client's callbacks, which the app subscribes to once it runs. */
const listeners = { unpaired: () => {}, answer: (_at: number) => {} };
const answers = () => ({
  onUnpaired: () => listeners.unpaired(),
  onAnswer: (at: number) => listeners.answer(at),
});

export interface Remote {
  client: Client;
  /** null until the PC (or what the phone remembers) says. */
  paired: boolean | null;
  /** The phone was paired, and the PC no longer knows it. */
  lostPairing: boolean;
  /** The pairing code of the QR link, tried and refused. */
  codeRefused: ApiError | null;
  pc: string | null;
  state: RemoteState | null;
  connection: Connection;
  shares: Share[];
  visit: Visit;
  /** Projects asked to wake, until the PC shows them awake (wake.ts). */
  wakes: Wakes;
  /**
   * The dev servers the phone may open, read while a screen shows them;
   * null before the PC has said, or when it cannot (an older Hangar).
   */
  apps: App[] | null;
  /** Wakes a project asleep; a failure lands in `wakes`. */
  wake: (projectId: string) => Promise<void>;
  /** Something sent to a project asleep wakes it on the PC: it says so here too. */
  noteWaking: (projectId: string) => void;
  pair: (code: string) => Promise<void>;
  refresh: () => Promise<void>;
}

export const RemoteContext = createContext<Remote | null>(null);

export function useRemoteContext(): Remote {
  const remote = useContext(RemoteContext);
  if (!remote) throw new Error("useRemoteContext outside RemoteContext");
  return remote;
}

/** `watchApps`: a screen shows the apps the agents build, which are then read every 10 s. */
export function useRemote(boot: Boot, { watchApps = false }: { watchApps?: boolean } = {}): Remote {
  const { client } = boot;
  const [paired, setPaired] = useState<boolean | null>(null);
  const [lostPairing, setLostPairing] = useState(false);
  const [codeRefused, setCodeRefused] = useState<ApiError | null>(null);
  const [pc, setPc] = useState(() => load("pc"));
  const [state, setState] = useState<RemoteState | null>(() => loadJson<RemoteState>("state"));
  const [shares, setShares] = useState<Share[]>(() => loadJson<Share[]>("shares") ?? []);
  const [connection, setConnection] = useState<Connection>(() => client.connection());
  const [visit, setVisit] = useState<Visit>(() => touchVisit(parseVisit(load("visit")), Date.now()));
  const [wakes, setWakes] = useState<Wakes>({});
  const [apps, setApps] = useState<App[] | null>(null);
  const sharesRef = useRef(shares);
  sharesRef.current = shares;

  useEffect(() => {
    listeners.answer = (at) => save("lastOk", String(at));
    listeners.unpaired = () => {
      setLostPairing(load("paired") === "1");
      save("paired", null);
      setPaired(false);
    };
  }, [client]);

  const welcome = useCallback(async () => {
    try {
      const health = await client.health();
      setPc(health.pc);
      save("pc", health.pc);
      save("paired", health.paired ? "1" : null);
      setPaired(health.paired);
    } catch {
      // The PC is away: the phone carries on with what it remembers.
      setPaired(load("paired") === "1");
    }
  }, [client]);

  const pair = useCallback(
    async (code: string) => {
      await client.pair(code, deviceName(navigator.userAgent, navigator.maxTouchPoints));
      save("paired", "1");
      setLostPairing(false);
      setCodeRefused(null);
      await welcome();
    },
    [client, welcome],
  );

  // First the QR code's pairing code, if the app was opened by one; then
  // whether the PC knows this phone. Once: a code works a single time.
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void (async () => {
      if (boot.code) {
        try {
          await client.pair(boot.code, deviceName(navigator.userAgent, navigator.maxTouchPoints));
          save("paired", "1");
        } catch (error) {
          if (error instanceof ApiError) setCodeRefused(error);
        }
        const params = new URLSearchParams(location.search);
        params.delete("code");
        const query = params.toString();
        history.replaceState(history.state, "", `${location.pathname}${query ? `?${query}` : ""}${location.hash}`);
      }
      await welcome();
    })();
  }, [boot.code, client, welcome]);

  const lastKept = useRef(0);
  const refresh = useCallback(async () => {
    try {
      const next = await client.state();
      setState(next);
      setWakes((current) => settleWakes(current, next.projects));
      if (Date.now() - lastKept.current >= 10_000) {
        lastKept.current = Date.now();
        save("state", JSON.stringify(next));
      }
    } finally {
      setConnection(client.connection());
    }
  }, [client]);

  usePolling(refresh, 2_000, paired === true);

  const noteWaking = useCallback((projectId: string) => {
    setWakes((current) => startWake(current, projectId, Date.now()));
  }, []);

  const wake = useCallback(
    async (projectId: string) => {
      noteWaking(projectId);
      try {
        await client.wake(projectId);
      } catch (error) {
        setWakes((current) => failWake(current, projectId, error, Date.now()));
        return;
      }
      // Awake on the PC within a render: the next state says so.
      await refresh().catch(() => {});
    },
    [client, noteWaking, refresh],
  );

  // An older Hangar has no `apps`: nothing to offer, and nothing to say about it.
  usePolling(
    async () => {
      try {
        setApps((await client.apps()).apps);
      } catch (error) {
        if (error instanceof ApiError && error.kind === "refused") setApps(null);
      }
    },
    APPS_POLL_MS,
    paired === true && watchApps,
  );

  // The banner needs to know when the PC went quiet, polls or not.
  usePolling(
    async () => {
      setConnection(client.connection());
    },
    1_000,
    paired !== null,
  );

  // What agents shared: newest first, polled for what is new since the last.
  usePolling(
    async () => {
      const newest = sharesRef.current[0]?.at;
      const { shares: incoming } = await client.shares(newest);
      if (!incoming.length) return;
      const merged = mergeShares(sharesRef.current, incoming).slice(0, 200);
      setShares(merged);
      save("shares", JSON.stringify(merged));
    },
    15_000,
    paired === true,
  );

  // A visit lasts as long as the app is in use; away for a while, the next is new.
  usePolling(
    async () => {
      setVisit((visit) => {
        const next = touchVisit(visit, Date.now());
        save("visit", JSON.stringify(next));
        return next;
      });
    },
    30_000,
  );

  return { client, paired, lostPairing, codeRefused, pc, state, connection, shares, visit, wakes, apps, wake, noteWaking, pair, refresh };
}
