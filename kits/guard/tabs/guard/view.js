"use strict";
// Draws the controller's state. Only text nodes and elements made by hand: a command
// an agent wrote is never parsed as markup.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.GuardView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  function make(doc, tag, className, text) {
    const element = doc.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined && text !== null) element.textContent = text;
    return element;
  }

  /** Renders `state` into `container`.
   *  @param {{ doc, logic, lang, now: () => number, actions: { allow(key), clear(), retry() } }} ctx */
  function render(container, state, ctx) {
    const { doc, logic, lang, actions } = ctx;
    const t = (key, vars) => logic.t(lang, key, vars);
    const now = ctx.now();

    function button(label, handler) {
      const element = make(doc, "button", "", label);
      element.type = "button";
      element.onclick = handler;
      return element;
    }

    function limits() {
      const section = make(doc, "section", "limits");
      section.append(make(doc, "h2", "", t("limitsHeading")), make(doc, "p", "", t("limits")));
      return section;
    }

    function passLine(row) {
      const pass = logic.passState(row, state.grants[row.key], now);
      const box = make(doc, "div", "pass");
      if (pass.state === "used") {
        box.append(make(doc, "p", "pass-used", t("passUsed", { when: logic.relativeTime(pass.at, now, lang) })));
        return box;
      }
      if (pass.state === "active") {
        box.setAttribute("role", "status");
        box.append(make(doc, "p", "pass-active", t("passActive", { remaining: logic.formatRemaining(pass.remainingMs), agent: row.agent })));
        return box;
      }
      if (pass.state === "expired") box.append(make(doc, "p", "pass-expired", t("passExpired")));
      const problem = state.problems[row.key];
      if (problem) {
        const message = make(doc, "p", "pass-problem", t(`problem.${problem.code}`, { detail: problem.detail }));
        message.setAttribute("role", "alert");
        box.append(message);
      }
      const allow = button(state.pending[row.key] ? t("allowing") : t("allow"), () => actions.allow(row.key));
      if (state.pending[row.key]) allow.disabled = true;
      box.append(allow);
      return box;
    }

    function refusal(row) {
      const item = make(doc, "li", "refusal");
      const facts = make(doc, "p", "refusal-facts");
      facts.append(make(doc, "strong", "", row.agent));
      const when = make(doc, "time", "", logic.relativeTime(row.at, now, lang));
      when.setAttribute("title", new Date(row.at).toLocaleString(lang));
      facts.append(" · ", when);
      const folder = logic.folderName(row.folder);
      if (folder) facts.append(" · ", make(doc, "span", "", folder));
      if (row.count > 1) facts.append(" · ", make(doc, "span", "", t("refusedTimes", { count: row.count })));
      item.append(facts);
      const pre = make(doc, "pre", "refusal-command");
      pre.append(make(doc, "code", "", row.command));
      item.append(pre);
      if (row.length > row.command.length) item.append(make(doc, "p", "refusal-truncated", t("truncated", { length: row.length })));
      item.append(make(doc, "p", "refusal-why", logic.whyFor(row, lang)), passLine(row));
      return item;
    }

    const page = make(doc, "main", `view-${state.view}`);

    if (state.view === "loading") {
      page.append(make(doc, "p", "", t("loading")));
    } else if (state.view === "denied" || state.view === "error") {
      const box = make(doc, "section", "problem");
      box.setAttribute("role", "alert");
      box.append(make(doc, "h2", "", t(`${state.view}.title`)));
      if (state.view === "denied") box.append(make(doc, "p", "", t("denied.hint")));
      else if (state.detail) box.append(make(doc, "pre", "", state.detail));
      box.append(button(t("retry"), actions.retry));
      page.append(box, limits());
    } else {
      const head = make(doc, "header", "bar");
      head.append(make(doc, "h1", "", t("heading")));
      if (state.rows.length) head.append(button(t("clear"), actions.clear));
      page.append(head);
      if (state.stale) page.append(make(doc, "p", "warning", t("staleWarning")));
      if (state.clearFailed) {
        const message = make(doc, "p", "warning", t("clearFailed"));
        message.setAttribute("role", "alert");
        page.append(message);
      }
      if (state.errors) {
        const box = make(doc, "section", "guard-errors");
        box.setAttribute("role", "alert");
        const detail = make(doc, "pre");
        detail.append(make(doc, "code", "", state.errors.last));
        box.append(make(doc, "h2", "", t("errorsHeading")), make(doc, "p", "", t("errorsBody", { count: state.errors.count })), detail);
        page.append(box);
      }
      if (!state.rows.length) {
        page.append(make(doc, "p", "empty", t("empty")));
      } else {
        const list = make(doc, "ul", "refusals");
        for (const row of state.rows) list.append(refusal(row));
        page.append(list);
      }
      page.append(limits());
    }

    container.replaceChildren(page);
  }

  return { render };
});
