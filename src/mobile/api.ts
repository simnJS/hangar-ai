/**
 * The phone's side of the contract with the PC's mobile server: the shapes it
 * answers with, and one client that makes every call.
 *
 * Same origin, a cookie session set by pairing, and every POST or DELETE
 * carries `X-Hangar: 1`. A 401 anywhere means the phone has to pair again; a 503
 * `pc-busy` means the window did not answer in time and the call can be made
 * again. The client also keeps the time of the last call the PC answered, so
 * the app can say how long it has been unreachable.
 *
 * Task M6 switches these types to the shared src/remote/types.ts.
 */

/* ── The contract's shapes ─────────────────────────────────────── */

/** What an agent pane is doing; null when no agent the PC knows of runs in it. */
export type Activity = "working" | "waiting" | "yours" | "idle";

export interface Health {
  ok: boolean;
  /** The PC's name. */
  pc: string;
  version: string;
  /** Whether this phone's cookie is a paired device's. */
  paired: boolean;
}

export interface Device {
  id: string;
  name: string;
}

export interface Manager {
  paneId: string;
  name: string;
  activity: Activity | null;
}

export type WaitingKind = "pane" | "needsYou" | "readyToLand";

/** Something that waits for the person, in one project. */
export interface Waiting {
  kind: WaitingKind;
  workspaceId: string;
  workspace: string;
  paneId?: string;
  pane?: string;
  taskId?: string;
  title?: string;
  /** Since when, in ms. */
  since: number;
}

export interface Pane {
  id: string;
  name: string;
  /** The agent running in it: claude, codex… */
  agent: string;
  /** orchestrator, worker, or nothing in particular. */
  role: string | null;
  activity: Activity | null;
}

export interface Task {
  id: string;
  title: string;
}

export interface Project {
  id: string;
  name: string;
  /** Its panes are up in a window on the PC. */
  open: boolean;
  asleep: boolean;
  git: boolean | null;
  orchestrator: { name: string; activity: Activity | null } | null;
  panes: Pane[];
  board: { todo: number; doing: number; review: number; done: number };
  /** The "Status:" line of the orchestrator's plan; null without a plan. */
  planStatus: string | null;
  needsYou: Task[];
  readyToLand: Task[];
}

export interface RemoteState {
  at: number;
  manager: Manager | null;
  waiting: Waiting[];
  projects: Project[];
}

export interface Screen {
  paneId: string;
  pane: string;
  workspace: string;
  agent: string;
  activity: Activity | null;
  /** The pane's last lines, as text. */
  text: string;
}

export type Key = "enter" | "escape" | "tab" | "up" | "down" | "ctrl+c";

export interface SendArgs {
  /** A pane's id or name. */
  pane: string;
  text?: string;
  keys?: Key[];
  /** Press Enter after the text. */
  submit?: boolean;
}

export interface HistoryEntry {
  at: number;
  workspace: string;
  kind: string;
  text: string;
}

/** A file an agent shared with the phone. */
export interface Share {
  id: string;
  at: number;
  workspace: string;
  pane: string;
  name: string;
  mime: string;
  size: number;
  caption: string | null;
}

/* ── Errors ────────────────────────────────────────────────────── */

/**
 * - unpaired: the PC does not know this phone (any 401): pair again.
 * - busy: the PC answered, its window did not in time (503): try again.
 * - offline: no answer at all, or none in time.
 * - refused: the PC answered no, with its reason in `code`.
 */
export type ApiErrorKind = "unpaired" | "busy" | "offline" | "refused";

export class ApiError extends Error {
  constructor(
    readonly kind: ApiErrorKind,
    /** The server's `error`, or `timeout`, `network`, `too-big`. */
    readonly code: string,
    readonly status?: number,
  ) {
    super(code);
    this.name = "ApiError";
  }
}

/* ── The client ────────────────────────────────────────────────── */

/** How long a call to the window may take: the server gives it 10 s. */
export const CALL_TIMEOUT_MS = 12_000;
/** The upload route takes at most this much. */
export const UPLOAD_MAX_BYTES = 25 * 1024 * 1024;
/** Without an answer for this long, the PC counts as unreachable. */
export const UNREACHABLE_AFTER_MS = 5_000;

export interface Connection {
  /** When the PC last answered; null if it never has. */
  lastOk: number | null;
  /** The PC has not answered for a while: since `lastOk`. */
  unreachable: boolean;
}

export interface ClientOptions {
  fetch?: typeof fetch;
  now?: () => number;
  /** The last answer of an earlier session, to say "unreachable since…". */
  lastOk?: number | null;
  /** Where a shared file is read from; the fake PC has its own. */
  fileUrl?: (id: string) => string;
  /** Called on every 401. */
  onUnpaired?: () => void;
  /** Called whenever the PC answers. */
  onAnswer?: (at: number) => void;
}

interface CallOptions {
  timeoutMs?: number;
  /** Times a `pc-busy` is tried again, a second apart. */
  retries?: number;
}

export type Client = ReturnType<typeof createClient>;

export function createClient(options: ClientOptions = {}) {
  const doFetch = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
  const now = options.now ?? Date.now;
  let lastOk = options.lastOk ?? null;
  /** The first failure since the last answer. */
  let failingSince: number | null = null;
  /** Requests waiting for an answer, by start time. */
  const pending = new Set<{ at: number }>();

  const answered = () => {
    lastOk = now();
    failingSince = null;
    options.onAnswer?.(lastOk);
  };
  const failed = () => {
    failingSince ??= now();
  };

  async function request(
    method: "GET" | "POST" | "DELETE",
    path: string,
    body: BodyInit | undefined,
    headers: Record<string, string>,
    timeoutMs: number,
    { pairing = false } = {},
  ): Promise<unknown> {
    const abort = new AbortController();
    const entry = { at: now() };
    pending.add(entry);
    const timer = setTimeout(() => abort.abort(), timeoutMs);
    let response: Response;
    try {
      response = await doFetch(path, {
        method,
        body,
        headers: method === "GET" ? headers : { "X-Hangar": "1", ...headers },
        credentials: "same-origin",
        cache: "no-store",
        signal: abort.signal,
      });
    } catch {
      failed();
      throw new ApiError("offline", abort.signal.aborted ? "timeout" : "network");
    } finally {
      clearTimeout(timer);
      pending.delete(entry);
    }
    const payload: unknown = await response.json().catch(() => null);
    const reason =
      payload && typeof payload === "object" && "error" in payload ? String(payload.error) : null;
    if (response.status === 503) {
      failed();
      throw new ApiError("busy", reason ?? "pc-busy", 503);
    }
    answered();
    if (response.status === 401) {
      if (!pairing) options.onUnpaired?.();
      throw new ApiError("unpaired", reason ?? "unpaired", 401);
    }
    if (!response.ok) throw new ApiError("refused", reason ?? `http-${response.status}`, response.status);
    return payload;
  }

  async function withRetries<T>(retries: number, run: () => Promise<T>): Promise<T> {
    for (let attempt = 0; ; attempt++) {
      try {
        return await run();
      } catch (error) {
        if (!(error instanceof ApiError && error.kind === "busy") || attempt >= retries) throw error;
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
    }
  }

  const post = (path: string, body: unknown, { timeoutMs = CALL_TIMEOUT_MS, retries = 0 }: CallOptions = {}) =>
    withRetries(retries, () =>
      request("POST", path, JSON.stringify(body), { "Content-Type": "application/json" }, timeoutMs),
    );

  const call = <T>(op: string, args: object, callOptions?: CallOptions) =>
    post(`/m/api/call/${op}`, args, callOptions) as Promise<T>;

  return {
    health: () => request("GET", "/m/api/health", undefined, {}, 6_000) as Promise<Health>,
    /** A code the PC turns down is `refused`, whatever its status: the phone was never paired by it. */
    pair: async (code: string, device: string) => {
      try {
        return (await request(
          "POST",
          "/m/api/pair",
          JSON.stringify({ code, device }),
          { "Content-Type": "application/json" },
          CALL_TIMEOUT_MS,
          { pairing: true },
        )) as { device: Device };
      } catch (error) {
        if (error instanceof ApiError && error.kind === "unpaired") throw new ApiError("refused", error.code, error.status);
        throw error;
      }
    },

    state: () => call<RemoteState>("state", {}),
    screen: (pane: string, lines?: number) =>
      call<Screen>("screen", lines === undefined ? { pane } : { pane, lines }),
    send: (args: SendArgs) => call<{ sent: boolean }>("send", args, { retries: 1 }),
    permission: (pane: string, allow: boolean) =>
      call<{ answered: boolean }>("permission", { pane, allow }, { retries: 1 }),
    manager: (text: string) => call<{ sent: boolean }>("manager", { text }, { retries: 1 }),
    history: (since?: number) =>
      call<{ entries: HistoryEntry[] }>("history", since === undefined ? {} : { since }),

    shares: (since?: number) =>
      request(
        "GET",
        since === undefined ? "/m/api/shares" : `/m/api/shares?since=${since}`,
        undefined,
        {},
        CALL_TIMEOUT_MS,
      ) as Promise<{ shares: Share[] }>,
    /** Sends a file into a workspace's folder; its path goes into the message. */
    upload: async (workspaceId: string, file: Blob, name: string) => {
      if (file.size > UPLOAD_MAX_BYTES) throw new ApiError("refused", "too-big", 413);
      const query = `workspace=${encodeURIComponent(workspaceId)}&name=${encodeURIComponent(name)}`;
      // A photo over a phone's uplink: a minute and a half per 25 MB.
      return request(
        "POST",
        `/m/api/upload?${query}`,
        file,
        { "Content-Type": file.type || "application/octet-stream" },
        Math.max(CALL_TIMEOUT_MS, (file.size / UPLOAD_MAX_BYTES) * 90_000),
      ) as Promise<{ path: string }>;
    },
    fileUrl: options.fileUrl ?? ((id: string) => `/m/files/${encodeURIComponent(id)}`),

    /** The PC's public VAPID key, base64url: what the phone subscribes with. */
    pushKey: () => request("GET", "/m/api/push/key", undefined, {}, CALL_TIMEOUT_MS) as Promise<{ key: string }>,
    /** Where this phone's notifications go, and the language they are written in. */
    pushSubscribe: (subscription: PushSubscriptionJSON, lang: "en" | "fr") =>
      request(
        "POST",
        "/m/api/push/subscribe",
        JSON.stringify({ subscription, lang }),
        { "Content-Type": "application/json" },
        CALL_TIMEOUT_MS,
      ) as Promise<{ subscribed: boolean }>,
    pushUnsubscribe: () =>
      request("DELETE", "/m/api/push/subscribe", undefined, {}, CALL_TIMEOUT_MS) as Promise<{ subscribed: boolean }>,

    /** Whether the PC answers, and since when it has not. */
    connection: (): Connection => {
      const at = now();
      const lastAnswer = lastOk ?? -Infinity;
      const silentFor = at - lastAnswer;
      const stalled = [...pending].some((request) => at - request.at >= UNREACHABLE_AFTER_MS);
      return {
        lastOk,
        unreachable: silentFor >= UNREACHABLE_AFTER_MS && (failingSince !== null || stalled),
      };
    },
  };
}
