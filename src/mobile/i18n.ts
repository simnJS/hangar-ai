import { typeset } from "../lib/typography";

/**
 * The phone app's words, in English and French, picked from the phone's
 * languages (or `?lang=fr|en`). A dictionary of its own: the desktop's is
 * far larger than these few screens need.
 *
 * `en` is the reference; `fr` is declared as `Record<Key, string>`, so a key
 * missing from it fails the build. Plural keys come in `_one` / `_other`
 * pairs, chosen by `Intl.PluralRules`. Strings are written with plain spaces;
 * French punctuation and units get their no-break spaces when the dictionary
 * loads (src/lib/typography, shared with the desktop).
 */

export type Locale = "en" | "fr";

const en = {
  "tab.forYou": "For you",
  "tab.projects": "Projects",
  "tab.shares": "Shared",
  "tab.manager": "Manager",
  "tab.label": "Sections",

  "activity.working": "Working",
  "activity.waiting": "Waiting for your answer",
  "activity.yours": "Done, your turn",
  "activity.idle": "Idle",
  "activity.none": "No agent",

  "pc.reachable": "Connected to {pc}",
  "pc.unreachable": "{pc} does not answer",

  "banner.unreachableSince": "PC unreachable since {time}",
  "banner.unreachable": "PC unreachable",
  "banner.hint": "Check that it is on and connected to Tailscale.",

  "forYou.title": "For you",
  "forYou.count_one": "{n} thing waits for you",
  "forYou.count_other": "{n} things wait for you",
  "forYou.empty": "Nothing waits for you",
  "forYou.emptyHint": "Whatever needs you will show up here.",
  "forYou.emptyQuiet": "No agent is working right now. Whatever needs you will show up here.",
  "forYou.panes": "Waiting for your answer",
  "forYou.questions": "Questions for you",
  "forYou.lands": "Ready to merge",
  "forYou.since": "for {duration}",
  "forYou.answer": "Answer",
  "forYou.seeProject": "See the project",
  "forYou.landOnPc": "Merge them from Hangar, on the PC.",
  "forYou.history": "Since your last visit",
  "forYou.historyCount_one": "{n} event",
  "forYou.historyCount_other": "{n} events",
  "forYou.historyNone": "Nothing new",
  "forYou.loading": "Reaching your PC…",

  "fleet.label": "Agents, by state",
  "fleet.working_one": "{n} working",
  "fleet.working_other": "{n} working",
  "fleet.waiting_one": "{n} waiting for your answer",
  "fleet.waiting_other": "{n} waiting for your answer",
  "fleet.yours_one": "{n} done, your turn",
  "fleet.yours_other": "{n} done, your turn",
  "fleet.idle_one": "{n} idle",
  "fleet.idle_other": "{n} idle",

  "projects.title": "Projects",
  "projects.empty": "No project yet",
  "projects.emptyHint": "Create a workspace in Hangar on your PC: it shows up here.",
  "project.orchestrator": "{name} orchestrates",
  "project.noOrchestrator": "No orchestrator",
  "project.workers_one": "{n} worker",
  "project.workers_other": "{n} workers",
  "project.questions_one": "{n} question for you",
  "project.questions_other": "{n} questions for you",
  "project.lands_one": "{n} ready to merge",
  "project.lands_other": "{n} ready to merge",
  "project.asleep": "Asleep",
  "project.closed": "Closed on the PC",
  "project.asleepHint": "Its agents were stopped to free memory. Writing to one of them wakes the project.",
  "project.plan": "Plan",
  "project.panes": "Panes",
  "project.noPanes": "No pane is open in this project.",
  "project.board": "Board",
  "project.open": "Open {name}",
  "board.todo": "To do",
  "board.doing": "Doing",
  "board.review": "Review",
  "board.done": "Done",

  "role.orchestrator": "Orchestrator",
  "role.worker": "Worker",
  "role.manager": "Project manager",

  "pane.back": "Back",
  "pane.loading": "Reading the screen…",
  "pane.empty": "Nothing on screen yet.",
  "pane.toEnd": "Latest lines",
  "pane.screen": "Screen of {name}",
  "pane.gone": "This pane is no longer open on the PC.",

  "composer.placeholder": "Message {name}",
  "composer.send": "Send",
  "composer.attach": "Attach a photo or a file",
  "composer.keys": "Keys",
  "composer.uploading": "Sending {name} to the PC…",
  "composer.uploaded": "{name} is on the PC: its path is in your message.",
  "composer.sent": "Sent",
  "key.escape": "Esc",
  "key.enter": "Enter",
  "key.up": "↑",
  "key.down": "↓",
  "key.tab": "Tab",
  "key.ctrlC": "Ctrl+C",
  "key.escapeHint": "Press Escape",
  "key.enterHint": "Press Enter",
  "key.upHint": "Press the up arrow",
  "key.downHint": "Press the down arrow",
  "key.tabHint": "Press Tab",
  "key.ctrlCHint": "Press Ctrl+C",

  "permission.title": "{name} asks for your permission",
  "permission.hint": "Read the screen above, then answer.",
  "permission.allow": "Allow",
  "permission.deny": "Deny",
  "permission.allowed": "Allowed",
  "permission.denied": "Denied",

  "error.offline": "The PC does not answer.",
  "error.busy": "The PC is busy. Try again in a moment.",
  "error.tooBig": "This file is over 25 MB, the most the PC accepts.",
  "error.refused": "The PC refused: {reason}",
  "error.notSent": "Not sent. {reason}",
  "error.retry": "Try again",

  "shares.title": "Shared",
  "shares.empty": "Nothing shared yet",
  "shares.emptyHint": "When an agent sends you an image or a file, it shows up here.",
  "shares.open": "Open",
  "shares.from": "{pane} in {workspace}",

  "manager.title": "Project manager",
  "manager.none": "No project manager open",
  "manager.noneHint":
    "The project manager follows every project for you. On the PC, click “Open the project manager” at the top of Hangar's left bar: it then shows up here.",
  "manager.placeholder": "Message the project manager",

  "history.title": "Since your last visit",
  "history.since": "Last visit: {time}",
  "history.lastDay": "The last 24 hours",
  "history.empty": "Nothing new",
  "history.emptyHint": "Your projects were quiet.",
  "history.showDay": "Show the last 24 hours",
  "history.loading": "Loading the history…",

  "pair.title": "Pair this phone",
  "pair.hint":
    "On the PC, open Hangar's Settings, then Mobile, and show a pairing code. Scan its QR code with the camera, or type its 6 digits here.",
  "pair.again": "This phone is no longer paired with the PC. Pair it again to carry on.",
  "pair.code": "Pairing code",
  "pair.submit": "Pair",
  "pair.pairing": "Pairing…",
  "pair.refused": "This code does not work. Check it, or show a new one on the PC.",
  "pair.done": "Paired with {pc}",

  "install.dismiss": "Got it",

  "push.title": "Notifications on this phone",
  "push.body": "Know when something waits for you or an agent sends you a file, even with Hangar closed.",
  "push.turnOn": "Turn on notifications",
  "push.turnOnShort": "Turn on",
  "push.turningOn": "Turning on…",
  "push.notNow": "Not now",
  "push.row": "Notifications",
  "push.on": "On for this phone",
  "push.off": "Off",
  "push.blocked": "Blocked in the phone's settings",
  "push.needsInstall": "Add Hangar to the Home Screen first",
  "push.needsUpdate": "Need iOS 16.4 or later",
  "push.notHere": "Not available in this browser",
  "push.turnOff": "Turn off",
  "push.why": "Learn more",
  "push.installTitle": "First, add Hangar to your Home Screen",
  "push.install":
    "On an iPhone, only apps on the Home Screen get notifications, from iOS 16.4. In Safari, tap Share, then “Add to Home Screen”. Open Hangar from there: it runs full screen, like an app, and you can turn them on.",
  "push.oldIos": "Notifications need iOS 16.4 or later. Update the iPhone in Settings, then turn them on here.",
  "push.unsupported": "This browser cannot receive notifications. Open Hangar in Safari, Chrome, Edge or Firefox.",
  "push.denied": "Notifications are blocked for Hangar. Allow them in the phone's settings, then come back here.",
  "push.failed": "Notifications could not be turned on. {reason}",
  "push.browserFailed": "The phone's browser could not subscribe: try again in a moment.",

  "time.now": "just now",
} as const;

export type Key = keyof typeof en;

const fr: Record<Key, string> = {
  "tab.forYou": "Pour vous",
  "tab.projects": "Projets",
  "tab.shares": "Partages",
  "tab.manager": "Chef de projet",
  "tab.label": "Sections",

  "activity.working": "Au travail",
  "activity.waiting": "Attend votre réponse",
  "activity.yours": "Terminé, à vous",
  "activity.idle": "Inactif",
  "activity.none": "Pas d'agent",

  "pc.reachable": "Connecté à {pc}",
  "pc.unreachable": "{pc} ne répond pas",

  "banner.unreachableSince": "PC injoignable depuis {time}",
  "banner.unreachable": "PC injoignable",
  "banner.hint": "Vérifiez qu'il est allumé et connecté à Tailscale.",

  "forYou.title": "Pour vous",
  "forYou.count_one": "{n} chose vous attend",
  "forYou.count_other": "{n} choses vous attendent",
  "forYou.empty": "Rien ne vous attend",
  "forYou.emptyHint": "Ce qui a besoin de vous apparaîtra ici.",
  "forYou.emptyQuiet": "Aucun agent ne travaille en ce moment. Ce qui a besoin de vous apparaîtra ici.",
  "forYou.panes": "Attendent votre réponse",
  "forYou.questions": "Questions pour vous",
  "forYou.lands": "Prêtes à fusionner",
  "forYou.since": "depuis {duration}",
  "forYou.answer": "Répondre",
  "forYou.seeProject": "Voir le projet",
  "forYou.landOnPc": "À fusionner depuis Hangar, sur le PC.",
  "forYou.history": "Depuis votre dernière visite",
  "forYou.historyCount_one": "{n} événement",
  "forYou.historyCount_other": "{n} événements",
  "forYou.historyNone": "Rien de nouveau",
  "forYou.loading": "Connexion à votre PC…",

  "fleet.label": "Agents, par état",
  "fleet.working_one": "{n} au travail",
  "fleet.working_other": "{n} au travail",
  "fleet.waiting_one": "{n} attend votre réponse",
  "fleet.waiting_other": "{n} attendent votre réponse",
  "fleet.yours_one": "{n} terminé, à vous",
  "fleet.yours_other": "{n} terminés, à vous",
  "fleet.idle_one": "{n} inactif",
  "fleet.idle_other": "{n} inactifs",

  "projects.title": "Projets",
  "projects.empty": "Aucun projet",
  "projects.emptyHint": "Créez un workspace dans Hangar, sur le PC : il apparaîtra ici.",
  "project.orchestrator": "{name} orchestre",
  "project.noOrchestrator": "Pas d'orchestrateur",
  "project.workers_one": "{n} worker",
  "project.workers_other": "{n} workers",
  "project.questions_one": "{n} question pour vous",
  "project.questions_other": "{n} questions pour vous",
  "project.lands_one": "{n} prête à fusionner",
  "project.lands_other": "{n} prêtes à fusionner",
  "project.asleep": "En veille",
  "project.closed": "Fermé sur le PC",
  "project.asleepHint":
    "Ses agents ont été arrêtés pour libérer de la mémoire. Écrire à l'un d'eux réveille le projet.",
  "project.plan": "Plan",
  "project.panes": "Panneaux",
  "project.noPanes": "Aucun panneau n'est ouvert dans ce projet.",
  "project.board": "Tableau",
  "project.open": "Ouvrir {name}",
  "board.todo": "À faire",
  "board.doing": "En cours",
  "board.review": "En revue",
  "board.done": "Fait",

  "role.orchestrator": "Orchestrateur",
  "role.worker": "Worker",
  "role.manager": "Chef de projet",

  "pane.back": "Retour",
  "pane.loading": "Lecture de l'écran…",
  "pane.empty": "Rien à l'écran pour l'instant.",
  "pane.toEnd": "Dernières lignes",
  "pane.screen": "Écran de {name}",
  "pane.gone": "Ce panneau n'est plus ouvert sur le PC.",

  "composer.placeholder": "Écrire à {name}",
  "composer.send": "Envoyer",
  "composer.attach": "Joindre une photo ou un fichier",
  "composer.keys": "Touches",
  "composer.uploading": "Envoi de {name} au PC…",
  "composer.uploaded": "{name} est sur le PC : son chemin est dans votre message.",
  "composer.sent": "Envoyé",
  "key.escape": "Échap",
  "key.enter": "Entrée",
  "key.up": "↑",
  "key.down": "↓",
  "key.tab": "Tab",
  "key.ctrlC": "Ctrl+C",
  "key.escapeHint": "Appuyer sur Échap",
  "key.enterHint": "Appuyer sur Entrée",
  "key.upHint": "Appuyer sur la flèche du haut",
  "key.downHint": "Appuyer sur la flèche du bas",
  "key.tabHint": "Appuyer sur Tab",
  "key.ctrlCHint": "Appuyer sur Ctrl+C",

  "permission.title": "{name} demande votre permission",
  "permission.hint": "Lisez l'écran ci-dessus, puis répondez.",
  "permission.allow": "Autoriser",
  "permission.deny": "Refuser",
  "permission.allowed": "Autorisé",
  "permission.denied": "Refusé",

  "error.offline": "Le PC ne répond pas.",
  "error.busy": "Le PC est occupé. Réessayez dans un instant.",
  "error.tooBig": "Ce fichier dépasse 25 Mo, le maximum que le PC accepte.",
  "error.refused": "Le PC a refusé : {reason}",
  "error.notSent": "Pas envoyé. {reason}",
  "error.retry": "Réessayer",

  "shares.title": "Partages",
  "shares.empty": "Rien de partagé pour l'instant",
  "shares.emptyHint": "Quand un agent vous envoie une image ou un fichier, il apparaît ici.",
  "shares.open": "Ouvrir",
  "shares.from": "{pane}, dans {workspace}",

  "manager.title": "Chef de projet",
  "manager.none": "Aucun chef de projet ouvert",
  "manager.noneHint":
    "Le chef de projet suit tous vos projets pour vous. Sur le PC, cliquez sur « Ouvrir le chef de projet » en haut de la barre de gauche de Hangar : il apparaîtra ici.",
  "manager.placeholder": "Écrire au chef de projet",

  "history.title": "Depuis votre dernière visite",
  "history.since": "Dernière visite : {time}",
  "history.lastDay": "Les dernières 24 h",
  "history.empty": "Rien de nouveau",
  "history.emptyHint": "Vos projets ont été calmes.",
  "history.showDay": "Voir les dernières 24 h",
  "history.loading": "Chargement de l'historique…",

  "pair.title": "Associer ce téléphone",
  "pair.hint":
    "Sur le PC, ouvrez les Réglages de Hangar, puis Mobile, et affichez un code d'appairage. Scannez son QR code avec l'appareil photo, ou tapez ses 6 chiffres ici.",
  "pair.again": "Ce téléphone n'est plus associé au PC. Associez-le de nouveau pour continuer.",
  "pair.code": "Code d'appairage",
  "pair.submit": "Associer",
  "pair.pairing": "Association…",
  "pair.refused": "Ce code ne fonctionne pas. Vérifiez-le, ou affichez-en un nouveau sur le PC.",
  "pair.done": "Associé à {pc}",

  "install.dismiss": "Compris",

  "push.title": "Notifications sur ce téléphone",
  "push.body": "Sachez quand quelque chose vous attend ou qu'un agent vous envoie un fichier, même Hangar fermé.",
  "push.turnOn": "Activer les notifications",
  "push.turnOnShort": "Activer",
  "push.turningOn": "Activation…",
  "push.notNow": "Plus tard",
  "push.row": "Notifications",
  "push.on": "Activées sur ce téléphone",
  "push.off": "Désactivées",
  "push.blocked": "Bloquées dans les réglages du téléphone",
  "push.needsInstall": "Ajoutez d'abord Hangar à l'écran d'accueil",
  "push.needsUpdate": "iOS 16.4 ou plus récent requis",
  "push.notHere": "Indisponibles dans ce navigateur",
  "push.turnOff": "Désactiver",
  "push.why": "En savoir plus",
  "push.installTitle": "D'abord, ajoutez Hangar à l'écran d'accueil",
  "push.install":
    "Sur iPhone, seules les apps de l'écran d'accueil reçoivent des notifications, à partir d'iOS 16.4. Dans Safari, touchez Partager, puis « Sur l'écran d'accueil ». Ouvrez Hangar depuis l'écran d'accueil : il s'ouvre en plein écran, comme une app, et vous pourrez les activer.",
  "push.oldIos":
    "Les notifications demandent iOS 16.4 ou plus récent. Mettez l'iPhone à jour dans les Réglages, puis activez-les ici.",
  "push.unsupported":
    "Ce navigateur ne peut pas recevoir de notifications. Ouvrez Hangar dans Safari, Chrome, Edge ou Firefox.",
  "push.denied":
    "Les notifications sont bloquées pour Hangar. Autorisez-les dans les réglages du téléphone, puis revenez ici.",
  "push.failed": "Impossible d'activer les notifications. {reason}",
  "push.browserFailed": "Le navigateur du téléphone n'a pas pu s'abonner : réessayez dans un instant.",

  "time.now": "à l'instant",
};

export const DICTIONARIES: Record<Locale, Record<Key, string>> = { en, fr };

/** French when the phone prefers it, English otherwise; `override` wins. */
export function pickLocale(languages: readonly string[], override?: string | null): Locale {
  if (override === "fr" || override === "en") return override;
  for (const language of languages) {
    const base = language.toLowerCase().split("-")[0];
    if (base === "fr" || base === "en") return base;
  }
  return "en";
}

type Vars = Record<string, string | number>;
type PluralBase = Key extends infer K ? (K extends `${infer B}_one` ? B : never) : never;

export interface Translator {
  locale: Locale;
  t: (key: Key, vars?: Vars) => string;
  /** A plural key, chosen by `n`, which also fills `{n}`. */
  tn: (base: PluralBase, n: number, vars?: Vars) => string;
}

export function translator(locale: Locale): Translator {
  const dictionary = Object.fromEntries(
    Object.entries(DICTIONARIES[locale]).map(([key, text]) => [key, typeset(text, locale)]),
  ) as Record<Key, string>;
  const rules = new Intl.PluralRules(locale);
  const fill = (text: string, vars?: Vars) =>
    vars ? text.replace(/\{(\w+)\}/g, (all, name: string) => (name in vars ? String(vars[name]) : all)) : text;
  const t = (key: Key, vars?: Vars) => fill(dictionary[key], vars);
  const tn = (base: PluralBase, n: number, vars?: Vars) => {
    const form = rules.select(n) === "one" ? "one" : "other";
    return t(`${base}_${form}` as Key, { n, ...vars });
  };
  return { locale, t, tn };
}

const search = typeof location === "undefined" ? "" : location.search;
const languages = typeof navigator === "undefined" ? [] : navigator.languages ?? [navigator.language];

/** The app's language, chosen once when it loads. */
export const i18n = translator(pickLocale(languages, new URLSearchParams(search).get("lang")));
export const { t, tn, locale } = i18n;
