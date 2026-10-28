import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { ApiError, createClient, type Client, type Connection, type RemoteState, type Share } from "./api";
import { deviceName } from "./device";
import { usePolling } from "./hooks";
import { mergeShares } from "./model";
import { keepMockApart, load, loadJson, loadNumber, save } from "./storage";
import { parseVisit, touchVisit, type Visit } from "./visit";

/**
 * The app's link to the PC: the client, whether this phone is paired, the
 * PC's state polled every 2 s, what agents shared, and since when the PC has
 * not answered. The last state and the time of the last answer are kept on
 * the phone, so the app opens on what it last knew even with the PC away.
 */

export interface Boot {
  client: Client;
  mock: boolean;
  /** A pairing code from the QR code's link (`?code=`). */
  code: string | null;
  /** The fake PC's offline scenario: what it last said, and when. */
  seed: { lastOk: number; state: RemoteState } | null;
}

/** Reads the address: the fake PC or the real one, and a code to pair with. */
export async function bootstrap(): Promise<Boot> {
  const params = new URLSearchParams(location.search);
  const mock = params.has("mock") || import.meta.env.DEV;
  const code = params.get("code");
  if (!mock) {
    return { client: createClient({ lastOk: loadNumber("lastOk"), ...answers() }), mock, code, seed: null };
  }
  keepMockApart();
  const { createMock, scenarioOf } = await import("./mock");
  const fake = createMock(scenarioOf(params.get("mock")));
  if (fake.seed) {
    save("lastOk", String(fake.seed.lastOk));
    save("state", JSON.stringify(fake.seed.state));
    save("paired", "1");
  }
  const client = createClient({ fetch: fake.fetch, fileUrl: fake.fileUrl, lastOk: loadNumber("lastOk"), ...answers() });
  return { client, mock, code, seed: fake.seed };
}

/** The client's callbacks, which the app subscribes to once it runs. */
const listeners = { unpaired: () => {}, answer: (_at: number) => {} };
const answers = () => ({
  onUnpaired: () => listeners.unpaired(),
  onAnswer: (at: number) => listeners.answer(at),
});

/** Writes at most every `ms`, so a 2 s poll does not rewrite storage each time. */
function throttled(ms: number, write: () => void) {
  let last = 0;
  return () => {
    const now = Date.now();
    if (now - last < ms) return;
    last = now;
    write();
  };
}

export interface Remote {
  client: Client;
  mock: boolean;
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
  pair: (code: string) => Promise<void>;
  refresh: () => Promise<void>;
}

export const RemoteContext = createContext<Remote | null>(null);

export function useRemoteContext(): Remote {
  const remote = useContext(RemoteContext);
  if (!remote) throw new Error("useRemoteContext outside RemoteContext");
  return remote;
}

export function useRemote(boot: Boot): Remote {
  const { client } = boot;
  const [paired, setPaired] = useState<boolean | null>(null);
  const [lostPairing, setLostPairing] = useState(false);
  const [codeRefused, setCodeRefused] = useState<ApiError | null>(null);
  const [pc, setPc] = useState(() => load("pc"));
  const [state, setState] = useState<RemoteState | null>(() => loadJson<RemoteState>("state"));
  const [shares, setShares] = useState<Share[]>(() => loadJson<Share[]>("shares") ?? []);
  const [connection, setConnection] = useState<Connection>(() => client.connection());
  const [visit, setVisit] = useState<Visit>(() => touchVisit(parseVisit(load("visit")), Date.now()));
  const sharesRef = useRef(shares);
  sharesRef.current = shares;

  useEffect(() => {
    listeners.answer = throttled(10_000, () => save("lastOk", String(client.connection().lastOk)));
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
      if (Date.now() - lastKept.current >= 10_000) {
        lastKept.current = Date.now();
        save("state", JSON.stringify(next));
      }
    } finally {
      setConnection(client.connection());
    }
  }, [client]);

  usePolling(refresh, 2_000, paired === true);

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

  return { client, mock: boot.mock, paired, lostPairing, codeRefused, pc, state, connection, shares, visit, pair, refresh };
}
