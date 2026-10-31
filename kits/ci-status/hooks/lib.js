"use strict";
// The hook's logic, free of any side effect so a test can drive it: what counts
// as a `git push`, which runs belong to the push, and how the tab is opened.

const KIT = "ci-status";
const TAB = "ci";

/** Splits a shell command line into simple commands, each a list of words.
 *  Quotes keep their content in one word (and out of the way of `&&`, `;`…). */
function simpleCommands(line) {
  const commands = [];
  let words = [];
  let word = "";
  let inWord = false;
  let quote = null;
  const endWord = () => {
    if (inWord) words.push(word);
    word = "";
    inWord = false;
  };
  const endCommand = () => {
    endWord();
    if (words.length) commands.push(words);
    words = [];
  };
  const text = String(line);
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      if (c === quote) quote = null;
      else if (c === "\\" && quote === '"' && i + 1 < text.length) word += text[++i];
      else word += c;
    } else if (c === "'" || c === '"') {
      quote = c;
      inWord = true;
    } else if (c === "\\" && i + 1 < text.length) {
      word += text[++i];
      inWord = true;
    } else if (";&|\n(){}".includes(c)) {
      endCommand();
    } else if (c === " " || c === "\t" || c === "\r") {
      endWord();
    } else {
      word += c;
      inWord = true;
    }
  }
  endCommand();
  return commands;
}

// Options of `git` that take a value, before the subcommand.
const GIT_VALUE_OPTIONS = new Set(["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path"]);

/** The arguments after `git push`, or null when this simple command is not one. */
function pushArguments(words) {
  let i = 0;
  while (i < words.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[i])) i++; // FOO=bar git push
  if (words[i] === "command" || words[i] === "exec") i++;
  if (words[i] !== "git") return null;
  i++;
  while (i < words.length && words[i].startsWith("-")) {
    i += GIT_VALUE_OPTIONS.has(words[i]) ? 2 : 1;
  }
  if (words[i] !== "push") return null;
  return words.slice(i + 1);
}

// A push that sends nothing, or only deletes, starts no run.
const NO_RUN_OPTIONS = new Set(["--help", "-h", "--dry-run", "-n", "--delete", "-d"]);

/** Whether the Bash command pushes to a remote (so a CI run may have started). */
function isGitPush(command) {
  if (typeof command !== "string" || !command) return false;
  return simpleCommands(command).some((words) => {
    const args = pushArguments(words);
    return args !== null && !args.some((arg) => NO_RUN_OPTIONS.has(arg));
  });
}

const RUN_WINDOW_MS = 3 * 60 * 1000;

/** The runs of `gh run list --commit <sha> --json databaseId,createdAt`
 *  that this push can have started: recent ones, newest first, at most `max`.
 *  An old run for the same commit (a push with nothing new) is left alone. */
function recentRuns(list, since, max = 3) {
  if (!Array.isArray(list)) return [];
  return list
    .map((run) => ({ id: String(run && run.databaseId), created: Date.parse(run && run.createdAt) }))
    .filter((run) => /^[0-9]+$/.test(run.id) && Number.isFinite(run.created) && run.created >= since - RUN_WINDOW_MS)
    .sort((a, b) => b.created - a.created)
    .slice(0, max)
    .map((run) => run.id);
}

/** Looks for the runs of the commit, retrying: a run takes seconds to appear.
 *  `io` is { git(args) → text, gh(args) → text, sleep(ms), now() }, each
 *  rejecting when its program is absent or fails. Resolves the run ids found. */
async function findRuns(io, { attempts = 8, delayMs = 4000 } = {}) {
  const since = io.now();
  let sha;
  try {
    sha = (await io.git(["rev-parse", "HEAD"])).trim();
  } catch {
    return [];
  }
  if (!/^[0-9a-f]{7,64}$/i.test(sha)) return [];
  for (let attempt = 0; attempt < attempts; attempt++) {
    if (attempt) await io.sleep(delayMs);
    try {
      const out = await io.gh(["run", "list", "--commit", sha, "--limit", "10", "--json", "databaseId,createdAt"]);
      const ids = recentRuns(JSON.parse(out), since);
      if (ids.length) return ids;
    } catch (error) {
      // gh absent or not signed in: nothing will change by waiting.
      if (error && (error.code === "ENOENT" || /auth|login|token/i.test(String(error.stderr || error.message)))) return [];
    }
  }
  return [];
}

/** Asks Hangar to open the tab for a run. Never throws. */
async function openTab({ url, token, pane, id, fetchImpl = globalThis.fetch }) {
  if (!url || !token || !pane || !/^[0-9]+$/.test(String(id)) || typeof fetchImpl !== "function") return false;
  try {
    const response = await fetchImpl(`${url}/api/kit/tab/open?pane=${encodeURIComponent(pane)}`, {
      method: "POST",
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body: JSON.stringify({ kit: KIT, tab: TAB, params: { id: String(id) }, reveal: true }),
      signal: AbortSignal.timeout(2000),
    });
    return response.ok;
  } catch {
    return false;
  }
}

module.exports = { KIT, TAB, simpleCommands, isGitPush, recentRuns, findRuns, openTab };
