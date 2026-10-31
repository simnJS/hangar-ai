"use strict";
// The tab's pure logic: the script's documents, checked and turned into what the page shows, the
// strings in both languages, and the brief an agent is handed. No DOM, no Hangar: the page and
// the tests both load this file (a plain script in the page, where it sets `DbLogic`).
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.DbLogic = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const STATE_FILE = "state.json";
  const PAGE_SIZE = 50;
  const CELL_CHARS = 200;
  const FULL_CHARS = 100000;
  // The same rules as the manifest's patterns, which Hangar holds every value to.
  const PATH_RE = /^[A-Za-z0-9_.][A-Za-z0-9_./ -]{0,254}$/;
  const TABLE_RE = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/;
  const ERRORS = ["bad-args", "not-found", "outside", "not-database", "bad-table", "locked", "no-sqlite", "failed"];

  // In the shape hangar.ui.i18n reads: a count `n` picks `key_one` or `key_other`.
  const STRINGS = {
    en: {
      tab: "Database",
      readOnly: "Read-only",
      refresh: "Refresh",
      databaseLabel: "Database",
      loading: "Reading the database…",
      noDbTitle: "No database found",
      noDbText: "This tab looks for .db, .sqlite, .sqlite3 and .db3 files in the workspace folder and opens them read-only. Create one, or ask an agent to open one here.",
      unusable: "Not openable: its path has characters this kit does not accept.",
      lockedTitle: "The database is locked",
      lockedText: "Another program is writing to it. Wait until it is done, then try again. Nothing was changed.",
      noSqliteTitle: "No SQLite reader on this computer",
      noSqliteText: "The kit reads databases with Node.js 22.12 or later, or else with the sqlite3 program (3.33 or later). Install one of them, then try again.",
      deniedTitle: "This tab may not read the database",
      deniedText: "Accept command:list, command:tables, command:schema and command:rows in Settings › Kits, then try again. The tab only runs the read-only script it declares.",
      notDatabaseTitle: "This file is not a SQLite database",
      notDatabaseText: "Its header is not SQLite's, or it is encrypted. Pick another file.",
      goneTitle: "This file is not in the workspace any more",
      goneText: "Refresh the list, then pick another.",
      errorTitle: "The database could not be read",
      "error.timeout": "It took longer than the time allowed. Try again.",
      "error.not_found": "Node.js was not found on this computer. Install it, then try again.",
      "error.busy": "Hangar is already running the most commands it allows. Try again in a moment.",
      "error.unreadable": "The script printed something that is not an answer. Reload the tab, then try again.",
      "error.bad-table": "This database has no such table any more.",
      "error.bad-args": "The kit refused that request. Reload the tab, then try again.",
      "error.outside": "That path leads outside the workspace, which the kit never opens.",
      "error.failed": "The script could not read it: {detail}",
      retry: "Try again",
      tables: "Tables",
      view: "view",
      rowsCount_one: "{n} row",
      rowsCount_other: "{n} rows",
      columnsCount_one: "{n} column",
      columnsCount_other: "{n} columns",
      unsupportedName: "Name not supported by this kit",
      noTables: "This database has no table.",
      structure: "Structure",
      indexes: "Indexes",
      key: "key",
      required: "required",
      defaultValue: "default {value}",
      unique: "unique",
      columnsCut: "Only the first 200 columns are shown.",
      rowsTitle: "Rows",
      range: "{from}–{to} of {total}",
      previous: "Previous",
      next: "Next",
      emptyTable: "This table has no row.",
      sortBy: "Sort by {column}",
      "null": "NULL",
      blob_one: "{n} byte (binary)",
      blob_other: "{n} bytes (binary)",
      showFull: "Show all {n} characters",
      showLess: "Show less",
      loadingFull: "Reading…",
      fullFailed: "The whole value could not be read.",
      fullCut: "Shown up to {n} characters.",
      capped: "The page was shortened to keep the answer small.",
      secretNote: "Values may be secrets. They stay on this screen: nothing goes to an agent unless you click “Hand over to an agent”, and then only the table and your question, never its rows.",
      handOver: "Hand over to an agent",
      handOverTitle: "Hand over this table",
      questionLabel: "Your question",
      questionHint: "Sent with the table’s name and columns, never its rows.",
      questionPlaceholder: "What do you want the agent to find out?",
      agentLabel: "Agent",
      send: "Send",
      sending: "Sending…",
      cancel: "Cancel",
      sentTo: "Handed to {agent}",
      noAgent: "No Claude Code agent can receive text now.",
      panesDenied: "This tab may not list the agents. Accept panes.read in Settings › Kits to hand a table over.",
      "problem.denied": "This tab may not send to agents. Accept panes.send in Settings › Kits.",
      "problem.not_found": "{agent} is not in this workspace any more.",
      "problem.rate_limited": "Too many sends in a minute. Try again in a moment.",
      "problem.unavailable": "{agent} cannot receive text now. Pick another agent, or try again.",
      "problem.failed": "It could not be sent: {detail}",
    },
    fr: {
      tab: "Base de données",
      readOnly: "Lecture seule",
      refresh: "Actualiser",
      databaseLabel: "Base de données",
      loading: "Lecture de la base…",
      noDbTitle: "Aucune base de données trouvée",
      noDbText: "Cet onglet cherche les fichiers .db, .sqlite, .sqlite3 et .db3 dans le dossier de l’espace de travail et les ouvre en lecture seule. Créez-en une, ou demandez à un agent d’en ouvrir une ici.",
      unusable: "Ouverture impossible : son chemin contient des caractères que ce kit n’accepte pas.",
      lockedTitle: "La base de données est verrouillée",
      lockedText: "Un autre programme y écrit. Attendez qu’il ait fini, puis réessayez. Rien n’a été modifié.",
      noSqliteTitle: "Aucun lecteur SQLite sur cet ordinateur",
      noSqliteText: "Le kit lit les bases avec Node.js 22.12 ou plus récent, ou à défaut avec le programme sqlite3 (3.33 ou plus récent). Installez l’un des deux, puis réessayez.",
      deniedTitle: "Cet onglet n’a pas le droit de lire la base",
      deniedText: "Acceptez command:list, command:tables, command:schema et command:rows dans Réglages › Kits, puis réessayez. L’onglet ne lance que le script en lecture seule qu’il déclare.",
      notDatabaseTitle: "Ce fichier n’est pas une base SQLite",
      notDatabaseText: "Son en-tête n’est pas celui de SQLite, ou il est chiffré. Choisissez un autre fichier.",
      goneTitle: "Ce fichier n’est plus dans l’espace de travail",
      goneText: "Actualisez la liste, puis choisissez-en un autre.",
      errorTitle: "La base n’a pas pu être lue",
      "error.timeout": "La lecture a pris plus de temps que prévu. Réessayez.",
      "error.not_found": "Node.js est introuvable sur cet ordinateur. Installez-le, puis réessayez.",
      "error.busy": "Hangar lance déjà le maximum de commandes autorisé. Réessayez dans un instant.",
      "error.unreadable": "Le script a affiché autre chose qu’une réponse. Rechargez l’onglet, puis réessayez.",
      "error.bad-table": "Cette base n’a plus de table de ce nom.",
      "error.bad-args": "Le kit a refusé cette demande. Rechargez l’onglet, puis réessayez.",
      "error.outside": "Ce chemin sort de l’espace de travail, que le kit n’ouvre jamais.",
      "error.failed": "Le script n’a pas pu la lire : {detail}",
      retry: "Réessayer",
      tables: "Tables",
      view: "vue",
      rowsCount_one: "{n} ligne",
      rowsCount_other: "{n} lignes",
      columnsCount_one: "{n} colonne",
      columnsCount_other: "{n} colonnes",
      unsupportedName: "Nom non pris en charge par ce kit",
      noTables: "Cette base ne contient aucune table.",
      structure: "Structure",
      indexes: "Index",
      key: "clé",
      required: "obligatoire",
      defaultValue: "défaut {value}",
      unique: "unique",
      columnsCut: "Seules les 200 premières colonnes sont affichées.",
      rowsTitle: "Lignes",
      range: "{from}–{to} sur {total}",
      previous: "Précédent",
      next: "Suivant",
      emptyTable: "Cette table ne contient aucune ligne.",
      sortBy: "Trier par {column}",
      "null": "NULL",
      blob_one: "{n} octet (binaire)",
      blob_other: "{n} octets (binaire)",
      showFull: "Afficher les {n} caractères",
      showLess: "Réduire",
      loadingFull: "Lecture…",
      fullFailed: "La valeur entière n’a pas pu être lue.",
      fullCut: "Affichée jusqu’à {n} caractères.",
      capped: "La page a été raccourcie pour garder une réponse légère.",
      secretNote: "Les valeurs peuvent être des secrets. Elles restent sur cet écran : rien n’est envoyé à un agent tant que vous ne cliquez pas sur « Confier à un agent », et alors seulement la table et votre question, jamais ses lignes.",
      handOver: "Confier à un agent",
      handOverTitle: "Confier cette table",
      questionLabel: "Votre question",
      questionHint: "Envoyée avec le nom de la table et ses colonnes, jamais ses lignes.",
      questionPlaceholder: "Que voulez-vous que l’agent cherche ?",
      agentLabel: "Agent",
      send: "Envoyer",
      sending: "Envoi…",
      cancel: "Annuler",
      sentTo: "Confié à {agent}",
      noAgent: "Aucun agent Claude Code ne peut recevoir de texte pour l’instant.",
      panesDenied: "Cet onglet n’a pas le droit de lister les agents. Acceptez panes.read dans Réglages › Kits pour confier une table.",
      "problem.denied": "Cet onglet n’a pas le droit d’écrire aux agents. Acceptez panes.send dans Réglages › Kits.",
      "problem.not_found": "{agent} n’est plus dans cet espace de travail.",
      "problem.rate_limited": "Trop d’envois en une minute. Réessayez dans un instant.",
      "problem.unavailable": "{agent} ne peut pas recevoir de texte pour l’instant. Choisissez un autre agent, ou réessayez.",
      "problem.failed": "L’envoi a échoué : {detail}",
    },
  };

  const pickLang = (language) => (/^fr\b/i.test(String(language || "")) ? "fr" : "en");

  function t(lang, key, vars) {
    const table = STRINGS[lang] || STRINGS.en;
    const values = vars || {};
    const find = (dictionary) => {
      if (typeof values.n === "number") {
        const form = dictionary[`${key}_${values.n === 1 ? "one" : "other"}`];
        if (form !== undefined) return form;
      }
      return dictionary[key];
    };
    const template = find(table) ?? find(STRINGS.en) ?? key;
    return template.replace(/\{(\w+)\}/g, (whole, name) => (name in values ? String(values[name]) : whole));
  }

  const isRecord = (value) => value && typeof value === "object" && !Array.isArray(value);
  const text = (value, max) => (typeof value === "string" ? value.slice(0, max) : "");
  const count = (value) => (Number.isFinite(value) && value >= 0 ? Math.floor(value) : 0);
  const list = (value) => (Array.isArray(value) ? value : []);

  /** What a script printed: one JSON document with `ok`, else null. Both are data, and may be anything. */
  function parseAnswer(source) {
    let answer;
    try {
      answer = JSON.parse(String(source).replace(/^﻿/, ""));
    } catch {
      return null;
    }
    if (!isRecord(answer) || typeof answer.ok !== "boolean") return null;
    if (!answer.ok) return { ok: false, error: ERRORS.includes(answer.error) ? answer.error : "failed", detail: text(answer.detail, 300) };
    return answer;
  }

  /** `list`'s files, checked: the databases to pick from. */
  function cleanFiles(answer) {
    return list(answer.files)
      .filter((file) => isRecord(file) && typeof file.path === "string")
      .slice(0, 100)
      .map((file) => ({ path: text(file.path, 255), size: count(file.size), modified: count(file.modified), usable: file.usable === true && PATH_RE.test(file.path) }));
  }

  /** `tables`' answer, checked. */
  function cleanTables(answer) {
    return {
      size: count(answer.size),
      tables: list(answer.tables)
        .filter((item) => isRecord(item) && typeof item.name === "string")
        .slice(0, 500)
        .map((item) => ({
          name: text(item.name, 200),
          type: item.type === "view" ? "view" : "table",
          rows: count(item.rows),
          columns: count(item.columns),
          viewable: item.viewable === true && TABLE_RE.test(item.name),
        })),
    };
  }

  /** `schema`'s answer, checked. */
  function cleanSchema(answer) {
    return {
      table: text(answer.table, 64),
      type: answer.type === "view" ? "view" : "table",
      columns: list(answer.columns)
        .filter((column) => isRecord(column))
        .slice(0, 200)
        .map((column) => ({
          name: text(column.name, 200),
          type: text(column.type, 60),
          notNull: column.notNull === true,
          primaryKey: column.primaryKey === true,
          default: typeof column.default === "string" ? column.default.slice(0, 200) : null,
        })),
      columnsTruncated: answer.columnsTruncated === true,
      indexes: list(answer.indexes)
        .filter((index) => isRecord(index) && typeof index.name === "string")
        .slice(0, 50)
        .map((index) => ({ name: text(index.name, 200), unique: index.unique === true })),
    };
  }

  /** One cell of `rows`, checked: `{ type, value?, length?, cut? }`. */
  function cleanCell(cell) {
    if (!isRecord(cell)) return { type: "null" };
    if (cell.type === "blob") return { type: "blob", length: count(cell.length) };
    if (cell.type === "text") return { type: "text", value: text(cell.value, FULL_CHARS), length: count(cell.length), cut: cell.cut === true };
    if (cell.type === "integer" || cell.type === "real") return { type: cell.type, value: text(cell.value, 400) };
    return { type: "null" };
  }

  /** `rows`' answer, checked. */
  function cleanRows(answer) {
    return {
      columns: list(answer.columns)
        .filter((column) => isRecord(column))
        .slice(0, 200)
        .map((column) => ({ name: text(column.name, 200), type: text(column.type, 60), primaryKey: column.primaryKey === true })),
      rows: list(answer.rows)
        .filter(Array.isArray)
        .slice(0, 200)
        .map((row) => row.slice(0, 200).map(cleanCell)),
      offset: count(answer.offset),
      total: count(answer.total),
      sort: count(answer.sort),
      direction: answer.direction === "desc" ? "desc" : "asc",
      capped: answer.capped === true,
    };
  }

  /** The Claude Code agents that can be sent text now. */
  function candidates(panes) {
    return (Array.isArray(panes) ? panes : []).filter((pane) => pane && pane.agent === "claude" && pane.canSend === true && typeof pane.name === "string");
  }

  /** The agent a hand-over goes to by default: the orchestrator when there is one, else the first reachable. */
  function defaultTarget(panes) {
    const reachable = candidates(panes);
    return (reachable.find((pane) => pane.role === "orchestrator") || reachable[0] || {}).name || null;
  }

  /**
   * The text sent to an agent: the database's path, the table, its columns and the person's
   * question, and nothing read from a row. English whatever the page's language: it is for an agent.
   * The question is the person's own words, so it is not put between markers.
   */
  function briefFor({ path, table, type, total, columns, question }) {
    const cols = columns.map((column) => `${column.name}${column.type ? ` ${column.type}` : ""}${column.primaryKey ? " (primary key)" : ""}`);
    return [
      "The person is looking at a database of this project in Hangar's Database kit and handed you a table with a question. This message comes from the kit, on the person's click. No row of the table is in it: its values may be secrets.",
      "",
      `Database file: ${path}`,
      `${type === "view" ? "View" : "Table"}: ${table} (${total} ${total === 1 ? "row" : "rows"})`,
      `Columns: ${cols.join(", ") || "none"}`,
      "",
      `Question: ${question}`,
      "",
      "To look at the data yourself, use the kit's read-only script, whose commands are listed in its skill, or the project's own tools. Do not copy values you read into messages, files or commits unless the person asks you to.",
    ].join("\n");
  }

  return {
    STATE_FILE, PAGE_SIZE, CELL_CHARS, FULL_CHARS, PATH_RE, TABLE_RE, STRINGS, pickLang, t, parseAnswer, cleanFiles, cleanTables,
    cleanSchema, cleanRows, candidates, defaultTarget, briefFor,
  };
});
