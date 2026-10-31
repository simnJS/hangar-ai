"use strict";
// The tab's behaviour, apart from any DOM: reads the last audit from the kit's data folder, runs
// a new one only when asked, keeps the badge, lists the agents and hands a finding to one. It
// reports through `onChange(state)` when what is shown changes and `onTick(state)` when only the
// time has passed. Opening the tab makes no call to the audit command, hence no network call.
// `hangar` is `window.hangar`, or a fake of it.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.DepsController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const TICK_MS = 30000;

  function createController(options) {
    const { hangar, logic } = options;
    let lang = options.lang || "en";
    const setTimer = options.setTimer || ((fn, ms) => setTimeout(fn, ms));
    const clearTimer = options.clearTimer || ((handle) => clearTimeout(handle));
    const onChange = options.onChange || (() => undefined);
    const onTick = options.onTick || (() => undefined);

    const state = {
      view: "loading", // loading | ready | denied | error
      running: false,
      result: null, // the last audit, checked
      error: null, // { code, detail } when a run did not complete
      panes: [],
      panesDenied: false,
      choice: null, // the agent the person picked
      pending: {}, // finding key → true, while it is being sent
      sent: {}, // finding key → { agent }
      problems: {}, // finding key → { code, detail, agent }
      expanded: {}, // outdated kind → true once "show more" was pressed
    };

    let lastBadge;
    let lastDrawn = null;
    let timer = null;
    let disposed = false;
    let unsubscribe = null;

    const drawn = () =>
      JSON.stringify([
        state.view, state.running, state.error, state.panes, state.panesDenied, state.choice, state.pending, state.sent, state.problems, state.expanded,
        state.result && state.result.at,
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
        /* the page says it too */
      }
    }

    const badgeNow = () => (state.running ? { state: "running" } : logic.badgeFor(state.result, lang));

    async function loadLast() {
      try {
        const { text } = await hangar.call("data.read", { path: logic.LAST });
        state.result = logic.parseResult(text);
      } catch {
        state.result = null; // nothing run yet, or the data folder is not ours to read: the page still works
      }
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

    function tick() {
      if (disposed) return;
      timer = setTimer(() => {
        timer = null;
        update();
        tick();
      }, TICK_MS);
    }

    const failRun = (code, detail = "") => {
      state.error = { code, detail: String(detail).slice(0, 200) };
    };

    return {
      state,
      async start() {
        await setTitle();
        await Promise.all([loadLast(), loadPanes()]);
        state.view = "ready";
        await setBadge(badgeNow());
        if (typeof hangar.on === "function") unsubscribe = hangar.on("panes.changed", loadPanes);
        update();
        tick();
      },
      /** Hangar's language changed: the strip's title and badge follow. */
      setLang(next) {
        if (next === lang) return undefined;
        lang = next;
        lastBadge = undefined;
        return Promise.all([setTitle(), setBadge(badgeNow())]);
      },
      /** Runs the audit, on the person's click. */
      async run() {
        if (state.running || disposed) return;
        state.running = true;
        state.error = null;
        if (state.view === "denied") state.view = "ready";
        update();
        await setBadge(badgeNow());
        try {
          const answer = await hangar.call("command.run", { name: "audit" });
          if (answer.timedOut) failRun("timeout");
          else {
            const result = answer.exitCode === 0 ? logic.parseResult(answer.stdout) : null;
            if (!result) failRun(answer.exitCode === 0 ? "unreadable" : "failed", (answer.stderr || "").split(/\r?\n/).find(Boolean) || `exit code ${answer.exitCode}`);
            else {
              state.result = result;
              state.sent = {};
              state.problems = {};
              state.expanded = {};
              try {
                await hangar.call("data.write", { path: logic.LAST, text: JSON.stringify(result) });
              } catch {
                /* it shows now; the next opening starts without it */
              }
            }
          }
        } catch (error) {
          const code = error && error.code;
          if (code === "denied") state.view = "denied";
          else if (code === "not_found" || code === "busy") failRun(code);
          else failRun("failed", (error && error.message) || "");
        }
        state.running = false;
        await setBadge(badgeNow());
        update();
      },
      choose(agent) {
        state.choice = agent;
        update();
      },
      showMore(kind) {
        state.expanded[kind] = true;
        update();
      },
      /** Hands one finding (`kind`, and its `key` in the result) to the chosen agent. */
      async handOver(kind, key) {
        const agent = state.choice || logic.defaultTarget(state.panes);
        const item = (kind === "vulnerability" ? logic.vulnerabilitiesOf(state.result) : logic.outdatedGroups(state.result).flatMap((group) => group.items)).find(
          (entry) => entry.key === key,
        );
        if (!state.result || !item || !agent || state.pending[key]) return;
        state.pending[key] = true;
        delete state.problems[key];
        update();
        try {
          await hangar.call("panes.send", { pane: agent, text: logic.briefFor(kind, item) });
          state.sent[key] = { agent };
        } catch (error) {
          const code = error && error.code;
          state.problems[key] = {
            code: ["denied", "not_found", "rate_limited", "unavailable"].includes(code) ? code : "failed",
            detail: String((error && error.message) || ""),
            agent,
          };
          if (code === "not_found" || code === "unavailable") await loadPanes();
        }
        delete state.pending[key];
        update();
      },
      dispose() {
        disposed = true;
        if (timer !== null) clearTimer(timer);
        if (typeof unsubscribe === "function") unsubscribe();
      },
    };
  }

  return { createController, TICK_MS };
});
