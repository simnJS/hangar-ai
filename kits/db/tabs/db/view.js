"use strict";
// Draws the controller's state with Hangar UI's components (docs/kit-ui.md), and no colour of its
// own. Only text nodes and elements made by hand: a cell's value is never parsed as markup.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.DbView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  function make(doc, tag, className, text) {
    const element = doc.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined && text !== null) element.textContent = text;
    return element;
  }

  /**
   * Renders `state` into `container`.
   * @param {{ doc, logic, lang, actions, t?: (key, vars) => string, format?: { bytes(size) } }} ctx
   *   `actions`: refresh(), pick(path), selectTable(name), setPanel(id), sortBy(index), goTo(offset), toggleFull(row, column),
   *   choose(agent), openHandOver(), closeHandOver(), setQuestion(text), sendHandOver().
   *   `t` and `format` are hangar.ui's in the page.
   */
  function render(container, state, ctx) {
    const { doc, logic, lang, actions } = ctx;
    const t = ctx.t || ((key, vars) => logic.t(lang, key, vars));
    const bytes = (size) => (ctx.format ? ctx.format.bytes(size) : `${size} B`);
    const number = (value) => (ctx.format ? ctx.format.number(value) : String(value));

    const button = (label, handler, className = "kit-btn--small") => {
      const element = make(doc, "button", `kit-btn ${className}`, label);
      element.type = "button";
      element.onclick = handler;
      return element;
    };

    function empty({ icon, title, text, loading, action }) {
      const box = make(doc, "div", "kit-empty");
      if (loading) {
        box.append(make(doc, "span", "kit-state kit-state--running", loading));
        return box;
      }
      if (icon) box.append(make(doc, "span", `kit-icon kit-icon--${icon} kit-empty__icon`));
      box.append(make(doc, "p", "kit-empty__title", title));
      if (text) box.append(make(doc, "p", "kit-empty__text", text));
      if (action) {
        const actions_ = make(doc, "div", "kit-empty__actions");
        actions_.append(action);
        box.append(actions_);
      }
      return box;
    }

    function banner(kind, message, role) {
      const box = make(doc, "p", `kit-banner${kind ? ` kit-banner--${kind}` : ""}`);
      if (role) box.setAttribute("role", role);
      box.append(make(doc, "span", "kit-banner__text", message));
      return box;
    }

    function section(title, count) {
      const box = make(doc, "section", "kit-section");
      const head = make(doc, "div", "kit-section__head");
      head.append(make(doc, "h2", "kit-section__title", title));
      if (count !== undefined && count !== null) head.append(make(doc, "span", "kit-section__count", number(count)));
      box.append(head);
      return { box, head };
    }

    const retry = () => button(t("retry"), () => actions.refresh());

    function header() {
      const head = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      main.append(make(doc, "h1", "kit-header__title", t("tab")));
      if (state.path && state.db && state.db.phase === "ready") {
        const line = make(doc, "p", "kit-header__state");
        line.append(make(doc, "span", "kit-state kit-state--neutral", t("readOnly")));
        line.append(` ${bytes(state.db.size)}`);
        main.append(line);
      }
      head.append(main);
      const side = make(doc, "div", "kit-header__actions");
      if (state.view === "ready" || state.view === "error") side.append(button(t("refresh"), () => actions.refresh()));
      head.append(side);
      return head;
    }

    function picker() {
      const box = make(doc, "div", "kit-field");
      box.append(make(doc, "span", "kit-field__label", t("databaseLabel")));
      if (state.files.length > 1) {
        const wrap = make(doc, "span", "kit-select");
        const select = make(doc, "select");
        select.setAttribute("aria-label", t("databaseLabel"));
        for (const file of state.files) {
          const option = make(doc, "option", "", file.usable ? file.path : `${file.path} — ${t("unusable")}`);
          option.value = file.path;
          option.disabled = !file.usable;
          if (file.path === state.path) option.selected = true;
          select.append(option);
        }
        select.onchange = () => actions.pick(select.value);
        wrap.append(select);
        box.append(wrap);
      } else {
        const only = state.files[0];
        box.append(make(doc, "span", "kit-mono kit-wrap", only.path));
        if (!only.usable) box.append(make(doc, "span", "kit-field__hint", t("unusable")));
      }
      return box;
    }

    function databaseProblem(db) {
      const code = db.error && db.error.code;
      if (db.phase === "locked") return empty({ icon: "lock", title: t("lockedTitle"), text: t("lockedText"), action: retry() });
      if (db.phase === "not-database") return empty({ icon: "alert", title: t("notDatabaseTitle"), text: t("notDatabaseText") });
      if (db.phase === "gone") return empty({ icon: "alert", title: t("goneTitle"), text: t("goneText"), action: retry() });
      return empty({ icon: "alert", title: t("errorTitle"), text: errorText(code, db.error && db.error.detail), action: retry() });
    }

    function errorText(code, detail) {
      const key = `error.${code}`;
      const text = t(key, { detail: detail || code || "" });
      return text === key ? t("error.failed", { detail: detail || code || "" }) : text;
    }

    function tablesSection(db) {
      const { box } = section(t("tables"), db.tables.length);
      if (!db.tables.length) {
        box.append(make(doc, "p", "kit-dim", t("noTables")));
        return box;
      }
      const rows = make(doc, "ul", "kit-rows");
      for (const item of db.tables) {
        const li = make(doc, "li");
        const row = make(doc, item.viewable ? "button" : "div", `kit-row${item.name === state.table ? " is-selected" : ""}`);
        if (item.viewable) {
          row.type = "button";
          row.onclick = () => actions.selectTable(item.name);
          if (item.name === state.table) row.setAttribute("aria-current", "true");
        }
        const main = make(doc, "span", "kit-row__main");
        main.append(make(doc, "span", "kit-row__title", item.name));
        main.children[0].setAttribute("title", item.name);
        const sub = [t("columnsCount", { n: item.columns })];
        if (item.type === "view") sub.unshift(t("view"));
        if (!item.viewable) sub.push(t("unsupportedName"));
        main.append(make(doc, "span", "kit-row__sub", sub.join(" · ")));
        row.append(main);
        row.append(make(doc, "span", "kit-row__end", t("rowsCount", { n: item.rows })));
        li.append(row);
        rows.append(li);
      }
      box.append(rows);
      return box;
    }

    function structurePanel(schema) {
      const box = make(doc, "div", "db-panel");
      const rows = make(doc, "ul", "kit-rows kit-rows--group");
      for (const column of schema.columns) {
        const li = make(doc, "li", "kit-row");
        const main = make(doc, "span", "kit-row__main");
        main.append(make(doc, "span", "kit-row__title kit-mono", column.name));
        const bits = [column.type || "—"];
        if (column.primaryKey) bits.push(t("key"));
        if (column.notNull) bits.push(t("required"));
        if (column.default !== null) bits.push(t("defaultValue", { value: column.default }));
        main.append(make(doc, "span", "kit-row__sub", bits.join(" · ")));
        li.append(main);
        rows.append(li);
      }
      box.append(rows);
      if (schema.columnsTruncated) box.append(make(doc, "p", "kit-dim kit-small", t("columnsCut")));
      if (schema.indexes.length) {
        const chips = make(doc, "div", "kit-chips");
        for (const index of schema.indexes) chips.append(make(doc, "span", "kit-chip", index.unique ? `${index.name} · ${t("unique")}` : index.name));
        box.append(make(doc, "p", "kit-dim kit-small", t("indexes")), chips);
      }
      return box;
    }

    function cellNode(cell, row, column) {
      const td = make(doc, "td", "db-cell");
      if (cell.type === "null") {
        td.className = "db-cell db-cell--dim";
        td.textContent = t("null");
      } else if (cell.type === "blob") {
        td.className = "db-cell db-cell--dim";
        td.textContent = t("blob", { n: cell.length });
      } else if (cell.type === "integer" || cell.type === "real") {
        td.className = "db-cell db-cell--num";
        td.textContent = cell.value;
      } else {
        const full = state.full[`${row}:${column}`];
        const shown = full && full.phase === "done" ? full.value : cell.value;
        td.setAttribute("title", cell.length > 200 ? shown.slice(0, 200) : shown);
        if (full && full.phase === "done") td.className = "db-cell db-cell--open";
        const isOpen = Boolean(full && full.phase === "done");
        const textNode = make(doc, "span", "db-cell__text", cell.cut && !isOpen ? `${shown}…` : shown);
        if (isOpen) textNode.setAttribute("tabindex", "0");
        if (cell.cut || isOpen) {
          const open = full && full.phase !== "failed";
          let label = open ? t("showLess") : t("showFull", { n: number(cell.length) });
          if (full && full.phase === "loading") label = t("loadingFull");
          const more = button(label, () => actions.toggleFull(row, column), "kit-btn--small kit-btn--ghost db-more");
          if (full && full.phase === "loading") more.disabled = true;
          if (isOpen) td.append(more, textNode);
          else td.append(textNode, more);
          if (full && full.phase === "failed") td.append(make(doc, "span", "kit-field__error", t("fullFailed")));
          if (isOpen && full.cut) td.append(make(doc, "span", "kit-dim kit-small", t("fullCut", { n: number(logic.FULL_CHARS) })));
        } else td.append(textNode);
      }
      return td;
    }

    function dataTable(page, schema) {
      const scroller = make(doc, "div", "db-scroll");
      scroller.setAttribute("tabindex", "0");
      scroller.setAttribute("role", "region");
      scroller.setAttribute("aria-label", t("rowsTitle"));
      const table = make(doc, "table", "db-table");
      const head = make(doc, "tr");
      page.columns.forEach((column, index) => {
        const th = make(doc, "th", /INT|REAL|NUM|FLOA|DOUB|DEC/i.test(column.type) ? "db-th--num" : "");
        th.setAttribute("scope", "col");
        const active = page.sort === index + 1;
        th.setAttribute("aria-sort", active ? (page.direction === "desc" ? "descending" : "ascending") : "none");
        const sort = make(doc, "button", "db-sort");
        sort.type = "button";
        sort.setAttribute("title", t("sortBy", { column: column.name }));
        sort.setAttribute("aria-label", t("sortBy", { column: column.name }));
        sort.append(make(doc, "span", "db-sort__name", column.name));
        if (active) sort.append(make(doc, "span", `kit-icon kit-icon--chevron-down db-sort__arrow${page.direction === "asc" ? " db-sort__arrow--up" : ""}`));
        sort.onclick = () => actions.sortBy(index + 1);
        th.append(sort);
        head.append(th);
      });
      const thead = make(doc, "thead");
      thead.append(head);
      table.append(thead);
      const body = make(doc, "tbody");
      page.rows.forEach((row, r) => {
        const tr = make(doc, "tr");
        row.forEach((cell, c) => tr.append(cellNode(cell, r, c)));
        body.append(tr);
      });
      table.append(body);
      scroller.append(table);
      return scroller;
    }

    function pager(page) {
      const bar = make(doc, "div", "kit-bar kit-bar--foot");
      const from = page.rows.length ? page.offset + 1 : 0;
      const to = page.offset + page.rows.length;
      const prev = button(t("previous"), () => actions.goTo(Math.max(0, page.offset - logic.PAGE_SIZE)));
      prev.disabled = page.offset === 0;
      const next = button(t("next"), () => actions.goTo(page.offset + logic.PAGE_SIZE));
      next.disabled = to >= page.total;
      const range = make(doc, "span", "kit-num kit-grow", t("range", { from: number(from), to: number(to), total: number(page.total) }));
      bar.append(prev, range, next);
      return bar;
    }

    function handOverPanel() {
      const handOver = state.handOver;
      const box = make(doc, "div", "db-handover");
      const target = state.choice || logic.defaultTarget(state.panes);
      if (!handOver.open) {
        if (handOver.sent) box.append(banner("", t("sentTo", { agent: handOver.sent.agent }), "status"));
        return box;
      }
      box.append(make(doc, "h3", "kit-section__title", t("handOverTitle")));
      const field = make(doc, "label", "kit-field");
      field.append(make(doc, "span", "kit-field__label", t("questionLabel")));
      const input = make(doc, "textarea", "kit-input");
      input.setAttribute("rows", "3");
      input.setAttribute("placeholder", t("questionPlaceholder"));
      input.value = handOver.question;
      input.oninput = () => {
        actions.setQuestion(input.value);
        send.disabled = !input.value.trim() || !target || handOver.sending;
      };
      field.append(input, make(doc, "span", "kit-field__hint", t("questionHint")));
      box.append(field);
      const reachable = logic.candidates(state.panes);
      if (state.panesDenied) box.append(banner("warning", t("panesDenied")));
      else if (!reachable.length) box.append(banner("warning", t("noAgent")));
      else if (reachable.length > 1) {
        const agentField = make(doc, "div", "kit-field");
        agentField.append(make(doc, "span", "kit-field__label", t("agentLabel")));
        const wrap = make(doc, "span", "kit-select");
        const select = make(doc, "select");
        select.setAttribute("aria-label", t("agentLabel"));
        for (const pane of reachable) {
          const option = make(doc, "option", "", pane.name);
          option.value = pane.name;
          if (pane.name === target) option.selected = true;
          select.append(option);
        }
        select.onchange = () => actions.choose(select.value);
        wrap.append(select);
        agentField.append(wrap);
        box.append(agentField);
      }
      if (handOver.problem) box.append(banner("error", t(`problem.${handOver.problem.code}`, { agent: handOver.problem.agent, detail: handOver.problem.detail }), "alert"));
      const row = make(doc, "div", "kit-actions");
      const send = button(handOver.sending ? t("sending") : t("send"), () => actions.sendHandOver(), "kit-btn--small kit-btn--primary");
      send.disabled = !handOver.question.trim() || !target || handOver.sending;
      row.append(send, button(t("cancel"), () => actions.closeHandOver(), "kit-btn--small kit-btn--ghost"));
      box.append(row);
      return box;
    }

    function rowsPanel(detail) {
      const box = make(doc, "div", "db-panel");
      if (detail.phase === "loading" && !detail.page) {
        box.append(empty({ loading: t("loading") }));
        return box;
      }
      if (detail.phase === "error") {
        box.append(empty({ icon: "alert", title: t("errorTitle"), text: errorText(detail.error.code, detail.error.detail), action: retry() }));
        return box;
      }
      const page = detail.page;
      if (!page.rows.length) {
        box.append(make(doc, "p", "kit-dim", t("emptyTable")));
        return box;
      }
      if (page.capped) box.append(banner("warning", t("capped")));
      box.append(dataTable(page, detail.schema), pager(page));
      return box;
    }

    function tableBlock(detail) {
      const { box, head } = section(state.table);
      head.children[0].className = "kit-section__title db-name";
      const side = make(doc, "div", "kit-section__actions");
      const hand = button(t("handOver"), () => actions.openHandOver(), "kit-btn--small");
      hand.disabled = !detail.schema || state.handOver.open;
      side.append(hand);
      head.append(side);
      const list = make(doc, "div", "kit-tabs");
      list.setAttribute("role", "tablist");
      list.setAttribute("aria-label", state.table);
      const shown = state.panel === "structure" ? "structure" : "rows";
      for (const [id, label] of [["rows", t("rowsTitle")], ["structure", t("structure")]]) {
        const tab = make(doc, "button", "kit-tab", label);
        tab.type = "button";
        tab.setAttribute("role", "tab");
        tab.setAttribute("aria-selected", id === shown ? "true" : "false");
        tab.onclick = () => actions.setPanel(id);
        list.append(tab);
      }
      box.append(handOverPanel(), list);
      if (shown === "structure") box.append(detail.schema ? structurePanel(detail.schema) : empty({ loading: t("loading") }));
      else box.append(rowsPanel(detail));
      return box;
    }

    const page = make(doc, "main", "kit-page");
    page.append(header());
    if (state.view === "loading") page.append(empty({ loading: t("loading") }));
    else if (state.view === "denied") page.append(empty({ icon: "lock", title: t("deniedTitle"), text: t("deniedText"), action: retry() }));
    else if (state.view === "nosqlite") page.append(empty({ icon: "alert", title: t("noSqliteTitle"), text: t("noSqliteText"), action: retry() }));
    else if (state.view === "error") page.append(empty({ icon: "alert", title: t("errorTitle"), text: errorText(state.error.code, state.error.detail), action: retry() }));
    else if (!state.files.length) page.append(empty({ icon: "info", title: t("noDbTitle"), text: t("noDbText"), action: button(t("refresh"), () => actions.refresh()) }));
    else {
      page.append(picker());
      const db = state.db;
      if (!db || db.phase === "loading") page.append(empty({ loading: t("loading") }));
      else if (db.phase !== "ready") page.append(databaseProblem(db));
      else {
        page.append(banner("", t("secretNote")));
        page.append(tablesSection(db));
        if (state.table && state.detail) {
          page.append(tableBlock(state.detail));
        }
      }
    }
    container.replaceChildren(page);
  }

  return { render };
});
