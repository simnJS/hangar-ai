/**
 * The strings of the git branch a pane's header shows. i18n.ts is not asked
 * to merge them: `paneBranchText` reads the dictionary itself, so nothing
 * outside this file changes. Keys start with `paneBranch.`.
 *
 * `en` is the reference. `fr` is typed against it, so the build fails on a
 * French key that is missing or misspelled. French follows the glossary of
 * i18n.ts: « branche », « détaché », « worktree ».
 */

import type { Locale } from "../i18n";

export const en = {
  "paneBranch.branch": "Git branch {branch}",
  "paneBranch.branchWorktree": "Git branch {branch}, in a linked worktree",
  "paneBranch.detached": "Detached HEAD on {head}",
  "paneBranch.detachedMark": "detached",
} as const;

export const fr: Record<keyof typeof en, string> = {
  "paneBranch.branch": "Branche git {branch}",
  "paneBranch.branchWorktree": "Branche git {branch}, dans un worktree lié",
  "paneBranch.detached": "HEAD détachée sur {head}",
  "paneBranch.detachedMark": "détaché",
};

export type PaneBranchKey = keyof typeof en;

const DICTIONARIES: Record<Locale, Record<PaneBranchKey, string>> = { en, fr };

export function paneBranchText(
  locale: Locale,
  key: PaneBranchKey,
  params: Record<string, string> = {},
): string {
  return DICTIONARIES[locale][key].replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? params[name] : whole,
  );
}
