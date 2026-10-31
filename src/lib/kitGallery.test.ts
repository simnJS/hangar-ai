import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { en, fr } from "../strings/kitsManage";
import type { KitInfo } from "./kits";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const { filterGallery, galleryInstallUrl, galleryStatus, isNewer } = await import("./kitGallery");
const { KitsGalleryView } = await import("../components/settings/KitsGallery");
type GalleryKit = import("./kitGallery").GalleryKit;
type Gallery = import("./kitGallery").Gallery;
type GalleryProps = Parameters<typeof KitsGalleryView>[0];

const entry = (over: Partial<GalleryKit> = {}): GalleryKit => ({
  id: "ci-status",
  name: "CI status",
  description: "Follows a GitHub Actions run.",
  path: "kits/ci-status",
  version: "0.2.0",
  permissionsSummary: "Runs gh.",
  tags: ["ci", "github"],
  ...over,
});

const installed = (over: Partial<KitInfo> = {}): KitInfo =>
  ({
    id: "ci-status",
    source: { kind: "git", url: "https://github.com/simnJS/hangar-ai", ref: "main", subdir: "kits/ci-status" },
    dir: "C:\\k",
    installedVersion: "0.1.0",
    addedAt: 0,
    manifest: null,
    error: null,
    update: null,
    ...over,
  }) as KitInfo;

describe("isNewer", () => {
  it("compares numbers, not text", () => {
    expect(isNewer("0.10.0", "0.9.0")).toBe(true);
    expect(isNewer("1.0.0", "1.0.0")).toBe(false);
    expect(isNewer("1.0.0", "1.0.1")).toBe(false);
  });
  it("puts a release above its pre-release", () => {
    expect(isNewer("1.0.0", "1.0.0-rc.1")).toBe(true);
    expect(isNewer("1.0.0-rc.1", "1.0.0")).toBe(false);
  });
  it("says no when either is not a version", () => {
    expect(isNewer("latest", "1.0.0")).toBe(false);
    expect(isNewer("1.0.0", "1.0")).toBe(false);
  });
});

describe("galleryStatus", () => {
  it("is new when nothing of that id is installed", () => {
    expect(galleryStatus(entry(), [])).toEqual({ kind: "new" });
  });
  it("is installed at the version it has", () => {
    expect(galleryStatus(entry({ version: "0.1.0" }), [installed()])).toEqual({ kind: "installed", version: "0.1.0" });
  });
  it("offers an update when the index has a newer version", () => {
    expect(galleryStatus(entry(), [installed()])).toEqual({ kind: "update", version: "0.1.0", latest: "0.2.0" });
  });
  it("does not offer an update over a linked folder", () => {
    const folder = installed({ source: { kind: "folder", path: "D:\\ci-status" } });
    expect(galleryStatus(entry(), [folder])).toEqual({ kind: "installed", version: "0.1.0" });
  });
});

describe("filterGallery", () => {
  const kits = [entry(), entry({ id: "roblox", name: "Roblox", description: "Places.", tags: ["game"] })];
  it("keeps everything without a query", () => {
    expect(filterGallery(kits, "  ")).toHaveLength(2);
  });
  it("matches name, description and tags, every word", () => {
    expect(filterGallery(kits, "GAME").map((k) => k.id)).toEqual(["roblox"]);
    expect(filterGallery(kits, "actions ci").map((k) => k.id)).toEqual(["ci-status"]);
    expect(filterGallery(kits, "actions game")).toEqual([]);
  });
});

describe("galleryInstallUrl", () => {
  it("names the kit's folder in the one repository", () => {
    expect(galleryInstallUrl("ci-status")).toBe("https://github.com/simnJS/hangar-ai/tree/main/kits/ci-status");
  });
});

describe("the library strings", () => {
  it("are in both languages, and the French is not the English", () => {
    const keys = Object.keys(en).filter((key) => key.startsWith("kitsManage.gallery"));
    expect(keys.length).toBeGreaterThan(10);
    for (const key of keys) {
      expect(fr[key as keyof typeof fr], key).toBeTruthy();
      expect(fr[key as keyof typeof fr], key).not.toBe(en[key as keyof typeof en]);
    }
    expect(fr["kitsManage.galleryTitle"]).toBe("Bibliothèque");
  });
});

describe("KitsGalleryView", () => {
  const t = (key: string, vars?: Record<string, string | number>) =>
    (en[key as keyof typeof en] ?? key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ""));
  const live: Gallery = { origin: "live", kits: [entry()], fetchedAt: 1 };
  const draw = (over: Partial<GalleryProps> = {}) =>
    renderToStaticMarkup(
      createElement(KitsGalleryView, {
        t: t as GalleryProps["t"],
        gallery: live,
        error: null,
        refreshing: false,
        installed: [],
        busy: null,
        failure: null,
        query: "",
        onQuery: () => undefined,
        onRefresh: () => undefined,
        onInstall: () => undefined,
        onUpdate: () => undefined,
        ...over,
      }),
    );

  it("shows the kit with its tags and an Install button, and says nothing is enabled", () => {
    const html = draw();
    expect(html).toContain("CI status");
    expect(html).toContain("Follows a GitHub Actions run.");
    expect(html).toContain(">github<");
    expect(html).toContain("Install");
    expect(html).toContain("nothing is enabled");
    expect(html).toContain("Runs gh.");
  });

  it("says which version is installed, and offers the update", () => {
    expect(draw({ installed: [installed({ installedVersion: "0.2.0" })] })).toContain("Installed (v0.2.0)");
    const html = draw({ installed: [installed()] });
    expect(html).toContain("Installed v0.1.0, v0.2.0 available");
    expect(html).toContain("Update to v0.2.0");
  });

  it("says when it shows the kept copy", () => {
    expect(draw({ gallery: { ...live, origin: "cached" } })).toContain("Offline: showing the copy kept on");
  });

  it("says plainly when there is no list at all", () => {
    const html = draw({ gallery: { origin: "unavailable", kits: [], fetchedAt: null } });
    expect(html).toContain("cannot be reached");
    expect(html).not.toContain("lists no kit");
  });

  it("filters, and says when nothing matches", () => {
    expect(draw({ query: "zzz" })).toContain("No kit matches this filter.");
  });

  it("has the filter and the buttons reachable by keyboard, with names", () => {
    const html = draw();
    expect(html).toContain('aria-label="Filter the library"');
    expect(html).toContain('aria-label="Install CI status"');
    expect(html).not.toContain("tabindex");
  });
});
