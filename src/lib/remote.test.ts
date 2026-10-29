import { describe, expect, it } from "vitest";
import {
  DEFAULT_SLOT_PORTS,
  appPortsOf,
  canPair,
  checkAppPort,
  formatCountdown,
  formatStamp,
  hangarPorts,
  newDevices,
  remoteAddress,
  secondsLeft,
  sortDevices,
  tailscaleStep,
  withAppPort,
  withoutAppPort,
  type AppSlot,
  type RemoteDevice,
  type RemoteStatus,
  type TailscaleStatus,
} from "./remote";

const ts = (patch: Partial<TailscaleStatus> = {}): TailscaleStatus => ({
  installed: true,
  running: true,
  dnsName: "pc.tail1234.ts.net",
  serving: true,
  ...patch,
});

const remote = (patch: Partial<RemoteStatus> = {}): RemoteStatus => ({
  enabled: true,
  running: true,
  port: 4000,
  error: null,
  ...patch,
});

const device = (id: string, created: number): RemoteDevice => ({
  id,
  name: id,
  created,
  lastSeen: 0,
});

describe("tailscaleStep", () => {
  it("is missing when the CLI was not found", () => {
    expect(
      tailscaleStep(ts({ installed: false, running: false, dnsName: null, serving: false })),
    ).toBe("missing");
  });

  it("is stopped when installed but not connected", () => {
    expect(tailscaleStep(ts({ running: false, dnsName: null, serving: false }))).toBe("stopped");
  });

  it("is stopped when connected without a name to give out", () => {
    expect(tailscaleStep(ts({ dnsName: null }))).toBe("stopped");
  });

  it("is unserved when connected but not published", () => {
    expect(tailscaleStep(ts({ serving: false }))).toBe("unserved");
  });

  it("is ready when connected and served", () => {
    expect(tailscaleStep(ts())).toBe("ready");
  });
});

describe("remoteAddress", () => {
  it("is the https address of the PC's name", () => {
    expect(remoteAddress(ts())).toBe("https://pc.tail1234.ts.net");
  });

  it("is nothing without Tailscale or a name", () => {
    expect(remoteAddress(ts({ installed: false, dnsName: null }))).toBeNull();
    expect(remoteAddress(ts({ dnsName: null }))).toBeNull();
  });
});

describe("canPair", () => {
  it("needs the server on and the address served", () => {
    expect(canPair(remote(), ts())).toBe(true);
    expect(canPair(remote({ enabled: false }), ts())).toBe(false);
    expect(canPair(remote({ running: false }), ts())).toBe(false);
    expect(canPair(remote(), ts({ serving: false }))).toBe(false);
    expect(canPair(null, ts())).toBe(false);
    expect(canPair(remote(), null)).toBe(false);
  });
});

describe("countdown", () => {
  it("rounds up to whole seconds and stops at zero", () => {
    expect(secondsLeft(10_000, 9_001)).toBe(1);
    expect(secondsLeft(10_000, 0)).toBe(10);
    expect(secondsLeft(10_000, 10_000)).toBe(0);
    expect(secondsLeft(10_000, 20_000)).toBe(0);
  });

  it("reads m:ss", () => {
    expect(formatCountdown(300)).toBe("5:00");
    expect(formatCountdown(65)).toBe("1:05");
    expect(formatCountdown(9)).toBe("0:09");
    expect(formatCountdown(-3)).toBe("0:00");
  });
});

describe("devices", () => {
  it("finds the phones a refresh brought in", () => {
    const after = [device("a", 1), device("b", 2)];
    expect(newDevices(new Set(["a"]), after).map((d) => d.id)).toEqual(["b"]);
    expect(newDevices(new Set(["a", "b"]), after)).toEqual([]);
  });

  it("lists the latest pairing first, without touching the input", () => {
    const input = [device("a", 1), device("c", 3), device("b", 2)];
    expect(sortDevices(input).map((d) => d.id)).toEqual(["c", "b", "a"]);
    expect(input.map((d) => d.id)).toEqual(["a", "c", "b"]);
  });

  it("dates this year's stamps without the year", () => {
    const now = new Date(2026, 9, 28, 12).getTime();
    const thisYear = formatStamp(new Date(2026, 9, 27, 20, 41).getTime(), "en", now);
    const lastYear = formatStamp(new Date(2025, 9, 27, 20, 41).getTime(), "en", now);
    expect(thisYear).not.toContain("2026");
    expect(lastYear).toContain("2025");
  });
});

describe("the ports the phone may open", () => {
  const slot = (index: number, patch: Partial<AppSlot> = {}): AppSlot => ({
    slot: index,
    httpsPort: 8443 + index,
    localPort: 47822 + index,
    mapping: "free",
    app: null,
    ...patch,
  });
  const slots = [0, 1, 2, 3, 4, 5].map((index) => slot(index));

  it("takes a whole port from 1024 to 65535, spaces around it or not", () => {
    expect(checkAppPort("5173", [], [])).toEqual({ port: 5173 });
    expect(checkAppPort(" 3000 ", [], [])).toEqual({ port: 3000 });
    expect(checkAppPort("1024", [], [])).toEqual({ port: 1024 });
    expect(checkAppPort("65535", [], [])).toEqual({ port: 65535 });
  });

  it("says what is wrong with anything else", () => {
    for (const typed of ["", "  ", "abc", "51 73", "5173.5", "-80", "0x1F90", "1e4"]) {
      expect(checkAppPort(typed, [], []), JSON.stringify(typed)).toEqual({ problem: "invalid" });
    }
    for (const typed of ["0", "80", "1023", "65536", "99999", "123456"]) {
      expect(checkAppPort(typed, [], []), typed).toEqual({ problem: "range" });
    }
  });

  it("never takes Hangar's own: the mobile server's port and the slots' listeners", () => {
    const reserved = hangarPorts(remote({ port: 47821 }), slots);
    expect(reserved).toEqual([47821, 47822, 47823, 47824, 47825, 47826, 47827]);
    expect(checkAppPort("47821", [], reserved)).toEqual({ problem: "hangar" });
    expect(checkAppPort("47824", [], reserved)).toEqual({ problem: "hangar" });
    // The HTTPS ports are Tailscale's, on the tailnet: a dev server may use them on the PC.
    expect(checkAppPort("8443", [], reserved)).toEqual({ port: 8443 });
  });

  it("keeps the slots' default ports out before the PC has listed them", () => {
    expect(hangarPorts(null, null)).toEqual([...DEFAULT_SLOT_PORTS]);
    expect(hangarPorts(remote({ port: 4000 }), [])).toEqual([4000, ...DEFAULT_SLOT_PORTS]);
    expect(hangarPorts(remote({ port: 4000 }), [slot(0, { localPort: 50000 })])).toEqual([4000, 50000]);
  });

  it("does not add a port twice", () => {
    expect(checkAppPort("5173", [3000, 5173], [])).toEqual({ problem: "duplicate" });
  });

  it("keeps a workspace's list in order, each port once", () => {
    expect(withAppPort([3000, 8080], 5173)).toEqual([3000, 5173, 8080]);
    expect(withAppPort([5173], 5173)).toEqual([5173]);
    expect(withoutAppPort([3000, 5173, 8080], 5173)).toEqual([3000, 8080]);
    expect(withoutAppPort([3000], 5173)).toEqual([3000]);
  });

  it("reads a workspace's ports as saved, leaving out what is not one", () => {
    expect(appPortsOf({})).toEqual([]);
    expect(appPortsOf({ mobilePorts: [8080, 3000, 8080] })).toEqual([3000, 8080]);
    expect(appPortsOf({ mobilePorts: [80, 3000.5, 70000, "5173", null, 4000] })).toEqual([4000]);
    expect(appPortsOf({ mobilePorts: "5173" })).toEqual([]);
  });
});
