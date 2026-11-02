/**
 * The strings of the consent card shown before an agent's clone
 * (components/GitCloneConsent, keys `gitConsent.`), of the « From a Git
 * repository » entry of the new-workspace dialog (keys `create.git…`) and of
 * the « Projects folder » setting (keys `settings.projectsFolder…`). i18n.ts
 * merges them for `t()`; the consent card, outside any hook, reads them
 * through `workspaceFromGitText`.
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
  "gitConsent.logAllow": "Clone of {repo} into the folder \"{folder}\": you clicked Clone",
  "gitConsent.logRefuse": "Clone of {repo} into the folder \"{folder}\": you clicked Refuse",
  "gitConsent.logTimeout": "Clone of {repo} into the folder \"{folder}\": no answer in time, nothing cloned",
  "create.source": "Source",
  "create.sourceFolder": "Existing folder",
  "create.sourceGit": "From a Git repository",
  "create.gitRepo": "Repository",
  "create.gitRepoPlaceholder": "owner/repo, or an https or ssh address",
  "create.gitChecking": "Reading the address…",
  "create.gitRefused": "{reason}",
  "create.gitParent": "Clone into",
  "create.gitParentHint": "empty: the Projects folder from the settings, else next to the current workspace",
  "create.gitParentDialog": "Folder to clone into",
  "create.gitDestination": "It will be cloned to {path}",
  "create.gitNoFolder": "There is no folder to clone into: pick one",
  "create.gitSubmit": "Clone and create workspace",
  "create.gitRunning": "Cloning…",
  "create.gitStop": "Cancel the clone",
  "create.gitStarting": "Starting the clone…",
  "create.gitFailed": "Could not create the workspace: {reason}",
  "create.gitCancelled": "Clone cancelled",
  "create.gitReused": "{path} already held this repository: it was used as it was",
  "create.gitExisting": "A workspace is already on {path}: it is shown",
  "settings.projectsFolder": "Projects folder",
  "settings.projectsFolderHint":
    "Where repositories cloned from Git go. Empty: next to the current workspace's folder",
  "settings.projectsFolderPlaceholder": "Next to the current workspace",
  "settings.projectsFolderDialog": "Projects folder",
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
  "gitConsent.logAllow": "Clonage de {repo} dans le dossier « {folder} » : tu as cliqué sur Cloner",
  "gitConsent.logRefuse": "Clonage de {repo} dans le dossier « {folder} » : tu as cliqué sur Refuser",
  "gitConsent.logTimeout": "Clonage de {repo} dans le dossier « {folder} » : pas de réponse à temps, rien cloné",
  "create.source": "Source",
  "create.sourceFolder": "Dossier existant",
  "create.sourceGit": "Depuis un dépôt Git",
  "create.gitRepo": "Dépôt",
  "create.gitRepoPlaceholder": "owner/repo, ou une adresse https ou ssh",
  "create.gitChecking": "Lecture de l'adresse…",
  "create.gitRefused": "{reason}",
  "create.gitParent": "Cloner dans",
  "create.gitParentHint": "vide : le dossier des projets des réglages, sinon à côté du workspace actuel",
  "create.gitParentDialog": "Dossier où cloner",
  "create.gitDestination": "Il sera cloné dans {path}",
  "create.gitNoFolder": "Aucun dossier où cloner : choisis-en un",
  "create.gitSubmit": "Cloner et créer le workspace",
  "create.gitRunning": "Clonage…",
  "create.gitStop": "Annuler le clonage",
  "create.gitStarting": "Démarrage du clonage…",
  "create.gitFailed": "Impossible de créer le workspace : {reason}",
  "create.gitCancelled": "Clonage annulé",
  "create.gitReused": "{path} contenait déjà ce dépôt : il a été réutilisé tel quel",
  "create.gitExisting": "Un workspace existe déjà sur {path} : le voici",
  "settings.projectsFolder": "Dossier des projets",
  "settings.projectsFolderHint":
    "Où vont les dépôts clonés depuis Git. Vide : à côté du dossier du workspace actuel",
  "settings.projectsFolderPlaceholder": "À côté du workspace actuel",
  "settings.projectsFolderDialog": "Dossier des projets",
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
