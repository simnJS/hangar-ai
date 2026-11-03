import { describe, expect, it, vi } from "vitest";
import SOURCE from "./sw.js?raw";

/**
 * The service worker as the phone runs it: sw.js evaluated against a stand-in
 * for its global scope, its events dispatched by hand.
 */

const SCOPE = "https://pc.tail1234.ts.net/";

interface FakeWindow {
  focused: boolean;
  focus: ReturnType<typeof vi.fn>;
  postMessage: ReturnType<typeof vi.fn>;
}

function worker(windows: FakeWindow[] = []) {
  const handlers = new Map<string, (event: unknown) => void>();
  const shown: { title: string; options: NotificationOptions }[] = [];
  const opened: string[] = [];
  const self = {
    addEventListener: (type: string, handler: (event: unknown) => void) => handlers.set(type, handler),
    location: new URL(SCOPE),
    navigator: { language: "fr-FR" },
    registration: {
      scope: SCOPE,
      showNotification: vi.fn(async (title: string, options: NotificationOptions) => {
        shown.push({ title, options });
      }),
    },
    clients: {
      matchAll: vi.fn(async () => windows),
      openWindow: vi.fn(async (url: string) => {
        opened.push(url);
      }),
      claim: vi.fn(),
    },
    skipWaiting: vi.fn(),
  };
  new Function("self", "caches", "fetch", SOURCE)(self, {}, vi.fn());

  async function dispatch(type: string, event: Record<string, unknown>) {
    const waits: Promise<unknown>[] = [];
    handlers.get(type)!({ ...event, waitUntil: (promise: Promise<unknown>) => waits.push(promise) });
    await Promise.all(waits);
  }
  const push = (data: unknown) =>
    dispatch("push", {
      data:
        data === undefined
          ? null
          : {
              json: () => (typeof data === "string" ? JSON.parse(data) : data),
              text: () => String(data),
            },
    });
  const click = (url: unknown) => {
    const notification = { data: { url }, close: vi.fn() };
    return dispatch("notificationclick", { notification }).then(() => notification);
  };
  return { shown, opened, push, click, self };
}

const someWindow = (focused = false): FakeWindow => ({ focused, focus: vi.fn(async () => undefined), postMessage: vi.fn() });

describe("the service worker's notifications", () => {
  it("shows what the PC pushed, with the screen it is about", async () => {
    const sw = worker();
    await sw.push({ title: "Ada vous attend", body: "Shop : une permission", url: "./#/pane/w1" });
    expect(sw.shown).toEqual([
      {
        title: "Ada vous attend",
        options: {
          body: "Shop : une permission",
          icon: "./icon-192.png",
          badge: "./icon-192.png",
          data: { url: `${SCOPE}#/pane/w1` },
        },
      },
    ]);
  });

  it("shows something for every push, even one it cannot read", async () => {
    const sw = worker();
    await sw.push(undefined);
    await sw.push("not json");
    expect(sw.shown.map((n) => n.title)).toEqual(["Hangar", "Hangar"]);
    expect(sw.shown[1].options.body).toBe("not json");
    expect(sw.shown[0].options.data).toEqual({ url: SCOPE + "#/" });
  });

  it("never sends a tap off the PC's site", async () => {
    const sw = worker();
    await sw.push({ title: "x", body: "", url: "https://elsewhere.example/#/pane/w1" });
    await sw.push({ title: "x", body: "", url: "javascript:alert(1)" });
    expect(sw.shown.map((n) => (n.options.data as { url: string }).url)).toEqual([SCOPE, SCOPE]);
  });

  it("brings the open app forward and tells it the screen", async () => {
    const background = someWindow();
    const front = someWindow(true);
    const sw = worker([background, front]);
    const notification = await sw.click(`${SCOPE}#/shares`);
    expect(notification.close).toHaveBeenCalled();
    expect(front.focus).toHaveBeenCalled();
    expect(front.postMessage).toHaveBeenCalledWith({ type: "hangar:open", url: `${SCOPE}#/shares` });
    expect(background.postMessage).not.toHaveBeenCalled();
    expect(sw.opened).toEqual([]);
  });

  it("opens the app on the screen when it is closed, or cannot be brought forward", async () => {
    const closed = worker();
    await closed.click(`${SCOPE}#/pane/w1`);
    expect(closed.opened).toEqual([`${SCOPE}#/pane/w1`]);

    const stuck = someWindow();
    stuck.focus.mockRejectedValue(new Error("not allowed"));
    const sw = worker([stuck]);
    await sw.click(`${SCOPE}#/`);
    expect(sw.opened).toEqual([`${SCOPE}#/`]);
  });
});
