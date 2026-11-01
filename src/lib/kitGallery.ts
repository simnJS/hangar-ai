import { invoke } from "@tauri-apps/api/core";
import { kitPinnedTag, type KitInfo } from "./kits";

/**
 * The kit gallery: the public kits of the `hangar-ai` organization, listed by a
 * static file of its catalog repository that Rust reads and checks. No server of
 * ours, and one source. Each kit is pinned to a tag of its own repository, and
 * installing is the existing `kit_add_git`, on the link to that tag.
 */

export interface GalleryKit {
  id: string;
  name: string;
  description: string;
  /** The kit's repository on GitHub, `owner/name`. */
  repo: string;
  /** The tag of that repository the catalog pins it to. */
  tag: string;
  version: string;
  /** What it will ask to run, in a sentence; the activation shows the truth. */
  permissionsSummary: string;
  tags: string[];
  /** Checked by Rust: the catalog says so and the owner is `hangar-ai`. Otherwise a community kit. */
  verified: boolean;
}

export interface Gallery {
  /**
   * `live`: just read, a catalog that is not published being an empty list.
   * `cached`: the last good copy, the catalog could not be read. `unavailable`: neither.
   */
  origin: "live" | "cached" | "unavailable";
  kits: GalleryKit[];
  /** ms since the epoch; null when unavailable. */
  fetchedAt: number | null;
}

export const kitGalleryList = () => invoke<Gallery>("kit_gallery_list");

/** The link `kit_add_git` is given for a gallery kit. */
export const galleryInstallUrl = (kit: Pick<GalleryKit, "repo" | "tag">) =>
  `https://github.com/${kit.repo}/tree/${kit.tag}`;

/** What a gallery card says of a kit on this machine; `pinned` is the tag it is pinned to. */
export type GalleryStatus =
  | { kind: "new" }
  | { kind: "installed"; version: string; pinned: string | null }
  /** A folder of the same id is linked: it is the person's own copy, not this catalog kit. */
  | { kind: "linked"; version: string }
  | { kind: "update"; version: string; latest: string; pinned: string };

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

const fromRepo = (kit: KitInfo, repo: string) =>
  kit.source.kind === "git" && kit.source.url.toLowerCase() === `https://github.com/${repo}`.toLowerCase();

export function galleryStatus(entry: GalleryKit, installed: readonly KitInfo[]): GalleryStatus {
  const kit = installed.find((item) => item.id === entry.id);
  if (!kit) return { kind: "new" };
  const version = kit.installedVersion;
  // Not installed from the catalog, and installing would clash on the id: say so.
  if (kit.source.kind === "folder") return { kind: "linked", version };
  const pinned = kitPinnedTag(kit.source);
  // As Rust goes by (kits/gallery.rs, `tag_for`): the catalog only has a say on a
  // kit pinned from this very repository. A linked folder is the person's own copy,
  // a kit on a branch follows its branch, and another repository is another kit.
  return pinned !== null && pinned !== entry.tag && fromRepo(kit, entry.repo) && isNewer(entry.version, version)
    ? { kind: "update", version, latest: entry.version, pinned }
    : { kind: "installed", version, pinned };
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
