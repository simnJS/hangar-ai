import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ApiError, type App } from "./api";
import {
  afterListing,
  appChip,
  appDetail,
  appName,
  appsOf,
  MISSES_BEFORE_STOPPED,
  openFailure,
  workerApps,
  type Viewer,
} from "./apps";
import { AppButton, AppList } from "./components/Apps";
import { t, translator } from "./i18n";
import { AppView, type AppViewProps } from "./screens/AppViewer";

const en = translator("en");
const fr = translator("fr");

const cora: App = {
  workspaceId: "ws-iabench",
  workspace: "IaBench",
  port: 21040,
  source: "worker",
  pane: "Cora",
  task: "The phone app (PWA)",
};
const ada: App = { workspaceId: "ws-iabench", workspace: "IaBench", port: 21003, source: "worker", pane: "Ada" };
const preview: App = { workspaceId: "ws-storefront", workspace: "storefront", port: 4173, source: "custom" };
const all = [cora, ada, preview];

/** The text a person reads, without the markup. */
const text = (html: string) =>
  html
    .replace(/<[^>]+>/g, " ")
    .replace(/&#x27;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ");

describe("the apps a project offers", () => {
  it("are its own, in the PC's order", () => {
    expect(appsOf(all, "ws-iabench")).toEqual([cora, ada]);
    expect(appsOf(all, "ws-billing")).toEqual([]);
    expect(appsOf(null, "ws-iabench")).toEqual([]);
  });

  it("a worker's are those of its port block, by its pane's name", () => {
    expect(workerApps(all, "ws-iabench", "Cora")).toEqual([cora]);
    expect(workerApps(all, "ws-iabench", "Jack")).toEqual([]);
    // A port the person added is no worker's.
    expect(workerApps([{ ...preview, pane: "Mia" }], "ws-storefront", "Mia")).toEqual([]);
  });

  it("offer a chip: the app itself when there is one, the list when there are more, nothing without", () => {
    expect(appChip([])).toBeNull();
    expect(appChip([preview])).toEqual({ kind: "one", app: preview });
    expect(appChip([cora, ada])).toEqual({ kind: "many", count: 2 });
  });

  it("are named after their worker, or as the person's own server, in both languages", () => {
    expect(appName(cora, en)).toBe("Cora");
    expect(appName(preview, en)).toBe("Your dev server");
    expect(appName(preview, fr)).toBe("Votre serveur de dev");
    expect(appDetail(cora, en)).toBe("The phone app (PWA)");
    expect(appDetail(ada, fr)).toBe("Le serveur de son worker");
    expect(appDetail(preview, fr)).toBe("Port ajouté dans Réglages › Mobile");
  });
});

describe("opening an app", () => {
  const refusal = (code: string, status: number, detail?: string, kind: ApiError["kind"] = "refused") =>
    new ApiError(kind, code, status, detail);

  it("says the dev server stopped when nothing listens on its port any more", () => {
    expect(openFailure(refusal("not-listening", 409), 5173, en)).toEqual({ kind: "stopped" });
  });

  it("says an app it has seen that the PC no longer offers stopped, and refuses one it never offered", () => {
    expect(openFailure(refusal("not-allowed", 403), 5173, en, true)).toEqual({ kind: "stopped" });
    const refused = openFailure(refusal("not-allowed", 403), 5173, en);
    expect(refused).toEqual({ kind: "refused", reason: en.t("app.notAllowed", { port: 5173 }) });
    expect((refused as { reason: string }).reason).toContain("5173");
  });

  it("gives Tailscale's own words, and says when every slot is taken", () => {
    expect(openFailure(refusal("tailscale", 503, "Tailscale is signed out", "busy"), 5173, en)).toEqual({
      kind: "refused",
      reason: "Tailscale: Tailscale is signed out",
    });
    expect(openFailure(refusal("no-free-slot", 503, undefined, "busy"), 5173, fr)).toEqual({
      kind: "refused",
      reason: fr.t("app.noSlot"),
    });
  });

  it("keeps the window's words when it refused", () => {
    expect(openFailure(refusal("refused", 400, "Hangar.AI is still starting."), 5173, en)).toEqual({
      kind: "refused",
      reason: "Hangar.AI is still starting.",
    });
  });

  it("offers to try again when the PC could not be asked, or was busy", () => {
    expect(openFailure(new ApiError("offline", "timeout"), 5173, en)).toEqual({ kind: "failed", reason: t("error.offline") });
    expect(openFailure(refusal("pc-busy", 503, undefined, "busy"), 5173, en)).toEqual({ kind: "failed", reason: t("error.busy") });
  });
});

describe("watching an app on screen", () => {
  const shown: Viewer = { kind: "shown", url: "https://pc.ts.net:8443/" };

  it("says it stopped once the PC has left it out of its list twice in a row", () => {
    expect(MISSES_BEFORE_STOPPED).toBe(2);
    const first = afterListing(shown, false, 0);
    expect(first).toEqual({ viewer: shown, misses: 1 });
    expect(afterListing(first.viewer, false, first.misses)).toEqual({ viewer: { kind: "stopped" }, misses: 0 });
  });

  it("forgets a miss as soon as the app is listed again: a server restarting", () => {
    const missed = afterListing(shown, false, 0);
    expect(afterListing(missed.viewer, true, missed.misses)).toEqual({ viewer: shown, misses: 0 });
  });

  it("watches an app on its way too, and leaves any other state as it is", () => {
    const loading: Viewer = { kind: "loading", url: "https://pc.ts.net:8443/" };
    expect(afterListing(loading, false, 1).viewer).toEqual({ kind: "stopped" });
    for (const viewer of [{ kind: "starting" }, { kind: "stopped" }, { kind: "refused", reason: "no" }] as Viewer[]) {
      const after = afterListing(viewer, false, 1);
      expect(after.viewer).toBe(viewer);
      expect(after.misses).toBe(0);
    }
  });
});

describe("the « Voir l'app » chip", () => {
  const render = (apps: App[], variant: "card" | "head" = "card") =>
    renderToStaticMarkup(createElement(AppButton, { apps, projectId: "ws-iabench", variant }));

  it("is not there without an app running", () => {
    expect(render([])).toBe("");
  });

  it("opens the one app, named in its label", () => {
    const html = render([cora]);
    expect(text(html)).toContain(t("apps.see"));
    expect(html).toContain(`aria-label="${t("apps.open", { name: "Cora", port: 21040 }).replace(/'/g, "&#x27;")}"`);
    expect(html).toContain('class="app-chip"');
  });

  it("leads to the project's list when there are several", () => {
    expect(text(render([cora, ada]))).toContain(t("apps.seeMany", { n: 2 }));
  });

  it("sits in a worker's header as its actions do", () => {
    expect(render([cora], "head")).toContain('class="head-action head-action--app"');
  });
});

describe("a project's apps running", () => {
  it("lists each, with its port and its worker's task or where its port was added", () => {
    const html = renderToStaticMarkup(createElement(AppList, { apps: [cora, preview] }));
    const read = text(html);
    expect(read).toContain(t("apps.title"));
    expect(read).toContain("Cora");
    expect(read).toContain(t("apps.port", { port: 21040 }));
    expect(read).toContain("The phone app (PWA)");
    expect(read).toContain(t("apps.custom"));
    expect(read).toContain(t("apps.customHint"));
    expect(html).toContain('href="#/app/ws-iabench/21040"');
    expect(html).toContain('href="#/app/ws-storefront/4173"');
  });

  it("is not drawn without any", () => {
    expect(renderToStaticMarkup(createElement(AppList, { apps: [] }))).toBe("");
  });
});

describe("the viewer", () => {
  const URL = "https://pc.tail1234.ts.net:8443/";
  const render = (viewer: Viewer, patch: Partial<AppViewProps> = {}) =>
    renderToStaticMarkup(
      createElement(AppView, {
        name: "Cora",
        where: "IaBench",
        port: 21040,
        viewer,
        frame: 1,
        onBack: () => {},
        onReload: () => {},
        onLoaded: () => {},
        ...patch,
      }),
    );

  it("names the app and its port in its bar, with back, reload and Safari", () => {
    const html = render({ kind: "shown", url: URL });
    expect(text(html)).toContain("Cora");
    expect(text(html)).toContain(`IaBench · ${t("apps.port", { port: 21040 })}`);
    expect(html).toContain(`aria-label="${t("app.back")}"`);
    expect(html).toContain(`aria-label="${t("app.reload")}"`);
    expect(html).toContain(`aria-label="${t("app.safari")}"`);
  });

  it("starting: says so while the PC opens it, nothing to reload or take to Safari yet", () => {
    const html = render({ kind: "starting" });
    expect(text(html)).toContain(t("app.starting"));
    expect(text(html)).toContain(t("app.startingHint"));
    expect(html).toContain('role="status"');
    expect(html).not.toContain("<iframe");
    expect(html.match(/disabled=""/g)).toHaveLength(2);
  });

  it("loading: the app's page in its frame, the wait over it", () => {
    const html = render({ kind: "loading", url: URL });
    expect(html).toContain(`<iframe class="viewer__frame" src="${URL}"`);
    expect(html).toContain(`title="${t("app.frame", { name: "Cora", port: 21040 })}"`);
    expect(text(html)).toContain(t("app.loading"));
    expect(html).toContain("viewer__state--over");
  });

  it("shown: the app alone, and Safari opens its address", () => {
    const html = render({ kind: "shown", url: URL });
    expect(html).toContain("<iframe");
    expect(html).not.toContain("viewer__state");
    expect(html).toContain(`href="${URL}" target="_blank" rel="noopener noreferrer"`);
    expect(html).not.toContain('disabled=""');
  });

  it("stopped: the dev server stopped, on which port, and to try again", () => {
    const html = render({ kind: "stopped" });
    expect(text(html)).toContain(t("app.stopped"));
    expect(text(html)).toContain(t("app.stoppedHint", { port: 21040 }));
    expect(text(html)).toContain(t("error.retry"));
    expect(html).toContain('role="alert"');
    expect(html).not.toContain("<iframe");
  });

  it("refused: why, in the PC's words", () => {
    const html = render({ kind: "refused", reason: "Tailscale: Tailscale is signed out" });
    expect(text(html)).toContain(t("app.refused"));
    expect(text(html)).toContain("Tailscale: Tailscale is signed out");
    expect(text(html)).toContain(t("error.retry"));
  });

  it("failed: the PC did not answer, and to try again", () => {
    const html = render({ kind: "failed", reason: t("error.offline") });
    expect(text(html)).toContain(t("app.failed"));
    expect(text(html)).toContain(t("error.offline"));
  });
});
