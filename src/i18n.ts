import { useCallback } from "react";
import { useStore } from "./store";

/**
 * Minimal typed i18n. `en` is the reference: `fr` is declared as
 * `Record<Key, string>`, so TypeScript fails the build if a key is missing or
 * misspelled in a translation.
 *
 * Plural keys come in `_one` / `_other` pairs and are selected through
 * `Intl.PluralRules`, which applies each language's own rules.
 */

export type Locale = "en" | "fr";

export const LOCALES: { id: Locale; label: string }[] = [
  { id: "en", label: "English" },
  { id: "fr", label: "Français" },
];

const en = {
  "app.loading": "Loading…",

  "sidebar.workspaces": "Workspaces",
  "sidebar.empty": "No workspace yet.\nCreate one to get started.",
  "sidebar.new": "+ New workspace",
  "sidebar.settings": "⚙ Appearance & settings",
  "sidebar.remove": "Delete workspace",

  "view.terminals": "Terminals",
  "view.board": "Board",
  "view.group": "View",
  "topbar.layout": "Layout",
  "topbar.restartAll": "↻ Restart all",
  "topbar.restartAllHint": "Restart every pane",
  "topbar.panes_one": "{n} pane",
  "topbar.panes_other": "{n} panes",

  "placeholder.title": "No workspace open",
  "placeholder.body":
    "Create a workspace from the sidebar: pick a project folder, then spread your agents ({agents}) across 1, 2, 4 or 8 panes.",

  "broadcast.placeholder": "Send the same instruction to all {n} panes…",
  "broadcast.send": "Broadcast",

  "pane.agent": "Agent running in this pane",
  "pane.missing": " (not installed)",
  "pane.shell": "Shell",
  "pane.inherited": "{name} (inherited)",
  "pane.shellTitle": "Shell: {program}",
  "pane.newSession": "⟲ new",
  "pane.resuming": "Resuming session: {id}",
  "pane.noSession": "No session stored — click to pick one",
  "pane.restart": "Restart pane",
  "pane.label": "Pane {n}",
  "pane.spawnFailed": "Failed to start shell",
  "pane.notOnPath": "{agent} not found on PATH",

  "settings.title": "Appearance & settings",
  "settings.close": "Close",
  "settings.theme": "Theme",
  "settings.themePerWorkspace": "specific to this workspace",
  "settings.language": "Language",
  "settings.languageLabel": "Interface",
  "settings.shell": "Shell",
  "settings.shellDefault": "Default",
  "settings.shellDetected": "{n} detected",
  "settings.shellAuto": "Auto ({name})",
  "settings.shellWorkspace": "This workspace",
  "settings.shellWorkspaceHint": "overrides the default",
  "settings.shellFollow": "Follow the default",
  "settings.typography": "Typography",
  "settings.font": "Font",
  "settings.fontCustom": "Custom",
  "settings.size": "Size",
  "settings.lineHeight": "Line height",
  "settings.letterSpacing": "Letter spacing",
  "settings.padding": "Padding",
  "settings.cursorSection": "Cursor & history",
  "settings.cursor": "Cursor",
  "settings.cursorBar": "Bar",
  "settings.cursorBlock": "Block",
  "settings.cursorUnderline": "Underline",
  "settings.blink": "Blink",
  "settings.scrollback": "Scrollback",
  "settings.scrollbackLines": "{n} lines",
  "settings.sessions": "Sessions",
  "settings.autoResume": "Auto resume",
  "settings.autoResumeHint": "relaunch agents on their last session",
  "settings.launchDelay": "Launch delay",

  "sessions.title": "{agent} sessions",
  "sessions.new": "New session",
  "sessions.newHint": "start the agent without resuming",
  "sessions.loading": "Reading transcripts…",
  "sessions.empty": "No session recorded for this folder.",
  "sessions.untitled": "untitled",
  "sessions.note": "The pane restarts to apply the chosen session.",
  "sessions.justNow": "just now",
  "sessions.minutesAgo": "{n} min ago",
  "sessions.hoursAgo": "{n} h ago",
  "sessions.daysAgo": "{n} d ago",

  "create.title": "New workspace",
  "create.folder": "Project folder",
  "create.folderEmpty": "No folder selected",
  "create.browse": "Browse…",
  "create.folderDialog": "Project folder",
  "create.name": "Name",
  "create.namePlaceholder": "My project",
  "create.shellDetected_one": "{n} detected on this machine",
  "create.shellDetected_other": "{n} detected on this machine",
  "create.shellDefault": "Default ({name})",
  "create.shellNone": "none",
  "create.terminals": "Number of terminals",
  "create.agents": "Agent split",
  "create.freeSlots_one": "{n} pane left as a plain shell",
  "create.freeSlots_other": "{n} panes left as plain shells",
  "create.allAssigned": "every pane is assigned",
  "create.notInstalled": "not installed",
  "create.layout": "Arrangement",
  "create.layoutHint": "click a cell to change its agent",
  "create.cancel": "Cancel",
  "create.submit": "Create workspace",

  "board.tasks_one": "{n} task",
  "board.tasks_other": "{n} tasks",
  "board.hint": "Agents read and edit this board over MCP",
  "board.mcp": "⚙ MCP connection",
  "board.add": "+ Add…",
  "board.todo": "To do",
  "board.doing": "In progress",
  "board.review": "Review",
  "board.done": "Done",
  "board.assignee": "Agent in charge",
  "board.dependsOn": "Depends on other tasks",

  "task.title": "Task",
  "task.name": "Title",
  "task.description": "Description",
  "task.descriptionHint": "visible to agents",
  "task.column": "Column",
  "task.priority": "Priority",
  "task.assignee": "Assigned to",
  "task.release": "Release",
  "task.free": "free — any agent can take it",
  "task.dependencies": "Depends on",
  "task.thread": "Thread",
  "task.threadHint": "this is where agents talk to each other",
  "task.noComments": "No message yet.",
  "task.writeComment": "Write a message…",
  "task.send": "Send",
  "task.createdAt": "Created {date}",
  "task.delete": "Delete",
  "task.close": "Close",

  "mcp.title": "Connect agents to the board",
  "mcp.intro":
    "IaBench registers itself as an MCP server in each tool's configuration. The port and token are never written to those files: they are resolved at launch, so the configuration stays valid across restarts.",
  "mcp.playbook": "Write the agent playbook",
  "mcp.playbookHint": "AGENTS.md + CLAUDE.md · adds .iabench/ to .gitignore",
  "mcp.alreadyThere": "already present",
  "mcp.configured": "already configured",
  "mcp.notDetected": "not detected",
  "mcp.restartHint": "Restart the affected agents so they load the server.",
  "mcp.manual": "Or do it manually",
  "mcp.manualHint": "if you prefer the CLI",
  "mcp.close": "Close",
  "mcp.install": "Configure ({n})",
  "mcp.installing": "Installing…",

  "update.available": "Version {version} available",
  "update.later": "Later",
  "update.install": "Update",
  "update.downloading": "Downloading… {progress}",
  "update.ready": "Update installed",
  "update.restart": "Restart",
  "update.failed": "Update failed",
  "update.retry": "Retry",
} as const;

type Key = keyof typeof en;

const fr: Record<Key, string> = {
  "app.loading": "Chargement…",

  "sidebar.workspaces": "Workspaces",
  "sidebar.empty": "Aucun workspace.\nCrée-en un pour démarrer.",
  "sidebar.new": "+ Nouveau workspace",
  "sidebar.settings": "⚙ Apparence & réglages",
  "sidebar.remove": "Supprimer le workspace",

  "view.terminals": "Terminaux",
  "view.board": "Tableau",
  "view.group": "Vue",
  "topbar.layout": "Disposition",
  "topbar.restartAll": "↻ Tout relancer",
  "topbar.restartAllHint": "Relancer tous les panneaux",
  "topbar.panes_one": "{n} panneau",
  "topbar.panes_other": "{n} panneaux",

  "placeholder.title": "Aucun workspace ouvert",
  "placeholder.body":
    "Crée un workspace depuis la barre latérale : choisis un dossier de projet, puis répartis tes agents ({agents}) dans 1, 2, 4 ou 8 panneaux.",

  "broadcast.placeholder": "Envoyer la même instruction aux {n} panneaux…",
  "broadcast.send": "Diffuser",

  "pane.agent": "Agent lancé dans ce panneau",
  "pane.missing": " (absent)",
  "pane.shell": "Shell",
  "pane.inherited": "{name} (hérité)",
  "pane.shellTitle": "Shell : {program}",
  "pane.newSession": "⟲ nouvelle",
  "pane.resuming": "Session reprise : {id}",
  "pane.noSession": "Aucune session mémorisée — cliquer pour en choisir une",
  "pane.restart": "Relancer le panneau",
  "pane.label": "Panneau {n}",
  "pane.spawnFailed": "Échec du démarrage du shell",
  "pane.notOnPath": "{agent} introuvable dans le PATH",

  "settings.title": "Apparence & réglages",
  "settings.close": "Fermer",
  "settings.theme": "Thème",
  "settings.themePerWorkspace": "spécifique à ce workspace",
  "settings.language": "Langue",
  "settings.languageLabel": "Interface",
  "settings.shell": "Shell",
  "settings.shellDefault": "Par défaut",
  "settings.shellDetected": "{n} détecté(s)",
  "settings.shellAuto": "Auto ({name})",
  "settings.shellWorkspace": "Ce workspace",
  "settings.shellWorkspaceHint": "prioritaire sur le défaut",
  "settings.shellFollow": "Suivre le défaut",
  "settings.typography": "Typographie",
  "settings.font": "Police",
  "settings.fontCustom": "Personnalisée",
  "settings.size": "Taille",
  "settings.lineHeight": "Interligne",
  "settings.letterSpacing": "Espacement",
  "settings.padding": "Marge interne",
  "settings.cursorSection": "Curseur & historique",
  "settings.cursor": "Curseur",
  "settings.cursorBar": "Barre",
  "settings.cursorBlock": "Bloc",
  "settings.cursorUnderline": "Souligné",
  "settings.blink": "Clignotement",
  "settings.scrollback": "Scrollback",
  "settings.scrollbackLines": "{n} lignes",
  "settings.sessions": "Sessions",
  "settings.autoResume": "Reprise auto",
  "settings.autoResumeHint": "relance les agents sur leur dernière session",
  "settings.launchDelay": "Délai de lancement",

  "sessions.title": "Sessions {agent}",
  "sessions.new": "Nouvelle session",
  "sessions.newHint": "démarre l'agent sans reprise",
  "sessions.loading": "Lecture des transcriptions…",
  "sessions.empty": "Aucune session enregistrée pour ce dossier.",
  "sessions.untitled": "sans titre",
  "sessions.note": "Le panneau redémarre pour appliquer la session choisie.",
  "sessions.justNow": "à l'instant",
  "sessions.minutesAgo": "il y a {n} min",
  "sessions.hoursAgo": "il y a {n} h",
  "sessions.daysAgo": "il y a {n} j",

  "create.title": "Nouveau workspace",
  "create.folder": "Dossier du projet",
  "create.folderEmpty": "Aucun dossier sélectionné",
  "create.browse": "Parcourir…",
  "create.folderDialog": "Dossier du projet",
  "create.name": "Nom",
  "create.namePlaceholder": "Mon projet",
  "create.shellDetected_one": "{n} détecté sur ce PC",
  "create.shellDetected_other": "{n} détectés sur ce PC",
  "create.shellDefault": "Par défaut ({name})",
  "create.shellNone": "aucun",
  "create.terminals": "Nombre de terminaux",
  "create.agents": "Répartition des agents",
  "create.freeSlots_one": "{n} panneau en shell simple",
  "create.freeSlots_other": "{n} panneaux en shell simple",
  "create.allAssigned": "tous les panneaux sont attribués",
  "create.notInstalled": "non installé",
  "create.layout": "Disposition",
  "create.layoutHint": "clique une case pour changer son agent",
  "create.cancel": "Annuler",
  "create.submit": "Créer le workspace",

  "board.tasks_one": "{n} tâche",
  "board.tasks_other": "{n} tâches",
  "board.hint": "Les agents lisent et modifient ce tableau via MCP",
  "board.mcp": "⚙ Connexion MCP",
  "board.add": "+ Ajouter…",
  "board.todo": "À faire",
  "board.doing": "En cours",
  "board.review": "Revue",
  "board.done": "Terminé",
  "board.assignee": "Agent en charge",
  "board.dependsOn": "Dépend d'autres tâches",

  "task.title": "Tâche",
  "task.name": "Titre",
  "task.description": "Description",
  "task.descriptionHint": "visible par les agents",
  "task.column": "Colonne",
  "task.priority": "Priorité",
  "task.assignee": "Assigné à",
  "task.release": "Libérer",
  "task.free": "libre — un agent peut la prendre",
  "task.dependencies": "Dépend de",
  "task.thread": "Échanges",
  "task.threadHint": "c'est ici que les agents se parlent",
  "task.noComments": "Aucun message pour l'instant.",
  "task.writeComment": "Écrire un message…",
  "task.send": "Envoyer",
  "task.createdAt": "Créée le {date}",
  "task.delete": "Supprimer",
  "task.close": "Fermer",

  "mcp.title": "Connecter les agents au tableau",
  "mcp.intro":
    "IaBench s'enregistre comme serveur MCP dans la configuration de chaque outil. Le port et le jeton ne sont jamais écrits dans ces fichiers : ils sont résolus au lancement, donc la configuration reste valable après un redémarrage.",
  "mcp.playbook": "Écrire le mode d'emploi pour les agents",
  "mcp.playbookHint": "AGENTS.md + CLAUDE.md · ajoute .iabench/ au .gitignore",
  "mcp.alreadyThere": "déjà présent",
  "mcp.configured": "déjà configuré",
  "mcp.notDetected": "non détecté",
  "mcp.restartHint": "Redémarre les agents concernés pour qu'ils chargent le serveur.",
  "mcp.manual": "Ou à la main",
  "mcp.manualHint": "si tu préfères passer par la CLI",
  "mcp.close": "Fermer",
  "mcp.install": "Configurer ({n})",
  "mcp.installing": "Installation…",

  "update.available": "Version {version} disponible",
  "update.later": "Plus tard",
  "update.install": "Mettre à jour",
  "update.downloading": "Téléchargement… {progress}",
  "update.ready": "Mise à jour installée",
  "update.restart": "Redémarrer",
  "update.failed": "Échec de la mise à jour",
  "update.retry": "Réessayer",
};

const DICTIONARIES: Record<Locale, Record<Key, string>> = { en, fr };

/** `topbar.panes_one` also exposes `topbar.panes` as a callable key. */
type PluralBase<K> = K extends `${infer Base}_one` ? Base : never;

export type TranslateKey = Key | PluralBase<Key>;

/** Falls back to English for anything the browser reports that we do not ship. */
export function systemLocale(): Locale {
  const tag = navigator.language?.slice(0, 2).toLowerCase();
  return tag === "fr" ? "fr" : "en";
}

export type TranslateParams = Record<string, string | number>;

export function translate(
  locale: Locale,
  key: TranslateKey,
  params?: TranslateParams,
): string {
  const dict = DICTIONARIES[locale];
  const direct = key as Key;
  let template: string = dict[direct] ?? en[direct] ?? key;

  // Plural keys are addressed by their base name; pick the right variant.
  if (params && typeof params.n === "number") {
    const plural = new Intl.PluralRules(locale).select(params.n);
    const variant = `${key}_${plural}` as Key;
    const fallback = `${key}_other` as Key;
    const resolved = dict[variant] ?? dict[fallback];
    if (resolved) template = resolved;
  }

  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

export type Translator = (key: TranslateKey, params?: TranslateParams) => string;

/** Resolves the active locale from settings, falling back to the OS language. */
export function useLocale(): Locale {
  const { state } = useStore();
  return state.settings.locale ?? systemLocale();
}

export function useT(): Translator {
  const locale = useLocale();
  return useCallback(
    (key: TranslateKey, params?: TranslateParams) => translate(locale, key, params),
    [locale],
  );
}
