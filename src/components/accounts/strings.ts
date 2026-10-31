import { useCallback } from "react";
import { useLocale, type Locale } from "../../i18n";
import { typeset } from "../../lib/typography";

/**
 * The strings of the « Comptes Claude » components. The central dictionary
 * (src/i18n.ts) is held elsewhere, so they live here, written the same way:
 * plain spaces in the source, typeset once when the module loads — the narrow
 * no-break spaces of French and the bound units come from the typesetter, never
 * from the hand. `{name}`-style placeholders are filled after, untouched.
 *
 * Plural keys come in `_one` / `_other` pairs and are addressed by their base
 * name with a numeric `n`.
 */
export const en = {
  // Settings page
  "settings.title": "Claude accounts",
  "settings.intro":
    "Each account is a separate Claude subscription. Choose one per workspace; Hangar never switches on its own when a limit is reached.",
  "settings.listLabel": "Accounts",
  "settings.loggedIn": "Logged in",
  "settings.loggedOut": "Not logged in",
  "settings.logIn": "Log in",
  "settings.default": "Default",
  "settings.setDefault": "Make default",
  "settings.usedBy": "Used by {names}",
  "settings.usedByNone": "No workspace uses it",
  "settings.rename": "Rename",
  "settings.renameLabel": "New name for {name}",
  "settings.save": "Save",
  "settings.cancel": "Cancel",
  "settings.remove": "Remove",
  "settings.removeAsk": "Remove “{name}”?",
  "settings.removeBody":
    "Its login is deleted from this computer. Workspaces on it go back to the default account.",
  "settings.removeConfirm": "Remove the account",
  "settings.mainFixed":
    "The main account is Claude Code’s own configuration: it cannot be removed.",
  "settings.addTitle": "Add an account",
  "settings.addName": "Name",
  "settings.addPlaceholder": "Work, Personal…",
  "settings.add": "Add and log in",
  "settings.addHint": "A pane opens so you can log in through your browser.",
  "settings.nameEmpty": "Give the account a name.",
  "settings.nameTaken": "Another account already has this name.",
  "settings.unsupportedTitle": "Logins are not kept apart on this computer",
  "settings.unsupportedBody":
    "Logging in to one account here may log another one out. You may have to log in again after switching.",
  "settings.noPane":
    "No room for the login pane. Close a pane in the open workspace, then choose “Log in” on the account.",
  "settings.failed": "Could not do that: {detail}",
  // Account switcher
  "switcher.button": "Claude account: {name}",
  "switcher.menu": "Claude accounts",
  "switcher.heading": "This workspace uses",
  "switcher.useHere": "Use {name} in this workspace",
  "switcher.useEverywhere": "For every workspace",
  "switcher.useEverywhereLabel": "Make {name} the default for every workspace",
  "switcher.notLoggedIn": "not logged in",
  "switcher.five": "5 h",
  "switcher.week": "week",
  "switcher.percent": "{n}%",
  "switcher.unknown": "unknown",
  "switcher.usage": "{five} {fiveValue} · {week} {weekValue}",
  "switcher.askHere": "Switch this workspace to {name}",
  "switcher.askEverywhere": "Make {name} the default for every workspace",
  "switcher.running_one": "{n} agent running.",
  "switcher.running_other": "{n} agents running.",
  "switcher.restartHere": "Restart this workspace’s agents now (conversations kept)",
  "switcher.restartEverywhere": "Restart the agents that follow the default now (conversations kept)",
  "switcher.nextLaunch": "On next launch",
  "switcher.back": "Back",
  // Workspace picker, badge, menu
  "picker.label": "Claude account",
  "picker.default": "Default ({name})",
  "picker.unknown": "unknown",
  "picker.missing": "The account “{id}” no longer exists: the default applies.",
  "badge.title": "Claude account: {name}",
  "menu.title": "Claude account",
  "pane.badgeTitle": "This agent runs on the “{name}” account, not its workspace’s",
  "pane.menuHeading": "Claude account for this pane",
} as const;

export type AccountsKey = keyof typeof en;

export const fr: Record<AccountsKey, string> = {
  "settings.title": "Comptes Claude",
  "settings.intro":
    "Chaque compte est un abonnement Claude distinct. Choisis-en un par workspace ; Hangar ne bascule jamais tout seul quand une limite est atteinte.",
  "settings.listLabel": "Comptes",
  "settings.loggedIn": "Connecté",
  "settings.loggedOut": "Non connecté",
  "settings.logIn": "Se connecter",
  "settings.default": "Par défaut",
  "settings.setDefault": "Définir par défaut",
  "settings.usedBy": "Utilisé par {names}",
  "settings.usedByNone": "Aucun workspace ne l’utilise",
  "settings.rename": "Renommer",
  "settings.renameLabel": "Nouveau nom de {name}",
  "settings.save": "Enregistrer",
  "settings.cancel": "Annuler",
  "settings.remove": "Retirer",
  "settings.removeAsk": "Retirer « {name} » ?",
  "settings.removeBody":
    "Sa connexion est supprimée de cet ordinateur. Les workspaces qui l’utilisent reviennent au compte par défaut.",
  "settings.removeConfirm": "Retirer le compte",
  "settings.mainFixed":
    "Le compte principal est la configuration propre de Claude Code : il ne peut pas être retiré.",
  "settings.addTitle": "Ajouter un compte",
  "settings.addName": "Nom",
  "settings.addPlaceholder": "Travail, Perso…",
  "settings.add": "Ajouter et se connecter",
  "settings.addHint": "Un volet s’ouvre pour te connecter depuis ton navigateur.",
  "settings.nameEmpty": "Donne un nom au compte.",
  "settings.nameTaken": "Un autre compte porte déjà ce nom.",
  "settings.unsupportedTitle": "Les connexions ne sont pas séparées sur cet ordinateur",
  "settings.unsupportedBody":
    "Se connecter à un compte ici peut en déconnecter un autre. Tu devras peut-être te reconnecter après un changement.",
  "settings.noPane":
    "Pas de place pour le volet de connexion. Ferme un volet du workspace ouvert, puis choisis « Se connecter » sur le compte.",
  "settings.failed": "Impossible : {detail}",
  "switcher.button": "Compte Claude : {name}",
  "switcher.menu": "Comptes Claude",
  "switcher.heading": "Ce workspace utilise",
  "switcher.useHere": "Utiliser {name} dans ce workspace",
  "switcher.useEverywhere": "Pour tous les workspaces",
  "switcher.useEverywhereLabel": "Faire de {name} le compte par défaut de tous les workspaces",
  "switcher.notLoggedIn": "non connecté",
  "switcher.five": "5 h",
  "switcher.week": "semaine",
  "switcher.percent": "{n} %",
  "switcher.unknown": "inconnu",
  "switcher.usage": "{five} {fiveValue} · {week} {weekValue}",
  "switcher.askHere": "Passer ce workspace sur {name}",
  "switcher.askEverywhere": "Faire de {name} le compte par défaut de tous les workspaces",
  "switcher.running_one": "{n} agent en cours.",
  "switcher.running_other": "{n} agents en cours.",
  "switcher.restartHere": "Redémarrer maintenant les agents de ce workspace (conversations conservées)",
  "switcher.restartEverywhere":
    "Redémarrer maintenant les agents qui suivent le défaut (conversations conservées)",
  "switcher.nextLaunch": "Au prochain lancement",
  "switcher.back": "Retour",
  "picker.label": "Compte Claude",
  "picker.default": "Par défaut ({name})",
  "picker.unknown": "inconnu",
  "picker.missing": "Le compte « {id} » n’existe plus : le compte par défaut s’applique.",
  "badge.title": "Compte Claude : {name}",
  "menu.title": "Compte Claude",
  "pane.badgeTitle": "Cet agent tourne sur le compte « {name} », pas celui de son workspace",
  "pane.menuHeading": "Compte Claude de ce volet",
};

function typesetAll(dict: Record<AccountsKey, string>, locale: Locale): Record<AccountsKey, string> {
  return Object.fromEntries(
    Object.entries(dict).map(([key, text]) => [key, typeset(text, locale)]),
  ) as Record<AccountsKey, string>;
}

/** The dictionaries as they are shown. */
export const ACCOUNT_DICTIONARIES: Record<Locale, Record<AccountsKey, string>> = {
  en: typesetAll(en, "en"),
  fr: typesetAll(fr, "fr"),
};

type PluralBase<K> = K extends `${infer Base}_one` ? Base : never;
export type AccountsTranslateKey = AccountsKey | PluralBase<AccountsKey>;
export type AccountsParams = Record<string, string | number>;
export type AccountsT = (key: AccountsTranslateKey, params?: AccountsParams) => string;

export function translateAccounts(
  locale: Locale,
  key: AccountsTranslateKey,
  params?: AccountsParams,
): string {
  const dict = ACCOUNT_DICTIONARIES[locale];
  let template: string = dict[key as AccountsKey] ?? ACCOUNT_DICTIONARIES.en[key as AccountsKey] ?? key;
  if (params && typeof params.n === "number") {
    const plural = new Intl.PluralRules(locale).select(params.n);
    const resolved = dict[`${key}_${plural}` as AccountsKey] ?? dict[`${key}_other` as AccountsKey];
    if (resolved) template = resolved;
  }
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) => {
    if (!(name in params)) return match;
    const value = params[name];
    return typeof value === "number" ? new Intl.NumberFormat(locale).format(value) : String(value);
  });
}

/** The translator of these components, in the language the app is set to. */
export function useAccountsT(): AccountsT {
  const locale = useLocale();
  return useCallback((key, params) => translateAccounts(locale, key, params), [locale]);
}
