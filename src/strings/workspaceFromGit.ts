/**
 * The strings of the consent card shown before an agent's clone
 * (components/GitCloneConsent). i18n.ts is not asked to merge them:
 * `workspaceFromGitText` reads the dictionary itself. Keys start with
 * `gitConsent.`.
 *
 * `en` is the reference. `fr` is typed against it, so the build fails on a
 * French key that is missing or misspelled. French follows the glossary of
 * i18n.ts: « workspace » (masculine), « panneau », « tu », infinitive buttons.
 */

import type { Locale } from "../i18n";

export const en = {
  "gitConsent.title": "{by} wants to clone a repository",
  "gitConsent.titleUnknown": "An agent wants to clone a repository",
  "gitConsent.repo": "Repository",
  "gitConsent.owner": "Owner",
  "gitConsent.destination": "Cloned into",
  "gitConsent.notYours": "It is not yours: {reason}.",
  "gitConsent.warning":
    "Installing its dependencies runs its code on this machine. Its orchestrator tells you what the install would run before it installs anything.",
  "gitConsent.clone": "Clone",
  "gitConsent.refuse": "Refuse",
  "gitConsent.more": "{n} more waiting",
} as const;

export const fr: Record<keyof typeof en, string> = {
  "gitConsent.title": "{by} veut cloner un dépôt",
  "gitConsent.titleUnknown": "Un agent veut cloner un dépôt",
  "gitConsent.repo": "Dépôt",
  "gitConsent.owner": "Propriétaire",
  "gitConsent.destination": "Cloné dans",
  "gitConsent.notYours": "Il n'est pas à toi : {reason}.",
  "gitConsent.warning":
    "Installer ses dépendances exécute son code sur cette machine. Son orchestrateur te dit ce que l'installation exécuterait avant d'installer quoi que ce soit.",
  "gitConsent.clone": "Cloner",
  "gitConsent.refuse": "Refuser",
  "gitConsent.more": "{n} autres en attente",
};

export type WorkspaceFromGitKey = keyof typeof en;

const DICTIONARIES: Record<Locale, Record<WorkspaceFromGitKey, string>> = { en, fr };

export function workspaceFromGitText(
  locale: Locale,
  key: WorkspaceFromGitKey,
  params: Record<string, string> = {},
): string {
  return DICTIONARIES[locale][key].replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? params[name] : whole,
  );
}
