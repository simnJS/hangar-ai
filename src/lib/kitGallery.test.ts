import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { en, fr } from "../strings/kitsManage";
import ui from "../../crates/hangar-core/kit-sdk/kit-ui.js?raw";
import sdk from "../../crates/hangar-core/kit-sdk/kit.js?raw";
import type { KitInfo } from "./kits";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("../store", () => ({ useStore: () => ({}) }));

const { filterGallery, galleryInstallUrl, galleryStatus, isNewer } = await import("./kitGallery");
const { kitPinnedTag } = await import("./kits");
const { KitsGalleryView } = await import("../components/settings/KitsGallery");
const { KitsView } = await import("../components/settings/KitsSettings");
type GalleryKit = import("./kitGallery").GalleryKit;
type Gallery = import("./kitGallery").Gallery;
type GalleryProps = Parameters<typeof KitsGalleryView>[0];

const entry = (over: Partial<GalleryKit> = {}): GalleryKit => ({
  id: "ci-status",
  name: "CI status",
  description: "Follows a GitHub Actions run.",
  repo: "hangar-ai/ci-status",
  tag: "v0.2.0",
  version: "0.2.0",
  permissionsSummary: "Runs gh.",
  tags: ["ci", "github"],
  verified: true,
  ...over,
});

const installed = (over: Partial<KitInfo> = {}): KitInfo =>
  ({
    id: "ci-status",
    source: { kind: "git", url: "https://github.com/hangar-ai/ci-status", ref: "refs/tags/v0.1.0" },
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
  const current = { kind: "installed", version: "0.1.0", pinned: "v0.1.0" };
  it("is installed at the version it has, pinned to its tag", () => {
    expect(galleryStatus(entry({ tag: "v0.1.0", version: "0.1.0" }), [installed()])).toEqual(current);
  });
  it("offers an update when the catalog names a new tag with a newer version", () => {
    expect(galleryStatus(entry(), [installed()])).toEqual({
      kind: "update",
      version: "0.1.0",
      latest: "0.2.0",
      pinned: "v0.1.0",
    });
  });
  it("finds the repository whatever its case, as GitHub does", () => {
    expect(galleryStatus(entry({ repo: "Hangar-AI/CI-status" }), [installed()]).kind).toBe("update");
  });
  it("offers nothing while the catalog names the same tag, whatever version it says", () => {
    expect(galleryStatus(entry({ tag: "v0.1.0" }), [installed()])).toEqual(current);
  });
  it("offers nothing when the new tag is not a newer version", () => {
    expect(galleryStatus(entry({ version: "0.1.0" }), [installed()])).toEqual(current);
    expect(galleryStatus(entry({ version: "0.0.9" }), [installed()])).toEqual(current);
  });
  it("offers nothing for the same id in another repository", () => {
    expect(galleryStatus(entry({ repo: "someone/ci-status" }), [installed()])).toEqual(current);
  });
  it("says a folder of that id is linked, and offers neither install nor update", () => {
    const folder = installed({ source: { kind: "folder", path: "D:\\ci-status" } });
    expect(galleryStatus(entry(), [folder])).toEqual({ kind: "linked", version: "0.1.0" });
  });
  it("does not offer an update over a kit on a branch or a plain link, which follow their own source", () => {
    const plain = { kind: "installed", version: "0.1.0", pinned: null };
    const url = "https://github.com/hangar-ai/ci-status";
    expect(galleryStatus(entry(), [installed({ source: { kind: "git", url } })])).toEqual(plain);
    expect(galleryStatus(entry(), [installed({ source: { kind: "git", url, ref: "main" } })])).toEqual(plain);
  });
});

describe("kitPinnedTag", () => {
  const url = "https://github.com/o/r";
  it("reads the tag of a kit pinned to one", () => {
    expect(kitPinnedTag({ kind: "git", url, ref: "refs/tags/v1.2.0" })).toBe("v1.2.0");
    expect(kitPinnedTag({ kind: "git", url, ref: "refs/tags/release/1", subdir: "kit" })).toBe("release/1");
  });
  it("is null for anything else", () => {
    expect(kitPinnedTag({ kind: "git", url })).toBeNull();
    expect(kitPinnedTag({ kind: "git", url, ref: "main" })).toBeNull();
    expect(kitPinnedTag({ kind: "git", url, ref: "v1.2.0" })).toBeNull();
    expect(kitPinnedTag({ kind: "folder", path: "D:\\kit" })).toBeNull();
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
  it("names the tag of the kit's own repository, with no folder", () => {
    expect(galleryInstallUrl(entry())).toBe("https://github.com/hangar-ai/ci-status/tree/v0.2.0");
    expect(galleryInstallUrl(entry({ repo: "someone/kit", tag: "1.0.0" }))).toBe(
      "https://github.com/someone/kit/tree/1.0.0",
    );
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

describe("the row of an installed kit", () => {
  type ViewProps = Parameters<typeof KitsView>[0];
  const noop = () => undefined;
  const row = (kit: KitInfo) =>
    renderToStaticMarkup(
      createElement(KitsView, {
        t: ((key: string, vars?: Record<string, string | number>) =>
          (en[key as keyof typeof en] ?? key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ""))) as ViewProps["t"],
        kits: [kit],
        error: null,
        flash: null,
        notes: {},
        removing: null,
        adding: null,
        folderError: null,
        gitUrl: "",
        gitError: null,
        onGitUrl: noop,
        onAddFolder: noop,
        onAddGit: noop,
        onCheck: noop,
        onUpdate: noop,
        onAskRemove: noop,
        onRemove: noop,
        workspaces: [],
        onManageWorkspace: noop,
        everywhere: [],
        hasActiveWorkspace: true,
        restartFor: null,
        onDismissRestart: noop,
        onToggleEverywhere: noop,
        onReviewEverywhere: noop,
      }),
    );

  it("says what a kit is pinned to in words, not by git's name for the tag", () => {
    const html = row(installed());
    expect(html).toContain("GitHub: https://github.com/hangar-ai/ci-status · pinned to v0.1.0");
    expect(html).not.toContain("refs/tags/");
  });

  it("keeps the folder of a pinned kit, and says nothing of a kit on a branch", () => {
    const url = "https://github.com/o/r";
    expect(row(installed({ source: { kind: "git", url, ref: "refs/tags/v1", subdir: "kit" } }))).toContain(
      "GitHub: https://github.com/o/r (kit) · pinned to v1",
    );
    const branch = row(installed({ source: { kind: "git", url, ref: "main" } }));
    expect(branch).toContain("GitHub: https://github.com/o/r (main)");
    expect(branch).not.toContain("pinned to");
  });
});

describe("confirm() of kit-ui.js, and the focus", () => {
  /** A page with a focus that moves: the box of the confirmation holds it, or something else does. */
  function stand() {
    const state: { active: any } = { active: null };
    const make = (tag: string) => {
      const node: Record<string, any> = { tag, children: [] as any[], hidden: false, focusCalls: 0 };
      node.append = (...kids: any[]) => node.children.push(...kids);
      node.after = (kid: any) => (node.next = kid);
      node.remove = () => {};
      node.setAttribute = () => {};
      node.addEventListener = () => {};
      node.contains = (other: any) => other === node || node.children.some((kid: any) => kid.contains?.(other));
      node.focus = () => {
        node.focusCalls++;
        state.active = node;
      };
      return node;
    };
    const document = {
      createElement: make,
      documentElement: { style: { setProperty() {}, removeProperty() {} }, setAttribute() {}, getAttribute: () => null },
      get activeElement() {
        return state.active;
      },
    };
    const page: Record<string, any> = { parent: { postMessage() {} }, addEventListener() {} };
    const globals = { window: page, document, navigator: { language: "en" }, console };
    for (const code of [sdk, ui]) new Function(...Object.keys(globals), code)(...Object.values(globals));
    return { make, state, confirm: page.hangar.ui.confirm };
  }

  it("does not take the focus back when the page cancels while it is elsewhere", async () => {
    const { make, state, confirm } = stand();
    const trigger = make("button");
    const elsewhere = make("input");
    const answer = confirm(trigger, { message: "Sure?" });
    elsewhere.focus();
    answer.cancel();
    await expect(answer).resolves.toBe(false);
    expect(trigger.focusCalls).toBe(0);
    expect(state.active).toBe(elsewhere);
  });

  it("gives the focus back to the button when the confirmation held it", async () => {
    const { make, state, confirm } = stand();
    const trigger = make("button");
    const answer = confirm(trigger, { message: "Sure?" });
    answer.cancel();
    await expect(answer).resolves.toBe(false);
    expect(trigger.focusCalls).toBe(1);
    expect(state.active).toBe(trigger);
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

  it("says what a kit installed from a tag is pinned to, with its version", () => {
    const current = entry({ tag: "v0.1.0", version: "0.1.0" });
    const html = draw({ gallery: { ...live, kits: [current] }, installed: [installed()] });
    expect(html).toContain("Installed (v0.1.0)");
    expect(html).toContain("Pinned to v0.1.0");
    expect(draw({ installed: [installed()] })).toContain("Pinned to v0.1.0");
  });

  it("says a linked folder holds that id, not that the kit is installed, and offers nothing", () => {
    const folder = installed({ source: { kind: "folder", path: "D:\\ci-status" } });
    const html = draw({ installed: [folder] });
    expect(html).toContain("linked, read live (v0.1.0)");
    expect(html).not.toContain("Installed");
    expect(html).not.toContain("Pinned to");
    expect(html).not.toContain("btn--primary");
  });

  it("does not say pinned of a kit that is not", () => {
    const branch = installed({ source: { kind: "git", url: "https://github.com/o/r", ref: "main" } });
    expect(draw({ installed: [branch] })).not.toContain("Pinned to");
    expect(draw()).not.toContain("Pinned to");
  });

  it("names the repository and the tag a kit installs from", () => {
    expect(draw()).toContain("hangar-ai/ci-status · v0.2.0");
  });

  it("shows a verified kit with its badge, and nothing about a review it did not have", () => {
    const html = draw();
    expect(html).toContain("Verified");
    expect(html).toContain("kit-act__tag--ok");
    expect(html).not.toContain("Community kit");
    expect(html).not.toContain("not reviewed");
  });

  it("shows anything else as a community kit, in one plain sentence, installed the same way", () => {
    const html = draw({ gallery: { ...live, kits: [entry({ verified: false })] } });
    expect(html).toContain("Community kit");
    expect(html).toContain("It was not reviewed by Hangar.");
    expect(html).not.toContain("Verified");
    expect(html).toContain('aria-label="Install CI status"');
    expect(html).toContain("nothing is enabled");
  });

  it("says when it shows the kept copy", () => {
    expect(draw({ gallery: { ...live, origin: "cached" } })).toContain(
      "The library could not be read: showing the copy kept on",
    );
  });

  it("reads an empty catalog as an empty library, not as a failure", () => {
    const html = draw({ gallery: { origin: "live", kits: [], fetchedAt: 1 } });
    expect(html).toContain("The library lists no kit yet.");
    expect(html).not.toContain("cannot be reached");
    expect(html).not.toContain("could not be read");
    expect(html).not.toContain('role="alert"');
    expect(html).not.toContain("No kit matches");
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
