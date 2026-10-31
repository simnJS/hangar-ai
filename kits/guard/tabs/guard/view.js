"use strict";
// Draws the controller's state with Hangar UI's components (docs/kit-ui.md), and no
// colour of its own. Only text nodes and elements made by hand: a command an agent
// wrote is never parsed as markup.
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

  /**
   * Renders `state` into `container`, and answers the function that brings what depends
   * on the time up to date, touching nothing else.
   * @param {{ doc, logic, lang, now: () => number, actions: { allow(key), clear(), retry() },
   *           t?: (key, vars) => string, format?: { since(at, now), date(at, now) } }} ctx
   *   `t` and `format` are hangar.ui's in the page.
   */
  function render(container, state, ctx) {
    const { doc, logic, lang, actions } = ctx;
    const t = ctx.t || ((key, vars) => logic.t(lang, key, vars));
    const since = (at) => (ctx.format ? ctx.format.since(at, ctx.now()) : logic.relativeTime(at, ctx.now(), lang));
    const timed = []; // [element, () => its text]

    function live(element, text) {
      timed.push([element, text]);
      element.textContent = text();
      return element;
    }

    function button(label, handler, className = "kit-btn--small") {
      const element = make(doc, "button", `kit-btn ${className}`, label);
      element.type = "button";
      element.onclick = handler;
      return element;
    }

    function banner(kind, parts, role) {
      const box = make(doc, "p", `kit-banner${kind ? ` kit-banner--${kind}` : ""}`);
      if (role) box.setAttribute("role", role);
      const text = make(doc, "span", "kit-banner__text");
      text.append(...parts);
      box.append(text);
      return box;
    }

    function limits() {
      return banner("", [make(doc, "strong", "", t("limitsHeading")), " ", t("limits")]);
    }

    function empty(icon, title, text, actionButton) {
      const box = make(doc, "div", "kit-empty");
      if (icon) box.append(make(doc, "span", `kit-icon kit-icon--${icon} kit-empty__icon`));
      box.append(make(doc, "p", "kit-empty__title", title), make(doc, "p", "kit-empty__text", text));
      if (actionButton) {
        const holder = make(doc, "div", "kit-empty__actions");
        holder.append(actionButton);
        box.append(holder);
      }
      return box;
    }

    function passLine(row) {
      const box = make(doc, "div", "kit-stack");
      const pass = logic.passState(row, state.grants[row.key], ctx.now());
      if (pass.state === "used") {
        box.append(live(make(doc, "span", "kit-state kit-state--success kit-small"), () => t("passUsed", { when: since(pass.at) })));
        return box;
      }
      if (pass.state === "active") {
        const status = make(doc, "div", "");
        status.setAttribute("role", "status");
        const remaining = () => {
          const current = logic.passState(row, state.grants[row.key], ctx.now());
          return t("passActive", { remaining: logic.formatRemaining(current.remainingMs || 0) });
        };
        status.append(
          live(make(doc, "span", "kit-state kit-state--waiting kit-small"), remaining),
          make(doc, "div", "kit-small kit-dim", t("passActiveHint", { agent: row.agent })),
        );
        box.append(status);
        return box;
      }
      if (pass.state === "expired") box.append(make(doc, "div", "kit-small kit-dim", t("passExpired")));
      const problem = state.problems[row.key];
      if (problem) box.append(banner("error", [t(`problem.${problem.code}`, { detail: problem.detail })], "alert"));
      const allow = button(state.pending[row.key] ? t("allowing") : t("allow"), () => actions.allow(row.key));
      allow.id = `allow-${row.key}`;
      if (state.pending[row.key]) allow.disabled = true;
      const holder = make(doc, "div", "kit-actions");
      holder.append(allow);
      box.append(holder);
      return box;
    }

    function refusal(row) {
      const item = make(doc, "li", "kit-row kit-row--wrap");
      const lead = make(doc, "span", "kit-row__lead");
      const glyph = make(doc, "span", `kit-glyph kit-glyph--${row.usedAt === null ? "failure" : "success"}`);
      glyph.setAttribute("role", "img");
      glyph.setAttribute("aria-label", t(row.usedAt === null ? "refusedLabel" : "letThroughLabel"));
      lead.append(glyph);

      const main = make(doc, "div", "kit-row__main");
      const body = make(doc, "div", "kit-stack");
      const who = make(doc, "div", "");
      const facts = make(doc, "div", "kit-row__sub");
      const when = live(make(doc, "span"), () => since(row.at));
      if (ctx.format) when.setAttribute("title", ctx.format.date(row.at, ctx.now()));
      facts.append(when);
      const folder = logic.folderName(row.folder);
      if (folder) facts.append(` · ${folder}`);
      if (row.count > 1) facts.append(` · ${t("refusedTimes", { n: row.count })}`);
      who.append(make(doc, "div", "kit-row__title", row.agent), facts);

      const command = make(doc, "pre", "kit-pre");
      command.setAttribute("tabindex", "0");
      command.setAttribute("style", "--kit-log-height: 160px");
      command.append(make(doc, "code", "", row.command));

      body.append(who, command);
      if (row.length > row.command.length) body.append(make(doc, "div", "kit-small kit-dim", t("truncated", { n: row.length })));
      body.append(make(doc, "div", "kit-small", logic.whyFor(row, lang, ctx.t)), passLine(row));
      main.append(body);
      item.append(lead, main);
      return item;
    }

    function header() {
      const head = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      main.append(make(doc, "h1", "kit-header__title", t("tab")));
      if (state.view === "list") {
        const count = logic.refusedCount(state.rows);
        const line = make(doc, "p", "kit-header__state");
        line.append(
          count
            ? make(doc, "span", "kit-state kit-state--failure", t("stateRefused", { n: count }))
            : make(doc, "span", "kit-state kit-state--neutral", t("stateClear")),
        );
        main.append(line);
      }
      head.append(main);
      if (state.view === "list" && state.rows.length) {
        const buttons = make(doc, "div", "kit-header__actions");
        const clear = button(t("clear"), actions.clear, "kit-btn--small kit-btn--ghost");
        clear.id = "clear";
        buttons.append(clear);
        head.append(buttons);
      }
      return head;
    }

    const page = make(doc, "main", "kit-page");

    if (state.view === "loading") {
      const box = make(doc, "div", "kit-empty");
      box.append(make(doc, "span", "kit-state kit-state--running", t("loading")));
      page.append(box);
    } else if (state.view === "denied") {
      page.append(empty("lock", t("deniedTitle"), t("deniedText"), button(t("retry"), actions.retry)), limits());
    } else if (state.view === "error") {
      page.append(empty("alert", t("errorTitle"), t("errorText"), button(t("retry"), actions.retry, "kit-btn--small kit-btn--primary")), limits());
    } else {
      if (state.stale) page.append(banner("warning", [t("staleWarning")]));
      if (state.clearFailed) page.append(banner("error", [t("clearFailed")], "alert"));
      if (state.errors) {
        page.append(banner("error", [t("errors", { n: state.errors.count }), " ", make(doc, "span", "kit-code", state.errors.last)], "alert"));
      }
      if (!state.rows.length) {
        page.append(empty(null, t("emptyTitle"), t("emptyText")));
      } else {
        const section = make(doc, "section", "kit-section");
        const head = make(doc, "div", "kit-section__head");
        head.append(make(doc, "h2", "kit-section__title", t("heading")));
        const list = make(doc, "ul", "kit-rows kit-rows--group");
        for (const row of state.rows) list.append(refusal(row));
        section.append(head, list);
        page.append(section);
      }
      page.append(limits());
    }

    // What had the keyboard keeps it: the page is drawn again, the person did not move.
    const focused = doc.activeElement && doc.activeElement.id;
    container.replaceChildren(header(), page);
    if (focused && doc.getElementById) {
      const again = doc.getElementById(focused);
      if (again) again.focus();
    }

    return function tick() {
      for (const [element, text] of timed) {
        const value = text();
        if (element.textContent !== value) element.textContent = value;
      }
    };
  }

  return { render };
});
