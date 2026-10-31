"use strict";
// The tab's behaviour, apart from any DOM: follows the hook's records, keeps the badge,
// lists the agents and hands a failure to one. It reports through `onChange(state)` when
// what is listed changes, so the page draws again, and `onTick(state)` when only the time
// has passed, so a focus or a selection survives a poll. `hangar` is `window.hangar`, or a
// fake of it.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.TestsController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const POLL_MS = 3000;

  function createController(options) {
    const { hangar, logic } = options;
    let lang = options.lang || "en";
    const now = options.now || Date.now;
    const setTimer = options.setTimer || ((fn, ms) => setTimeout(fn, ms));
    const clearTimer = options.clearTimer || ((handle) => clearTimeout(handle));
    const onChange = options.onChange || (() => undefined);
    const onTick = options.onTick || (() => undefined);

    const state = {
      view: "loading", // loading | list | denied | error
      runs: [],
      panes: [],
      panesDenied: false,
      choice: {}, // run key → the agent the person picked
      pending: {}, // id → true, while it is being sent
      sent: {}, // id → { agent, runAt }
      problems: {}, // id → { code, detail, agent }
      stale: false,
    };

    const cache = new Map(); // key → run, as last read
    let lastIndex = null;
    let lastBadge;
    let lastDrawn = null;
    let timer = null;
    let disposed = false;
    let unsubscribe = null;

    const drawn = () =>
      JSON.stringify([
        state.view, state.stale, state.panes, state.panesDenied, state.choice, state.pending, state.sent, state.problems,
        state.runs.map((run) => [run.key, run.at]),
      ]);

    function update() {
      if (disposed) return;
      const next = drawn();
      if (next === lastDrawn) {
        onTick(state);
      } else {
        lastDrawn = next;
        onChange(state);
      }
    }

    async function setTitle() {
      try {
        await hangar.call("tab.setTitle", { title: logic.t(lang, "tab") });
      } catch {
        /* the manifest's title stays */
      }
    }

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

    /** The text of a data file, null when it is not there yet. */
    async function read(path) {
      try {
        return (await hangar.call("data.read", { path })).text;
      } catch (error) {
        if (error && error.code === "not_found") return null;
        throw error;
      }
    }

    async function readRuns(index) {
      const runs = [];
      for (const entry of index) {
        let run = cache.get(entry.key);
        if (!run || run.at !== entry.at) {
          const source = await read(logic.runPath(entry.key));
          run = source === null ? null : logic.parseRun(source);
          if (run) cache.set(entry.key, run);
          else cache.delete(entry.key);
        }
        if (run) runs.push(run);
      }
      for (const key of [...cache.keys()]) if (!index.some((entry) => entry.key === key)) cache.delete(key);
      return runs;
    }

    async function poll() {
      try {
        const source = await read(logic.INDEX);
        if (disposed) return;
        const index = logic.parseIndex(source);
        const id = JSON.stringify(index);
        if (id !== lastIndex) {
          const runs = await readRuns(index);
          if (disposed) return;
          lastIndex = id;
          state.runs = runs;
          // What was handed over, or went wrong, is about a run that has since been replaced.
          for (const table of [state.sent, state.problems]) {
            for (const key of Object.keys(table)) {
              const run = runs.find((item) => key.startsWith(`${item.key}:`));
              if (!run || (table === state.sent && table[key].runAt !== run.at)) delete table[key];
            }
          }
          await setBadge(logic.badgeFor(runs, lang));
        }
        state.view = "list";
        state.stale = false;
      } catch (error) {
        if (disposed) return;
        if (error && error.code === "denied") {
          state.view = "denied";
          lastIndex = null;
          cache.clear();
          state.runs = [];
          await setBadge(null);
        } else if (state.view === "list") {
          state.stale = true; // what is listed stays, the next poll may do better
        } else {
          state.view = "error";
        }
      }
      update();
    }

    async function loadPanes() {
      try {
        state.panes = await hangar.call("panes.list");
        state.panesDenied = false;
      } catch (error) {
        state.panes = [];
        state.panesDenied = Boolean(error && error.code === "denied");
      }
      update();
    }

    function schedule() {
      if (disposed) return;
      timer = setTimer(async () => {
        timer = null;
        await poll();
        schedule();
      }, POLL_MS);
    }

    const targetOf = (run) => state.choice[run.key] || logic.defaultTarget(run, state.panes);

    return {
      state,
      async start() {
        await setTitle();
        await Promise.all([poll(), loadPanes()]);
        if (typeof hangar.on === "function") unsubscribe = hangar.on("panes.changed", loadPanes);
        schedule();
      },
      /** Hangar's language changed: the strip's title and badge follow. */
      setLang(next) {
        if (next === lang) return undefined;
        lang = next;
        lastBadge = undefined;
        return Promise.all([setTitle(), setBadge(logic.badgeFor(state.runs, lang))]);
      },
      /** The agent the person picked for a run. */
      choose(key, agent) {
        state.choice[key] = agent;
        update();
      },
      /** Sends one failure (its index in the run), or the run's first few (`index` null), to the run's agent. */
      async handOver(key, index) {
        const run = state.runs.find((item) => item.key === key);
        const id = `${key}:${index === null ? "all" : index}`;
        const agent = run && targetOf(run);
        if (!run || !agent || state.pending[id]) return;
        const failure = index === null ? null : run.failures[index];
        if (index !== null && !failure) return;
        state.pending[id] = true;
        delete state.problems[id];
        update();
        try {
          await hangar.call("panes.send", { pane: agent, text: logic.briefFor(run, failure) });
          state.sent[id] = { agent, runAt: run.at };
        } catch (error) {
          const code = error && error.code;
          state.problems[id] = {
            code: ["denied", "not_found", "rate_limited", "unavailable"].includes(code) ? code : "failed",
            detail: String((error && error.message) || ""),
            agent,
          };
          if (code === "not_found" || code === "unavailable") await loadPanes();
        }
        delete state.pending[id];
        update();
      },
      retry: async () => {
        await Promise.all([poll(), loadPanes()]);
      },
      dispose() {
        disposed = true;
        if (timer !== null) clearTimer(timer);
        if (typeof unsubscribe === "function") unsubscribe();
      },
    };
  }

  return { createController, POLL_MS };
});
