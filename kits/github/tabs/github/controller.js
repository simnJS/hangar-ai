"use strict";
// The tab's behaviour, apart from any DOM: reads the list through the declared
// commands, refreshes it every 60 s while the tab is visible, keeps the filters,
// sets the badge, and hands an item to an agent. It reports every change through
// `onChange(state)`; the page draws, the tests read. `hangar` is `window.hangar`,
// or a fake of it. One controller serves both tabs: `kind` is "pulls" or "issues".
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.GithubController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const REFRESH_MS = 60000;
  const SAVE_DELAY_MS = 700; // data.write is limited to 10 a minute
  const MIN_GAP_MS = 5000; // a tab brought back to view does not refresh twice in a row

  const COMMANDS = {
    pulls: { list: "pr-list", view: "pr-view" },
    issues: { list: "issue-list", view: "issue-view" },
  };

  /**
   * @param {object} options
   * @param {"pulls"|"issues"} options.kind
   * @param {{call(method: string, params?: object): Promise<any>}} options.hangar
   * @param {object} options.logic  GithubLogic
   * @param {string} [options.lang]
   * @param {() => number} [options.now]
   * @param {(fn: Function, ms: number) => any} [options.setTimer]
   * @param {(handle: any) => void} [options.clearTimer]
   * @param {(state: object) => void} [options.onChange]
   */
  function createController(options) {
    const { hangar, logic, kind } = options;
    const names = COMMANDS[kind];
    const isPull = kind === "pulls";
    let lang = options.lang || "en";
    const now = options.now || Date.now;
    const setTimer = options.setTimer || ((fn, ms) => setTimeout(fn, ms));
    const clearTimer = options.clearTimer || ((handle) => clearTimeout(handle));
    const onChange = options.onChange || (() => undefined);

    const state = {
      view: "loading", // loading | list | detail | error
      rows: [],
      repo: null, // { owner, name, nameWithOwner }
      filters: logic.sanitizeFilters(kind, null),
      error: null, // { kind, detail }
      stale: false, // a refresh failed; the last list stays
      refreshing: false,
      refreshedAt: null,
      detail: null, // { number, loading, error, data }
      busy: null, // number of the item being handed over
    };

    let generation = 0; // a later refresh makes the earlier answer stale
    let detailGeneration = 0; // and so does a later detail
    let refreshTimer = null;
    let saveTimer = null;
    let visible = true;
    let disposed = false;
    let lastBadge = null;
    let started = false;

    const emit = () => {
      if (!disposed) onChange(state);
    };

    /** Runs a declared command. → { ok: true, stdout } or { ok: false, kind, detail } */
    async function command(name, args) {
      let answer;
      try {
        answer = await hangar.call("command.run", args ? { name, args } : { name });
      } catch (error) {
        return { ok: false, kind: logic.classify({ code: error && error.code }), detail: String((error && error.message) || "") };
      }
      if (answer && answer.exitCode === 0) return { ok: true, stdout: String(answer.stdout || "") };
      const fault = { exitCode: answer ? answer.exitCode : null, stderr: answer && answer.stderr, timedOut: answer && answer.timedOut };
      return {
        ok: false,
        kind: logic.classify(fault),
        detail: logic.stripAnsi((answer && (answer.stderr || answer.stdout)) || "").trim().slice(0, 500),
      };
    }

    async function json(name, args) {
      const result = await command(name, args);
      if (!result.ok) return result;
      try {
        return { ok: true, value: JSON.parse(result.stdout) };
      } catch {
        return { ok: false, kind: "error", detail: result.stdout.slice(0, 300) };
      }
    }

    // The tab's badge is its own business and never worth failing for.
    async function setBadge(badge) {
      const key = badge ? `${badge.state}|${badge.text || ""}` : "none";
      if (key === lastBadge) return;
      lastBadge = key;
      try {
        await hangar.call("tab.setBadge", badge ? { state: badge.state, text: badge.text } : { state: null });
      } catch {
        /* a refused badge leaves the tab's own */
      }
    }

    function schedule() {
      if (refreshTimer !== null) clearTimer(refreshTimer);
      refreshTimer = null;
      if (disposed || !visible || !started) return;
      refreshTimer = setTimer(() => {
        refreshTimer = null;
        refresh();
      }, REFRESH_MS);
    }

    async function loadRepo() {
      if (state.repo) return null;
      const result = await json("repo-info");
      if (!result.ok) return result;
      const info = result.value || {};
      const owner = String((info.owner && (info.owner.login || info.owner)) || "");
      const name = String(info.name || "");
      state.repo = { owner, name, nameWithOwner: String(info.nameWithOwner || (owner && name ? `${owner}/${name}` : name)) };
      return null;
    }

    /** Reads the list again. Shown over the previous one, which stays if this fails. */
    async function refresh() {
      if (disposed) return;
      const token = ++generation;
      state.refreshing = true;
      if (state.view === "error") state.view = "loading";
      emit();

      let failure = await loadRepo();
      let rows = null;
      if (!failure) {
        const result = await json(names.list);
        if (result.ok && !Array.isArray(result.value)) {
          failure = { ok: false, kind: "error", detail: JSON.stringify(result.value).slice(0, 300) };
        } else if (!result.ok) {
          failure = result;
        } else {
          rows = isPull ? logic.pullRows(result.value) : logic.issueRows(result.value);
        }
      }
      if (token !== generation || disposed) return;

      state.refreshing = false;
      if (rows) {
        state.rows = rows;
        state.error = null;
        state.stale = false;
        state.refreshedAt = now();
        if (state.view === "loading" || state.view === "error") state.view = "list";
        if (isPull) await setBadge(logic.badgeFor(rows, lang));
        else await setBadge(null);
      } else if (state.view === "detail" || (state.rows.length && state.view !== "error" && !logic.isFatal(failure.kind))) {
        state.stale = true; // a blip does not erase the list
      } else {
        state.view = "error";
        state.error = { kind: failure.kind, detail: failure.detail };
        state.stale = false;
        await setBadge(null);
      }
      if (token !== generation || disposed) return;
      emit();
      schedule();
    }

    // ----------------------------------------------------------------- filters

    const filtersFile = `filters-${kind}.json`;

    async function loadFilters() {
      try {
        const answer = await hangar.call("data.read", { path: filtersFile });
        state.filters = logic.sanitizeFilters(kind, JSON.parse((answer && answer.text) || "{}"));
      } catch {
        /* none saved yet, or not granted: the defaults */
      }
    }

    function saveSoon() {
      if (saveTimer !== null) clearTimer(saveTimer);
      saveTimer = setTimer(async () => {
        saveTimer = null;
        try {
          await hangar.call("data.write", { path: filtersFile, text: JSON.stringify(state.filters) });
        } catch {
          /* filters are a convenience */
        }
      }, SAVE_DELAY_MS);
    }

    function setFilters(patch) {
      state.filters = logic.sanitizeFilters(kind, { ...state.filters, ...patch });
      saveSoon();
      emit();
    }

    function clearFilters() {
      state.filters = logic.sanitizeFilters(kind, null);
      saveSoon();
      emit();
    }

    // ------------------------------------------------------------------ detail

    /** Reads one item in full. → { ok: true, data } or { ok: false, kind, detail } */
    async function fetchDetail(number) {
      const id = String(number);
      if (!isPull) {
        const result = await json(names.view, { id });
        if (!result.ok) return result;
        return { ok: true, data: logic.issueDetail(result.value) };
      }
      const [view, threads] = await Promise.all([json(names.view, { id }), loadThreads(id)]);
      if (!view.ok) return view;
      return { ok: true, data: logic.pullDetail(view.value, threads) };
    }

    async function loadThreads(id) {
      if (!state.repo) return null;
      const result = await json("pr-threads", { id, owner: state.repo.owner, name: state.repo.name });
      return result.ok ? logic.parseThreads(result.value) : null;
    }

    async function open(number) {
      const token = ++detailGeneration;
      state.view = "detail";
      state.detail = { number, loading: true, error: null, data: null };
      emit();
      const result = await fetchDetail(number);
      if (token !== detailGeneration || disposed) return;
      state.detail = result.ok
        ? { number, loading: false, error: null, data: result.data }
        : { number, loading: false, error: { kind: result.kind, detail: result.detail }, data: null };
      emit();
    }

    function back() {
      detailGeneration++;
      state.view = "list";
      state.detail = null;
      emit();
      if (state.refreshedAt === null || now() - state.refreshedAt > MIN_GAP_MS) refresh();
      else schedule();
    }

    // ----------------------------------------------------------------- hand-off

    /** The panes that can receive a brief now. → { ok: true, panes } or { ok: false, kind } */
    async function panes() {
      let list;
      try {
        list = await hangar.call("panes.list");
      } catch (error) {
        return { ok: false, kind: error && error.code === "denied" ? "list-denied" : "error" };
      }
      const sendable = (Array.isArray(list) ? list : []).filter((pane) => pane && pane.canSend && pane.agent === "claude");
      return sendable.length ? { ok: true, panes: sendable } : { ok: false, kind: "no-agent" };
    }

    /** Reads the item, makes the brief, sends it. → { ok: true, pane } or { ok: false, kind, detail? } */
    async function handOff(number, pane) {
      if (state.busy !== null) return { ok: false, kind: "busy" };
      state.busy = number;
      emit();
      try {
        let data = state.detail && state.detail.number === number && state.detail.data ? state.detail.data : null;
        if (!data) {
          const result = await fetchDetail(number);
          if (!result.ok) return { ok: false, kind: "read", detail: result.detail };
          data = result.data;
        }
        const text = logic.buildBrief(kind, data, { repo: state.repo && state.repo.nameWithOwner });
        try {
          await hangar.call("panes.send", { pane, text });
        } catch (error) {
          const code = error && error.code;
          return { ok: false, kind: code === "denied" ? "send-denied" : "send", detail: String((error && error.message) || "") };
        }
        return { ok: true, pane };
      } finally {
        state.busy = null;
        emit();
      }
    }

    // --------------------------------------------------------------- lifecycle

    function setVisible(value) {
      const was = visible;
      visible = Boolean(value);
      if (!visible) {
        if (refreshTimer !== null) clearTimer(refreshTimer);
        refreshTimer = null;
        return;
      }
      if (!was && started && state.view !== "detail" && (state.refreshedAt === null || now() - state.refreshedAt > MIN_GAP_MS)) refresh();
      else if (!was) schedule();
    }

    return {
      state,
      async start() {
        started = true;
        emit();
        await loadFilters();
        emit();
        return refresh();
      },
      refresh,
      open,
      back,
      panes,
      handOff,
      setFilters,
      clearFilters,
      setVisible,
      setLang(value) {
        lang = value;
        lastBadge = null;
        if (isPull && state.rows.length) setBadge(logic.badgeFor(state.rows, lang));
      },
      dispose() {
        disposed = true;
        generation++;
        detailGeneration++;
        if (refreshTimer !== null) clearTimer(refreshTimer);
        if (saveTimer !== null) clearTimer(saveTimer);
      },
    };
  }

  return { createController, REFRESH_MS, SAVE_DELAY_MS, MIN_GAP_MS };
});
