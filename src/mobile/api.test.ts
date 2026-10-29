import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, CALL_TIMEOUT_MS, UNREACHABLE_AFTER_MS, UPLOAD_MAX_BYTES, createClient, decodeHead } from "./api";

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

const EMPTY_STATE = { at: 1, manager: null, waiting: [], projects: [] };

/** A fetch answering `answers` in turn, recording what it was asked. */
function fakeFetch(...answers: (Response | Error | "hang")[]) {
  const calls: { path: string; init: RequestInit }[] = [];
  const fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ path: String(input), init: init ?? {} });
    const answer = answers.length > 1 ? answers.shift()! : answers[0];
    if (answer === "hang") {
      return new Promise<Response>((_resolve, reject) =>
        init?.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError"))),
      );
    }
    if (answer instanceof Error) throw answer;
    return answer.clone();
  });
  return { fetch: fetch as unknown as typeof globalThis.fetch, calls };
}

async function failure(promise: Promise<unknown>): Promise<ApiError> {
  try {
    await promise;
  } catch (error) {
    if (error instanceof ApiError) return error;
    throw error;
  }
  throw new Error("expected the call to fail");
}

describe("the client's requests", () => {
  it("posts each op as JSON to /m/api/call/<op>, with X-Hangar and the session cookie", async () => {
    const { fetch, calls } = fakeFetch(json(200, EMPTY_STATE));
    const client = createClient({ fetch });
    expect(await client.state()).toEqual(EMPTY_STATE);
    expect(calls[0].path).toBe("/m/api/call/state");
    expect(calls[0].init.method).toBe("POST");
    expect(calls[0].init.credentials).toBe("same-origin");
    expect(calls[0].init.headers).toMatchObject({ "X-Hangar": "1", "Content-Type": "application/json" });
    expect(JSON.parse(String(calls[0].init.body))).toEqual({});
  });

  it("sends each op's arguments as the contract names them", async () => {
    const { fetch, calls } = fakeFetch(json(200, { ok: true }));
    const client = createClient({ fetch });
    await client.screen("Ben");
    await client.screen("p-ben", 40);
    await client.send({ pane: "p-ben", text: "go on", submit: true });
    await client.send({ pane: "p-ben", keys: ["ctrl+c"] });
    await client.permission("p-ben", false);
    await client.manager("where are we?");
    await client.history();
    await client.history(1_000);
    await client.wake("ws-billing");
    expect(calls.map((call) => [call.path, JSON.parse(String(call.init.body))])).toEqual([
      ["/m/api/call/screen", { pane: "Ben" }],
      ["/m/api/call/screen", { pane: "p-ben", lines: 40 }],
      ["/m/api/call/send", { pane: "p-ben", text: "go on", submit: true }],
      ["/m/api/call/send", { pane: "p-ben", keys: ["ctrl+c"] }],
      ["/m/api/call/permission", { pane: "p-ben", allow: false }],
      ["/m/api/call/manager", { text: "where are we?" }],
      ["/m/api/call/history", {}],
      ["/m/api/call/history", { since: 1_000 }],
      ["/m/api/call/wake", { workspace: "ws-billing" }],
    ]);
  });

  it("asks for the chat's pages and answers its questions as the contract names them", async () => {
    const { fetch, calls } = fakeFetch(json(200, { answered: true }));
    const client = createClient({ fetch });
    await client.chat("p-ada");
    await client.chat("p-ada", { limit: 30 });
    await client.chat("p-ada", { before: "ada-12", limit: 60 });
    await client.answer("p-cora", "toolu_1", [{ selected: ["Submit now"] }, { selected: [], other: "Later" }]);
    expect(calls.map((call) => [call.path, JSON.parse(String(call.init.body))])).toEqual([
      ["/m/api/call/chat", { pane: "p-ada" }],
      ["/m/api/call/chat", { pane: "p-ada", limit: 30 }],
      ["/m/api/call/chat", { pane: "p-ada", before: "ada-12", limit: 60 }],
      [
        "/m/api/call/answer",
        { pane: "p-cora", toolUseId: "toolu_1", answers: [{ selected: ["Submit now"] }, { selected: [], other: "Later" }] },
      ],
    ]);
    expect(calls.every((call) => (call.init.headers as Record<string, string>)["X-Hangar"] === "1")).toBe(true);
  });

  it("reads health and shares with GET, without the POST header", async () => {
    const { fetch, calls } = fakeFetch(json(200, { ok: true, pc: "PC", version: "1.1.0", paired: true }), json(200, { shares: [] }));
    const client = createClient({ fetch });
    expect((await client.health()).paired).toBe(true);
    await client.shares(42);
    expect(calls[0].path).toBe("/m/api/health");
    expect(calls[0].init.method).toBe("GET");
    expect(calls[0].init.headers).not.toHaveProperty("X-Hangar");
    expect(calls[1].path).toBe("/m/api/shares?since=42");
  });

  it("pairs with the code and the device's name", async () => {
    const { fetch, calls } = fakeFetch(json(200, { device: { id: "d1", name: "iPhone (Safari)" } }));
    const client = createClient({ fetch });
    expect(await client.pair("123456", "iPhone (Safari)")).toEqual({ device: { id: "d1", name: "iPhone (Safari)" } });
    expect(calls[0].path).toBe("/m/api/pair");
    expect(JSON.parse(String(calls[0].init.body))).toEqual({ code: "123456", device: "iPhone (Safari)" });
  });

  it("uploads the raw file, its workspace and name in the query", async () => {
    const { fetch, calls } = fakeFetch(json(200, { path: "C:/w/.hangar/uploads/x.jpg" }));
    const client = createClient({ fetch });
    const photo = new Blob(["jpeg bytes"], { type: "image/jpeg" });
    expect(await client.upload("ws 1", photo, "photo d'été.jpg")).toEqual({ path: "C:/w/.hangar/uploads/x.jpg" });
    expect(calls[0].path).toBe("/m/api/upload?workspace=ws%201&name=photo%20d'%C3%A9t%C3%A9.jpg");
    expect(calls[0].init.body).toBe(photo);
    expect(calls[0].init.headers).toMatchObject({ "X-Hangar": "1", "Content-Type": "image/jpeg" });
  });

  it("refuses a file over 25 MB without sending it", async () => {
    const { fetch, calls } = fakeFetch(json(200, { path: "x" }));
    const client = createClient({ fetch });
    const huge = { size: UPLOAD_MAX_BYTES + 1, type: "video/mp4" } as Blob;
    const error = await failure(client.upload("ws", huge, "clip.mp4"));
    expect([error.kind, error.code]).toEqual(["refused", "too-big"]);
    expect(calls).toHaveLength(0);
  });

  it("reads a shared file from /m/files/<id>, unless told otherwise", () => {
    expect(createClient().fileUrl("a/b c")).toBe("/m/files/a%2Fb%20c");
    expect(createClient({ fileUrl: (id) => `data:${id}` }).fileUrl("x")).toBe("data:x");
  });

  it("reads a short text whole, without a range", async () => {
    const { fetch, calls } = fakeFetch(new Response("fn main() {}\n", { status: 200 }));
    const client = createClient({ fetch });
    expect(await client.fileText("s 1", 13, 1024)).toEqual({ text: "fn main() {}\n", partial: false });
    expect(calls[0].path).toBe("/m/files/s%201");
    expect(calls[0].init.headers).toEqual({});
    expect(calls[0].init.method).toBe("GET");
  });

  it("reads a long text's head as a byte range, cut after its last whole line", async () => {
    const { fetch, calls } = fakeFetch(new Response("line one\nline two\nline th", { status: 206 }));
    const client = createClient({ fetch });
    expect(await client.fileText("log", 5_000_000, 26)).toEqual({ text: "line one\nline two\n", partial: true });
    expect(calls[0].init.headers).toEqual({ Range: "bytes=0-25" });
  });

  it("cuts a text the PC sent whole although a head was asked for", async () => {
    const { fetch } = fakeFetch(new Response("aaaa\nbbbb\ncccc\n", { status: 200 }));
    expect(await createClient({ fetch }).fileText("log", 15, 12)).toEqual({ text: "aaaa\nbbbb\n", partial: true });
  });

  it("says when a file cannot be read, and when the phone is no longer paired", async () => {
    const onUnpaired = vi.fn();
    const missing = fakeFetch(json(404, { error: "not-found" }));
    const gone = await failure(createClient({ fetch: missing.fetch }).fileText("x", 1, 10));
    expect([gone.kind, gone.status]).toEqual(["refused", 404]);
    const unpaired = fakeFetch(json(401, { error: "unpaired" }));
    const out = await failure(createClient({ fetch: unpaired.fetch, onUnpaired }).fileText("x", 1, 10));
    expect(out.kind).toBe("unpaired");
    expect(onUnpaired).toHaveBeenCalledOnce();
    const offline = fakeFetch(new TypeError("Failed to fetch"));
    expect((await failure(createClient({ fetch: offline.fetch }).fileText("x", 1, 10))).kind).toBe("offline");
  });

  it("reads the push key, subscribes in the phone's language, and unsubscribes", async () => {
    const { fetch, calls } = fakeFetch(json(200, { key: "BKey" }), json(200, { subscribed: true }), json(200, { subscribed: false }));
    const client = createClient({ fetch });
    const subscription = { endpoint: "https://web.push.apple.com/QGx1", keys: { p256dh: "BP", auth: "au" } };
    expect(await client.pushKey()).toEqual({ key: "BKey" });
    expect(await client.pushSubscribe(subscription, "fr")).toEqual({ subscribed: true });
    expect(await client.pushUnsubscribe()).toEqual({ subscribed: false });

    expect(calls.map((call) => [call.init.method, call.path])).toEqual([
      ["GET", "/m/api/push/key"],
      ["POST", "/m/api/push/subscribe"],
      ["DELETE", "/m/api/push/subscribe"],
    ]);
    expect(calls[0].init.headers).not.toHaveProperty("X-Hangar");
    expect(JSON.parse(String(calls[1].init.body))).toEqual({ subscription, lang: "fr" });
    // Whatever is not a read carries the header the server asks of it.
    expect(calls[1].init.headers).toMatchObject({ "X-Hangar": "1", "Content-Type": "application/json" });
    expect(calls[2].init.headers).toMatchObject({ "X-Hangar": "1" });
  });
});

describe("the client's errors", () => {
  it("turns a 401 into `unpaired`, and says so", async () => {
    const onUnpaired = vi.fn();
    const { fetch } = fakeFetch(json(401, { error: "unpaired" }));
    const client = createClient({ fetch, onUnpaired });
    const error = await failure(client.state());
    expect([error.kind, error.code, error.status]).toEqual(["unpaired", "unpaired", 401]);
    expect(onUnpaired).toHaveBeenCalledOnce();
  });

  it("reads a pairing code turned down as refused, not as a lost pairing", async () => {
    const onUnpaired = vi.fn();
    const { fetch } = fakeFetch(json(401, { error: "bad-code" }), json(403, { error: "expired" }));
    const client = createClient({ fetch, onUnpaired });
    const wrong = await failure(client.pair("000000", "phone"));
    expect([wrong.kind, wrong.code, wrong.status]).toEqual(["refused", "bad-code", 401]);
    expect((await failure(client.pair("111111", "phone"))).code).toBe("expired");
    expect(onUnpaired).not.toHaveBeenCalled();
  });

  it("turns a 503 pc-busy into `busy`", async () => {
    const { fetch } = fakeFetch(json(503, { error: "pc-busy" }));
    const error = await failure(createClient({ fetch }).state());
    expect([error.kind, error.code]).toEqual(["busy", "pc-busy"]);
  });

  it("passes on the PC's reason for any other refusal", async () => {
    const { fetch } = fakeFetch(json(409, { error: "Open the project manager on the PC" }));
    const error = await failure(createClient({ fetch }).manager("hello"));
    expect([error.kind, error.code, error.status]).toEqual(["refused", "Open the project manager on the PC", 409]);
  });

  it("copes with an answer that is not JSON", async () => {
    const { fetch } = fakeFetch(new Response("<html>Bad gateway</html>", { status: 502 }));
    const error = await failure(createClient({ fetch }).state());
    expect([error.kind, error.code]).toEqual(["refused", "http-502"]);
  });

  it("turns a network failure into `offline`", async () => {
    const { fetch } = fakeFetch(new TypeError("Failed to fetch"));
    const error = await failure(createClient({ fetch }).state());
    expect([error.kind, error.code]).toEqual(["offline", "network"]);
  });

  describe("with the clock in hand", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("tries an action again once, a second later, when the PC is busy", async () => {
      const { fetch, calls } = fakeFetch(json(503, { error: "pc-busy" }), json(200, { sent: true }));
      const sent = createClient({ fetch }).send({ pane: "p", text: "hi", submit: true });
      await vi.advanceTimersByTimeAsync(999);
      expect(calls).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(1);
      expect(await sent).toEqual({ sent: true });
      expect(calls).toHaveLength(2);
    });

    it("gives up on an action busy twice", async () => {
      const { fetch, calls } = fakeFetch(json(503, { error: "pc-busy" }));
      const sent = failure(createClient({ fetch }).permission("p", true));
      await vi.advanceTimersByTimeAsync(1_000);
      expect((await sent).kind).toBe("busy");
      expect(calls).toHaveLength(2);
    });

    it("leaves polling to try again itself", async () => {
      const { fetch, calls } = fakeFetch(json(503, { error: "pc-busy" }));
      expect((await failure(createClient({ fetch }).state())).kind).toBe("busy");
      expect(calls).toHaveLength(1);
    });
  });
});

describe("when the PC stops answering", () => {
  beforeEach(() => vi.useFakeTimers({ now: new Date(2026, 9, 28, 14, 0) }));
  afterEach(() => vi.useRealTimers());

  it("is reachable while it answers", async () => {
    const { fetch } = fakeFetch(json(200, EMPTY_STATE));
    const client = createClient({ fetch });
    await client.state();
    expect(client.connection()).toEqual({ lastOk: Date.now(), unreachable: false });
  });

  it("counts as unreachable once it has been silent for a while, since its last answer", async () => {
    const { fetch } = fakeFetch(json(200, EMPTY_STATE), new TypeError("Failed to fetch"));
    const client = createClient({ fetch });
    await client.state();
    const answeredAt = Date.now();

    vi.advanceTimersByTime(2_000);
    await failure(client.state());
    // One missed poll is not an outage.
    expect(client.connection().unreachable).toBe(false);

    vi.advanceTimersByTime(UNREACHABLE_AFTER_MS - 2_000);
    expect(client.connection()).toEqual({ lastOk: answeredAt, unreachable: true });
  });

  it("counts a call left without an answer, before it even times out", async () => {
    const { fetch } = fakeFetch(json(200, EMPTY_STATE), "hang");
    const client = createClient({ fetch });
    await client.state();
    const answeredAt = Date.now();

    const hanging = failure(client.state());
    await vi.advanceTimersByTimeAsync(UNREACHABLE_AFTER_MS - 1);
    expect(client.connection().unreachable).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(client.connection()).toEqual({ lastOk: answeredAt, unreachable: true });

    await vi.advanceTimersByTimeAsync(CALL_TIMEOUT_MS - UNREACHABLE_AFTER_MS);
    const error = await hanging;
    expect([error.kind, error.code]).toEqual(["offline", "timeout"]);
    expect(client.connection().unreachable).toBe(true);
  });

  it("is reachable again with the next answer", async () => {
    const { fetch } = fakeFetch(new TypeError("Failed to fetch"), json(200, EMPTY_STATE));
    const onAnswer = vi.fn();
    const client = createClient({ fetch, onAnswer });
    await failure(client.state());
    expect(client.connection()).toEqual({ lastOk: null, unreachable: true });
    vi.advanceTimersByTime(30_000);
    await client.state();
    expect(client.connection()).toEqual({ lastOk: Date.now(), unreachable: false });
    expect(onAnswer).toHaveBeenCalledWith(Date.now());
  });

  it("says since when from an earlier session's last answer", async () => {
    const yesterday = Date.now() - 86_400_000;
    const { fetch } = fakeFetch(new TypeError("Failed to fetch"));
    const client = createClient({ fetch, lastOk: yesterday });
    expect(client.connection().unreachable).toBe(false);
    await failure(client.state());
    expect(client.connection()).toEqual({ lastOk: yesterday, unreachable: true });
  });

  it("counts a refusal or a 401 as an answer: the PC is there", async () => {
    const { fetch } = fakeFetch(new TypeError("Failed to fetch"), json(401, { error: "unpaired" }));
    const client = createClient({ fetch });
    await failure(client.state());
    await failure(client.state());
    expect(client.connection()).toEqual({ lastOk: Date.now(), unreachable: false });
  });

  it("counts the window not answering in time as the PC not answering", async () => {
    const { fetch } = fakeFetch(json(200, EMPTY_STATE), json(503, { error: "pc-busy" }));
    const client = createClient({ fetch });
    await client.state();
    vi.advanceTimersByTime(UNREACHABLE_AFTER_MS);
    await failure(client.state());
    expect(client.connection().unreachable).toBe(true);
  });
});

describe("a text's head", () => {
  const bytes = (text: string) => new TextEncoder().encode(text);

  it("is the whole text when it is not a head", () => {
    expect(decodeHead(bytes("été\nfin sans saut"), false)).toBe("été\nfin sans saut");
  });

  it("ends on its last whole line", () => {
    expect(decodeHead(bytes("un\ndeux\ntro"), true)).toBe("un\ndeux\n");
  });

  it("drops a character cut in two when it has no line to end on", () => {
    const cut = bytes("déjà").subarray(0, 5); // d, é, j and the first byte of à
    expect(decodeHead(cut, true)).toBe("déj");
  });
});
