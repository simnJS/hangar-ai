/**
 * The strings of a kit's tab: the frame around its page, and what Hangar
 * says about the calls it makes. i18n.ts merges both dictionaries into its
 * own, so a key reads through `t()` like any other; keys start with
 * `kitsTab.`.
 *
 * `en` is the reference. `fr` is typed against it, so the build fails on a
 * French key that is missing or misspelled — here, without i18n.ts having
 * to change. French follows the glossary and the typography of i18n.ts:
 * plain spaces before ? ! ; : » and after «, the typesetter does the rest.
 */

export const en = {
  "kitsTab.loadFailed": "This kit's tab could not be loaded.",
  // The toolbar's « Kits » button and its menu.
  "kitsTab.menu": "Kits",
  "kitsTab.menuHint": "Open a tab of this workspace's kits",
  "kitsTab.menuLoading": "Reading the kits…",
  "kitsTab.menuNone": "No kit enabled on this workspace has tabs.",
  // The command palette.
  "kitsTab.paletteGroup": "Kits",
  "kitsTab.paletteOpen": "Open the kit tab {title}",
  // The tab and its bar.
  "kitsTab.kind": "Kit tab",
  "kitsTab.tabHint": "{title}, a tab of the kit {kit}",
  "kitsTab.tools": "Tools of the kit tab {title}",
  "kitsTab.reload": "Reload the tab",
  "kitsTab.tabParams": "With {params}",
  // The small glyph a page sets in the tab strip (tab.setBadge), read aloud.
  "kitsTab.badge.running": "In progress",
  "kitsTab.badge.success": "Succeeded",
  "kitsTab.badge.failure": "Failed",
  "kitsTab.badge.neutral": "No particular state",
  "kitsTab.isolated": "Runs isolated",
  "kitsTab.isolatedHint":
    "This page of the kit {kit} runs in a frame of its own: it reaches neither Hangar nor the network, only what the workspace granted it: {permissions}.",
  "kitsTab.isolatedNothing":
    "This page of the kit {kit} runs in a frame of its own: it reaches neither Hangar nor the network, and the workspace granted it nothing.",
  "kitsTab.permission.workspace.read": "read the workspace",
  "kitsTab.permission.panes.read": "see the panes",
  "kitsTab.permission.panes.send": "write to the Claude Code agents",
  "kitsTab.permission.files.read": "read the workspace's text files",
  "kitsTab.permission.files.write": "change the workspace's text files, hidden ones included",
  "kitsTab.permission.data.read": "read this kit's own data",
  "kitsTab.permission.data.write": "write this kit's own data",
  "kitsTab.permission.loopback": "talk to the local server on port {port}",
  "kitsTab.permission.command": "run the command {name}",
  "kitsTab.permission.preview.show": "open files in the preview",
  "kitsTab.permission.notify": "raise notifications",
  // The activation view: what a tab can do to the project's files.
  "kitsTab.permFilesWrite": "change text files in your project, hidden ones like .env included",
  "kitsTab.filesSecrets":
    "This tab can both read and change your project's files, so it can also read hidden files such as .env, and the secrets they hold. Its page has no network of its own, but the kit's code can pass them on: to an agent with panes.send, and to whatever else the tab holds.",
  // The activation view: what a kit's tabs may run on the computer.
  "kitsTab.permCommand": "run the command {name} on your computer, with your logins",
  "kitsTab.commands": "Commands its tabs can run",
  "kitsTab.commandsNone": "No command: its tabs run nothing on your computer.",
  "kitsTab.commandsChanged":
    "These changed since they were accepted here. Until you accept them, this kit's tabs run none of them.",
  "kitsTab.commandsLive":
    "A linked folder is read live: what you change in these commands applies without being asked again.",
  "kitsTab.commandHint":
    "A tab of this kit can run this command on your computer whenever it wants, with your logins, for instance your gh login; only the parts in braces change, and they must match their rule.",
  "kitsTab.commandHintFixed":
    "A tab of this kit can run this command on your computer whenever it wants, with your logins, for instance your gh login; nothing in it changes.",
  "kitsTab.commandKitDir":
    "{kitDir} is this kit's own folder, which Hangar fills in: this command runs a script of the kit. You see its name here, not what it does, and an update can change that without asking again.",
  "kitsTab.everywhereReviewCommands":
    "Changed commands to accept: this kit's tabs run none of them until then.",
  "kitsTab.commandLabel": "Command {name}",
  "kitsTab.commandRules": "What changes in the command {name}",
  // When the page cannot run.
  "kitsTab.loading": "Reading the kit…",
  "kitsTab.disabled": "This kit is no longer enabled on this workspace.",
  "kitsTab.disabledHint": "Enable it again on the workspace to use this tab.",
  "kitsTab.missing": "This tab is no longer in an installed kit.",
  "kitsTab.missingHint": "The kit was removed, or an update took this tab out of it.",
  "kitsTab.broken": "This kit cannot be used right now.",
  "kitsTab.left": "This tab left its page.",
  "kitsTab.leftHint":
    "A kit's tab only speaks to Hangar from the page it opened on, so the page it went to was closed. Reload the tab to start again.",
} as const;

export const fr: Record<keyof typeof en, string> = {
  "kitsTab.loadFailed": "L'onglet de ce kit n'a pas pu être chargé.",
  "kitsTab.menu": "Kits",
  "kitsTab.menuHint": "Ouvrir un onglet des kits de ce workspace",
  "kitsTab.menuLoading": "Lecture des kits…",
  "kitsTab.menuNone": "Aucun kit activé sur ce workspace n'a d'onglet.",
  "kitsTab.paletteGroup": "Kits",
  "kitsTab.paletteOpen": "Ouvrir l'onglet de kit {title}",
  "kitsTab.kind": "Onglet de kit",
  "kitsTab.tabHint": "{title}, un onglet du kit {kit}",
  "kitsTab.tools": "Outils de l'onglet de kit {title}",
  "kitsTab.reload": "Recharger l'onglet",
  "kitsTab.tabParams": "Avec {params}",
  "kitsTab.badge.running": "En cours",
  "kitsTab.badge.success": "Réussi",
  "kitsTab.badge.failure": "Échoué",
  "kitsTab.badge.neutral": "Sans état particulier",
  "kitsTab.isolated": "Exécuté à l'écart",
  "kitsTab.isolatedHint":
    "Cette page du kit {kit} tourne dans un cadre à part : elle n'atteint ni Hangar ni le réseau, seulement ce que le workspace lui a accordé : {permissions}.",
  "kitsTab.isolatedNothing":
    "Cette page du kit {kit} tourne dans un cadre à part : elle n'atteint ni Hangar ni le réseau, et le workspace ne lui a rien accordé.",
  "kitsTab.permission.workspace.read": "lire le workspace",
  "kitsTab.permission.panes.read": "voir les panneaux",
  "kitsTab.permission.panes.send": "écrire aux agents Claude Code",
  "kitsTab.permission.files.read": "lire les fichiers texte du workspace",
  "kitsTab.permission.files.write": "modifier les fichiers texte du workspace, y compris les fichiers cachés",
  "kitsTab.permission.data.read": "lire les données propres à ce kit",
  "kitsTab.permission.data.write": "écrire les données propres à ce kit",
  "kitsTab.permission.loopback": "parler au serveur local sur le port {port}",
  "kitsTab.permission.command": "lancer la commande {name}",
  "kitsTab.permission.preview.show": "ouvrir des fichiers dans l'aperçu",
  "kitsTab.permission.notify": "envoyer des notifications",
  "kitsTab.permFilesWrite": "modifier les fichiers texte de votre projet, y compris les fichiers cachés comme .env",
  "kitsTab.filesSecrets":
    "Cet onglet peut à la fois lire et modifier les fichiers de votre projet : il peut donc aussi lire les fichiers cachés comme .env, et les secrets qu'ils contiennent. Sa page n'a pas de réseau, mais le code du kit peut les transmettre : à un agent avec panes.send, et à tout ce que l'onglet détient d'autre.",
  "kitsTab.permCommand": "lancer la commande {name} sur votre ordinateur, avec vos identifiants",
  "kitsTab.commands": "Commandes que ses onglets peuvent lancer",
  "kitsTab.commandsNone": "Aucune commande : ses onglets ne lancent rien sur votre ordinateur.",
  "kitsTab.commandsChanged":
    "Elles ont changé depuis qu'elles ont été acceptées ici. Tant que vous ne les acceptez pas, les onglets de ce kit n'en lancent aucune.",
  "kitsTab.commandsLive":
    "Un dossier lié est lu en direct : ce que vous changez dans ces commandes s'applique sans nouvelle question.",
  "kitsTab.commandHint":
    "Un onglet de ce kit peut lancer cette commande sur votre ordinateur quand il le veut, avec vos identifiants, par exemple votre connexion gh ; seules les parties entre accolades changent, et elles doivent respecter leur règle.",
  "kitsTab.commandHintFixed":
    "Un onglet de ce kit peut lancer cette commande sur votre ordinateur quand il le veut, avec vos identifiants, par exemple votre connexion gh ; rien n'y change.",
  "kitsTab.commandKitDir":
    "{kitDir} est le dossier de ce kit, que Hangar remplit lui-même : cette commande lance un script du kit. Vous voyez ici son nom, pas ce qu'il fait, et une mise à jour peut le changer sans nouvelle question.",
  "kitsTab.everywhereReviewCommands":
    "Commandes modifiées à accepter : les onglets de ce kit n'en lancent aucune d'ici là.",
  "kitsTab.commandLabel": "Commande {name}",
  "kitsTab.commandRules": "Ce qui change dans la commande {name}",
  "kitsTab.loading": "Lecture du kit…",
  "kitsTab.disabled": "Ce kit n'est plus activé sur ce workspace.",
  "kitsTab.disabledHint": "Réactive-le sur le workspace pour te servir de cet onglet.",
  "kitsTab.missing": "Cet onglet ne fait plus partie d'un kit installé.",
  "kitsTab.missingHint": "Le kit a été retiré, ou une mise à jour en a retiré cet onglet.",
  "kitsTab.broken": "Ce kit ne peut pas servir pour le moment.",
  "kitsTab.left": "Cet onglet a quitté sa page.",
  "kitsTab.leftHint":
    "L'onglet d'un kit ne parle à Hangar que depuis la page où il s'est ouvert : la page où il est allé a donc été fermée. Recharge l'onglet pour repartir.",
};
