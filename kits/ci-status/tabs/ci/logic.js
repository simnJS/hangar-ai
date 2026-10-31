"use strict";
// The tab's pure logic: what `gh` answered, turned into what the page shows.
// No DOM, no Hangar: the page and the tests both load this file (a plain script
// in the page, where it sets `CiLogic`; `require` in Node). Saying a duration or
// an age is the library's (`hangar.ui.format`), and so is choosing the language.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.CiLogic = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  /** For `hangar.ui.i18n`, which sets their typography and picks the language. */
  const STRINGS = {
    en: {
      tab: "CI",
      runs: "Recent runs",
      refresh: "Refresh",
      back: "Back to the recent runs",
      retry: "Try again",
      copy: "Copy",
      copyLink: "Copy the link",
      loadingRuns: "Reading the runs…",
      loadingRun: "Reading the run…",
      "noRuns.title": "No run yet",
      "noRuns.hint": "A push to this repository starts one, and it shows here.",
      run: "Run #{id}",
      onBranch: "{name} on {branch}",
      jobs: "Jobs",
      failedJobs: "Failed jobs",
      otherJobs: "Other jobs",
      noJobs: "No job yet.",
      noJobsRan: "No job ran.",
      stepRunning: "{step}, step {at} of {total}",
      stepFailed: "Failed at the step {step}",
      queued: "Queued",
      running: "Running",
      success: "Passed",
      failure: "Failed",
      neutral: "Ended",
      cancelled: "Cancelled",
      skipped: "Skipped",
      "job.queued": "Queued",
      "job.running": "Running",
      "job.success": "Passed",
      "job.failure": "Failed",
      "job.neutral": "Ended",
      "job.cancelled": "Cancelled",
      "job.skipped": "Skipped",
      badgeRunning: "running",
      badgeQueued: "queued",
      badgeSuccess: "passed",
      badgeFailure: "failed",
      badgeNeutral: "ended",
      notifyTitle: "{name}: {state}",
      failedLog: "Log of the failed steps",
      logLoading: "Reading the log…",
      logCut: "Only the first megabyte of this log is shown.",
      "logEmpty.title": "The log is empty",
      "logEmpty.hint": "GitHub may still be storing it. Try again in a moment.",
      logError: "The log could not be read: {reason}.",
      warning: "The last refresh failed. Trying again…",
      "err.no-gh.title": "gh is not installed",
      "err.no-gh.hint": "This tab reads the CI with GitHub CLI. Install it, sign in with gh auth login, then try again.",
      "err.no-auth.title": "gh is not signed in",
      "err.no-auth.hint": "This tab reads the CI with your gh login. Run this in a terminal, then try again.",
      "err.no-repo.title": "No GitHub repository here",
      "err.no-repo.hint": "Open this tab in a workspace whose folder is a git clone with a GitHub remote.",
      "err.run-not-found.title": "Run not found",
      "err.run-not-found.hint": "That run does not exist in this repository, or gh cannot see it.",
      "err.denied.title": "This tab may not read the CI",
      "err.denied.hint": "Accept the commands of CI status in Settings › Kits, or in the workspace's Kits dialog, then try again.",
      "err.unavailable.title": "Hangar cannot run gh now",
      "err.unavailable.hint": "The kit cannot be used at the moment. Try again in a moment.",
      "err.timeout.title": "gh took too long",
      "err.timeout.hint": "GitHub did not answer in time. Check your connection, then try again.",
      "err.error.title": "gh failed",
      "err.error.hint": "This is what it said:",
    },
    fr: {
      tab: "CI",
      runs: "Exécutions récentes",
      refresh: "Actualiser",
      back: "Retour aux exécutions récentes",
      retry: "Réessayer",
      copy: "Copier",
      copyLink: "Copier le lien",
      loadingRuns: "Lecture des exécutions…",
      loadingRun: "Lecture de l'exécution…",
      "noRuns.title": "Aucune exécution pour l'instant",
      "noRuns.hint": "Un push sur ce dépôt en lance une, et elle s'affiche ici.",
      run: "Exécution n° {id}",
      onBranch: "{name} sur {branch}",
      jobs: "Jobs",
      failedJobs: "Jobs en échec",
      otherJobs: "Autres jobs",
      noJobs: "Pas encore de job.",
      noJobsRan: "Aucun job n'a tourné.",
      stepRunning: "{step}, étape {at} sur {total}",
      stepFailed: "Échec à l'étape {step}",
      queued: "En attente",
      running: "En cours",
      success: "Réussie",
      failure: "Échouée",
      neutral: "Terminée",
      cancelled: "Annulée",
      skipped: "Ignorée",
      // Une exécution, mais un job.
      "job.queued": "En attente",
      "job.running": "En cours",
      "job.success": "Réussi",
      "job.failure": "Échoué",
      "job.neutral": "Terminé",
      "job.cancelled": "Annulé",
      "job.skipped": "Ignoré",
      badgeRunning: "en cours",
      badgeQueued: "en attente",
      badgeSuccess: "réussie",
      badgeFailure: "échec",
      badgeNeutral: "terminée",
      notifyTitle: "{name} : {state}",
      failedLog: "Journal des étapes en échec",
      logLoading: "Lecture du journal…",
      logCut: "Seul le premier mégaoctet de ce journal est affiché.",
      "logEmpty.title": "Le journal est vide",
      "logEmpty.hint": "GitHub est peut-être encore en train de l'enregistrer. Réessaie dans un instant.",
      logError: "Le journal est illisible : {reason}.",
      warning: "La dernière actualisation a échoué. Nouvel essai…",
      "err.no-gh.title": "gh n'est pas installé",
      "err.no-gh.hint": "Cet onglet lit la CI avec GitHub CLI. Installe-le, connecte-toi avec gh auth login, puis réessaie.",
      "err.no-auth.title": "gh n'est pas connecté",
      "err.no-auth.hint": "Cet onglet lit la CI avec ta connexion gh. Lance ceci dans un terminal, puis réessaie.",
      "err.no-repo.title": "Aucun dépôt GitHub ici",
      "err.no-repo.hint": "Ouvre cet onglet dans un workspace dont le dossier est un clone git avec un remote GitHub.",
      "err.run-not-found.title": "Exécution introuvable",
      "err.run-not-found.hint": "Cette exécution n'existe pas dans ce dépôt, ou gh ne la voit pas.",
      "err.denied.title": "Cet onglet n'a pas le droit de lire la CI",
      "err.denied.hint": "Accepte les commandes de CI status dans Réglages › Kits, ou dans la fenêtre Kits du workspace, puis réessaie.",
      "err.unavailable.title": "Hangar ne peut pas lancer gh maintenant",
      "err.unavailable.hint": "Le kit est inutilisable pour le moment. Réessaie dans un instant.",
      "err.timeout.title": "gh a mis trop de temps",
      "err.timeout.hint": "GitHub n'a pas répondu à temps. Vérifie ta connexion, puis réessaie.",
      "err.error.title": "gh a échoué",
      "err.error.hint": "Voici ce qu'il a répondu :",
    },
  };

  /** What a failure gives the person to copy: where to get gh, or what to run. */
  const ERROR_COPY = { "no-gh": "https://cli.github.com", "no-auth": "gh auth login" };

  const ID = /^[0-9]+$/;
  const isRunId = (value) => typeof value === "string" && ID.test(value);

  // eslint-disable-next-line no-control-regex
  const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]/g;
  const stripAnsi = (text) => String(text).replace(ANSI, "");

  // Control, zero-width and bidi characters: what tab.setTitle would refuse.
  const INVISIBLE = new RegExp("[\u0000-\u001f\u007f-\u009f​-‏ - ⁠-⁯﻿]", "g");

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

  /** The key of the word for its state: a neutral one says what it was, when that is known. */
  function wordOf(item) {
    const kind = kindOf(item);
    return kind === "neutral" && (item.conclusion === "cancelled" || item.conclusion === "skipped") ? item.conclusion : kind;
  }

  const isDone = (item) => Boolean(item) && item.status === "completed";

  function badgeFor(run, t) {
    const kind = kindOf(run);
    if (kind === "running") return { state: "running", text: t("badgeRunning") };
    if (kind === "queued") return { state: "running", text: t("badgeQueued") };
    if (kind === "success") return { state: "success", text: t("badgeSuccess") };
    if (kind === "failure") return { state: "failure", text: t("badgeFailure") };
    return { state: "neutral", text: t("badgeNeutral") };
  }

  function time(value) {
    const ms = Date.parse(value);
    // GitHub writes 0001-01-01 for a start that has not happened.
    return Number.isFinite(ms) && ms > 86400000 ? ms : null;
  }

  /** The progress of a job: its steps done, the step running now, the step that failed. */
  function jobView(job) {
    const steps = (Array.isArray(job && job.steps) ? job.steps : []).filter(Boolean);
    const current = steps.find((step) => step.status === "in_progress");
    const failed = steps.find((step) => kindOf(step) === "failure");
    const done = steps.filter((step) => step.status === "completed").length;
    return {
      name: oneLine(job && job.name, 80),
      kind: kindOf(job),
      word: wordOf(job),
      total: steps.length,
      at: done + 1,
      step: current ? oneLine(current.name, 80) : null,
      failedStep: failed ? oneLine(failed.name, 80) : null,
      startedAt: time(job && job.startedAt),
      completedAt: time(job && job.completedAt),
    };
  }

  /** How long a job has taken so far, or took; null before it starts. */
  function jobDuration(job, now) {
    if (job.startedAt === null) return null;
    return Math.max(0, (job.completedAt === null ? now : job.completedAt) - job.startedAt);
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

  /** Everything the page shows of one run. */
  function runView(run, now) {
    const jobs = (Array.isArray(run && run.jobs) ? run.jobs : []).map(jobView);
    return {
      kind: kindOf(run),
      word: wordOf(run),
      done: isDone(run),
      name: oneLine((run && run.workflowName) || (run && run.displayTitle), 40),
      title: oneLine(run && run.displayTitle, 200),
      branch: oneLine(run && run.headBranch, 80),
      url: typeof (run && run.url) === "string" ? run.url : "",
      elapsed: elapsedMs(run, now),
      endedAt: isDone(run) ? time(run.updatedAt) : null,
      jobs,
      failedJobs: jobs.filter((job) => job.kind === "failure"),
      otherJobs: jobs.filter((job) => job.kind !== "failure"),
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

  // What the tools a CI runs write when something is wrong, each at a place in
  // the line where it says so: not any line that holds the word, since a list of
  // « test a_refused_error ... ok » holds it hundreds of times.
  const ERROR_LINE = new RegExp(
    [
      "(?:^|\\s)(?:error|fatal)(?:\\[[^\\]]*\\])?:", // error: …, error[E0599]: …, fatal: …
      "\\bpanicked at\\b", // Rust
      "\\bassertion\\b.*\\bfailed\\b",
      "\\.\\.\\. FAILED\\s*$", // a test of Rust's
      "\\btest result: FAILED\\b",
      "^\\s*(?:--- )?FAIL(?:ED)?\\b", // Go, Jest, Vitest, pytest
      "^\\s*npm ERR!",
      "^\\s*(?:[\\w$]+\\.)*[\\w$]*(?:Error|Exception):", // TypeError: …, java.io.IOException: …
    ].join("|"),
    "i",
  );
  const NOT_AN_ERROR = /\.\.\. ok\s*$|(?:^|\s)0 (?:errors?|failed|failures?)\b|\bno errors?\b/i;

  /** Whether a line of a failed step's log says what went wrong. */
  const isErrorLine = (text) => ERROR_LINE.test(text) && !NOT_AN_ERROR.test(text);

  /** The longest line kept, so one enormous line cannot stop the log from being read. */
  const LINE_MAX = 2000;

  /** `##[error]…` and its kin: GitHub's marks, which the log shows as what they mean. */
  function readMark(text) {
    const mark = text.match(/^##\[(error|warning|group|endgroup)\]/);
    if (!mark) return { text };
    const rest = text.slice(mark[0].length);
    if (mark[1] === "endgroup") return null;
    return { text: rest, kind: mark[1] === "group" ? undefined : mark[1] };
  }

  /** The log of the failed steps (`gh run view --log-failed`), readable: `job<TAB>step<TAB>timestamp
   *  text` lines become text, with a heading when the step changes. `errors` are the indexes of the
   *  lines that say what went wrong, in order. Nothing is cut here: `hangar.ui.log` keeps only what
   *  is near the view. */
  function readLog(raw, truncated) {
    const stamp = /^﻿?\d{4}-\d\d-\d\dT[\d:.]+Z ?/;
    const lines = [];
    const errors = [];
    let heading = null;
    for (const line of stripAnsi(raw).replace(/^﻿/, "").split(/\r?\n/)) {
      const parts = line.split("\t");
      const columns = parts.length >= 3;
      const text = (columns ? parts.slice(2).join("\t") : line).replace(stamp, "");
      if (!columns && text.trim() === "") continue;
      const marked = readMark(text);
      if (!marked) continue;
      const label = columns ? `${parts[0].trim()} › ${parts[1].trim()}`.slice(0, 160) : "";
      if (label && label !== heading) {
        lines.push({ text: label, kind: "head" });
        heading = label;
      }
      const shown = marked.text.length > LINE_MAX ? marked.text.slice(0, LINE_MAX - 1) + "…" : marked.text;
      const kind = marked.kind || (isErrorLine(shown) ? "error" : undefined);
      if (kind === "error") errors.push(lines.length);
      lines.push(kind ? { text: shown, kind } : { text: shown });
    }
    return { lines, errors, truncated: Boolean(truncated) };
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
        word: wordOf(run),
        createdAt: time(run.createdAt),
      }));
  }

  return {
    STRINGS, ERROR_COPY, isRunId, stripAnsi, oneLine, outcomeOf, kindOf, wordOf, isDone, badgeFor, jobView, jobDuration,
    elapsedMs, runView, classify, isFatal, isErrorLine, readLog, runList,
  };
});
