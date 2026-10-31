"use strict";
// Draws the controller's state with Hangar UI's components (docs/kit-ui.md), and no colour
// of its own. Only text nodes and elements made by hand: a test's name or message is never
// parsed as markup.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.TestsView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  function make(doc, tag, className, text) {
    const element = doc.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined && text !== null) element.textContent = text;
    return element;
  }

  /**
   * Renders `state` into `container`, and answers the function that brings what depends on
   * the time up to date, touching nothing else.
   * @param {{ doc, logic, lang, now: () => number, actions: { handOver(key, index), choose(key, agent), retry() },
   *           t?: (key, vars) => string, format?: { since(at, now), date(at, now), duration(ms) } }} ctx
   *   `t` and `format` are hangar.ui's in the page.
   */
  function render(container, state, ctx) {
    const { doc, logic, lang, actions } = ctx;
    const t = ctx.t || ((key, vars) => logic.t(lang, key, vars));
    const since = (at) => (ctx.format ? ctx.format.since(at, ctx.now()) : logic.relativeTime(at, ctx.now(), lang));
    const duration = (ms) => (ctx.format ? ctx.format.duration(ms) : logic.formatDuration(ms));
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
      const body = make(doc, "span", "kit-banner__text");
      body.append(...parts);
      box.append(body);
      return box;
    }

    function empty(icon, title, body, actionButton) {
      const box = make(doc, "div", "kit-empty");
      if (icon) box.append(make(doc, "span", `kit-icon kit-icon--${icon} kit-empty__icon`));
      box.append(make(doc, "p", "kit-empty__title", title), make(doc, "p", "kit-empty__text", body));
      if (actionButton) {
        const holder = make(doc, "div", "kit-empty__actions");
        holder.append(actionButton);
        box.append(holder);
      }
      return box;
    }

    function glyph(kind, label) {
      const element = make(doc, "span", `kit-glyph kit-glyph--${kind}`);
      element.setAttribute("role", "img");
      element.setAttribute("aria-label", label);
      return element;
    }

    /** `3 failed · 12 passed · 1 skipped`: only what the run said. */
    function counts(run) {
      const parts = [];
      if (run.failed) parts.push(t("failedCount", { n: run.failed }));
      if (run.passed !== null) parts.push(t("passedCount", { n: run.passed }));
      if (run.skipped) parts.push(t("skippedCount", { n: run.skipped }));
      if (run.duration !== null) parts.push(duration(run.duration));
      return parts.join(" · ");
    }

    function problemLine(id) {
      const problem = state.problems[id];
      if (!problem) return null;
      return banner("error", [t(`problem.${problem.code}`, { agent: problem.agent, detail: problem.detail })], "alert");
    }

    /** The button that hands failure `index` (or all of a run's, when null) to the run's agent, or what stands in for it. */
    function handOver(run, index, label, className) {
      const id = `${run.key}:${index === null ? "all" : index}`;
      const sent = state.sent[id];
      if (sent) {
        const done = make(doc, "span", "kit-state kit-state--success kit-small", t("sentTo", { agent: sent.agent }));
        done.setAttribute("role", "status");
        return done;
      }
      const agent = state.choice[run.key] || logic.defaultTarget(run, state.panes);
      const element = button(state.pending[id] ? t("handingOver") : label, () => actions.handOver(run.key, index), className);
      element.id = `hand-${id.replace(":", "-")}`;
      if (state.pending[id] || !agent) element.disabled = true;
      return element;
    }

    /** One line saying who the failures go to: a choice when there are several agents. */
    function agentPicker(run) {
      const list = logic.candidates(state.panes);
      const box = make(doc, "div", "kit-small kit-dim");
      if (state.panesDenied) {
        box.textContent = t("panesDenied");
        return box;
      }
      if (!list.length) {
        box.textContent = t("noAgent");
        return box;
      }
      if (list.length === 1) {
        box.textContent = `${t("agentLabel")} : ${list[0].name}`;
        return box;
      }
      const chosen = state.choice[run.key] || logic.defaultTarget(run, state.panes);
      const label = make(doc, "label", "kit-actions");
      const wrap = make(doc, "span", "kit-select");
      const select = make(doc, "select", "");
      select.setAttribute("aria-label", t("agentLabel"));
      select.id = `agent-${run.key}`;
      for (const pane of list) {
        const option = make(doc, "option", "", pane.name);
        option.value = pane.name;
        if (pane.name === chosen) option.selected = true;
        select.append(option);
      }
      select.value = chosen;
      select.onchange = () => actions.choose(run.key, select.value);
      wrap.append(select);
      label.append(make(doc, "span", "kit-small kit-dim", t("agentLabel")), wrap);
      return label;
    }

    function failureRow(run, failure, index) {
      const item = make(doc, "li", "kit-row kit-row--wrap");
      const lead = make(doc, "span", "kit-row__lead");
      lead.append(glyph("failure", t("failedLabel")));
      const main = make(doc, "div", "kit-row__main");
      const body = make(doc, "div", "kit-stack");
      const head = make(doc, "div", "");
      head.append(make(doc, "div", "kit-row__title kit-wrap", failure.name));
      const where = logic.whereOf(failure);
      if (where) head.append(make(doc, "div", "kit-row__sub kit-wrap", where));
      body.append(head);
      if (failure.message) {
        const message = make(doc, "pre", "kit-pre");
        message.setAttribute("tabindex", "0");
        message.setAttribute("style", "--kit-log-height: 160px");
        message.append(make(doc, "code", "", failure.message));
        body.append(message);
      }
      const problem = problemLine(`${run.key}:${index}`);
      if (problem) body.append(problem);
      const holder = make(doc, "div", "kit-actions");
      holder.append(handOver(run, index, t("handOver"), "kit-btn--small"));
      body.append(holder);
      main.append(body);
      item.append(lead, main);
      return item;
    }

    function summaryRow(run) {
      const item = make(doc, "li", "kit-row kit-row--wrap");
      const kind = run.status === "passed" ? "success" : run.status === "failed" ? "failure" : "neutral";
      const label = run.status === "passed" ? t("passedLabel") : run.status === "failed" ? t("failedLabel") : t("unknownLabel");
      const lead = make(doc, "span", "kit-row__lead");
      lead.append(glyph(kind, label));
      const main = make(doc, "div", "kit-row__main");
      const body = make(doc, "div", "kit-stack");

      const head = make(doc, "div", "");
      head.append(make(doc, "div", "kit-row__title kit-wrap", run.framework ? t("runLabel", { agent: run.agent, framework: run.framework }) : t("runLabelNone", { agent: run.agent })));
      const facts = make(doc, "div", "kit-row__sub");
      const when = live(make(doc, "span"), () => since(run.at));
      if (ctx.format) when.setAttribute("title", ctx.format.date(run.at, ctx.now()));
      const line = run.status === "passed" && run.failed === 0 ? [t("allGreen"), counts({ ...run, failed: 0 })].filter(Boolean).join(" · ") : run.status === "not-understood" ? "" : counts(run);
      if (line) facts.append(`${line} · `);
      facts.append(when);
      head.append(facts);
      body.append(head);

      const command = make(doc, "div", "kit-small kit-dim kit-wrap");
      command.append(make(doc, "span", "kit-code", run.command));
      command.setAttribute("title", run.command);
      body.append(command);

      if (run.status === "not-understood") body.append(banner("", [run.reason === "no-tests" ? t("noTests") : t("notUnderstood")]));
      if (run.partial) body.append(banner("warning", [t("partial")]));
      if (run.status === "failed" && run.failed === 0 && run.failures.length === 0 && !run.partial && run.exitCode !== null) {
        body.append(banner("", [t("exitCode", { code: run.exitCode })]));
      }
      if (run.status === "failed" && run.failures.length === 0 && run.failed > 0) body.append(banner("", [t("noDetail")]));
      if (run.status === "failed" && run.failures.length && run.failed > run.failures.length) {
        body.append(make(doc, "div", "kit-small kit-dim", t("showing", { n: run.failures.length, total: run.failed })));
      }

      main.append(body);
      item.append(lead, main);
      return item;
    }

    function runBlock(run) {
      const list = make(doc, "ul", "kit-rows kit-rows--group");
      list.append(summaryRow(run));
      run.failures.forEach((failure, index) => list.append(failureRow(run, failure, index)));
      const block = make(doc, "div", "kit-stack");
      block.append(list);
      if (run.failures.length) {
        const footer = make(doc, "div", "kit-stack");
        footer.append(agentPicker(run));
        if (run.failures.length > 1) {
          const problem = problemLine(`${run.key}:all`);
          if (problem) footer.append(problem);
          const holder = make(doc, "div", "kit-actions");
          holder.append(handOver(run, null, t("handOverAll", { n: run.failures.length }), "kit-btn--small"));
          footer.append(holder);
        }
        block.append(footer);
      }
      return block;
    }

    function header() {
      const head = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      main.append(make(doc, "h1", "kit-header__title", t("tab")));
      if (state.view === "list") {
        const overall = logic.overall(state.runs);
        const line = make(doc, "p", "kit-header__state");
        if (overall.kind === "failed") line.append(make(doc, "span", "kit-state kit-state--failure", overall.n ? t("stateFailed", { n: overall.n }) : t("failedLabel")));
        else if (overall.kind === "green") line.append(make(doc, "span", "kit-state kit-state--success", t("stateGreen")));
        else if (overall.kind === "unknown") line.append(make(doc, "span", "kit-state kit-state--neutral", t("stateUnknown")));
        else line.append(make(doc, "span", "kit-state kit-state--neutral", t("stateNone")));
        main.append(line);
      }
      head.append(main);
      return head;
    }

    const page = make(doc, "main", "kit-page");

    if (state.view === "loading") {
      const box = make(doc, "div", "kit-empty");
      box.append(make(doc, "span", "kit-state kit-state--running", t("loading")));
      page.append(box);
    } else if (state.view === "denied") {
      page.append(empty("lock", t("deniedTitle"), t("deniedText"), button(t("retry"), actions.retry)));
    } else if (state.view === "error") {
      page.append(empty("alert", t("errorTitle"), t("errorText"), button(t("retry"), actions.retry, "kit-btn--small kit-btn--primary")));
    } else {
      if (state.stale) page.append(banner("warning", [t("staleWarning")]));
      if (!state.runs.length) {
        page.append(empty(null, t("emptyTitle"), t("emptyText")));
      } else {
        for (const group of logic.groupRuns(state.runs)) {
          const section = make(doc, "section", "kit-section");
          const head = make(doc, "div", "kit-section__head");
          const title = make(doc, "h2", "kit-section__title kit-truncate", group.project);
          title.setAttribute("title", group.project);
          head.append(title);
          if (group.failed) head.append(make(doc, "span", "kit-section__count", String(group.failed)));
          section.append(head);
          const blocks = make(doc, "div", "kit-stack");
          for (const run of group.runs) blocks.append(runBlock(run));
          section.append(blocks);
          page.append(section);
        }
      }
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
