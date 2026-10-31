"use strict";
// The tab's behaviour, apart from any DOM: reads the list the agent wrote, keeps the
// person's answers in the kit's data folder, sets the tab's title and badge, and sends
// the result to the agent that asked. It reports every change through `onChange(state)`;
// the page draws, the tests read. `hangar` is `window.hangar`, or a fake of it.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.ChecklistController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  // data.write is limited to 10 a minute: answers are written after a pause, never closer than this.
  const SAVE_DELAY_MS = 1500;
  const SAVE_SPACING_MS = 7000;
  const RETRY_MS = 15000;

  /**
   * @param {object} options
   * @param {{call(method: string, params?: object): Promise<any>, on?(event: string, fn: Function): Function}} options.hangar
   * @param {object} options.logic   ChecklistLogic
   * @param {(key: string, vars?: object) => string} options.t
   * @param {() => number} [options.now]
   * @param {(fn: Function, ms: number) => any} [options.setTimer]
   * @param {(handle: any) => void} [options.clearTimer]
   * @param {(state: object) => void} [options.onChange]
   */
  function createController(options) {
    const { hangar, logic, t } = options;
    const now = options.now || Date.now;
    const setTimer = options.setTimer || ((fn, ms) => setTimeout(fn, ms));
    const clearTimer = options.clearTimer || ((handle) => clearTimeout(handle));
    const onChange = options.onChange || (() => undefined);

    const state = {
      view: "loading", // loading | list | error
      path: null,
      list: null,
      keys: [],
      saved: { answers: {}, redo: {}, sentAt: null },
      counts: null,
      error: null, // { kind, detail?, code?, vars? }
      caller: null, // { pane } | null
      persist: true,
      send: { status: "idle", pane: null, error: null }, // idle | sending | sent | error
    };

    let generation = 0; // a later load makes the earlier answers stale
    let disposed = false;
    let dirty = false;
    let saveTimer = null;
    let lastWrite = -Infinity;
    let writing = Promise.resolve();
    let lastTitle = null;
    let lastBadge = null;
    let offParams = null;
    let lastMessage = { params: {}, caller: null };

    const emit = () => {
      if (!disposed) onChange(state);
    };

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

    function recount() {
      state.counts = logic.progress(state.keys, state.saved);
      return state.counts;
    }

    function syncBadge() {
      if (state.view !== "list") return setBadge(null);
      return setBadge(logic.badgeFor(recount(), t));
    }

    // --- the answers, kept ---------------------------------------------------------

    function schedule() {
      if (!state.persist || state.view !== "list") return;
      dirty = true;
      if (saveTimer !== null) return;
      const wait = Math.max(SAVE_DELAY_MS, lastWrite + SAVE_SPACING_MS - now());
      saveTimer = setTimer(() => {
        saveTimer = null;
        flush();
      }, wait);
    }

    /** Writes what is not written yet. Never throws; a refused write turns saving off. */
    function flush() {
      if (saveTimer !== null) {
        clearTimer(saveTimer);
        saveTimer = null;
      }
      writing = writing.then(async () => {
        if (!dirty || !state.persist || state.view !== "list") return;
        dirty = false;
        lastWrite = now();
        try {
          await hangar.call("data.write", { path: logic.dataPath(state.path), text: logic.writeSaved(state.saved) });
        } catch (error) {
          dirty = true;
          if (error && error.code === "rate_limited") {
            if (saveTimer === null) {
              saveTimer = setTimer(() => {
                saveTimer = null;
                flush();
              }, RETRY_MS);
            }
          } else {
            dirty = false;
            state.persist = false;
            emit();
          }
        }
      });
      return writing;
    }

    // --- loading ----------------------------------------------------------------

    function fail(kind, extra) {
      Object.assign(state, { view: "error", list: null, keys: [], counts: null, error: { kind, ...extra } });
      setBadge(null);
      emit();
    }

    async function load(message) {
      const token = ++generation;
      lastMessage = message || lastMessage;
      await flush();
      const params = (message && message.params) || {};
      Object.assign(state, {
        view: "loading",
        caller: message && message.caller && typeof message.caller.pane === "string" ? { pane: message.caller.pane } : null,
        send: { status: "idle", pane: null, error: null },
        persist: true,
        error: null,
      });
      dirty = false;
      emit();

      const read = logic.readPath(params);
      if (!read.ok) return token === generation && fail(read.kind);
      state.path = read.path;

      let text;
      try {
        text = await hangar.call("files.read", { path: read.path });
      } catch (error) {
        if (token !== generation) return;
        const kind = logic.classify(error);
        return fail(kind, { path: read.path, detail: String((error && error.message) || "").slice(0, 300) });
      }
      if (token !== generation || disposed) return;

      const parsed = logic.parseList(text, read.path);
      if (!parsed.ok) return fail(parsed.kind, { path: read.path, detail: parsed.detail, code: parsed.code, vars: parsed.vars });

      const keys = logic.stepKeys(parsed.list.steps);
      let saved = { answers: {}, redo: {}, sentAt: null };
      try {
        const answer = await hangar.call("data.read", { path: logic.dataPath(read.path) });
        saved = logic.readSaved(answer && answer.text, keys);
      } catch (error) {
        if (error && error.code !== "not_found") state.persist = false; // denied, or a folder that fails: work on, unsaved
      }
      if (token !== generation || disposed) return;

      Object.assign(state, { view: "list", list: parsed.list, keys, saved, error: null });
      recount();
      if (logic.markFailedForRedo(state.saved)) {
        recount();
        schedule();
      }
      await setTitle(logic.tabTitle(parsed.list) || t("tab"));
      await syncBadge();
      if (token !== generation || disposed) return;
      emit();
    }

    // --- answering ----------------------------------------------------------------

    function changed() {
      // An answer after the result went out is a result not yet sent.
      if (state.send.status === "sent" || state.send.status === "error") state.send = { status: "idle", pane: null, error: null };
      if (state.saved.sentAt !== null) state.saved.sentAt = null;
      schedule();
      syncBadge();
      emit();
    }

    function answer(index, status) {
      if (state.view !== "list" || !state.keys[index] || !["ok", "fail", "skip"].includes(status)) return;
      const key = state.keys[index];
      const previous = state.saved.answers[key];
      state.saved.answers[key] = status === "fail" ? { status, note: previous && previous.status === "fail" ? previous.note : "" } : { status };
      delete state.saved.redo[key];
      changed();
    }

    /** Back to « to do »: the answer, and the reminder of an earlier failure, are gone. */
    function clear(index) {
      if (state.view !== "list" || !state.keys[index]) return;
      const key = state.keys[index];
      delete state.saved.answers[key];
      delete state.saved.redo[key];
      changed();
    }

    /** The note of a failed step, typed: saved, but nothing redrawn under the cursor. */
    function setNote(index, note) {
      const key = state.keys[index];
      const entry = state.saved.answers[key];
      if (state.view !== "list" || !entry || entry.status !== "fail") return;
      entry.note = String(note).slice(0, 2000);
      if (state.saved.sentAt !== null) state.saved.sentAt = null;
      if (state.send.status === "sent") {
        state.send = { status: "idle", pane: null, error: null };
        emit();
      }
      schedule();
    }

    // --- sending ------------------------------------------------------------------

    const summary = () => (state.view === "list" ? logic.buildSummary(state.list, state.keys, state.saved, state.path) : "");

    async function send() {
      if (state.view !== "list" || !state.caller || state.send.status === "sending") return false;
      if (!state.counts.answered) return false;
      const pane = state.caller.pane;
      const text = summary();
      state.send = { status: "sending", pane, error: null };
      emit();
      try {
        await hangar.call("panes.send", { pane, text });
      } catch (error) {
        const code = error && ["denied", "not_found", "unavailable", "rate_limited"].includes(error.code) ? error.code : "error";
        state.send = { status: "error", pane, error: { code, detail: String((error && error.message) || "").slice(0, 300) } };
        emit();
        return false;
      }
      state.saved.sentAt = now();
      state.send = { status: "sent", pane, error: null };
      dirty = true;
      emit();
      try {
        await hangar.call("notify", { title: t("notifySent", { pane }), body: undefined });
      } catch {
        /* not granted, or over the rate limit: the page says it too */
      }
      flush();
      return true;
    }

    return {
      state,
      /** Reads the tab's params, and again whenever an agent asks for the tab once more. */
      async start() {
        if (typeof hangar.on === "function") offParams = hangar.on("tab.params", (data) => load(data));
        let message = { params: {}, caller: null };
        try {
          message = await hangar.call("tab.params");
        } catch {
          /* no params: nothing to show */
        }
        return load(message);
      },
      answer,
      clear,
      setNote,
      send,
      summary,
      flush,
      /** Tries again what failed: the same params, read once more. */
      retry: () => load(lastMessage),
      dispose() {
        disposed = true;
        generation++;
        if (saveTimer !== null) clearTimer(saveTimer);
        saveTimer = null;
        if (typeof offParams === "function") offParams();
      },
    };
  }

  return { createController, SAVE_DELAY_MS, SAVE_SPACING_MS, RETRY_MS };
});
