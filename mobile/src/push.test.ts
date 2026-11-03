import { describe, expect, it, vi } from "vitest";
import { hashOf } from "./hooks";
import { PUSH_KEY } from "./mock";
import { keyBytes, pushStatus, pushSupport, sameKey, subscribe, type Environment } from "./push";

const browser: Environment = { ios: false, standalone: false, serviceWorker: true, pushManager: true, notification: true };

describe("whether the phone can have notifications", () => {
  it("on an iPhone, only from the Home Screen", () => {
    // Safari shows no push at all to a page in a tab.
    expect(pushSupport({ ...browser, ios: true, pushManager: false, notification: false })).toBe("install");
    expect(pushSupport({ ...browser, ios: true })).toBe("install");
    expect(pushSupport({ ...browser, ios: true, standalone: true })).toBe("ok");
    // On the Home Screen without push: iOS older than 16.4.
    expect(pushSupport({ ...browser, ios: true, standalone: true, pushManager: false })).toBe("oldIos");
  });

  it("elsewhere, wherever the browser has push", () => {
    expect(pushSupport(browser)).toBe("ok");
    expect(pushSupport({ ...browser, pushManager: false })).toBe("unsupported");
    expect(pushSupport({ ...browser, serviceWorker: false })).toBe("unsupported");
  });

  it("says where they stand", () => {
    expect(pushStatus("install", null, null)).toBe("install");
    expect(pushStatus("ok", "denied", true)).toBe("denied");
    expect(pushStatus("ok", "default", null)).toBe("checking");
    expect(pushStatus("ok", "default", false)).toBe("off");
    expect(pushStatus("ok", "granted", false)).toBe("off");
    expect(pushStatus("ok", "granted", true)).toBe("on");
    // A subscription the person no longer allows is no notification.
    expect(pushStatus("ok", "default", true)).toBe("off");
  });
});

describe("the PC's key", () => {
  it("is read from base64url, padded or not", () => {
    const bytes = keyBytes(PUSH_KEY);
    expect(bytes).toHaveLength(65);
    expect(bytes[0]).toBe(0x04);
    expect(keyBytes(`${PUSH_KEY}=`)).toEqual(bytes);
    expect(keyBytes("AQID")).toEqual(new Uint8Array([1, 2, 3]));
  });

  it("tells a subscription made with it from one made with another", () => {
    expect(sameKey(keyBytes(PUSH_KEY).buffer, PUSH_KEY)).toBe(true);
    const other = keyBytes(PUSH_KEY);
    other[64] ^= 1;
    expect(sameKey(other.buffer, PUSH_KEY)).toBe(false);
    expect(sameKey(null, PUSH_KEY)).toBe(false);
  });
});

function worker(options: { existing?: Uint8Array; refuseFirst?: boolean } = {}) {
  const made = { endpoint: "https://push.example/new", options: { applicationServerKey: keyBytes(PUSH_KEY).buffer } };
  const old = options.existing
    ? { options: { applicationServerKey: options.existing.buffer }, unsubscribe: vi.fn(async () => true) }
    : null;
  let calls = 0;
  const pushManager = {
    subscribe: vi.fn(async () => {
      calls++;
      if (options.refuseFirst && calls === 1) throw new DOMException("another key", "InvalidStateError");
      return made;
    }),
    getSubscription: vi.fn(async () => old),
  };
  return { registration: { pushManager } as unknown as ServiceWorkerRegistration, pushManager, old, made };
}

describe("subscribing", () => {
  it("asks the browser before waiting on anything, with the PC's key", async () => {
    const { registration, pushManager, made } = worker();
    const subscribing = subscribe(registration, PUSH_KEY);
    // Called within the tap itself, not after an await.
    expect(pushManager.subscribe).toHaveBeenCalledTimes(1);
    expect(pushManager.subscribe).toHaveBeenCalledWith({ userVisibleOnly: true, applicationServerKey: keyBytes(PUSH_KEY) });
    expect(await subscribing).toBe(made);
  });

  it("replaces a subscription left from another key", async () => {
    const stale = keyBytes(PUSH_KEY);
    stale[10] ^= 0xff;
    const { registration, pushManager, old, made } = worker({ existing: stale, refuseFirst: true });
    expect(await subscribe(registration, PUSH_KEY)).toBe(made);
    expect(old?.unsubscribe).toHaveBeenCalled();
    expect(pushManager.subscribe).toHaveBeenCalledTimes(2);
  });

  it("passes on a refusal that is not about the key", async () => {
    const { registration } = worker({ existing: keyBytes(PUSH_KEY), refuseFirst: true });
    await expect(subscribe(registration, PUSH_KEY)).rejects.toThrow("another key");
  });
});

describe("a notification's screen", () => {
  it("is the hash of its address", () => {
    const base = "https://pc.tail1234.ts.net/";
    expect(hashOf("https://pc.tail1234.ts.net/#/pane/p-ben", base)).toBe("#/pane/p-ben");
    expect(hashOf("./#/shares", base)).toBe("#/shares");
    expect(hashOf("https://pc.tail1234.ts.net/", base)).toBe("#/");
  });
});
