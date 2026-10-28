import { describe, expect, it } from "vitest";
import {
  canPair,
  formatCountdown,
  formatStamp,
  newDevices,
  remoteAddress,
  secondsLeft,
  sortDevices,
  tailscaleStep,
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
