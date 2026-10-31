import { invoke } from "@tauri-apps/api/core";
import type { KitInfo } from "./kits";

/**
 * The kit gallery: the public kits of the Hangar repository, listed by a static
 * file (`kits/index.json`) that Rust reads. No server of ours, and one source.
 * Installing is the existing `kit_add_git`, on the kit's folder.
 */

/** The repository's kits folder: the one place a gallery kit is installed from. */
export const GALLERY_KITS_URL = "https://github.com/simnJS/hangar-ai/tree/main/kits";

export interface GalleryKit {
  id: string;
  name: string;
  description: string;
  /** The kit's folder in the repository, like `kits/ci-status`. */
  path: string;
  version: string;
  /** What it will ask to run, in a sentence; the activation shows the truth. */
  permissionsSummary: string;
  tags: string[];
}

export interface Gallery {
  /** `live`: just fetched. `cached`: the last good copy, offline. `unavailable`: neither. */
  origin: "live" | "cached" | "unavailable";
  kits: GalleryKit[];
  /** ms since the epoch; null when unavailable. */
  fetchedAt: number | null;
}

export const kitGalleryList = () => invoke<Gallery>("kit_gallery_list");

/** The link `kit_add_git` is given for a gallery kit. */
export const galleryInstallUrl = (id: string) => `${GALLERY_KITS_URL}/${id}`;

/** What a gallery card says of a kit on this machine. */
export type GalleryStatus =
  | { kind: "new" }
  | { kind: "installed"; version: string }
  | { kind: "update"; version: string; latest: string };

/** `1.2.3-rc.1` as numbers and whether it is a pre-release; null when it is not semver. */
function parse(version: string): { nums: [number, number, number]; pre: string | null } | null {
  const match = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(version.trim());
  if (!match) return null;
  return { nums: [Number(match[1]), Number(match[2]), Number(match[3])], pre: match[4] ?? null };
}

/** Whether `a` is a later version than `b`; false when either is not semver. */
export function isNewer(a: string, b: string): boolean {
  const left = parse(a);
  const right = parse(b);
  if (!left || !right) return false;
  for (let i = 0; i < 3; i++) {
    if (left.nums[i] !== right.nums[i]) return left.nums[i] > right.nums[i];
  }
  if (left.pre === right.pre) return false;
  if (left.pre === null) return true;
  if (right.pre === null) return false;
  return left.pre > right.pre;
}

export function galleryStatus(entry: GalleryKit, installed: readonly KitInfo[]): GalleryStatus {
  const kit = installed.find((item) => item.id === entry.id);
  if (!kit) return { kind: "new" };
  // A linked folder is the person's own copy: the index has no say on it.
  if (kit.source.kind === "folder") return { kind: "installed", version: kit.installedVersion };
  return isNewer(entry.version, kit.installedVersion)
    ? { kind: "update", version: kit.installedVersion, latest: entry.version }
    : { kind: "installed", version: kit.installedVersion };
}

/** The kits whose name, description or a tag holds every word of the query. */
export function filterGallery(kits: readonly GalleryKit[], query: string): GalleryKit[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...kits];
  return kits.filter((kit) => {
    const hay = [kit.id, kit.name, kit.description, ...kit.tags].join(" ").toLowerCase();
    return words.every((word) => hay.includes(word));
  });
}
