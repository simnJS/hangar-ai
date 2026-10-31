"use strict";
// Draws the controller's state with Hangar UI's components (docs/kit-ui.md), and no colour of its
// own. Only text nodes and elements made by hand: an advisory's title is never parsed as markup.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.DepsView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  function make(doc, tag, className, text) {
    const element = doc.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined && text !== null) element.textContent = text;
    return element;
  }

  /**
   * Renders `state` into `container`, and answers the function that brings what depends on the
   * time up to date, touching nothing else.
   * @param {{ doc, logic, lang, now: () => number, actions: { run(), handOver(kind, key), choose(agent), showMore(kind) },
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

    function banner(kind, message, role) {
      const box = make(doc, "p", `kit-banner${kind ? ` kit-banner--${kind}` : ""}`);
      if (role) box.setAttribute("role", role);
      box.append(make(doc, "span", "kit-banner__text", message));
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

    const result = state.result;
    const multiple = result && result.managers.length > 1;
    const target = state.choice || logic.defaultTarget(state.panes);

    /** The button that hands one finding to the chosen agent, or what stands in for it. */
    function handOver(kind, item) {
      const sent = state.sent[item.key];
      if (sent) {
        const done = make(doc, "span", "kit-state kit-state--success kit-small", t("sentTo", { agent: sent.agent }));
        done.setAttribute("role", "status");
        return done;
      }
      const element = button(state.pending[item.key] ? t("handingOver") : t("handOver"), () => actions.handOver(kind, item.key));
      element.id = `hand-${item.key.replace(/:/g, "-")}`;
      if (state.pending[item.key] || !target) element.disabled = true;
      return element;
    }

    function problemLine(item) {
      const problem = state.problems[item.key];
      return problem ? banner("error", t(`problem.${problem.code}`, { agent: problem.agent, detail: problem.detail }), "alert") : null;
    }

    function findingRow(kind, item, lead, title, sub, meta) {
      const row = make(doc, "li", "kit-row kit-row--wrap");
      const leading = make(doc, "span", "kit-row__lead");
      leading.append(lead);
      const main = make(doc, "div", "kit-row__main");
      const body = make(doc, "div", "kit-stack");
      const head = make(doc, "div", "");
      head.append(make(doc, "div", "kit-row__title kit-wrap", title));
      if (sub) head.append(make(doc, "div", "kit-row__sub kit-wrap", sub));
      if (meta) head.append(make(doc, "div", "kit-small kit-dim kit-wrap", meta));
      body.append(head);
      const problem = problemLine(item);
      if (problem) body.append(problem);
      const holder = make(doc, "div", "kit-actions");
      holder.append(handOver(kind, item));
      body.append(holder);
      main.append(body);
      row.append(leading, main);
      return row;
    }

    function vulnerabilityRow(item) {
      const bad = item.severity === "critical" || item.severity === "high";
      const versions = item.fixed ? `${item.installed || "?"} → ${item.fixed}` : item.installed || "";
      const title = [item.package, versions].filter(Boolean).join(" ");
      const severity = t(`severity.${item.severity}`);
      const sub = [severity, item.title].filter(Boolean).join(" · ");
      const fix = item.fixed ? t("fixedIn", { version: item.fixed }) : t("noFix");
      const meta = [item.id, fix, multiple ? item.manager : ""].filter(Boolean).join(" · ");
      return findingRow("vulnerability", item, glyph(bad ? "failure" : "neutral", severity), title, sub, meta);
    }

    function outdatedRow(item) {
      const title = `${item.package} ${item.current || "?"} → ${item.latest || "?"}`;
      const meta = [t(`kind.${item.kind}`), item.dev ? t("devDependency") : "", multiple ? item.manager : ""].filter(Boolean).join(" · ");
      return findingRow("outdated", item, glyph("neutral", t(`kind.${item.kind}`)), title, "", meta);
    }

    /** One line saying who the findings go to: a choice when there are several agents. */
    function agentPicker() {
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
      const label = make(doc, "label", "kit-actions");
      const wrap = make(doc, "span", "kit-select");
      const select = make(doc, "select", "");
      select.setAttribute("aria-label", t("agentLabel"));
      select.id = "agent";
      for (const pane of list) {
        const option = make(doc, "option", "", pane.name);
        option.value = pane.name;
        if (pane.name === target) option.selected = true;
        select.append(option);
      }
      select.value = target;
      select.onchange = () => actions.choose(select.value);
      wrap.append(select);
      label.append(make(doc, "span", "kit-small kit-dim", t("agentLabel")), wrap);
      return label;
    }

    function section(title, count) {
      const box = make(doc, "section", "kit-section");
      const head = make(doc, "div", "kit-section__head");
      head.append(make(doc, "h2", "kit-section__title", title));
      if (count !== undefined) head.append(make(doc, "span", "kit-section__count", String(count)));
      box.append(head);
      return box;
    }

    /** The rows of a list, the first few when it is long and the rest behind a button. */
    function rows(items, draw, kind) {
      const shown = state.expanded[kind] ? items : items.slice(0, logic.SHOWN_PER_GROUP);
      const ul = make(doc, "ul", "kit-rows kit-rows--group");
      for (const item of shown) ul.append(draw(item));
      const block = make(doc, "div", "kit-stack");
      block.append(ul);
      if (shown.length < items.length) {
        const holder = make(doc, "div", "kit-actions");
        const more = button(t("showMore", { n: items.length - shown.length }), () => actions.showMore(kind), "kit-btn--small kit-btn--ghost");
        more.id = `more-${kind}`;
        holder.append(more);
        block.append(holder);
      }
      return block;
    }

    /** What the managers said that is not a list: a tool to install, a check that failed. */
    function notices() {
      const out = [];
      for (const manager of result.managers) {
        for (const tool of manager.missing) {
          const command = logic.INSTALL[tool];
          out.push(banner("warning", command ? t("missingTool", { tool, command }) : t("missingToolPlain", { tool })));
        }
        for (const error of manager.errors) {
          out.push(banner("error", t("stepFailed", { manager: manager.manager, step: t(`step.${error.step}`), detail: error.detail }), "alert"));
        }
        if (manager.steps.outdated === "unsupported") out.push(banner("", t("stepUnsupported", { manager: manager.manager })));
        if (manager.truncated) out.push(banner("warning", t("truncated")));
      }
      return out;
    }

    function header() {
      const head = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      main.append(make(doc, "h1", "kit-header__title", t("tab")));
      if (state.view !== "loading") {
        const line = make(doc, "p", "kit-header__state");
        if (state.running) {
          line.append(make(doc, "span", "kit-state kit-state--running", t("running")));
        } else if (result && logic.answered(result)) {
          const vulns = logic.vulnerabilitiesOf(result);
          const high = logic.highCount(vulns);
          if (high) line.append(make(doc, "span", "kit-state kit-state--failure", t("stateHigh", { n: high })));
          else if (vulns.length) line.append(make(doc, "span", "kit-state kit-state--neutral", t("stateVulns", { n: vulns.length })));
          else if (!logic.vulnerabilityCheckComplete(result)) line.append(make(doc, "span", "kit-state kit-state--neutral", t("stateIncomplete")));
          else line.append(make(doc, "span", "kit-state kit-state--success", logic.outdatedGroups(result).length ? t("stateOutdatedOnly") : t("stateClean")));
        } else if (result && result.status === "no-lockfile") {
          line.append(make(doc, "span", "kit-state kit-state--neutral", t("stateNoLock")));
        } else if (result && result.status === "tool-missing") {
          line.append(make(doc, "span", "kit-state kit-state--neutral", t("stateToolMissing")));
        } else if (!result) {
          line.append(make(doc, "span", "kit-state kit-state--neutral", t("stateNever")));
        }
        main.append(line);
        if (result && !state.running) main.append(live(make(doc, "p", "kit-small kit-dim kit-wrap"), () => t("lastRun", { time: since(result.at) })));
      }
      head.append(main);
      if (state.view === "ready") {
        const actionsBox = make(doc, "div", "kit-header__actions");
        const run = button(result ? t("runAgain") : t("run"), actions.run, `kit-btn--small${result ? "" : " kit-btn--primary"}`);
        run.id = "run";
        if (state.running) run.disabled = true;
        actionsBox.append(run);
        head.append(actionsBox);
      }
      return head;
    }

    const page = make(doc, "main", "kit-page");

    if (state.view === "loading") {
      const box = make(doc, "div", "kit-empty");
      box.append(make(doc, "span", "kit-state kit-state--running", t("running")));
      page.append(box);
    } else if (state.view === "denied") {
      page.append(empty("lock", t("deniedTitle"), t("deniedText"), button(t("retry"), actions.run)));
    } else {
      if (state.running) {
        const bar = make(doc, "div", "kit-progress kit-progress--indeterminate");
        bar.setAttribute("role", "progressbar");
        bar.setAttribute("aria-label", t("running"));
        bar.append(make(doc, "div", "kit-progress__fill"));
        page.append(bar, banner("", t("runningText"), "status"));
      }
      if (state.error) {
        const message = t(`error.${["timeout", "not_found", "busy", "unreadable"].includes(state.error.code) ? state.error.code : "failed"}`, { detail: state.error.detail });
        page.append(empty("alert", t("errorTitle"), message, state.running ? null : button(t("retry"), actions.run, "kit-btn--small kit-btn--primary")));
      }
      if (!result) {
        if (!state.running && !state.error) page.append(empty(null, t("neverTitle"), t("neverText")));
      } else if (result.status === "no-lockfile") {
        page.append(empty("search", t("noLockTitle"), t("noLockText")));
      } else if (result.status === "tool-missing") {
        page.append(empty("alert", t("toolMissingTitle"), t("toolMissingText")), ...notices());
      } else {
        const stack = make(doc, "div", "kit-stack");
        for (const notice of notices()) stack.append(notice);
        const vulns = logic.vulnerabilitiesOf(result);
        const groups = logic.outdatedGroups(result);
        if (!vulns.length && !groups.length && logic.answered(result) && !result.managers.some((m) => m.errors.length || m.missing.length)) {
          stack.append(empty("check", t("cleanTitle"), t("cleanText")));
        } else {
          if (vulns.length || groups.length) stack.append(agentPicker());
          if (vulns.length) {
            const box = section(t("vulnerabilities"), vulns.length);
            box.append(rows(vulns, vulnerabilityRow, "vulnerability"));
            stack.append(box);
          }
          for (const group of groups) {
            const box = section(`${t("outdated")} · ${t(`kind.${group.kind}`)}`, group.items.length);
            box.append(rows(group.items, outdatedRow, group.kind));
            stack.append(box);
          }
        }
        page.append(stack);
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
