"use strict";
// The tab's behaviour, apart from any DOM: shows the last scan from the kit's data folder, scans
// again when the tab opens and when asked, keeps the badge, lists the agents, opens a file in the
// preview, reads who wrote a file's markers, and hands one marker to an agent. It reports through
// `onChange(state)` when what is shown changes and `onTick(state)` when only the time has passed.
// `hangar` is `window.hangar`, or a fake of it.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.TodoController = api;
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
      view: "loading", // loading | ready | denied
      scanning: false,
      scan: null, // the last scan, checked
      error: null, // { code, detail } when a run did not complete
      panes: [],
      panesDenied: false,
      choice: null, // the agent the person picked
      query: "",
      tags: [], // the tags kept; none means all
      by: "file", // file | tag
      limit: logic.SHOWN_STEP, // rows drawn, grown by "show more"
      pending: {}, // item key → true, while it is being sent
      sent: {}, // item key → { agent }
      problems: {}, // item key → { code, detail, agent }
      blame: {}, // file → { loading } | { denied } | { status, byLine }
    };

    let lastBadge;
    let lastDrawn = null;
    let timer = null;
    let disposed = false;
    let unsubscribe = null;

    const drawn = () => JSON.stringify([state, state.scan && state.scan.at]);

    function update() {
      if (disposed) return;
      const next = drawn();
      if (next === lastDrawn) onTick(state);
      else {
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

    const badgeNow = () => (state.scanning ? { state: "running" } : logic.badgeFor(state.scan, lang));

    async function loadLast() {
      try {
        const { text } = await hangar.call("data.read", { path: logic.LAST });
        state.scan = logic.parseScan(text);
      } catch {
        state.scan = null; // nothing scanned yet, or the data folder is not ours to read: the page still works
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

    const find = (key) => (state.scan ? state.scan.items.find((item) => item.key === key) : undefined);

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
        // Reading files of the workspace is local and quick: the list is current when the tab opens.
        await this.run();
      },
      /** Hangar's language changed: the strip's title and badge follow. */
      setLang(next) {
        if (next === lang) return undefined;
        lang = next;
        lastBadge = undefined;
        return Promise.all([setTitle(), setBadge(badgeNow())]);
      },
      /** Scans the workspace's files. */
      async run() {
        if (state.scanning || disposed) return;
        state.scanning = true;
        state.error = null;
        if (state.view === "denied") state.view = "ready";
        update();
        await setBadge(badgeNow());
        try {
          const answer = await hangar.call("command.run", { name: "scan", args: { file: "" } });
          if (answer.timedOut) failRun("timeout");
          else {
            const scan = answer.exitCode === 0 ? logic.parseScan(answer.stdout) : null;
            if (!scan) failRun(answer.exitCode === 0 ? "unreadable" : "failed", (answer.stderr || "").split(/\r?\n/).find(Boolean) || `exit code ${answer.exitCode}`);
            else {
              state.scan = scan;
              state.sent = {};
              state.problems = {};
              state.blame = {};
              state.limit = logic.SHOWN_STEP;
              try {
                await hangar.call("data.write", { path: logic.LAST, text: JSON.stringify(scan) });
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
        state.scanning = false;
        await setBadge(badgeNow());
        update();
      },
      choose(agent) {
        state.choice = agent;
        update();
      },
      setQuery(query) {
        state.query = String(query).slice(0, 200);
        state.limit = logic.SHOWN_STEP;
        update();
      },
      toggleTag(tag) {
        state.tags = state.tags.includes(tag) ? state.tags.filter((entry) => entry !== tag) : [...state.tags, tag];
        state.limit = logic.SHOWN_STEP;
        update();
      },
      clearFilters() {
        state.query = "";
        state.tags = [];
        state.limit = logic.SHOWN_STEP;
        update();
      },
      groupBy(by) {
        state.by = by === "tag" ? "tag" : "file";
        state.limit = logic.SHOWN_STEP;
        update();
      },
      showMore() {
        state.limit += logic.SHOWN_STEP;
        update();
      },
      /** Opens a file in Hangar's preview. */
      async open(key) {
        const item = find(key);
        if (!item || !logic.canOpen(item.file)) return;
        delete state.problems[key];
        try {
          await hangar.call("preview.show", { path: item.file });
        } catch (error) {
          const code = error && error.code;
          state.problems[key] = { code: code === "denied" ? "denied" : "open", detail: String((error && error.message) || "").slice(0, 200) };
        }
        update();
      },
      /** Reads, with git, who wrote the markers of one file and when. */
      async blame(file) {
        if (!state.scan || (state.blame[file] && state.blame[file].loading)) return;
        state.blame[file] = { loading: true };
        update();
        try {
          const answer = await hangar.call("command.run", { name: "scan", args: { file: file.startsWith("-") ? `./${file}` : file } });
          const blame = answer.exitCode === 0 && !answer.timedOut ? logic.parseBlame(answer.stdout) : null;
          state.blame[file] = blame ? { status: blame.status, byLine: blame.byLine } : { status: "error", byLine: {} };
        } catch (error) {
          state.blame[file] = error && error.code === "denied" ? { denied: true } : { status: "error", byLine: {} };
        }
        update();
      },
      /** Hands one marker (`key` in the scan) to the chosen agent. */
      async handOver(key) {
        const agent = state.choice || logic.defaultTarget(state.panes);
        const item = find(key);
        if (!item || !agent || state.pending[key]) return;
        state.pending[key] = true;
        delete state.problems[key];
        update();
        try {
          await hangar.call("panes.send", { pane: agent, text: logic.briefFor(item) });
          state.sent[key] = { agent };
        } catch (error) {
          const code = error && error.code;
          state.problems[key] = {
            code: ["denied", "not_found", "rate_limited", "unavailable"].includes(code) ? code : "failed",
            detail: String((error && error.message) || "").slice(0, 200),
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
