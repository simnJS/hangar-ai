import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createClient, type Share, type ShareKind } from "./api";
import { createMock } from "./mock";
import { fakeFile } from "./mockFiles";
import { languageOf, TEXT_HEAD_BYTES, viewerOf, wrapsByDefault } from "./viewer";

/** The fake PC, reached through the real client, as the app reaches it with `?mock`. */
function fakePc() {
  const mock = createMock("default");
  return createClient({ fetch: mock.fetch, fileUrl: mock.fileUrl });
}

/** Lets the fake PC's latency pass. */
async function settle<T>(promise: Promise<T>, ms = 400): Promise<T> {
  promise.catch(() => {});
  await vi.advanceTimersByTimeAsync(ms);
  return promise;
}

const share = (kind: ShareKind | undefined, mime = "application/octet-stream"): Share => ({
  id: "s",
  at: 1,
  workspace: "w",
  pane: "p",
  name: "x",
  mime,
  kind,
  size: 1,
  caption: null,
});

describe("the Shares screen, against the fake PC", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("opens each share in the viewer of its kind, one of each", async () => {
    const { shares } = await settle(fakePc().shares());
    const viewers = Object.fromEntries(shares.map((share) => [share.name, viewerOf(share)]));
    expect(viewers).toEqual({
      "share-flow.mp4": "video",
      "retry.ts": "text",
      "mobile-for-you.png": "image",
      "RELEASE_NOTES.md": "markdown",
      "lighthouse.html": "html",
      "build.log": "text",
      "standup-summary.wav": "audio",
      "invoice-2026-10.pdf": "pdf",
      "latency-p95.png": "image",
      "invoices-october.csv": "text",
      "storefront-build.zip": "file",
    });
    const kinds: ShareKind[] = ["image", "video", "audio", "text", "markdown", "pdf", "html", "file"];
    for (const kind of kinds) expect(Object.values(viewers), kind).toContain(kind);
  });

  it("serves each share as the PC types it, and its size is its bytes'", async () => {
    const { shares } = await settle(fakePc().shares());
    const typeOf = Object.fromEntries(shares.map((share) => [share.kind, share.mime]));
    expect(typeOf).toMatchObject({
      text: "text/plain; charset=utf-8",
      markdown: "text/plain; charset=utf-8",
      html: "text/html; charset=utf-8",
      video: "video/mp4",
      audio: "audio/wav",
      pdf: "application/pdf",
      file: "application/octet-stream",
    });
    for (const shared of shares) {
      const bytes = fakeFile(shared.id);
      if (bytes && shared.kind !== "file") expect(shared.size, shared.name).toBe(bytes.length);
    }
  });

  it("reads a long log's head only, with a byte range, and says it is one", async () => {
    const client = fakePc();
    const { shares } = await settle(client.shares());
    const log = shares.find((share) => share.name === "build.log")!;
    expect(log.size).toBeGreaterThan(TEXT_HEAD_BYTES * 10);

    const head = await settle(client.fileText(log.id, log.size, TEXT_HEAD_BYTES));
    expect(head.partial).toBe(true);
    const whole = new TextDecoder().decode(fakeFile(log.id));
    expect(whole.startsWith(head.text)).toBe(true);
    expect(head.text.endsWith("\n")).toBe(true);
    const length = new TextEncoder().encode(head.text).length;
    expect(length).toBeLessThanOrEqual(TEXT_HEAD_BYTES);
    expect(length).toBeGreaterThan(TEXT_HEAD_BYTES - 200);
  });

  it("reads a short text whole", async () => {
    const client = fakePc();
    const { shares } = await settle(client.shares());
    for (const name of ["retry.ts", "RELEASE_NOTES.md", "lighthouse.html", "invoices-october.csv"]) {
      const shared = shares.find((share) => share.name === name)!;
      const read = await settle(client.fileText(shared.id, shared.size, TEXT_HEAD_BYTES));
      expect(read.partial, name).toBe(false);
      expect(read.text, name).toBe(new TextDecoder().decode(fakeFile(shared.id)));
    }
  });

  it("answers a range as the PC does: 206, or 416 past the end", async () => {
    const mock = createMock("default");
    const log = fakeFile("s-log")!;
    const part = await settle(mock.fetch("/m/files/s-log", { headers: { Range: "bytes=10-19" } }));
    expect(part.status).toBe(206);
    expect(part.headers.get("Content-Range")).toBe(`bytes 10-19/${log.length}`);
    expect(new Uint8Array(await part.arrayBuffer())).toEqual(log.slice(10, 20));
    const past = await settle(mock.fetch("/m/files/s-log", { headers: { Range: `bytes=${log.length}-` } }));
    expect(past.status).toBe(416);
  });
});

describe("choosing a viewer", () => {
  it("keeps the Open button for what the phone does not show", () => {
    expect(viewerOf(share("file"))).toBe("file");
    // A kind a newer PC might tell, unknown here.
    expect(viewerOf(share("model" as ShareKind))).toBe("file");
  });

  it("shows a share kept from before kinds as it was: a picture, or a file", () => {
    expect(viewerOf(share(undefined, "image/png"))).toBe("image");
    expect(viewerOf(share(undefined, "text/html"))).toBe("file");
    expect(viewerOf(share(undefined, "text/plain"))).toBe("file");
  });
});

describe("a text's language", () => {
  it("comes from its extension, plain when it is prose, a log or data", () => {
    expect(languageOf("retry.ts")).toBe("js");
    expect(languageOf("main.RS")).toBe("rust");
    expect(languageOf("page.html")).toBe("markup");
    expect(languageOf("config.yml")).toBe("yaml");
    expect(languageOf("deploy.ps1")).toBe("shell");
    expect(languageOf("game.verse")).toBe("verse");
    for (const name of ["build.log", "notes.txt", "data.csv", "Makefile", "README.md"]) expect(languageOf(name), name).toBe("plain");
  });

  it("wraps prose and logs, and keeps code's lines", () => {
    expect(wrapsByDefault("plain")).toBe(true);
    expect(wrapsByDefault("js")).toBe(false);
  });
});
