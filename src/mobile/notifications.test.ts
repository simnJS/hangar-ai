import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { NotificationsView, type NotificationsViewProps } from "./components/Notifications";
import { t } from "./i18n";
import type { PushStatus } from "./push";

/** The notifications switch on For you, in each state it can be in. */
const render = (patch: Partial<NotificationsViewProps> & { status: PushStatus }) =>
  renderToStaticMarkup(
    createElement(NotificationsView, {
      busy: false,
      error: null,
      dismissed: false,
      explaining: false,
      onTurnOn: () => {},
      onTurnOff: () => {},
      onExplain: () => {},
      onNotNow: () => {},
      ...patch,
    }),
  );

const text = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'").replace(/&quot;/g, '"');

describe("the notifications switch", () => {
  it("shows nothing while it is being checked", () => {
    expect(render({ status: "checking" })).toBe("");
  });

  it("offers to turn them on, or not now", () => {
    const html = text(render({ status: "off" }));
    expect(html).toContain(t("push.title"));
    expect(html).toContain(t("push.turnOn"));
    expect(html).toContain(t("push.notNow"));
  });

  it("says it is at it while the browser asks", () => {
    const html = render({ status: "off", busy: true });
    expect(text(html)).toContain(t("push.turningOn"));
    expect(html).toMatch(/<button[^>]*disabled/);
  });

  it("offers the same on an iPhone in Safari, and explains there what it takes", () => {
    expect(text(render({ status: "install" }))).toContain(t("push.turnOn"));
    const html = text(render({ status: "install", explaining: true }));
    expect(html).toContain(t("push.installTitle"));
    expect(html).toContain(t("push.install"));
    expect(html).toContain("16.4");
  });

  it("explains an iPhone too old and a browser without push", () => {
    expect(text(render({ status: "oldIos", explaining: true }))).toContain(t("push.oldIos"));
    expect(text(render({ status: "unsupported", explaining: true }))).toContain(t("push.unsupported"));
  });

  it("is one row once on, with the switch to turn them off", () => {
    const html = text(render({ status: "on" }));
    expect(html).toContain(t("push.on"));
    expect(html).toContain(t("push.turnOff"));
    expect(html).not.toContain(t("push.title"));
  });

  it("is one row once turned down, to turn them on later", () => {
    const html = text(render({ status: "off", dismissed: true }));
    expect(html).toContain(t("push.off"));
    expect(html).toContain(t("push.turnOnShort"));
    expect(text(render({ status: "install", dismissed: true }))).toContain(t("push.needsInstall"));
    expect(text(render({ status: "unsupported", dismissed: true }))).toContain(t("push.why"));
  });

  it("says they are blocked, and how to allow them", () => {
    const html = text(render({ status: "denied" }));
    expect(html).toContain(t("push.blocked"));
    expect(html).toContain(t("push.why"));
    expect(text(render({ status: "denied", explaining: true }))).toContain(t("push.denied"));
  });

  it("says why they could not be turned on", () => {
    const error = t("push.failed", { reason: t("error.offline") });
    for (const status of ["off", "on"] as const) {
      const html = render({ status, error });
      expect(html).toContain('role="alert"');
      expect(text(html)).toContain(t("error.offline"));
    }
  });
});
