"use strict";
// What is neither a file format nor the behaviour: the tab's words in en and fr,
// its params read like form input, and the picker's order. No DOM, no Hangar.
(function (root, factory) {
  const dotenv = typeof module === "object" && module.exports ? require("./dotenv.js") : root.EditorDotenv;
  const api = factory(dotenv);
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.EditorLogic = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function (dotenv) {
  const STRINGS = {
    en: {
      tab: "Editor",
      loading: "Loading…",
      retry: "Try again",
      back: "← Files",
      pickTitle: "Choose a file",
      pickFilter: "Filter by name",
      groupEnv: "Environment files",
      groupConfig: "Configuration files",
      groupOther: "Other files",
      pickEmpty: "No file matches.",
      pickNew: "new",
      pickTruncated: "This folder holds more files than Hangar lists: use the filter, or ask for the file by its path.",
      pickMore: "{count} more: filter to find them.",
      askedBy: "{pane} asks:",
      asked: "Asked for:",
      newFile: "This file does not exist yet. Saving creates it.",
      save: "Save",
      saving: "Saving…",
      savedLocal: "Saved.",
      nothingToSave: "Nothing to save yet.",
      told: "{pane} was told, without any value.",
      toldText: "{pane} was told the file changed.",
      notTold: "Saved, but {pane} could not be told: {reason} No value was sent.",
      tellAgain: "Tell {pane} again",
      tellNoChange: "Tell {pane}: nothing to change",
      toldNoChange: "{pane} was told you changed nothing.",
      "tell.not_found": "that pane is gone.",
      "tell.denied": "only a Claude Code agent can be told, and this tab must be granted panes.send.",
      "tell.unavailable": "its session does not run Hangar's bridge.",
      "tell.rate_limited": "too many messages this minute.",
      "tell.other": "Hangar refused.",
      leaveTitle: "You have unsaved changes.",
      leaveHint: "Going back loses them.",
      leaveConfirm: "Lose them and go back",
      leaveCancel: "Keep editing",

      keyMissing: "missing",
      keyAsked: "asked",
      keySet: "set",
      keyTwice: "defined {count} times: this edits the last one, which is the one read",
      valueHidden: "Hidden",
      valueEmpty: "Empty",
      valueLines: "Several lines: show to edit",
      show: "Show",
      hide: "Hide",
      offeredTitle: "In {file}, missing here",
      add: "Add",
      addAll: "Add all",
      newKeyLabel: "Add a key",
      newKeyPlaceholder: "KEY_NAME",
      newKeyBad: "A key is letters, digits, _ . and -, not starting with a digit.",
      newKeyTaken: "This key is already in the list.",
      otherLines: "{count} line(s) that are not KEY=value are kept as they are.",
      noKeys: "No key in this file yet.",

      ignoreWarning: "{file} is not ignored by git: git would commit it, secrets included.",
      ignoreNoFile: "There is no .gitignore here: git would commit {file}, secrets included.",
      ignoreUnknown: "Could not read .gitignore, so this tab cannot say whether git ignores {file}.",
      ignoreAdd: "Add {pattern} to .gitignore",
      ignoreAdding: "Adding…",
      ignoreFailed: ".gitignore was not changed: {reason}",

      editorLabel: "File contents",
      review: "Review changes",
      backToEdit: "← Back to editing",
      diffTitle: "Changes to save",
      diffNone: "Nothing changed.",
      diffSkip: "{count} unchanged line(s)",
      checkOk: "Valid {kind}.",
      checkPartial: "{kind}: no problem found. This is a basic check, not a proof.",
      checkBad: "Broken {kind}: {reason}",
      checkBadAt: "Broken {kind}, line {line}: {reason}",
      ackBroken: "I know this file is broken: save it anyway.",
      "check.json": "this is not valid JSON.",
      "check.yaml-tab": "a tab in the indentation. YAML indents with spaces.",
      "check.yaml-quote": "a quote that never closes.",
      "check.yaml-bracket": "a bracket that does not match.",
      "check.yaml-reserved": "a value cannot start with @ or ` without quotes.",
      "check.toml-trailing": "something follows the value on this line.",
      "check.toml-string": "a string that never closes.",
      "check.toml-escape": "an escape TOML does not know.",
      "check.toml-key": "a key is expected here.",
      "check.toml-equals": "a key must be followed by =.",
      "check.toml-value": "this value cannot be read.",
      "check.toml-header": "a table header that does not close.",
      "check.toml-duplicate": "this key or table is given twice.",
      lineJumpFar: "Line {line} was asked for; the file has {total}.",

      conflictTitle: "This file changed while you were editing it",
      conflictHint: "Nothing was written. Choose what to keep.",
      conflictGone: "The file was deleted while you were editing it. Nothing was written.",
      conflictDiff: "− on disk now   + yours",
      conflictKeepMine: "Replace it with mine",
      conflictWriteMine: "Write mine",
      conflictTakeTheirs: "Keep what is on disk, lose mine",
      conflictEdit: "Back to editing, with mine",
      conflictRows: "Keys you and the file disagree on",
      conflictNone: "No key disagrees: your changes can go in the new file as they are.",
      conflictApply: "Save with these choices",
      conflictTheirsChanged: "{count} other key(s) changed on disk, and stay as they are there.",
      mine: "Yours",
      theirs: "On disk",
      absent: "absent",

      "err.denied.title": "This tab may not read or write here",
      "err.denied.hint": "Enable the kit on this workspace and accept files.read and files.write (Kits menu).",
      "err.invalid.title": "Hangar refused this path",
      "err.invalid.hint": "A hidden file needs both files.read and files.write. The path must stay inside the workspace folder, outside .git.",
      "err.bad-path.title": "This path cannot be opened",
      "err.bad-path.hint": "The path must be relative to the workspace folder, with no .. and no drive, outside .git.",
      "err.failed.title": "This file cannot be opened",
      "err.failed.hint": "Only UTF-8 text files of 1 MB at most can be opened here.",
      "err.unavailable.title": "Hangar cannot do this now",
      "err.unavailable.hint": "The workspace or its folder may be gone. Reload the tab in a moment.",
      "err.timeout.title": "Hangar did not answer in time",
      "err.timeout.hint": "Try again.",
      "err.error.title": "Something went wrong",
      "err.error.hint": "Hangar said:",
      "write.denied": "this tab was not granted files.write. Your changes are still here.",
      "write.rate_limited": "at most 10 saves a minute. Your changes are still here: try again shortly.",
      "write.invalid": "Hangar refused it (a file holds 1 MB at most). Your changes are still here.",
      "write.other": "it could not be written. Your changes are still here.",
      "write.conflict": ".gitignore changed in the meantime. Click again to use the new one.",
      badgeMissing: "{count} missing",
      badgeSaved: "saved",
    },
    fr: {
      tab: "Éditeur",
      loading: "Chargement…",
      retry: "Réessayer",
      back: "← Fichiers",
      pickTitle: "Choisir un fichier",
      pickFilter: "Filtrer par nom",
      groupEnv: "Fichiers d'environnement",
      groupConfig: "Fichiers de configuration",
      groupOther: "Autres fichiers",
      pickEmpty: "Aucun fichier ne correspond.",
      pickNew: "nouveau",
      pickTruncated: "Ce dossier contient plus de fichiers que Hangar n'en liste : utilisez le filtre, ou demandez le fichier par son chemin.",
      pickMore: "{count} de plus : filtrez pour les trouver.",
      askedBy: "{pane} demande :",
      asked: "Demandé :",
      newFile: "Ce fichier n'existe pas encore. L'enregistrer le crée.",
      save: "Enregistrer",
      saving: "Enregistrement…",
      savedLocal: "Enregistré.",
      nothingToSave: "Rien à enregistrer pour l'instant.",
      told: "{pane} a été prévenu, sans aucune valeur.",
      toldText: "{pane} a été prévenu que le fichier a changé.",
      notTold: "Enregistré, mais {pane} n'a pas pu être prévenu : {reason} Aucune valeur n'a été envoyée.",
      tellAgain: "Prévenir {pane} à nouveau",
      tellNoChange: "Dire à {pane} : rien à changer",
      toldNoChange: "{pane} sait que vous n'avez rien changé.",
      "tell.not_found": "ce panneau n'existe plus.",
      "tell.denied": "seul un agent Claude Code peut être prévenu, et cet onglet doit avoir panes.send.",
      "tell.unavailable": "sa session ne fait pas tourner le pont de Hangar.",
      "tell.rate_limited": "trop de messages cette minute.",
      "tell.other": "Hangar a refusé.",
      leaveTitle: "Vous avez des modifications non enregistrées.",
      leaveHint: "Revenir en arrière les fait perdre.",
      leaveConfirm: "Les perdre et revenir",
      leaveCancel: "Continuer à modifier",

      keyMissing: "manquante",
      keyAsked: "demandée",
      keySet: "remplie",
      keyTwice: "définie {count} fois : ceci modifie la dernière, celle qui est lue",
      valueHidden: "Masquée",
      valueEmpty: "Vide",
      valueLines: "Plusieurs lignes : afficher pour modifier",
      show: "Afficher",
      hide: "Masquer",
      offeredTitle: "Dans {file}, absentes ici",
      add: "Ajouter",
      addAll: "Tout ajouter",
      newKeyLabel: "Ajouter une clé",
      newKeyPlaceholder: "NOM_DE_CLE",
      newKeyBad: "Une clé s'écrit avec des lettres, des chiffres, _ . et -, sans commencer par un chiffre.",
      newKeyTaken: "Cette clé est déjà dans la liste.",
      otherLines: "{count} ligne(s) qui ne sont pas de la forme CLE=valeur restent telles quelles.",
      noKeys: "Aucune clé dans ce fichier pour l'instant.",

      ignoreWarning: "{file} n'est pas ignoré par git : git le committerait, secrets compris.",
      ignoreNoFile: "Il n'y a pas de .gitignore ici : git committerait {file}, secrets compris.",
      ignoreUnknown: "Le .gitignore n'a pas pu être lu : cet onglet ne peut pas dire si git ignore {file}.",
      ignoreAdd: "Ajouter {pattern} au .gitignore",
      ignoreAdding: "Ajout…",
      ignoreFailed: "Le .gitignore n'a pas été modifié : {reason}",

      editorLabel: "Contenu du fichier",
      review: "Voir les changements",
      backToEdit: "← Revenir à l'édition",
      diffTitle: "Changements à enregistrer",
      diffNone: "Rien n'a changé.",
      diffSkip: "{count} ligne(s) inchangée(s)",
      checkOk: "{kind} valide.",
      checkPartial: "{kind} : aucun problème trouvé. C'est une vérification de base, pas une preuve.",
      checkBad: "{kind} cassé : {reason}",
      checkBadAt: "{kind} cassé, ligne {line} : {reason}",
      ackBroken: "Je sais que ce fichier est cassé : l'enregistrer quand même.",
      "check.json": "ce n'est pas du JSON valide.",
      "check.yaml-tab": "une tabulation dans l'indentation. YAML indente avec des espaces.",
      "check.yaml-quote": "un guillemet jamais refermé.",
      "check.yaml-bracket": "un crochet ou une accolade qui ne correspond pas.",
      "check.yaml-reserved": "une valeur ne peut pas commencer par @ ou ` sans guillemets.",
      "check.toml-trailing": "quelque chose suit la valeur sur cette ligne.",
      "check.toml-string": "une chaîne jamais refermée.",
      "check.toml-escape": "un échappement que TOML ne connaît pas.",
      "check.toml-key": "une clé est attendue ici.",
      "check.toml-equals": "une clé doit être suivie de =.",
      "check.toml-value": "cette valeur n'est pas lisible.",
      "check.toml-header": "un en-tête de table qui ne se referme pas.",
      "check.toml-duplicate": "cette clé ou cette table apparaît deux fois.",
      lineJumpFar: "La ligne {line} était demandée ; le fichier en a {total}.",

      conflictTitle: "Ce fichier a changé pendant que vous le modifiiez",
      conflictHint: "Rien n'a été écrit. Choisissez ce que vous gardez.",
      conflictGone: "Le fichier a été supprimé pendant que vous le modifiiez. Rien n'a été écrit.",
      conflictDiff: "− sur le disque   + le vôtre",
      conflictKeepMine: "Le remplacer par le mien",
      conflictWriteMine: "Écrire le mien",
      conflictTakeTheirs: "Garder celui du disque, perdre le mien",
      conflictEdit: "Revenir à l'édition, avec le mien",
      conflictRows: "Clés sur lesquelles vous et le fichier divergez",
      conflictNone: "Aucune clé ne diverge : vos changements peuvent aller tels quels dans le nouveau fichier.",
      conflictApply: "Enregistrer avec ces choix",
      conflictTheirsChanged: "{count} autre(s) clé(s) ont changé sur le disque, et y restent telles quelles.",
      mine: "La vôtre",
      theirs: "Sur le disque",
      absent: "absente",

      "err.denied.title": "Cet onglet n'a pas le droit de lire ou d'écrire ici",
      "err.denied.hint": "Activez le kit sur cet espace de travail et acceptez files.read et files.write (menu Kits).",
      "err.invalid.title": "Hangar a refusé ce chemin",
      "err.invalid.hint": "Un fichier caché demande files.read et files.write à la fois. Le chemin doit rester dans le dossier de l'espace de travail, hors de .git.",
      "err.bad-path.title": "Ce chemin ne peut pas être ouvert",
      "err.bad-path.hint": "Le chemin doit être relatif au dossier de l'espace de travail, sans .. ni lecteur, hors de .git.",
      "err.failed.title": "Ce fichier ne peut pas être ouvert",
      "err.failed.hint": "Seuls les fichiers texte UTF-8 de 1 Mo au plus s'ouvrent ici.",
      "err.unavailable.title": "Hangar ne peut pas le faire maintenant",
      "err.unavailable.hint": "L'espace de travail ou son dossier n'existe peut-être plus. Rechargez l'onglet dans un instant.",
      "err.timeout.title": "Hangar n'a pas répondu à temps",
      "err.timeout.hint": "Réessayez.",
      "err.error.title": "Quelque chose s'est mal passé",
      "err.error.hint": "Hangar a dit :",
      "write.denied": "cet onglet n'a pas reçu files.write. Vos changements sont toujours là.",
      "write.rate_limited": "10 enregistrements par minute au plus. Vos changements sont toujours là : réessayez bientôt.",
      "write.invalid": "Hangar a refusé (un fichier fait 1 Mo au plus). Vos changements sont toujours là.",
      "write.other": "il n'a pas pu être écrit. Vos changements sont toujours là.",
      "write.conflict": "le .gitignore a changé entre-temps. Cliquez à nouveau pour partir du nouveau.",
      badgeMissing: "{count} à remplir",
      badgeSaved: "enregistré",
    },
  };

  const pickLang = (language) => (String(language || "").toLowerCase().startsWith("fr") ? "fr" : "en");

  function t(lang, key, vars) {
    const text = (STRINGS[lang] && STRINGS[lang][key]) || STRINGS.en[key] || key;
    if (!vars) return text;
    return text.replace(/\{(\w+)\}/g, (whole, name) => (vars[name] === undefined ? whole : String(vars[name])));
  }

  const baseName = (path) => path.slice(path.lastIndexOf("/") + 1);

  /** A name for the tab strip: one line, 40 characters at most, an emoji counting as one. */
  const tabTitle = (path) => Array.from(baseName(path)).slice(0, 40).join("");

  /**
   * The path in `/` form, or null when Hangar would refuse it. Hangar checks it again;
   * this is so that a path a caller got wrong is said to be wrong, not read as a failure.
   */
  function checkPath(raw) {
    if (typeof raw !== "string" || raw.trim() === "" || raw.length > 1024) return null;
    // eslint-disable-next-line no-control-regex
    if (/[\u0000-\u001f\u007f]/.test(raw)) return null;
    if (/^[/\\]/.test(raw)) return null;
    const parts = raw.split(/[/\\]/).filter((part) => part !== "" && part !== ".");
    if (!parts.length) return null;
    for (const part of parts) {
      if (part === ".." || part.includes(":") || part.toLowerCase() === ".git" || /[. ]$/.test(part)) return null;
    }
    return parts.join("/");
  }

  /** `keys` is one string: names separated by commas. What is not a key name is dropped. */
  function parseKeys(raw) {
    if (typeof raw !== "string") return [];
    return [...new Set(raw.split(",").map((key) => key.trim()).filter(dotenv.isKey))];
  }

  function parseLine(raw) {
    if (typeof raw !== "string" || !/^[1-9][0-9]{0,8}$/.test(raw)) return null;
    return Number(raw);
  }

  /** What `tab.params` said, read as anything typed into a form. */
  function readParams(message) {
    const params = (message && message.params) || {};
    const given = typeof params.path === "string" && params.path !== "";
    const path = given ? checkPath(params.path) : null;
    const why = typeof params.why === "string" && params.why.trim() ? params.why.trim() : null;
    const caller =
      message && message.caller && typeof message.caller.pane === "string" && message.caller.pane
        ? { pane: message.caller.pane }
        : null;
    return { path, badPath: given && !path, keys: parseKeys(params.keys), why, line: parseLine(params.line), caller };
  }

  /* ── the picker ───────────────────────────────────────────────── */

  const CONFIG_EXTENSION = /\.(json|jsonc|json5|ya?ml|toml|ini|cfg|conf|config|properties|xml|plist)$/i;
  const CONFIG_NAME = /^(\.[\w.-]*(rc|ignore|config|attributes)|dockerfile|.*\.dockerfile|docker-compose\..*)$/i;
  const GENERATED = /(^|[-.])lock(\.[a-z]+)?$|^package-lock\.json$|^pnpm-lock\.yaml$|^yarn\.lock$/i;
  const NOT_TEXT = /\.(png|jpe?g|gif|webp|avif|bmp|ico|tiff?|pdf|zip|gz|tgz|bz2|xz|7z|rar|tar|exe|dll|so|dylib|bin|class|o|obj|a|lib|wasm|woff2?|ttf|otf|eot|mp[34]|wav|ogg|m4a|webm|mov|avi|mkv|sqlite3?|db|pyc|node)$/i;
  const SHOWN_PER_GROUP = 200;

  function classify(path) {
    const name = baseName(path);
    if (NOT_TEXT.test(name)) return null;
    if (dotenv.isDotenvPath(path)) return "env";
    if (GENERATED.test(name)) return "other";
    return CONFIG_EXTENSION.test(name) || CONFIG_NAME.test(name) ? "config" : "other";
  }

  /** Env files first, then configuration, in Hangar's order (the nearest first); what is not text is left out. */
  function rankFiles(files) {
    const groups = { env: [], config: [], other: [] };
    for (const file of files || []) {
      if (!file || typeof file.path !== "string") continue;
      const group = classify(file.path);
      if (group) groups[group].push({ path: file.path, isNew: false });
    }
    // The file an agent or a person usually wants is the one that is not there yet.
    if (!groups.env.some((file) => file.path === ".env")) groups.env.unshift({ path: ".env", isNew: true });
    return groups;
  }

  /** The groups a filter leaves, each cut to what a page can hold, saying how many it cut. */
  function filterGroups(groups, query) {
    const needle = String(query || "").trim().toLowerCase();
    return ["env", "config", "other"].map((id) => {
      const all = groups[id].filter((file) => file.path.toLowerCase().includes(needle));
      return { id, files: all.slice(0, SHOWN_PER_GROUP), more: Math.max(0, all.length - SHOWN_PER_GROUP) };
    });
  }

  return { STRINGS, pickLang, t, baseName, tabTitle, checkPath, parseKeys, parseLine, readParams, classify, rankFiles, filterGroups };
});
