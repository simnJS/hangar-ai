"use strict";
// The tab's behaviour, apart from any DOM: follows the hook's log, keeps the badge,
// writes passes. It reports every change through `onChange(state)`; the page draws,
// the tests read. `hangar` is `window.hangar`, or a fake of it.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.GuardController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const POLL_MS = 2000;
  const PASSES_READ_AT_START = 16;

  function createController(options) {
    const { hangar, logic } = options;
    const lang = options.lang || "en";
    const now = options.now || Date.now;
    const setTimer = options.setTimer || ((fn, ms) => setTimeout(fn, ms));
    const clearTimer = options.clearTimer || ((handle) => clearTimeout(handle));
    const onChange = options.onChange || (() => undefined);

    const state = {
      view: "loading", // loading | list | denied | error
      rows: [],
      errors: null, // { count, last }
      grants: {}, // key → { expiresAt, grantedAt }
      problems: {}, // key → { code, detail }
      pending: {}, // key → true, while its pass is being written
      stale: false,
      clearFailed: false,
      detail: "",
    };

    let events = [];
    let errorEvents = [];
    let lastLog = null;
    let lastErrors = null;
    let clearedAt = 0;
    let lastBadge;
    let timer = null;
    let disposed = false;

    const emit = () => {
      if (!disposed) onChange(state);
    };

    async function setBadge(badge) {
      const id = badge ? `${badge.state}|${badge.text}` : "none";
      if (id === lastBadge) return;
      lastBadge = id;
      try {
        await hangar.call("tab.setBadge", badge || { state: null });
      } catch {
        /* the list says it too */
      }
    }

    function rebuild() {
      state.rows = logic.buildRows(events, clearedAt);
      state.errors = logic.errorSummary(errorEvents, clearedAt);
      return setBadge(logic.badgeFor(state.rows));
    }

    /** The text of a data file, "" when it is not there yet. */
    async function read(path) {
      try {
        return (await hangar.call("data.read", { path })).text;
      } catch (error) {
        if (error && error.code === "not_found") return "";
        throw error;
      }
    }

    async function poll() {
      try {
        const log = await read(logic.LOG);
        const errors = await read(logic.ERRORS);
        if (disposed) return;
        if (log !== lastLog || errors !== lastErrors) {
          lastLog = log;
          lastErrors = errors;
          events = logic.parseLines(log);
          errorEvents = logic.parseLines(errors);
          await rebuild();
        }
        state.view = "list";
        state.stale = false;
        state.detail = "";
      } catch (error) {
        if (disposed) return;
        const code = error && error.code;
        if (code === "denied") {
          state.view = "denied";
          lastLog = lastErrors = null;
          await setBadge(null);
        } else if (state.view === "list") {
          state.stale = true; // what is listed stays, the next poll may do better
        } else {
          state.view = "error";
          state.detail = String((error && error.message) || "");
        }
      }
      emit();
    }

    function schedule() {
      if (disposed) return;
      timer = setTimer(async () => {
        timer = null;
        await poll();
        schedule();
      }, POLL_MS);
    }

    async function readClearedAt() {
      try {
        const parsed = JSON.parse((await read(logic.STATE)) || "{}");
        if (Number.isFinite(parsed.clearedAt)) clearedAt = parsed.clearedAt;
      } catch {
        /* denied, or not ours: the first poll says which */
      }
    }

    /** After a reload, what the pass files say. Passes written before it still hold. */
    async function readGrants() {
      const waiting = state.rows.filter((row) => row.usedAt === null).slice(0, PASSES_READ_AT_START);
      for (const row of waiting) {
        try {
          const grant = logic.grantFromPass(await read(logic.passPath(row.key)), row.key, now());
          if (grant) state.grants[row.key] = grant;
        } catch {
          return;
        }
      }
    }

    return {
      state,
      async start() {
        try {
          await hangar.call("tab.setTitle", { title: logic.t(lang, "tab") });
        } catch {
          /* the manifest's title stays */
        }
        await readClearedAt();
        await poll();
        if (state.view === "list") {
          await readGrants();
          emit();
        }
        schedule();
      },
      /** Writes the one-time pass for this row. */
      async allow(key) {
        const row = state.rows.find((candidate) => candidate.key === key);
        if (!row || row.usedAt !== null || state.pending[key] || !logic.isKey(key)) return;
        state.pending[key] = true;
        delete state.problems[key];
        emit();
        const pass = logic.passFor(row, now());
        try {
          await hangar.call("data.write", { path: logic.passPath(key), text: JSON.stringify(pass) });
          state.grants[key] = { expiresAt: pass.expiresAt, grantedAt: pass.createdAt };
        } catch (error) {
          const code = error && error.code;
          state.problems[key] = {
            code: code === "denied" || code === "rate_limited" ? code : "failed",
            detail: String((error && error.message) || ""),
          };
        }
        delete state.pending[key];
        emit();
      },
      /** Empties the list: what the log holds stays, and is not shown. */
      async clear() {
        const at = now();
        try {
          await hangar.call("data.write", { path: logic.STATE, text: JSON.stringify({ clearedAt: at }) });
          clearedAt = at;
          state.clearFailed = false;
          await rebuild();
        } catch {
          state.clearFailed = true;
        }
        emit();
      },
      retry: poll,
      dispose() {
        disposed = true;
        if (timer !== null) clearTimer(timer);
      },
    };
  }

  return { createController, POLL_MS };
});
