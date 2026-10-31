"use strict";
// What is neither the behaviour nor the drawing: the tab's words in en and fr, the
// params read like form input, the list file checked, the answers kept, the badge and
// the summary an agent receives. No DOM, no Hangar: the page and the tests both load it.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.ChecklistLogic = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const MAX_STEPS = 200;
  const MAX_TEXT = 1000;
  const MAX_NOTE = 2000;
  const SUMMARY_LIMIT = 19000; // panes.send takes 20 000 characters
  const TITLE_LIMIT = 40; // tab.setTitle

  const STRINGS = {
    en: {
      tab: "Recette",
      loading: "Reading the list…",
      retry: "Try again",
      untitled: "Checklist",
      steps: "Steps",
      ok: "OK",
      problem: "Problem",
      skip: "Skip",
      problems: "Problems",
      skipped: "Skipped",
      expectedLine: "Expected: {text}",
      todo: "To do",
      noteLabel: "What went wrong",
      notePlaceholder: "Describe what you saw instead",
      redo: "To redo",
      redoWas: "It failed last time: {note}",
      redoWasBare: "It failed last time. Test it again.",
      stateRunning: "{done} of {total} answered",
      stateFailure_one: "{n} step failed",
      stateFailure_other: "{n} steps failed",
      stateSuccess: "Everything passed",
      stateSuccessSkipped: "Nothing failed",
      badgeFailure: "{n} failed",
      send: "Send to agent",
      sendAgain: "Send again",
      sending: "Sending…",
      sentTo: "Sent to {pane}. It fixes what failed: open this list again to test it.",
      needAnswer: "Answer at least one step to send the result.",
      pending_one: "{n} step is still unanswered: the agent will be told.",
      pending_other: "{n} steps are still unanswered: the agent will be told.",
      noCaller: "No agent asked for this list. Copy the summary to give it to one.",
      copy: "Copy the summary",
      copied: "Copied",
      notifySent: "Result sent to {pane}",
      persistOff: "Answers cannot be saved here: closing this tab loses them.",
      "send.denied": "This tab may not send to agents. Allow panes.send for the kit in Settings › Kits.",
      "send.not_found": "The agent that asked is no longer in this workspace. Copy the summary instead.",
      "send.unavailable": "That agent's session cannot receive messages right now. Try again in a moment.",
      "send.rate_limited": "Too many messages sent. Wait a minute, then send again.",
      "send.error": "The result could not be sent: {detail}",
      "err.missing.title": "No list to show",
      "err.missing.text": "Open this tab with the path of a JSON file, for example recette/login.json.",
      "err.path.title": "This path cannot be used",
      "err.path.text": "It must be relative to the workspace folder, with no .., no drive, no colon and no hidden folder.",
      "err.not_found.title": "List not found",
      "err.not_found.text": "{path} does not exist in the workspace folder. The agent writes the file before it opens this tab.",
      "err.denied.title": "This tab may not read files",
      "err.denied.text": "Allow files.read for the kit in Settings › Kits, then try again.",
      "err.json.title": "The list is not valid JSON",
      "err.json.text": "{detail}",
      "err.empty.title": "The list is empty",
      "err.empty.text": "{path} holds nothing. The agent has to write the list in it.",
      "err.shape.title": "The list is not in the expected shape",
      "err.shape.text": "{detail}",
      "err.error.title": "The list could not be read",
      "err.error.text": "{detail}",
      "shape.notObject": "The file must hold an object with a title and a list of steps.",
      "shape.noSteps": "steps must be a list with at least one step.",
      "shape.tooMany": "At most {max} steps; this list has {n}.",
      "shape.stepNotObject": "Step {n} must be an object with a text.",
      "shape.stepText": "Step {n} has no text.",
      "shape.stepExpect": "Step {n}: expect must be a text.",
      "shape.stepLong": "Step {n}: the text is longer than {max} characters.",
      "shape.titleType": "title must be a text.",
    },
    fr: {
      tab: "Recette",
      loading: "Lecture de la liste…",
      retry: "Réessayer",
      untitled: "Liste de vérifications",
      steps: "Étapes",
      ok: "OK",
      problem: "Problème",
      skip: "Passer",
      problems: "Problèmes",
      skipped: "Passées",
      expectedLine: "Attendu : {text}",
      todo: "À faire",
      noteLabel: "Ce qui ne va pas",
      notePlaceholder: "Décrivez ce que vous avez vu à la place",
      redo: "À refaire",
      redoWas: "Échec la dernière fois : {note}",
      redoWasBare: "Échec la dernière fois. Testez à nouveau.",
      stateRunning: "{done} sur {total} traitées",
      stateFailure_one: "{n} étape en échec",
      stateFailure_other: "{n} étapes en échec",
      stateSuccess: "Tout est validé",
      stateSuccessSkipped: "Aucun échec",
      badgeFailure: "{n} échec(s)",
      send: "Envoyer à l'agent",
      sendAgain: "Renvoyer",
      sending: "Envoi…",
      sentTo: "Envoyé à {pane}. Il corrige ce qui a échoué : rouvrez cette liste pour la retester.",
      needAnswer: "Répondez à au moins une étape pour envoyer le résultat.",
      pending_one: "{n} étape n'a pas de réponse : l'agent en sera informé.",
      pending_other: "{n} étapes n'ont pas de réponse : l'agent en sera informé.",
      noCaller: "Aucun agent n'a demandé cette liste. Copiez le résumé pour le donner à l'un d'eux.",
      copy: "Copier le résumé",
      copied: "Copié",
      notifySent: "Résultat envoyé à {pane}",
      persistOff: "Les réponses ne peuvent pas être enregistrées ici : fermer cet onglet les perd.",
      "send.denied": "Cet onglet ne peut pas écrire aux agents. Autorisez panes.send pour le kit dans Réglages › Kits.",
      "send.not_found": "L'agent qui l'a demandée n'est plus dans cet espace de travail. Copiez plutôt le résumé.",
      "send.unavailable": "La session de cet agent ne peut pas recevoir de message pour l'instant. Réessayez dans un instant.",
      "send.rate_limited": "Trop de messages envoyés. Attendez une minute, puis renvoyez.",
      "send.error": "Le résultat n'a pas pu être envoyé : {detail}",
      "err.missing.title": "Aucune liste à afficher",
      "err.missing.text": "Ouvrez cet onglet avec le chemin d'un fichier JSON, par exemple recette/login.json.",
      "err.path.title": "Ce chemin ne peut pas servir",
      "err.path.text": "Il doit être relatif au dossier de l'espace de travail, sans .., sans lecteur, sans deux-points et sans dossier caché.",
      "err.not_found.title": "Liste introuvable",
      "err.not_found.text": "{path} n'existe pas dans le dossier de l'espace de travail. L'agent écrit le fichier avant d'ouvrir cet onglet.",
      "err.denied.title": "Cet onglet ne peut pas lire de fichier",
      "err.denied.text": "Autorisez files.read pour le kit dans Réglages › Kits, puis réessayez.",
      "err.json.title": "La liste n'est pas du JSON valide",
      "err.json.text": "{detail}",
      "err.empty.title": "La liste est vide",
      "err.empty.text": "{path} ne contient rien. L'agent doit y écrire la liste.",
      "err.shape.title": "La liste n'a pas la forme attendue",
      "err.shape.text": "{detail}",
      "err.error.title": "La liste n'a pas pu être lue",
      "err.error.text": "{detail}",
      "shape.notObject": "Le fichier doit contenir un objet avec un titre et une liste d'étapes.",
      "shape.noSteps": "steps doit être une liste d'au moins une étape.",
      "shape.tooMany": "{max} étapes au plus ; cette liste en a {n}.",
      "shape.stepNotObject": "L'étape {n} doit être un objet avec un text.",
      "shape.stepText": "L'étape {n} n'a pas de text.",
      "shape.stepExpect": "Étape {n} : expect doit être un texte.",
      "shape.stepLong": "Étape {n} : le texte dépasse {max} caractères.",
      "shape.titleType": "title doit être un texte.",
    },
  };

  const pickLang = (language) => (String(language || "").toLowerCase().startsWith("fr") ? "fr" : "en");

  /** The words outside a tab. In one, `hangar.ui.i18n(STRINGS)` gives the same, typeset. */
  function t(lang, key, vars) {
    const form = vars && typeof vars.n === "number" ? `${key}_${new Intl.PluralRules(lang).select(vars.n)}` : key;
    const text = (STRINGS[lang] && (STRINGS[lang][form] || STRINGS[lang][key])) || STRINGS.en[form] || STRINGS.en[key] || key;
    if (!vars) return text;
    return text.replace(/\{(\w+)\}/g, (whole, name) => (vars[name] === undefined ? whole : String(vars[name])));
  }

  /**
   * The `path` param, read like form input: relative, inside the workspace folder, not
   * hidden (files.read refuses those), one line. → { ok: true, path } or { ok: false, kind }.
   */
  function readPath(params) {
    const value = params && params.path;
    if (value === undefined || value === null || String(value).trim() === "") return { ok: false, kind: "missing" };
    if (typeof value !== "string") return { ok: false, kind: "path" };
    const path = value.trim().replace(/\\/g, "/");
    const segments = path.split("/");
    const bad =
      path.length > 1024 ||
      /[:\r\n\0]/.test(path) ||
      path.startsWith("/") ||
      segments.some((segment) => segment === "" || segment === ".." || segment === "." || segment.startsWith("."));
    return bad ? { ok: false, kind: "path" } : { ok: true, path };
  }

  const baseName = (path) => path.slice(path.lastIndexOf("/") + 1);

  /**
   * The list, parsed and checked. → { ok: true, list: { title, steps: [{ text, expect }] } }
   * or { ok: false, kind: "empty" | "json" | "shape", detail?, code?, vars? }.
   */
  function parseList(text, path) {
    const source = String(text === undefined || text === null ? "" : text).replace(/^﻿/, "");
    if (!source.trim()) return { ok: false, kind: "empty" };
    let data;
    try {
      data = JSON.parse(source);
    } catch (error) {
      return { ok: false, kind: "json", detail: String((error && error.message) || error).slice(0, 300) };
    }
    const shape = (code, vars) => ({ ok: false, kind: "shape", code, vars });
    if (!data || typeof data !== "object" || Array.isArray(data)) return shape("notObject");
    if (data.title !== undefined && typeof data.title !== "string") return shape("titleType");
    if (!Array.isArray(data.steps) || data.steps.length === 0) return shape("noSteps");
    if (data.steps.length > MAX_STEPS) return shape("tooMany", { max: MAX_STEPS, n: data.steps.length });
    const steps = [];
    for (let index = 0; index < data.steps.length; index++) {
      const raw = data.steps[index];
      const n = index + 1;
      if (!raw || typeof raw !== "object" || Array.isArray(raw)) return shape("stepNotObject", { n });
      if (typeof raw.text !== "string" || !raw.text.trim()) return shape("stepText", { n });
      if (raw.expect !== undefined && raw.expect !== null && typeof raw.expect !== "string") return shape("stepExpect", { n });
      if (raw.text.length > MAX_TEXT || (raw.expect || "").length > MAX_TEXT) return shape("stepLong", { n, max: MAX_TEXT });
      steps.push({ text: raw.text.trim(), expect: typeof raw.expect === "string" && raw.expect.trim() ? raw.expect.trim() : "" });
    }
    const title = typeof data.title === "string" && data.title.trim() ? data.title.trim().slice(0, 200) : baseName(path || "") || "";
    return { ok: true, list: { title, steps } };
  }

  function fnv(text) {
    let hash = 0x811c9dc5;
    for (let i = 0; i < text.length; i++) {
      hash ^= text.charCodeAt(i);
      hash = Math.imul(hash, 0x01000193) >>> 0;
    }
    return hash.toString(16).padStart(8, "0");
  }

  /**
   * A step's identity: a hash of its text, and which of the identical texts it is. Answers
   * follow a step that the agent moved or reworded around, and only a changed text loses its own.
   */
  function stepKeys(steps) {
    const seen = new Map();
    return steps.map((step) => {
      const base = fnv(step.text);
      const count = seen.get(base) || 0;
      seen.set(base, count + 1);
      return `${base}-${count}`;
    });
  }

  /** Where the answers of the list at `path` live in the kit's data folder. */
  function dataPath(path) {
    const slug = path.replace(/[^A-Za-z0-9._-]+/g, "_").slice(-40);
    return `answers/${slug}-${fnv(path)}.json`;
  }

  const STATUSES = ["ok", "fail", "skip"];

  /** What the data folder held, made safe: only known keys, known statuses, bounded notes. */
  function readSaved(text, keys) {
    const saved = { answers: {}, redo: {}, sentAt: null };
    let data = null;
    try {
      data = JSON.parse(String(text || "").replace(/^﻿/, ""));
    } catch {
      return saved;
    }
    if (!data || typeof data !== "object") return saved;
    const known = new Set(keys);
    const note = (value) => (typeof value === "string" ? value.slice(0, MAX_NOTE) : "");
    for (const [key, answer] of Object.entries(data.answers && typeof data.answers === "object" ? data.answers : {})) {
      if (!known.has(key) || !answer || !STATUSES.includes(answer.status)) continue;
      saved.answers[key] = answer.status === "ok" ? { status: "ok" } : { status: answer.status, note: note(answer.note) };
    }
    for (const [key, value] of Object.entries(data.redo && typeof data.redo === "object" ? data.redo : {})) {
      if (known.has(key) && !saved.answers[key]) saved.redo[key] = note(value);
    }
    if (typeof data.sentAt === "number" && Number.isFinite(data.sentAt)) saved.sentAt = data.sentAt;
    return saved;
  }

  const writeSaved = (saved) => JSON.stringify({ v: 1, answers: saved.answers, redo: saved.redo, sentAt: saved.sentAt }, null, 1);

  /**
   * After the answers went to the agent, reopening the list is testing its fix: what
   * failed goes back to « to redo », its note kept to remind. → true when it changed anything.
   */
  function markFailedForRedo(saved) {
    if (saved.sentAt === null) return false;
    for (const [key, answer] of Object.entries(saved.answers)) {
      if (answer.status === "fail") {
        saved.redo[key] = answer.note || "";
        delete saved.answers[key];
      }
    }
    saved.sentAt = null;
    return true;
  }

  /** One step's status: ok | fail | skip | redo | todo. */
  function statusOf(saved, key) {
    if (saved.answers[key]) return saved.answers[key].status;
    return Object.prototype.hasOwnProperty.call(saved.redo, key) ? "redo" : "todo";
  }

  function progress(keys, saved) {
    const counts = { total: keys.length, ok: 0, fail: 0, skip: 0, redo: 0, todo: 0 };
    for (const key of keys) counts[statusOf(saved, key)]++;
    counts.answered = counts.ok + counts.fail + counts.skip;
    counts.pending = counts.redo + counts.todo;
    counts.done = counts.pending === 0;
    counts.state = !counts.done ? "running" : counts.fail > 0 ? "failure" : "success";
    return counts;
  }

  /** The tab's badge: running with a count, then success or failure. `text` is 12 characters at most. */
  function badgeFor(counts, translate) {
    if (counts.state === "running") return { state: "running", text: `${counts.answered}/${counts.total}` };
    if (counts.state === "failure") return { state: "failure", text: translate("badgeFailure", { n: counts.fail }).slice(0, 12) };
    return { state: "success", text: `${counts.answered}/${counts.total}` };
  }

  /** A name for the tab strip: one line, 40 characters at most. */
  const tabTitle = (list) => Array.from(list.title.replace(/\s+/g, " ")).slice(0, TITLE_LIMIT).join("");

  const clip = (text, max) => (text.length > max ? `${text.slice(0, max - 1)}…` : text);
  const oneLine = (text) => text.replace(/\s+/g, " ").trim();

  /**
   * What the agent receives: plain text, in English whatever the person reads, since an
   * agent handles it as it does any message. Failed steps with their notes first, then
   * skipped and unanswered ones, and what to do next.
   */
  function buildSummary(list, keys, saved, path) {
    const counts = progress(keys, saved);
    const lines = [`Checklist results: "${oneLine(list.title)}" (${path}).`];
    const parts = [`${counts.ok} OK`, `${counts.fail} failed`, `${counts.skip} skipped`];
    if (counts.pending) parts.push(`${counts.pending} not answered`);
    lines.push(`${list.steps.length} steps: ${parts.join(", ")}.`);

    const section = (title, status, detail) => {
      const rows = [];
      list.steps.forEach((step, index) => {
        if (statusOf(saved, keys[index]) !== status) return;
        const row = [`- Step ${index + 1}: ${clip(oneLine(step.text), 300)}`];
        if (step.expect && detail) row.push(`  Expected: ${clip(oneLine(step.expect), 300)}`);
        const note = status === "fail" ? (saved.answers[keys[index]] || {}).note : status === "redo" ? saved.redo[keys[index]] : "";
        if (status === "fail") row.push(`  Note: ${note ? clip(note.trim(), 1000) : "(none)"}`);
        else if (note) row.push(`  Failed before: ${clip(note.trim(), 300)}`);
        rows.push(row.join("\n"));
      });
      if (rows.length) lines.push("", `${title}:`, ...rows);
    };
    section("FAILED", "fail", true);
    section("SKIPPED", "skip", false);
    section("NOT ANSWERED", "todo", false);
    section("NOT ANSWERED (failed before, to redo)", "redo", false);

    const passed = [];
    keys.forEach((key, index) => {
      if (statusOf(saved, key) === "ok") passed.push(index + 1);
    });
    if (passed.length) lines.push("", `Passed: steps ${passed.join(", ")}.`);

    lines.push("");
    if (counts.fail) {
      lines.push(`Fix what failed, then ask for the same list again with kit_tab_open (path "${path}"): the failed steps come back to redo and you get a new summary.`);
    } else if (counts.pending) {
      lines.push("Nothing failed in what was answered.");
    } else {
      lines.push("Nothing failed.");
    }
    return clip(lines.join("\n"), SUMMARY_LIMIT);
  }

  /** What to say of a refused or failed call: the kind the page has words for. */
  function classify(error) {
    const code = error && error.code;
    if (code === "denied") return "denied";
    if (code === "not_found") return "not_found";
    return "error";
  }

  return {
    STRINGS, MAX_STEPS, TITLE_LIMIT, pickLang, t, readPath, baseName, parseList, fnv, stepKeys, dataPath,
    readSaved, writeSaved, markFailedForRedo, statusOf, progress, badgeFor, tabTitle, buildSummary, classify,
  };
});
