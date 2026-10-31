"use strict";
// The tab's behaviour, apart from any DOM: finds the databases, opens one read-only through the
// kit's declared commands, follows the table, sort and page the person picks, and hands a table
// and a question (never a row) to an agent when asked. It reports through `onChange(state)`.
// `hangar` is `window.hangar`, or a fake of it.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.DbController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  function createController(options) {
    const { hangar, logic } = options;
    let lang = options.lang || "en";
    const onChange = options.onChange || (() => undefined);

    const state = {
      view: "loading", // loading | ready | denied | nosqlite | error
      error: null, // { code, detail } when the databases could not be listed
      files: [],
      path: null, // the database being looked at
      db: null, // { phase: loading | ready | locked | not-database | gone | error, error, size, tables }
      table: null, // the table being looked at
      detail: null, // { phase: loading | ready | error, error, schema, page }
      sort: 0,
      direction: "asc",
      offset: 0, // the first row of the page
      panel: "rows", // rows | structure
      full: {}, // "row:column" → { phase: loading | done | failed, value, cut }
      panes: [],
      panesDenied: false,
      choice: null, // the agent the person picked
      handOver: { open: false, question: "", sending: false, sent: null, problem: null },
    };

    let disposed = false;
    let dbToken = 0; // a newer request makes an older answer obsolete
    let detailToken = 0;
    let unsubscribe = [];

    const update = () => {
      if (!disposed) onChange(state);
    };

    async function setTitle() {
      try {
        await hangar.call("tab.setTitle", { title: logic.t(lang, "tab") });
      } catch {
        /* the manifest's title stays */
      }
    }

    /** Runs one declared command and answers its document, or `{ ok: false, error }` with the kit's codes plus denied, node-missing, busy, timeout, unreadable. */
    async function exec(name, args) {
      try {
        const answer = await hangar.call("command.run", args ? { name, args } : { name });
        if (answer.timedOut) return { ok: false, error: "timeout" };
        const parsed = answer.exitCode === 0 ? logic.parseAnswer(answer.stdout) : null;
        if (parsed) return parsed;
        return { ok: false, error: answer.exitCode === 0 ? "unreadable" : "failed", detail: (answer.stderr || "").split(/\r?\n/).find(Boolean) || `exit code ${answer.exitCode}` };
      } catch (error) {
        const code = error && error.code;
        if (code === "denied") return { ok: false, error: "denied" };
        if (code === "not_found") return { ok: false, error: "node-missing" };
        if (code === "busy") return { ok: false, error: "busy" };
        return { ok: false, error: "failed", detail: String((error && error.message) || "").slice(0, 200) };
      }
    }

    /** A failure that changes the whole page: the commands are not allowed, or nothing can read SQLite. */
    function fatal(failure) {
      if (failure.error === "denied") state.view = "denied";
      else if (failure.error === "no-sqlite") state.view = "nosqlite";
      else return false;
      return true;
    }

    const failureOf = (failure) => ({ code: failure.error, detail: failure.detail || "" });

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

    async function remember(path) {
      try {
        await hangar.call("data.write", { path: logic.STATE_FILE, text: JSON.stringify({ path }) });
      } catch {
        /* the next opening starts from the list */
      }
    }

    async function rememberedPath() {
      try {
        const { text } = await hangar.call("data.read", { path: logic.STATE_FILE });
        const path = JSON.parse(text).path;
        return typeof path === "string" && logic.PATH_RE.test(path) ? path : null;
      } catch {
        return null;
      }
    }

    async function paramsPath() {
      try {
        const answer = await hangar.call("tab.params");
        const path = answer && answer.params && answer.params.path;
        return typeof path === "string" && logic.PATH_RE.test(path) ? path : null;
      } catch {
        return null;
      }
    }

    async function loadFiles(preferred) {
      const answer = await exec("list");
      if (!answer.ok) {
        if (!fatal(answer)) {
          state.view = "error";
          state.error = failureOf(answer);
        }
        return null;
      }
      state.view = "ready";
      state.error = null;
      state.files = logic.cleanFiles(answer);
      return preferred;
    }

    function resetTable() {
      state.table = null;
      state.detail = null;
      state.sort = 0;
      state.direction = "asc";
      state.offset = 0;
      state.full = {};
      state.handOver = { ...state.handOver, open: false, sent: null, problem: null };
    }

    async function openDatabase(path, { save = false } = {}) {
      const token = ++dbToken;
      detailToken++;
      state.path = path;
      resetTable();
      state.db = path ? { phase: "loading", error: null, size: 0, tables: [] } : null;
      update();
      if (!path) return;
      if (save) remember(path);
      const answer = await exec("tables", { path });
      if (token !== dbToken || disposed) return;
      if (answer.ok) {
        const clean = logic.cleanTables(answer);
        state.db = { phase: "ready", error: null, size: clean.size, tables: clean.tables };
        const first = clean.tables.find((item) => item.viewable);
        update();
        if (first && clean.tables.length === 1) await selectTable(first.name);
        return;
      }
      if (fatal(answer)) {
        state.db = null;
      } else if (["locked", "not-database"].includes(answer.error)) {
        state.db = { phase: answer.error, error: failureOf(answer), size: 0, tables: [] };
      } else if (answer.error === "not-found") {
        state.db = { phase: "gone", error: failureOf(answer), size: 0, tables: [] };
      } else {
        state.db = { phase: "error", error: failureOf(answer), size: 0, tables: [] };
      }
      update();
    }

    async function loadDetail({ withSchema }) {
      const token = ++detailToken;
      const path = state.path;
      const table = state.table;
      const previous = state.detail;
      state.detail = { phase: "loading", error: null, schema: withSchema ? null : previous && previous.schema, page: previous && !withSchema ? previous.page : null };
      update();
      const pageArgs = { path, table, sort: String(state.sort), direction: state.direction, offset: String(state.offset || 0), limit: String(logic.PAGE_SIZE), chars: String(logic.CELL_CHARS) };
      const [schema, rows] = await Promise.all([withSchema ? exec("schema", { path, table }) : Promise.resolve(null), exec("rows", pageArgs)]);
      if (token !== detailToken || disposed) return;
      const bad = [schema, rows].find((answer) => answer && !answer.ok);
      if (bad) {
        if (fatal(bad)) state.db = null;
        else state.detail = { phase: "error", error: failureOf(bad), schema: previous && previous.schema, page: null };
      } else {
        state.detail = { phase: "ready", error: null, schema: schema ? logic.cleanSchema(schema) : previous.schema, page: logic.cleanRows(rows) };
      }
      update();
    }

    return {
      state,
      async start() {
        await setTitle();
        const [asked, remembered] = await Promise.all([paramsPath(), rememberedPath(), loadPanes()]);
        const chosen = await loadFiles(asked || remembered);
        if (state.view === "ready") {
          let path = chosen;
          if (path && !state.files.some((file) => file.path === path)) {
            if (asked) state.files = [{ path, size: 0, modified: 0, usable: true }, ...state.files];
            else path = null; // what was remembered is gone
          }
          if (!path) path = (state.files.find((file) => file.usable) || {}).path || null;
          await openDatabase(path);
        } else update();
        if (typeof hangar.on === "function") {
          unsubscribe = [
            hangar.on("panes.changed", loadPanes),
            hangar.on("tab.params", (event) => {
              const path = event && event.data && event.data.params && event.data.params.path;
              if (typeof path === "string" && logic.PATH_RE.test(path)) this.openAsked(path);
            }),
          ];
        }
      },
      /** An agent asked for a database: it joins the list if it is not there, and opens. */
      async openAsked(path) {
        if (!state.files.some((file) => file.path === path)) state.files = [{ path, size: 0, modified: 0, usable: true }, ...state.files];
        await openDatabase(path, { save: true });
      },
      setLang(next) {
        if (next === lang) return undefined;
        lang = next;
        return setTitle();
      },
      /** Lists the databases again and reads the one open again. */
      async refresh() {
        const keep = state.path;
        state.view = state.view === "ready" ? "ready" : "loading";
        update();
        await loadFiles(keep);
        if (state.view !== "ready") return update();
        if (keep && !state.files.some((file) => file.path === keep)) state.files = [{ path: keep, size: 0, modified: 0, usable: true }, ...state.files];
        const path = keep || (state.files.find((file) => file.usable) || {}).path || null;
        const table = state.table;
        await openDatabase(path);
        if (table && state.db && state.db.phase === "ready" && state.db.tables.some((item) => item.name === table && item.viewable)) await this.selectTable(table);
        return undefined;
      },
      pick(path) {
        if (path !== state.path) return openDatabase(path, { save: true });
        return undefined;
      },
      async selectTable(name) {
        await selectTable(name);
      },
      /** Sorts by column `index` (from 1): the first press ascending, the next descending, the next none. */
      sortBy(index) {
        if (state.sort !== index) {
          state.sort = index;
          state.direction = "asc";
        } else if (state.direction === "asc") state.direction = "desc";
        else {
          state.sort = 0;
          state.direction = "asc";
        }
        state.offset = 0;
        state.full = {};
        return loadDetail({ withSchema: false });
      },
      setPanel(id) {
        state.panel = id === "structure" ? "structure" : "rows";
        update();
      },
      goTo(offset) {
        state.offset = Math.max(0, offset);
        state.full = {};
        return loadDetail({ withSchema: false });
      },
      /** The whole value of the cell at `row` and `column` of the page shown, or back to the cut one. */
      async toggleFull(row, column) {
        const key = `${row}:${column}`;
        const page = state.detail && state.detail.page;
        if (!page || state.full[key]) {
          delete state.full[key];
          update();
          return;
        }
        const token = detailToken;
        state.full[key] = { phase: "loading" };
        update();
        const answer = await exec("rows", {
          path: state.path,
          table: state.table,
          sort: String(page.sort),
          direction: page.direction,
          offset: String(page.offset + row),
          limit: "1",
          chars: String(logic.FULL_CHARS),
        });
        if (token !== detailToken || disposed || !state.full[key]) return;
        const cell = answer.ok ? logic.cleanRows(answer).rows[0] && logic.cleanRows(answer).rows[0][column] : null;
        state.full[key] = cell && cell.type === "text" ? { phase: "done", value: cell.value, cut: cell.cut } : { phase: "failed" };
        update();
      },
      choose(agent) {
        state.choice = agent;
        update();
      },
      openHandOver() {
        state.handOver = { ...state.handOver, open: true, sent: null, problem: null };
        update();
      },
      closeHandOver() {
        state.handOver = { ...state.handOver, open: false, problem: null };
        update();
      },
      /** Typing does not redraw: the field keeps its focus. */
      setQuestion(text) {
        state.handOver.question = text;
      },
      /** Sends the table and the question to the chosen agent: no row is read for it. */
      async sendHandOver() {
        const agent = state.choice || logic.defaultTarget(state.panes);
        const detail = state.detail;
        const question = state.handOver.question.trim();
        const meta = state.db && state.db.tables.find((item) => item.name === state.table);
        if (!agent || !question || !detail || !detail.schema || !meta || state.handOver.sending) return;
        state.handOver = { ...state.handOver, sending: true, problem: null };
        update();
        try {
          await hangar.call("panes.send", {
            pane: agent,
            text: logic.briefFor({ path: state.path, table: meta.name, type: meta.type, total: meta.rows, columns: detail.schema.columns, question }),
          });
          state.handOver = { open: false, question: "", sending: false, sent: { agent }, problem: null };
        } catch (error) {
          const code = error && error.code;
          state.handOver = {
            ...state.handOver,
            sending: false,
            problem: { code: ["denied", "not_found", "rate_limited", "unavailable"].includes(code) ? code : "failed", detail: String((error && error.message) || ""), agent },
          };
          if (code === "not_found" || code === "unavailable") await loadPanes();
        }
        update();
      },
      dispose() {
        disposed = true;
        for (const stop of unsubscribe) if (typeof stop === "function") stop();
      },
    };

    async function selectTable(name) {
      const item = state.db && state.db.tables.find((entry) => entry.name === name);
      if (!item || !item.viewable) return;
      state.table = name;
      state.sort = 0;
      state.direction = "asc";
      state.offset = 0;
      state.full = {};
      state.handOver = { ...state.handOver, open: false, sent: null, problem: null };
      await loadDetail({ withSchema: true });
    }
  }

  return { createController };
});
