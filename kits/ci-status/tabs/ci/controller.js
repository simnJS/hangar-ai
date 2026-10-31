"use strict";
// The tab's behaviour, apart from any DOM: asks Hangar for what to show, polls
// while a run is going, sets the tab's title and badge, and notifies once when
// the run ends. It reports every change through `onChange(state)`; the page
// draws, the tests read. `hangar` is `window.hangar`, or a fake of it.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.CiController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const POLL_MS = 5000;
  const TICK_MS = 1000;
  const MAX_FAILURES = 3;

  /**
   * @param {object} options
   * @param {{call(method: string, params?: object): Promise<any>, on?(event: string, fn: Function): Function}} options.hangar
   * @param {object} options.logic   CiLogic
   * @param {string} [options.lang]
   * @param {() => number} [options.now]
   * @param {(fn: Function, ms: number) => any} [options.setTimer]
   * @param {(handle: any) => void} [options.clearTimer]
   * @param {(state: object) => void} [options.onChange]
   */
  function createController(options) {
    const { hangar, logic } = options;
    const lang = options.lang || "en";
    const now = options.now || Date.now;
    const setTimer = options.setTimer || ((fn, ms) => setTimeout(fn, ms));
    const clearTimer = options.clearTimer || ((handle) => clearTimeout(handle));
    const onChange = options.onChange || (() => undefined);

    const state = {
      view: "loading", // loading | picker | run | error
      id: null,
      fromPicker: false,
      run: null,
      log: null, // { lines, total, shown } | { empty: true } | { error } | null
      runs: null,
      error: null, // { kind, detail }
      warning: false,
    };

    let generation = 0; // a later request makes the earlier answers stale
    let pollTimer = null;
    let tickTimer = null;
    let sawRunning = false;
    let failures = 0;
    let lastTitle = null;
    let lastBadge = null;
    let disposed = false;
    let offParams = null;

    const emit = () => {
      if (!disposed) onChange(state);
    };

    function stopTimers() {
      if (pollTimer !== null) clearTimer(pollTimer);
      if (tickTimer !== null) clearTimer(tickTimer);
      pollTimer = tickTimer = null;
    }

    function reset() {
      generation++;
      stopTimers();
      sawRunning = false;
      failures = 0;
      Object.assign(state, { run: null, log: null, error: null, warning: false });
    }

    // Title and badge are the tab's own business and never worth failing for.
    async function setTitle(title) {
      if (title === lastTitle) return;
      lastTitle = title;
      try {
        await hangar.call("tab.setTitle", { title });
      } catch {
        /* a refused title leaves the tab's own */
      }
    }

    async function setBadge(badge) {
      const key = badge ? `${badge.state}|${badge.text || ""}` : "none";
      if (key === lastBadge) return;
      lastBadge = key;
      try {
        await hangar.call("tab.setBadge", badge ? { state: badge.state, text: badge.text } : { state: null });
      } catch {
        /* idem */
      }
    }

    /** Runs a declared command. Resolves { ok: true, stdout } or { ok: false, kind, detail }. */
    async function command(name, args) {
      let answer;
      try {
        answer = await hangar.call("command.run", args ? { name, args } : { name });
      } catch (error) {
        const kind = logic.classify({ code: error && error.code });
        return { ok: false, kind, detail: String((error && error.message) || "") };
      }
      if (answer && answer.exitCode === 0) return { ok: true, stdout: String(answer.stdout || "") };
      const fault = { exitCode: answer ? answer.exitCode : null, stderr: answer && answer.stderr, timedOut: answer && answer.timedOut };
      return { ok: false, kind: logic.classify(fault), detail: logic.stripAnsi((answer && (answer.stderr || answer.stdout)) || "").trim().slice(0, 500) };
    }

    async function notifyEnd(view) {
      const stateKey = view.kind === "queued" ? "neutral" : view.kind;
      try {
        await hangar.call("notify", {
          title: logic.t(lang, "notifyTitle", { name: view.name || logic.t(lang, "tab"), state: logic.t(lang, stateKey).toLowerCase() }),
          body: [view.title, view.branch].filter(Boolean).join(" · ").slice(0, 300) || undefined,
        });
      } catch {
        /* not granted, or over the rate limit: the badge says it too */
      }
    }

    async function loadLog(id, token) {
      const result = await command("ci-log", { id });
      if (token !== generation) return;
      if (!result.ok) state.log = { error: result.kind };
      else if (!result.stdout.trim()) state.log = { empty: true };
      else state.log = logic.trimLog(result.stdout);
      emit();
    }

    function startTick() {
      if (tickTimer !== null || disposed) return;
      const tick = () => {
        tickTimer = null;
        if (disposed || state.view !== "run" || !state.run || logic.isDone(state.run)) return;
        emit();
        tickTimer = setTimer(tick, TICK_MS);
      };
      tickTimer = setTimer(tick, TICK_MS);
    }

    async function fetchRun(id) {
      const token = generation;
      const result = await command("ci-run", { id });
      if (token !== generation || disposed) return;

      if (result.ok) {
        let run = null;
        try {
          run = JSON.parse(result.stdout);
        } catch {
          /* handled below */
        }
        if (!run || typeof run !== "object") {
          result.ok = false;
          result.kind = "error";
          result.detail = result.stdout.slice(0, 300);
        } else {
          failures = 0;
          state.warning = false;
          state.error = null;
          state.view = "run";
          state.run = run;
          const view = logic.runView(run, now());
          await setTitle(view.name || logic.t(lang, "run", { id }));
          await setBadge(logic.badgeFor(run, lang));
          if (token !== generation || disposed) return;
          if (view.done) {
            stopTimers();
            const ended = sawRunning;
            sawRunning = false;
            emit();
            if (ended) await notifyEnd(view);
            if (view.kind === "failure") await loadLog(id, token);
          } else {
            sawRunning = true;
            emit();
            startTick();
            pollTimer = setTimer(() => {
              pollTimer = null;
              fetchRun(id);
            }, POLL_MS);
          }
          return;
        }
      }

      // Failed. A transient failure while following a run is retried; the rest is shown.
      failures++;
      if (state.run && !logic.isFatal(result.kind) && failures < MAX_FAILURES) {
        state.warning = true;
        emit();
        pollTimer = setTimer(() => {
          pollTimer = null;
          fetchRun(id);
        }, POLL_MS);
        return;
      }
      stopTimers();
      state.view = "error";
      state.error = { kind: result.kind, detail: result.detail };
      state.warning = false;
      await setBadge(null);
      emit();
    }

    async function fetchList() {
      const token = generation;
      const result = await command("ci-list");
      if (token !== generation || disposed) return;
      if (result.ok) {
        let list = null;
        try {
          list = JSON.parse(result.stdout);
        } catch {
          /* handled below */
        }
        if (Array.isArray(list)) {
          state.runs = logic.runList(list);
          state.view = "picker";
          state.error = null;
          emit();
          return;
        }
        result.kind = "error";
        result.detail = result.stdout.slice(0, 300);
      }
      state.view = "error";
      state.error = { kind: result.kind, detail: result.detail };
      await setBadge(null);
      emit();
    }

    function openRun(id, fromPicker) {
      reset();
      Object.assign(state, { view: "loading", id, fromPicker: Boolean(fromPicker) });
      emit();
      return fetchRun(id);
    }

    async function showPicker() {
      reset();
      Object.assign(state, { view: "loading", id: null, fromPicker: false, runs: state.runs });
      emit();
      await setTitle(logic.t(lang, "tab"));
      await setBadge(null);
      return fetchList();
    }

    function handleParams(message) {
      const params = (message && message.params) || {};
      return logic.isRunId(params.id) ? openRun(params.id, false) : showPicker();
    }

    return {
      state,
      /** Reads the tab's params and shows the run, or the picker. */
      async start() {
        if (typeof hangar.on === "function") offParams = hangar.on("tab.params", (data) => handleParams(data));
        let message = { params: {} };
        try {
          message = await hangar.call("tab.params");
        } catch {
          /* no params: the picker */
        }
        return handleParams(message);
      },
      openRun: (id) => openRun(id, true),
      /** From a run opened in the picker, back to the picker. */
      back: showPicker,
      /** Tries again what failed, or refreshes the picker. */
      retry() {
        if (state.id) return openRun(state.id, state.fromPicker);
        return showPicker();
      },
      dispose() {
        disposed = true;
        generation++;
        stopTimers();
        if (typeof offParams === "function") offParams();
      },
    };
  }

  return { createController, POLL_MS, TICK_MS, MAX_FAILURES };
});
