"use strict";
// The tab's pure logic: what `gh` answered, turned into what the page shows.
// No DOM, no Hangar: the page and the tests both load this file (a plain script
// in the page, where it sets `CiLogic`; `require` in Node).
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.CiLogic = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const STRINGS = {
    en: {
      tab: "CI",
      loading: "Loading…",
      runs: "Recent runs",
      refresh: "Refresh",
      back: "← Runs",
      retry: "Try again",
      noRuns: "No run found for this repository.",
      run: "Run #{id}",
      branch: "Branch",
      elapsed: "Elapsed",
      jobs: "Jobs",
      noJobs: "No job yet.",
      steps: "{done}/{total} steps",
      queued: "Queued",
      running: "Running",
      success: "Passed",
      failure: "Failed",
      neutral: "Ended",
      badgeRunning: "running",
      badgeQueued: "queued",
      badgeSuccess: "passed",
      badgeFailure: "failed",
      badgeNeutral: "ended",
      notifyTitle: "{name}: {state}",
      failedLog: "Log of the failed steps",
      logTail: "last {shown} of {total} lines",
      logEmpty: "The log is empty.",
      logError: "The log could not be read ({reason}).",
      warning: "Last refresh failed; trying again…",
      now: "just now",
      ago: "{n} ago",
      "err.no-gh.title": "GitHub CLI (gh) not found",
      "err.no-gh.hint": "Install it from https://cli.github.com, then sign in with: gh auth login",
      "err.no-auth.title": "gh is not signed in",
      "err.no-auth.hint": "Run this in a terminal, then try again: gh auth login",
      "err.no-repo.title": "No GitHub repository here",
      "err.no-repo.hint": "Open this tab in a workspace whose folder is a git clone with a GitHub remote.",
      "err.run-not-found.title": "Run not found",
      "err.run-not-found.hint": "That run does not exist in this repository, or gh cannot see it.",
      "err.denied.title": "This tab may not run gh",
      "err.denied.hint": "Enable the kit on this workspace and accept its commands (Kits menu).",
      "err.unavailable.title": "Hangar cannot run gh now",
      "err.unavailable.hint": "The kit cannot be used at the moment. Reload the tab in a moment.",
      "err.timeout.title": "gh took too long",
      "err.timeout.hint": "GitHub did not answer in time. Try again.",
      "err.error.title": "gh failed",
      "err.error.hint": "gh said:",
    },
    fr: {
      tab: "CI",
      loading: "Chargement…",
      runs: "Exécutions récentes",
      refresh: "Actualiser",
      back: "← Exécutions",
      retry: "Réessayer",
      noRuns: "Aucune exécution trouvée pour ce dépôt.",
      run: "Exécution n° {id}",
      branch: "Branche",
      elapsed: "Durée",
      jobs: "Jobs",
      noJobs: "Pas encore de job.",
      steps: "{done}/{total} étapes",
      queued: "En attente",
      running: "En cours",
      success: "Réussie",
      failure: "Échouée",
      neutral: "Terminée",
      badgeRunning: "en cours",
      badgeQueued: "en attente",
      badgeSuccess: "réussie",
      badgeFailure: "échec",
      badgeNeutral: "terminée",
      notifyTitle: "{name} : {state}",
      failedLog: "Journal des étapes en échec",
      logTail: "{shown} dernières lignes sur {total}",
      logEmpty: "Le journal est vide.",
      logError: "Le journal est illisible ({reason}).",
      warning: "La dernière actualisation a échoué ; nouvel essai…",
      now: "à l’instant",
      ago: "il y a {n}",
      "err.no-gh.title": "GitHub CLI (gh) introuvable",
      "err.no-gh.hint": "Installez-le depuis https://cli.github.com, puis connectez-vous avec : gh auth login",
      "err.no-auth.title": "gh n’est pas connecté",
      "err.no-auth.hint": "Lancez ceci dans un terminal, puis réessayez : gh auth login",
      "err.no-repo.title": "Aucun dépôt GitHub ici",
      "err.no-repo.hint": "Ouvrez cet onglet dans un espace de travail dont le dossier est un clone git avec un remote GitHub.",
      "err.run-not-found.title": "Exécution introuvable",
      "err.run-not-found.hint": "Cette exécution n’existe pas dans ce dépôt, ou gh ne la voit pas.",
      "err.denied.title": "Cet onglet n’a pas le droit de lancer gh",
      "err.denied.hint": "Activez le kit sur cet espace de travail et acceptez ses commandes (menu Kits).",
      "err.unavailable.title": "Hangar ne peut pas lancer gh maintenant",
      "err.unavailable.hint": "Le kit est inutilisable pour le moment. Rechargez l’onglet dans un instant.",
      "err.timeout.title": "gh a mis trop de temps",
      "err.timeout.hint": "GitHub n’a pas répondu à temps. Réessayez.",
      "err.error.title": "gh a échoué",
      "err.error.hint": "gh a répondu :",
    },
  };

  function pickLang(language) {
    return /^fr\b/i.test(String(language || "")) ? "fr" : "en";
  }

  function t(lang, key, vars) {
    const table = STRINGS[lang] || STRINGS.en;
    const text = table[key] !== undefined ? table[key] : STRINGS.en[key] !== undefined ? STRINGS.en[key] : key;
    return text.replace(/\{(\w+)\}/g, (whole, name) => (vars && vars[name] !== undefined ? String(vars[name]) : whole));
  }

  const ID = /^[0-9]+$/;
  const isRunId = (value) => typeof value === "string" && ID.test(value);

  // eslint-disable-next-line no-control-regex
  const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]/g;
  const stripAnsi = (text) => String(text).replace(ANSI, "");

  // Control, zero-width and bidi characters: what tab.setTitle would refuse.
  const INVISIBLE = new RegExp("[\u0000-\u001f\u007f-\u009f\u200b-\u200f\u2028-\u202f\u2060-\u206f\ufeff]", "g");

  /** One line of plain text, at most `max` characters (an ellipsis counts), as tab.setTitle wants. */
  function oneLine(text, max) {
    const clean = String(text == null ? "" : text).replace(INVISIBLE, " ").replace(/\s+/g, " ").trim();
    const chars = Array.from(clean);
    return chars.length <= max ? clean : chars.slice(0, max - 1).join("") + "…";
  }

  /** What a conclusion comes to: success, failure, or neutral (cancelled, skipped…). */
  function outcomeOf(conclusion) {
    if (conclusion === "success") return "success";
    if (conclusion === "failure" || conclusion === "timed_out" || conclusion === "startup_failure") return "failure";
    return "neutral";
  }

  /** queued | running | success | failure | neutral — for a run, a job or a step. */
  function kindOf(item) {
    const status = item && item.status;
    if (status === "completed") return outcomeOf(item.conclusion);
    if (status === "in_progress") return "running";
    return "queued"; // queued, waiting, requested, pending, or unknown
  }

  const isDone = (item) => Boolean(item) && item.status === "completed";

  function badgeFor(run, lang) {
    const kind = kindOf(run);
    if (kind === "running") return { state: "running", text: t(lang, "badgeRunning") };
    if (kind === "queued") return { state: "running", text: t(lang, "badgeQueued") };
    if (kind === "success") return { state: "success", text: t(lang, "badgeSuccess") };
    if (kind === "failure") return { state: "failure", text: t(lang, "badgeFailure") };
    return { state: "neutral", text: t(lang, "badgeNeutral") };
  }

  function time(value) {
    const ms = Date.parse(value);
    // GitHub writes 0001-01-01 for a start that has not happened.
    return Number.isFinite(ms) && ms > 86400000 ? ms : null;
  }

  /** The progress of a job: its steps done, and the step running now. */
  function jobView(job) {
    const steps = Array.isArray(job && job.steps) ? job.steps : [];
    const current = steps.find((step) => step && step.status === "in_progress");
    return {
      name: oneLine(job && job.name, 80),
      kind: kindOf(job),
      done: steps.filter((step) => step && step.status === "completed").length,
      total: steps.length,
      step: current ? oneLine(current.name, 80) : null,
      startedAt: time(job && job.startedAt),
      completedAt: time(job && job.completedAt),
    };
  }

  /** Milliseconds since the run began (until it ended, once it has), or null. */
  function elapsedMs(run, now) {
    const jobs = Array.isArray(run && run.jobs) ? run.jobs : [];
    const starts = [time(run && run.startedAt), ...jobs.map((job) => time(job && job.startedAt))].filter((v) => v !== null);
    if (!starts.length) return null;
    const start = Math.min(...starts);
    let end = now;
    if (isDone(run)) {
      const ends = [time(run.updatedAt), ...jobs.map((job) => time(job && job.completedAt))].filter((v) => v !== null);
      end = ends.length ? Math.max(...ends) : now;
    }
    return Math.max(0, end - start);
  }

  function formatDuration(ms) {
    if (ms === null || !Number.isFinite(ms)) return "–";
    const total = Math.floor(ms / 1000);
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const s = total % 60;
    if (h) return `${h}h ${String(m).padStart(2, "0")}m`;
    if (m) return `${m}m ${String(s).padStart(2, "0")}s`;
    return `${s}s`;
  }

  /** Everything the page shows of one run. */
  function runView(run, now) {
    const jobs = (Array.isArray(run && run.jobs) ? run.jobs : []).map(jobView);
    return {
      kind: kindOf(run),
      done: isDone(run),
      name: oneLine((run && run.workflowName) || (run && run.displayTitle), 40),
      title: oneLine(run && run.displayTitle, 200),
      branch: oneLine(run && run.headBranch, 80),
      url: typeof (run && run.url) === "string" ? run.url : "",
      elapsed: elapsedMs(run, now),
      jobs,
      failedJobs: jobs.filter((job) => job.kind === "failure"),
    };
  }

  /** Sorts what went wrong into something the page can explain.
   *  `fault` is { code?, exitCode?, stderr?, timedOut? }: a rejected call or a command's answer. */
  function classify(fault) {
    const { code, exitCode, timedOut } = fault || {};
    const stderr = stripAnsi((fault && fault.stderr) || "");
    if (code === "not_found") return "no-gh";
    if (code === "denied") return "denied";
    if (code === "unavailable") return "unavailable";
    if (code === "timeout" || code === "busy" || timedOut || (code === undefined && exitCode === null)) return "timeout";
    if (/gh auth login|not logged in|GH_TOKEN|authentication|bad credentials|HTTP 401/i.test(stderr)) return "no-auth";
    if (/no git remotes|not a git repository|none of the git remotes|unable to determine base repo|could not determine base repo/i.test(stderr)) return "no-repo";
    if (/could not resolve to a run|could not find any workflow run|HTTP 404|not found/i.test(stderr)) return "run-not-found";
    return "error";
  }

  /** The failures that no waiting will cure. */
  const isFatal = (kind) => kind === "no-gh" || kind === "no-auth" || kind === "no-repo" || kind === "denied" || kind === "unavailable" || kind === "run-not-found";

  /** The log of failed steps (`gh run view --log-failed`) cut to its end, readable:
   *  `job<TAB>step<TAB>timestamp text` lines become text, with a heading when the step changes. */
  function trimLog(raw, maxLines) {
    const limit = maxLines || 40;
    const entries = stripAnsi(raw)
      .replace(/^﻿/, "")
      .split(/\r?\n/)
      .map((line) => {
        const parts = line.split("\t");
        if (parts.length >= 3) {
          const text = parts.slice(2).join("\t").replace(/^﻿?\d{4}-\d\d-\d\dT[\d:.]+Z ?/, "");
          return { job: parts[0].trim(), step: parts[1].trim(), text };
        }
        return { job: "", step: "", text: line.replace(/^\d{4}-\d\d-\d\dT[\d:.]+Z ?/, "") };
      })
      .filter((entry) => entry.text.trim() !== "" || entry.job);
    const kept = entries.slice(-limit);
    const lines = [];
    let heading = null;
    for (const entry of kept) {
      const label = entry.job || entry.step ? `${entry.job} › ${entry.step}`.slice(0, 160) : "";
      if (label && label !== heading) {
        lines.push({ kind: "head", text: label });
        heading = label;
      }
      const text = entry.text.length > 400 ? entry.text.slice(0, 399) + "…" : entry.text;
      lines.push({ kind: /##\[error\]|(^|\s)error[:\s]/i.test(text) ? "error" : "text", text: text.replace(/^##\[error\]/, "") });
    }
    return { lines, total: entries.length, shown: kept.length };
  }

  /** `gh run list --json …` → the picker's rows. */
  function runList(list) {
    if (!Array.isArray(list)) return [];
    return list
      .filter((run) => run && Number.isInteger(run.databaseId))
      .map((run) => ({
        id: String(run.databaseId),
        name: oneLine(run.workflowName || run.displayTitle, 60),
        title: oneLine(run.displayTitle, 120),
        branch: oneLine(run.headBranch, 60),
        kind: kindOf(run),
        createdAt: time(run.createdAt),
      }));
  }

  function relativeTime(then, now, lang) {
    if (then === null) return "";
    const seconds = Math.max(0, Math.floor((now - then) / 1000));
    if (seconds < 45) return t(lang, "now");
    const minutes = Math.round(seconds / 60);
    if (minutes < 60) return t(lang, "ago", { n: `${minutes} min` });
    const hours = Math.round(minutes / 60);
    if (hours < 24) return t(lang, "ago", { n: `${hours} h` });
    return t(lang, "ago", { n: `${Math.round(hours / 24)} ${lang === "fr" ? "j" : "d"}` });
  }

  return {
    STRINGS, pickLang, t, isRunId, stripAnsi, oneLine, outcomeOf, kindOf, isDone, badgeFor, jobView,
    elapsedMs, formatDuration, runView, classify, isFatal, trimLog, runList, relativeTime,
  };
});
