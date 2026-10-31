"use strict";
// The tab's pure logic: the hook's log, turned into what the page shows, and the
// one-time pass a click writes. No DOM, no Hangar: the page and the tests both load
// this file (a plain script in the page, where it sets `GuardLogic`; `require` in Node).
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.GuardLogic = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const PASS_TTL_MS = 2 * 60 * 1000;
  const LOG = "refusals.jsonl";
  const ERRORS = "errors.jsonl";
  const STATE = "state.json";

  const STRINGS = {
    en: {
      tab: "Guardrails",
      heading: "Refused calls",
      loading: "Loading…",
      empty: "Nothing refused yet.",
      clear: "Clear the list",
      clearFailed: "The list could not be cleared. Try again in a moment.",
      allow: "Allow once",
      allowing: "Writing the pass…",
      passActive: "Allowed once, {remaining} left. Tell {agent} to retry this exact command.",
      passExpired: "The pass expired unused. Refused again if retried.",
      passUsed: "Let through once, {when}.",
      "problem.denied": "This tab may not write passes: accept data.write in the Kits menu.",
      "problem.rate_limited": "Too many writes in a minute. Try again in a moment.",
      "problem.failed": "The pass could not be written: {detail}",
      refusedTimes: "refused {count} times",
      truncated: "Cut for display: {length} characters in all. The pass is for the whole command.",
      staleWarning: "The last read failed; trying again…",
      errorsHeading: "The guard failed by itself",
      errorsBody: "{count} call(s) went through unchecked, or with the kit's own rules, because of this. Latest:",
      limitsHeading: "What this does not protect against",
      limits:
        "It reads the command line, and nothing else. It is no sandbox: a script can do anything once started, and a program the rules do not list can read any file.",
      "denied.title": "This tab may not read the kit’s data",
      "denied.hint": "Enable the kit on this workspace and accept its permissions (Kits menu). The hook refuses either way, but nothing can be let through from here until then.",
      "error.title": "The refusals could not be read",
      retry: "Try again",
      now: "just now",
      ago: "{n} ago",
      "why.recursive-delete/outside": "It deletes {subject}, outside the project.",
    },
    fr: {
      tab: "Garde-fous",
      heading: "Appels refusés",
      loading: "Chargement…",
      empty: "Rien de refusé pour l’instant.",
      clear: "Vider la liste",
      clearFailed: "La liste n’a pas pu être vidée. Réessayez dans un instant.",
      allow: "Autoriser une fois",
      allowing: "Écriture du laissez-passer…",
      passActive: "Autorisé une fois, reste {remaining}. Dites à {agent} de réessayer cette commande exacte.",
      passExpired: "Le laissez-passer a expiré sans servir. Nouveau refus en cas de nouvel essai.",
      passUsed: "Laissé passer une fois, {when}.",
      "problem.denied": "Cet onglet n’a pas le droit d’écrire de laissez-passer : acceptez data.write dans le menu Kits.",
      "problem.rate_limited": "Trop d’écritures en une minute. Réessayez dans un instant.",
      "problem.failed": "Le laissez-passer n’a pas pu être écrit : {detail}",
      refusedTimes: "refusé {count} fois",
      truncated: "Coupé à l’affichage : {length} caractères en tout. Le laissez-passer vaut pour la commande entière.",
      staleWarning: "La dernière lecture a échoué ; nouvel essai…",
      errorsHeading: "Le garde-fou a échoué de lui-même",
      errorsBody: "{count} appel(s) sont passés sans contrôle, ou avec les seules règles du kit, à cause de cela. Le dernier :",
      limitsHeading: "Ce dont il ne protège pas",
      limits:
        "Il lit la ligne de commande, et rien d’autre. Ce n’est pas un bac à sable : un script peut tout faire une fois lancé, et un programme que les règles ne citent pas peut lire n’importe quel fichier.",
      "denied.title": "Cet onglet n’a pas le droit de lire les données du kit",
      "denied.hint": "Activez le kit sur cet espace de travail et acceptez ses permissions (menu Kits). Le hook refuse quand même, mais rien ne peut être autorisé d’ici tant que ce n’est pas fait.",
      "error.title": "Les refus n’ont pas pu être lus",
      retry: "Réessayer",
      now: "à l’instant",
      ago: "il y a {n}",
      "why.recursive-delete/outside": "Il supprime {subject}, hors du projet.",
      "why.recursive-delete/project": "Il supprime tout le projet ({subject}).",
      "why.recursive-delete/protected": "Il supprime {subject}, l’historique git du projet.",
      "why.recursive-delete/unreadable": "Il supprime {subject}, et la ligne de commande ne dit pas où cela mène.",
      "why.force-push/default": "Il force un push sur {subject}, une branche par défaut : cela réécrit son historique pour tout le monde.",
      "why.git-reset-hard/any": "git reset --hard jette le travail non commité, que git ne peut pas rendre.",
      "why.git-clean/any": "git clean --force supprime les fichiers non suivis, que git ne peut pas rendre.",
      "why.drop-database/any": "Il supprime une base de données entière.",
      "why.download-to-shell/any": "Il exécute du code tout droit sorti d’un téléchargement, sans que personne ne l’ait lu.",
      "why.secret-file/read": "Il lit ou affiche {subject}, un fichier secret.",
      "why.secret-file/copy": "Il copie {subject}, un fichier secret, là où il n’est plus traité comme tel.",
      "why.secret-file/write": "Il écrase {subject}, un fichier secret.",
      "why.guard-files/any": "Il modifie {subject}, un fichier du garde-fou lui-même, que seule la personne modifie.",
    },
  };

  const pickLang = (language) => (/^fr\b/i.test(String(language || "")) ? "fr" : "en");

  function t(lang, key, vars) {
    const table = STRINGS[lang] || STRINGS.en;
    let text = table[key] !== undefined ? table[key] : STRINGS.en[key] !== undefined ? STRINGS.en[key] : key;
    for (const [name, value] of Object.entries(vars || {})) text = text.split(`{${name}}`).join(String(value));
    return text;
  }

  /** The reason, in the page's language. English comes as the hook told the agent. */
  function whyFor(row, lang) {
    const key = `why.${row.rule}/${row.kind}`;
    if (lang !== "en" && STRINGS[lang] && STRINGS[lang][key]) return t(lang, key, { subject: row.subject });
    const text = String(row.why || "");
    return text ? `${text[0].toUpperCase()}${text.slice(1)}.` : "";
  }

  const isKey = (key) => typeof key === "string" && /^[0-9a-f]{64}$/.test(key);

  /** One JSON object a line. A line cut short by a crash is passed over. */
  function parseLines(text) {
    const events = [];
    for (const line of String(text || "").split("\n")) {
      if (!line.trim()) continue;
      try {
        const event = JSON.parse(line);
        if (event && typeof event === "object" && Number.isFinite(event.at)) events.push(event);
      } catch {
        /* passed over */
      }
    }
    return events;
  }

  const str = (value) => (typeof value === "string" ? value : "");

  /**
   * What to list, newest first: one row per agent and command, however many times
   * it was refused, until a pass is used for it. What follows starts a new row.
   */
  function buildRows(events, clearedAt = 0) {
    const open = new Map();
    const rows = [];
    for (const event of events) {
      if (event.at <= clearedAt || !isKey(event.key)) continue;
      if (event.type === "refusal") {
        const row = open.get(event.key);
        if (row) {
          row.count++;
          row.at = Math.max(row.at, event.at);
          continue;
        }
        const fresh = {
          key: event.key,
          agent: str(event.agent) || "?",
          agentId: str(event.agentId),
          tool: str(event.tool),
          command: str(event.command),
          length: Number.isFinite(event.length) ? event.length : str(event.command).length,
          rule: str(event.rule),
          kind: str(event.kind),
          subject: str(event.subject),
          why: str(event.why),
          folder: str(event.folder),
          at: event.at,
          count: 1,
          usedAt: null,
        };
        open.set(event.key, fresh);
        rows.push(fresh);
      } else if (event.type === "pass-used") {
        const row = open.get(event.key);
        if (row) {
          row.usedAt = event.at;
          open.delete(event.key);
        }
      }
    }
    return rows.sort((a, b) => b.at - a.at);
  }

  const passPath = (key) => `passes/${key}.json`;

  /** The file « Allow once » writes: this agent, this exact command, two minutes. */
  function passFor(row, now) {
    return {
      key: row.key,
      agentId: row.agentId,
      agent: row.agent,
      tool: row.tool,
      command: row.command,
      createdAt: now,
      expiresAt: now + PASS_TTL_MS,
    };
  }

  /** A pass read back from its file, while it can still be used. */
  function grantFromPass(text, key, now) {
    try {
      const pass = JSON.parse(text);
      if (pass && pass.key === key && Number.isFinite(pass.expiresAt) && pass.expiresAt > now) {
        return { expiresAt: pass.expiresAt, grantedAt: Number.isFinite(pass.createdAt) ? pass.createdAt : now };
      }
    } catch {
      /* no use */
    }
    return null;
  }

  /** Where a row stands: none | active | expired | used. */
  function passState(row, grant, now) {
    if (row.usedAt !== null) return { state: "used", at: row.usedAt };
    // Refused since the click: the hook took that pass already, or it was never good.
    if (!grant || grant.grantedAt < row.at) return { state: "none" };
    return grant.expiresAt > now ? { state: "active", remainingMs: grant.expiresAt - now } : { state: "expired" };
  }

  /** The tab's badge: what was refused and not let through. */
  function badgeFor(rows) {
    const count = rows.filter((row) => row.usedAt === null).length;
    return count ? { state: "failure", text: String(count) } : null;
  }

  function formatRemaining(ms) {
    const seconds = Math.max(0, Math.ceil(ms / 1000));
    return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  }

  function relativeTime(at, now, lang) {
    const seconds = Math.floor((now - at) / 1000);
    if (seconds < 30) return t(lang, "now");
    const minutes = Math.floor(seconds / 60);
    if (minutes < 1) return t(lang, "ago", { n: `${seconds} s` });
    if (minutes < 60) return t(lang, "ago", { n: `${minutes} min` });
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return t(lang, "ago", { n: `${hours} h` });
    return t(lang, "ago", { n: lang === "fr" ? `${Math.floor(hours / 24)} j` : `${Math.floor(hours / 24)} d` });
  }

  const folderName = (folder) => String(folder || "").replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "";

  /** The guard's own failures: how many since the list was cleared, and the latest. */
  function errorSummary(events, clearedAt = 0) {
    const kept = events.filter((event) => event.at > clearedAt && typeof event.message === "string");
    return kept.length ? { count: kept.length, last: kept[kept.length - 1].message } : null;
  }

  return {
    PASS_TTL_MS, LOG, ERRORS, STATE, STRINGS,
    pickLang, t, whyFor, isKey, parseLines, buildRows, passPath, passFor, grantFromPass, passState, badgeFor,
    formatRemaining, relativeTime, folderName, errorSummary,
  };
});
