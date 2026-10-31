/**
 * The strings of Settings › Kits and of the dialog that enables a kit on a
 * workspace. i18n.ts merges both dictionaries into its own, so a key reads
 * through `t()` like any other; keys start with `kitsManage.`.
 *
 * `en` is the reference. `fr` is typed against it, so the build fails on a
 * French key that is missing or misspelled — here, without i18n.ts having
 * to change. French follows the glossary and the typography of i18n.ts:
 * plain spaces before ? ! ; : » and after «, the typesetter does the rest.
 */

export const en = {
  "kitsManage.title": "Kits",
  "kitsManage.keywords": "kit plugin skills commands mcp tabs setup github folder install",
  "kitsManage.intro":
    "A kit brings a domain into Hangar: skills, commands, agents and MCP servers for Claude Code, tabs of its own, and setup commands. Kits reach Claude Code panes only.",
  "kitsManage.installed": "Installed kits",
  "kitsManage.empty": "No kit yet. Add one from a folder or from a GitHub link.",
  "kitsManage.loading": "Reading the kits…",
  "kitsManage.sourceFolder": "Folder: {path}",
  "kitsManage.sourceGit": "GitHub: {url}",
  "kitsManage.linked": "linked, read live",
  "kitsManage.version": "v{version}",
  "kitsManage.broken": "This kit cannot be used",
  "kitsManage.brokenName": "Unreadable kit ({id})",
  "kitsManage.add": "Add a kit",
  "kitsManage.addFolder": "Add from a folder",
  "kitsManage.addFolderTitle": "Choose the folder that holds hangar-kit.json",
  "kitsManage.addFolderHint":
    "The folder is linked, not copied: what you edit there is what the panes load.",
  "kitsManage.addGit": "Add from GitHub",
  "kitsManage.gitLabel": "GitHub link",
  "kitsManage.gitHint":
    "A repository, or a folder of one: {example}. Cloning runs nothing from it.",
  "kitsManage.adding": "Adding…",
  "kitsManage.added": "{name} added.",
  "kitsManage.checkUpdate": "Check for update",
  "kitsManage.checking": "Checking…",
  "kitsManage.upToDate": "Up to date (v{version}).",
  "kitsManage.folderNoUpdate": "A linked folder has no remote: edit it, then reload.",
  "kitsManage.updateAvailable": "Update available: v{current} → v{latest}",
  "kitsManage.update": "Update",
  "kitsManage.updating": "Updating…",
  "kitsManage.updated": "Updated: v{from} → v{to}.",
  "kitsManage.updatedPerms":
    "This version asks for new permissions, or changes what runs by itself: workspaces that use it hold these back until you accept them again.",
  "kitsManage.reload": "Reload",
  "kitsManage.reloaded": "Reloaded: v{version}.",
  "kitsManage.remove": "Remove",
  "kitsManage.removeAsk": "Remove {name}?",
  "kitsManage.removeGit": "The copy Hangar cloned is deleted. Workspaces that use it lose it.",
  "kitsManage.removeFolder":
    "The linked folder is left untouched; only the link goes. Workspaces that use it lose it.",
  "kitsManage.removeConfirm": "Remove the kit",
  "kitsManage.cancel": "Cancel",
  "kitsManage.errorPrefix": "Error: {detail}",

  "kitsManage.byWorkspace": "Kits per workspace",
  "kitsManage.byWorkspaceHint":
    "Enable kits on a workspace that exists: run their setup commands in it and restart its Claude Code panes.",
  "kitsManage.enabledCount_one": "{n} kit enabled",
  "kitsManage.enabledCount_other": "{n} kits enabled",
  "kitsManage.manage": "Manage kits",
  "kitsManage.menu": "Kits…",

  "kitsManage.sectionTitle": "Kits",
  "kitsManage.sectionHint":
    "Enable the kits this workspace uses. Their skills, commands, agents and MCP servers load in its Claude Code panes.",
  "kitsManage.sectionNone": "No kit installed. Add one in Settings › Kits.",
  "kitsManage.toggle": "Enable {name} on this workspace",
  "kitsManage.statusOn": "Enabled",
  "kitsManage.statusOff": "Off",
  "kitsManage.statusReview": "New permissions to accept",
  "kitsManage.statusReviewHooks": "Hooks to review: kept out of new panes",
  "kitsManage.statusBroken": "Unusable",
  "kitsManage.review": "Review",
  "kitsManage.details": "Details",
  "kitsManage.pendingNote":
    "Enabled once the workspace is created. Setup commands run afterwards, from the workspace's \"Kits…\" menu.",
  "kitsManage.editTitle": "Kits of {name}",
  "kitsManage.editDone": "Done",

  "kitsManage.actTitle": "Enable {name}",
  "kitsManage.actReviewTitle": "{name}: review",
  "kitsManage.actOnWorkspace": "On the workspace {name}",
  "kitsManage.actBrings": "What this kit brings",
  "kitsManage.actNothing": "Nothing for Claude Code and no tab.",
  "kitsManage.actSkills": "Skills",
  "kitsManage.actCommands": "Commands",
  "kitsManage.actAgents": "Agents",
  "kitsManage.actMcp": "MCP servers",
  "kitsManage.actTabs": "Tabs",
  "kitsManage.actPerms": "What its tabs ask for",
  "kitsManage.actNoPerms": "It asks for no permission.",
  "kitsManage.actPermsFor": "{tab} may:",
  "kitsManage.actPermsNew": "New",
  "kitsManage.actPermsGranted": "Already accepted",
  "kitsManage.actSetup": "Setup commands",
  "kitsManage.actNoSetup": "No setup command for this system.",
  "kitsManage.actSetupHint":
    "Shown exactly as they will run, in a pane you can see, only when you click Run, once per machine.",
  "kitsManage.actCommandOs": "Command for {os}",
  "kitsManage.actHooks": "Commands that run by themselves",
  "kitsManage.actNoHooks": "No hook: nothing runs by itself.",
  "kitsManage.actHooksHint": "Claude Code runs these by itself, without asking, each time they match.",
  "kitsManage.actHooksChanged":
    "These changed since they were accepted here. Until you accept them, this kit stays out of the workspace's new panes.",
  "kitsManage.actHooksLive":
    "A linked folder is read live: what you change in these commands applies without being asked again.",
  "kitsManage.actHookCommand": "Command of the hook {event}",
  "kitsManage.stepDone": "Done on this machine",
  "kitsManage.stepTodo": "Not run yet",
  "kitsManage.stepRun": "Run",
  "kitsManage.stepRunning": "Opening the pane…",
  "kitsManage.stepRunNeedsWs": "Available once the workspace exists.",
  "kitsManage.stepOpened":
    "Running in the pane {pane}. It is the visible pane of the workspace; close the Settings if they hide it. Hangar cannot read its exit code: once it ends, tell Hangar how it went.",
  "kitsManage.stepConfirm": "It worked, mark it done",
  "kitsManage.stepNotYet": "Not yet",
  "kitsManage.stepFailed": "Could not run it: {detail}",
  "kitsManage.stepNoPane": "The workspace has no room for another pane.",
  "kitsManage.stepPaneLost":
    "The pane did not start in time, so it was closed and nothing was typed. Open the workspace, then run it again.",
  "kitsManage.stepPaneLostDetached":
    "The workspace is in a window of its own and its pane did not start in time. Nothing was typed. Open that window, then run it again.",
  "kitsManage.actClaudeOnly": "Kits reach Claude Code panes only, not the other agents or plain shells.",
  "kitsManage.actAccept": "Accept and enable",
  "kitsManage.actAcceptPerms": "Accept the permissions",
  "kitsManage.actAcceptChanged": "Accept what changed",
  "kitsManage.actRefuse": "Not now",
  "kitsManage.actClose": "Close",
  "kitsManage.osWindows": "Windows",
  "kitsManage.osMacos": "macOS",
  "kitsManage.osLinux": "Linux",

  "kitsManage.permWorkspaceRead": "read this workspace's name and folder",
  "kitsManage.permPanesRead": "see your panes and when they change",
  "kitsManage.permPanesSend": "send messages to your agents",
  "kitsManage.permFilesRead": "read text files inside the workspace folder",
  "kitsManage.permDataRead": "read the data this kit keeps in its own folder",
  "kitsManage.permDataWrite": "write data in this kit's own folder",
  "kitsManage.permLoopback":
    "talk to the local server on port {port}, and make it do whatever that server allows",
  "kitsManage.permPreviewShow": "open files in Hangar's preview",
  "kitsManage.permNotify": "show desktop notifications",

  "kitsManage.warnHooks":
    "Kit “{id}”: its hooks changed and are not accepted on this workspace, so the pane starts without it. Review it in the workspace's “Kits…” menu.",

  "kitsManage.restartTitle": "Running agents do not have it yet",
  "kitsManage.restartBody":
    "A Claude Code that is already running keeps the kits it started with. It gets {name} when it restarts.",
  "kitsManage.restartAction": "Restart the Claude Code panes ({n})",
  "kitsManage.restartResume": "Each one resumes its conversation.",
  "kitsManage.restartLater": "Later",
  "kitsManage.restartDone": "Restarted.",
} as const;

export const fr: Record<keyof typeof en, string> = {
  "kitsManage.title": "Kits",
  "kitsManage.keywords": "kit plugin skills commandes mcp onglets installation github dossier",
  "kitsManage.intro":
    "Un kit apporte un domaine à Hangar : skills, commandes, agents et serveurs MCP pour Claude Code, onglets à lui et commandes d'installation. Les kits n'atteignent que les panes Claude Code.",
  "kitsManage.installed": "Kits installés",
  "kitsManage.empty": "Aucun kit pour l'instant. Ajoutez-en un depuis un dossier ou un lien GitHub.",
  "kitsManage.loading": "Lecture des kits…",
  "kitsManage.sourceFolder": "Dossier : {path}",
  "kitsManage.sourceGit": "GitHub : {url}",
  "kitsManage.linked": "lié, lu en direct",
  "kitsManage.version": "v{version}",
  "kitsManage.broken": "Ce kit est inutilisable",
  "kitsManage.brokenName": "Kit illisible ({id})",
  "kitsManage.add": "Ajouter un kit",
  "kitsManage.addFolder": "Ajouter depuis un dossier",
  "kitsManage.addFolderTitle": "Choisissez le dossier qui contient hangar-kit.json",
  "kitsManage.addFolderHint":
    "Le dossier est lié, pas copié : ce que vous y modifiez est ce que les panes chargent.",
  "kitsManage.addGit": "Ajouter depuis GitHub",
  "kitsManage.gitLabel": "Lien GitHub",
  "kitsManage.gitHint":
    "Un dépôt, ou un dossier de dépôt : {example}. Le clonage n'exécute rien de ce qu'il contient.",
  "kitsManage.adding": "Ajout…",
  "kitsManage.added": "{name} ajouté.",
  "kitsManage.checkUpdate": "Chercher une mise à jour",
  "kitsManage.checking": "Vérification…",
  "kitsManage.upToDate": "À jour (v{version}).",
  "kitsManage.folderNoUpdate": "Un dossier lié n'a pas de dépôt distant : modifiez-le, puis rechargez.",
  "kitsManage.updateAvailable": "Mise à jour disponible : v{current} → v{latest}",
  "kitsManage.update": "Mettre à jour",
  "kitsManage.updating": "Mise à jour…",
  "kitsManage.updated": "Mis à jour : v{from} → v{to}.",
  "kitsManage.updatedPerms":
    "Cette version demande de nouvelles autorisations, ou change ce qui se lance tout seul : les workspaces qui l'utilisent les tiennent à l'écart tant que vous ne les acceptez pas à nouveau.",
  "kitsManage.reload": "Recharger",
  "kitsManage.reloaded": "Rechargé : v{version}.",
  "kitsManage.remove": "Retirer",
  "kitsManage.removeAsk": "Retirer {name} ?",
  "kitsManage.removeGit": "La copie clonée par Hangar est supprimée. Les workspaces qui l'utilisent la perdent.",
  "kitsManage.removeFolder":
    "Le dossier lié n'est pas touché : seul le lien disparaît. Les workspaces qui l'utilisent le perdent.",
  "kitsManage.removeConfirm": "Retirer le kit",
  "kitsManage.cancel": "Annuler",
  "kitsManage.errorPrefix": "Erreur : {detail}",

  "kitsManage.byWorkspace": "Kits par workspace",
  "kitsManage.byWorkspaceHint":
    "Activez des kits sur un workspace existant : lancez-y leurs commandes d'installation et redémarrez ses panes Claude Code.",
  "kitsManage.enabledCount_one": "{n} kit activé",
  "kitsManage.enabledCount_other": "{n} kits activés",
  "kitsManage.manage": "Gérer les kits",
  "kitsManage.menu": "Kits…",

  "kitsManage.sectionTitle": "Kits",
  "kitsManage.sectionHint":
    "Activez les kits que ce workspace utilise. Leurs skills, commandes, agents et serveurs MCP se chargent dans ses panes Claude Code.",
  "kitsManage.sectionNone": "Aucun kit installé. Ajoutez-en un dans Réglages › Kits.",
  "kitsManage.toggle": "Activer {name} sur ce workspace",
  "kitsManage.statusOn": "Activé",
  "kitsManage.statusOff": "Désactivé",
  "kitsManage.statusReview": "Nouvelles autorisations à accepter",
  "kitsManage.statusReviewHooks": "Hooks à examiner : hors des nouveaux panes",
  "kitsManage.statusBroken": "Inutilisable",
  "kitsManage.review": "Examiner",
  "kitsManage.details": "Détails",
  "kitsManage.pendingNote":
    "Activé à la création du workspace. Les commandes d'installation se lancent ensuite depuis le menu « Kits… » du workspace.",
  "kitsManage.editTitle": "Kits de {name}",
  "kitsManage.editDone": "Terminé",

  "kitsManage.actTitle": "Activer {name}",
  "kitsManage.actReviewTitle": "{name} : examen",
  "kitsManage.actOnWorkspace": "Sur le workspace {name}",
  "kitsManage.actBrings": "Ce que ce kit apporte",
  "kitsManage.actNothing": "Rien pour Claude Code et aucun onglet.",
  "kitsManage.actSkills": "Skills",
  "kitsManage.actCommands": "Commandes",
  "kitsManage.actAgents": "Agents",
  "kitsManage.actMcp": "Serveurs MCP",
  "kitsManage.actTabs": "Onglets",
  "kitsManage.actPerms": "Ce que ses onglets demandent",
  "kitsManage.actNoPerms": "Il ne demande aucune autorisation.",
  "kitsManage.actPermsFor": "{tab} pourra :",
  "kitsManage.actPermsNew": "Nouveau",
  "kitsManage.actPermsGranted": "Déjà acceptée",
  "kitsManage.actSetup": "Commandes d'installation",
  "kitsManage.actNoSetup": "Aucune commande d'installation pour ce système.",
  "kitsManage.actSetupHint":
    "Affichées exactement comme elles s'exécuteront, dans un pane visible, seulement quand vous cliquez sur Lancer, une fois par machine.",
  "kitsManage.actCommandOs": "Commande pour {os}",
  "kitsManage.actHooks": "Commandes qui se lancent toutes seules",
  "kitsManage.actNoHooks": "Aucun hook : rien ne se lance tout seul.",
  "kitsManage.actHooksHint":
    "Claude Code les lance de lui-même, sans demander, chaque fois qu'elles correspondent.",
  "kitsManage.actHooksChanged":
    "Elles ont changé depuis qu'elles ont été acceptées ici. Tant que vous ne les acceptez pas, ce kit reste hors des nouveaux panes du workspace.",
  "kitsManage.actHooksLive":
    "Un dossier lié est lu en direct : ce que vous changez dans ces commandes s'applique sans nouvelle question.",
  "kitsManage.actHookCommand": "Commande du hook {event}",
  "kitsManage.stepDone": "Faite sur cette machine",
  "kitsManage.stepTodo": "Pas encore lancée",
  "kitsManage.stepRun": "Lancer",
  "kitsManage.stepRunning": "Ouverture du pane…",
  "kitsManage.stepRunNeedsWs": "Disponible une fois le workspace créé.",
  "kitsManage.stepOpened":
    "Elle tourne dans le pane {pane}. C'est le pane visible du workspace ; fermez les Réglages s'ils le cachent. Hangar ne peut pas lire son code de sortie : quand elle est finie, dites-lui comment ça s'est passé.",
  "kitsManage.stepConfirm": "Ça a marché, la marquer faite",
  "kitsManage.stepNotYet": "Pas encore",
  "kitsManage.stepFailed": "Impossible de la lancer : {detail}",
  "kitsManage.stepNoPane": "Le workspace n'a plus de place pour un autre pane.",
  "kitsManage.stepPaneLost":
    "Le pane n'a pas démarré à temps : il a été fermé et rien n'a été saisi. Ouvrez le workspace, puis relancez.",
  "kitsManage.stepPaneLostDetached":
    "Le workspace est dans une fenêtre à part et son pane n'a pas démarré à temps. Rien n'a été saisi. Ouvrez cette fenêtre, puis relancez.",
  "kitsManage.actClaudeOnly":
    "Les kits n'atteignent que les panes Claude Code, ni les autres agents ni les shells simples.",
  "kitsManage.actAccept": "Accepter et activer",
  "kitsManage.actAcceptPerms": "Accepter les autorisations",
  "kitsManage.actAcceptChanged": "Accepter ce qui a changé",
  "kitsManage.actRefuse": "Pas maintenant",
  "kitsManage.actClose": "Fermer",
  "kitsManage.osWindows": "Windows",
  "kitsManage.osMacos": "macOS",
  "kitsManage.osLinux": "Linux",

  "kitsManage.permWorkspaceRead": "lire le nom et le dossier de ce workspace",
  "kitsManage.permPanesRead": "voir vos panes et leurs changements",
  "kitsManage.permPanesSend": "envoyer des messages à vos agents",
  "kitsManage.permFilesRead": "lire des fichiers texte du dossier du workspace",
  "kitsManage.permDataRead": "lire les données que ce kit garde dans son propre dossier",
  "kitsManage.permDataWrite": "écrire des données dans le dossier propre à ce kit",
  "kitsManage.permLoopback":
    "parler au serveur local sur le port {port}, et lui faire faire tout ce que ce serveur permet",
  "kitsManage.permPreviewShow": "ouvrir des fichiers dans l'aperçu de Hangar",
  "kitsManage.permNotify": "afficher des notifications du bureau",

  "kitsManage.warnHooks":
    "Kit « {id} » : ses hooks ont changé et ne sont pas acceptés sur ce workspace, le pane démarre donc sans lui. Examinez-le depuis le menu « Kits… » du workspace.",

  "kitsManage.restartTitle": "Les agents en cours ne l'ont pas encore",
  "kitsManage.restartBody":
    "Un Claude Code déjà lancé garde les kits avec lesquels il a démarré. Il aura {name} à son prochain redémarrage.",
  "kitsManage.restartAction": "Redémarrer les panes Claude Code ({n})",
  "kitsManage.restartResume": "Chacun reprend sa conversation.",
  "kitsManage.restartLater": "Plus tard",
  "kitsManage.restartDone": "Redémarrés.",
};
