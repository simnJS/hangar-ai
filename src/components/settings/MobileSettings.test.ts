import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { RemoteDevice, RemoteStatus, TailscaleStatus } from "../../lib/remote";

// The view is drawn from props; the dictionaries need not the store, which talks to Tauri.
vi.mock("../../store", () => ({ useStore: () => ({}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const { MobileView } = await import("./MobileSettings");
const { translate } = await import("../../i18n");
type Props = Parameters<typeof MobileView>[0];

const NOW = Date.UTC(2026, 9, 28, 20, 0, 0);

const on: RemoteStatus = { enabled: true, running: true, port: 4000, error: null };
const off: RemoteStatus = { ...on, enabled: false, running: false };
const ts = (patch: Partial<TailscaleStatus> = {}): TailscaleStatus => ({
  installed: true,
  running: true,
  dnsName: "pc.tail1234.ts.net",
  serving: true,
  ...patch,
});
const phone: RemoteDevice = {
  id: "d1",
  name: "iPhone de Sam",
  created: NOW - 86_400_000,
  lastSeen: NOW - 60_000,
};

function props(locale: "en" | "fr", patch: Partial<Props> = {}): Props {
  const noop = () => undefined;
  return {
    t: (key, params) => translate(locale, key, params),
    locale,
    now: NOW,
    remote: on,
    tailscale: ts(),
    devices: [],
    pairing: null,
    justPaired: null,
    revoking: null,
    busy: { toggle: false, serve: false, pair: false, options: false },
    error: null,
    options: { keepAwake: false, keepAwakeSupported: true, autostart: false },
    onKeepAwake: noop,
    onAutostart: noop,
    onToggle: noop,
    onServe: noop,
    onRecheck: noop,
    onOpenLink: noop,
    onPairStart: noop,
    onPairClose: noop,
    onRevokeAsk: noop,
    onRevoke: noop,
    ...patch,
  };
}

const render = (locale: "en" | "fr", patch?: Partial<Props>) =>
  renderToStaticMarkup(createElement(MobileView, props(locale, patch)));

const pairing = (expiresInMs: number) => ({
  code: "123456",
  url: "https://pc.tail1234.ts.net/?code=123456",
  qrSvg: '<svg viewBox="0 0 1 1"><rect width="1" height="1"/></svg>',
  expiresAt: NOW + expiresInMs,
});

describe.each(["en", "fr"] as const)("Mobile settings (%s)", (locale) => {
  // As React writes it into markup.
  const text = (key: Parameters<typeof translate>[1], params?: Record<string, string>) =>
    translate(locale, key, params)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#x27;");

  it("off: only the switch, unchecked", () => {
    const html = render(locale, { remote: off });
    expect(html).toContain(text("mobile.switch"));
    expect(html).not.toContain("checked");
    expect(html).not.toContain(text("mobile.tailscale") + "</h3>");
    expect(html).not.toContain(text("mobile.pair"));
  });

  it("before the PC answers, the switch is out of reach", () => {
    expect(render(locale, { remote: null })).toContain("disabled");
  });

  it("Tailscale missing: says what it is and links to the download", () => {
    const html = render(locale, {
      tailscale: ts({ installed: false, running: false, dnsName: null, serving: false }),
    });
    expect(html).toContain(text("mobile.tailscaleMissing"));
    expect(html).toContain(text("mobile.tailscaleDownload"));
    expect(html).not.toContain(text("mobile.serve"));
  });

  it("Tailscale not running: says so, offers to check again", () => {
    const html = render(locale, { tailscale: ts({ running: false, dnsName: null, serving: false }) });
    expect(html).toContain(text("mobile.tailscaleStopped"));
    expect(html).toContain(text("mobile.recheck"));
  });

  it("not serving: a button, and what it changes", () => {
    const html = render(locale, { tailscale: ts({ serving: false }) });
    expect(html).toContain(text("mobile.serve"));
    expect(html).toContain(text("mobile.serveHint"));
    expect(html).not.toContain("https://pc.tail1234.ts.net<");
    expect(render(locale, { tailscale: ts({ serving: false }), busy: { toggle: false, serve: true, pair: false, options: false } })).toContain(
      text("mobile.serving"),
    );
  });

  it("ready: the address, and the pairing button is open", () => {
    const html = render(locale);
    expect(html).toContain("https://pc.tail1234.ts.net");
    expect(html).toContain(text("mobile.pairShow"));
    expect(html).not.toContain(text("mobile.pairBlocked"));
    expect(html).toContain(text("mobile.iphoneOpen"));
  });

  it("leaving for a while: the two options, off until the person turns them on", () => {
    const html = render(locale);
    expect(html).toContain(text("mobile.away"));
    expect(html).toContain(text("mobile.keepAwake"));
    expect(html).toContain(text("mobile.keepAwakeHint"));
    expect(html).toContain(text("mobile.autostart"));
    expect(html).toContain(text("mobile.autostartHint"));
    // The Hangar Mobile switch alone is on.
    expect(html.match(/checked/g)).toHaveLength(1);
  });

  it("the options as the PC has them", () => {
    const html = render(locale, { options: { keepAwake: true, keepAwakeSupported: true, autostart: false } });
    expect(html.match(/checked/g)).toHaveLength(2);
    expect(render(locale, { options: { keepAwake: true, keepAwakeSupported: true, autostart: true } }).match(/checked/g)).toHaveLength(3);
  });

  it("keeping the PC awake says it is not supported where it is not, and cannot be turned on", () => {
    const html = render(locale, { options: { keepAwake: false, keepAwakeSupported: false, autostart: false } });
    expect(html).toContain(text("mobile.keepAwakeUnsupported"));
    expect(html).not.toContain(text("mobile.keepAwakeHint"));
    // Only the unsupported option is out of reach.
    expect(html.match(/disabled/g)).toHaveLength(1);
  });

  it("the options are not drawn before the PC has answered, nor while Hangar Mobile is off", () => {
    expect(render(locale, { options: null })).not.toContain(text("mobile.away"));
    expect(render(locale, { remote: off })).not.toContain(text("mobile.away"));
  });

  it("pairing blocked until Tailscale is ready", () => {
    expect(render(locale, { tailscale: ts({ serving: false }) })).toContain(
      text("mobile.pairBlocked"),
    );
  });

  it("pairing: the QR code, the code and the countdown", () => {
    const html = render(locale, { pairing: pairing(272_000) });
    expect(html).toContain("<svg");
    expect(html).toContain("123456");
    expect(html).toContain(text("mobile.pairExpires", { time: "4:32" }));
    expect(html).not.toContain(text("mobile.pairAgain"));
  });

  it("pairing expired: a new code is offered", () => {
    const html = render(locale, { pairing: pairing(-1000) });
    expect(html).toContain(text("mobile.pairExpired"));
    expect(html).toContain(text("mobile.pairAgain"));
  });

  it("just paired: the phone is named", () => {
    expect(render(locale, { justPaired: "iPhone de Sam" })).toContain("iPhone de Sam");
  });

  it("devices: none, then the list with its dates", () => {
    expect(render(locale)).toContain(text("mobile.devicesEmpty"));
    const html = render(locale, { devices: [phone, { ...phone, id: "d2", name: "Pixel", lastSeen: 0 }] });
    expect(html).toContain("iPhone de Sam");
    expect(html).toContain(text("mobile.deviceNeverSeen"));
    expect(html).toContain(text("mobile.revoke"));
    expect(html).not.toContain(text("mobile.revokeAsk", { name: "iPhone de Sam" }));
  });

  it("revoke confirmation: inline, on that device only", () => {
    const html = render(locale, { devices: [phone, { ...phone, id: "d2", name: "Pixel" }], revoking: "d1" });
    expect(html).toContain(text("mobile.revokeAsk", { name: "iPhone de Sam" }));
    expect(html).not.toContain(text("mobile.revokeAsk", { name: "Pixel" }));
  });

  it("shows a failure to start", () => {
    expect(render(locale, { remote: { ...on, running: false, error: "port 4000 taken" } })).toContain(
      "port 4000 taken",
    );
  });
});
