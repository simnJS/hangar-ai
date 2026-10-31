"use strict";
// Draws the controller's state with Hangar UI's components (docs/kit-ui.md), and no colour of its
// own. Only text nodes and elements made by hand: a comment from the code is never parsed as markup.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.TodoView = api;
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
   * @param {{ doc, logic, lang, now: () => number,
   *           actions: { scan(), open(key), blame(file), handOver(key), choose(agent), setQuery(q), toggleTag(tag), clearFilters(), groupBy(by), showMore() },
   *           t?: (key, vars) => string, format?: { since(at, now) } }} ctx
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

    const scan = state.scan;
    const target = state.choice || logic.defaultTarget(state.panes);

    /** The button that hands one marker to the chosen agent, or what stands in for it. */
    function handOver(item) {
      const sent = state.sent[item.key];
      if (sent) {
        const done = make(doc, "span", "kit-state kit-state--success kit-small", t("sentTo", { agent: sent.agent }));
        done.setAttribute("role", "status");
        return done;
      }
      const element = button(state.pending[item.key] ? t("handingOver") : t("handOver"), () => actions.handOver(item.key));
      element.id = `hand-${item.key}`;
      if (state.pending[item.key] || !target) element.disabled = true;
      return element;
    }

    /** Who wrote a marker and when, once the file was blamed. */
    function authorLine(item) {
      const blame = state.blame[item.file];
      const entry = blame && blame.byLine ? blame.byLine[item.line] : null;
      if (!entry) return null;
      if (!entry.author) return make(doc, "div", "kit-small kit-dim kit-wrap", t("authorNotCommitted"));
      const line = make(doc, "div", "kit-small kit-dim kit-wrap");
      return live(line, () => t("authorBy", { author: entry.author, time: entry.at ? since(entry.at) : "" }).replace(/, $/, ""));
    }

    function row(item, showFile) {
      const li = make(doc, "li", "kit-row kit-row--wrap");
      const main = make(doc, "div", "kit-row__main");
      const body = make(doc, "div", "kit-stack");
      const head = make(doc, "div", "");
      head.append(make(doc, "div", "kit-row__title kit-wrap", item.text || t("noText")));
      const where = make(doc, "div", "kit-actions");
      where.append(make(doc, "span", "kit-chip", item.tag), make(doc, "span", "kit-row__sub kit-mono kit-wrap", showFile ? `${item.file}:${item.line}` : `:${item.line}`));
      head.append(where);
      const author = authorLine(item);
      if (author) head.append(author);
      body.append(head);
      const problem = state.problems[item.key];
      if (problem) body.append(banner("error", t(`problem.${problem.code}`, { agent: problem.agent, detail: problem.detail }), "alert"));
      const holder = make(doc, "div", "kit-actions");
      if (logic.canOpen(item.file)) {
        const open = button(t("open"), () => actions.open(item.key), "kit-btn--small kit-btn--ghost");
        open.id = `open-${item.key}`;
        open.setAttribute("aria-label", t("openLabel", { file: `${item.file}:${item.line}` }));
        holder.append(open);
      }
      holder.append(handOver(item));
      body.append(holder);
      main.append(body);
      li.append(main);
      return li;
    }

    /** One line saying who the markers go to: a choice when there are several agents. */
    function agentPicker() {
      const list = logic.candidates(state.panes);
      const box = make(doc, "div", "kit-small kit-dim kit-wrap");
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

    function controls() {
      const box = make(doc, "div", "kit-stack");
      const chips = make(doc, "div", "kit-chips");
      chips.setAttribute("role", "group");
      chips.setAttribute("aria-label", t("filterLabel"));
      for (const tag of logic.TAGS) {
        if (!scan.counts[tag]) continue;
        const chip = make(doc, "button", "kit-filter", tag);
        chip.type = "button";
        chip.id = `tag-${tag}`;
        chip.setAttribute("aria-pressed", state.tags.includes(tag) ? "true" : "false");
        chip.append(" ", make(doc, "span", "kit-filter__count", String(scan.counts[tag])));
        chip.onclick = () => actions.toggleTag(tag);
        chips.append(chip);
      }
      box.append(chips);

      const search = make(doc, "div", "kit-search");
      const input = make(doc, "input", "");
      input.type = "search";
      input.id = "query";
      input.placeholder = t("search");
      input.setAttribute("aria-label", t("searchLabel"));
      input.value = state.query;
      input.oninput = () => actions.setQuery(input.value);
      search.append(input);
      box.append(search);

      const groupLabel = make(doc, "label", "kit-actions");
      const wrap = make(doc, "span", "kit-select");
      const select = make(doc, "select", "");
      select.id = "group";
      select.setAttribute("aria-label", t("groupLabel"));
      for (const [value, key] of [["file", "byFile"], ["tag", "byTag"]]) {
        const option = make(doc, "option", "", t(key));
        option.value = value;
        if (state.by === value) option.selected = true;
        select.append(option);
      }
      select.value = state.by;
      select.onchange = () => actions.groupBy(select.value);
      wrap.append(select);
      groupLabel.append(make(doc, "span", "kit-small kit-dim", t("groupLabel")), wrap);
      box.append(groupLabel);
      return box;
    }

    /** The groups' rows, as many as `limit` allows, and a button for the rest. */
    function list(groups, total) {
      const stack = make(doc, "div", "kit-stack");
      let left = state.limit;
      for (const entry of groups) {
        if (left <= 0) break;
        const shown = entry.items.slice(0, left);
        left -= shown.length;
        const section = make(doc, "section", "kit-section");
        const head = make(doc, "div", "kit-section__head");
        const title =
          state.by === "file"
            ? make(doc, "div", "kit-row__title kit-mono kit-wrap", entry.title)
            : make(doc, "h2", "kit-section__title", entry.title);
        if (state.by === "file") {
          title.setAttribute("role", "heading");
          title.setAttribute("aria-level", "2");
        }
        head.append(title, make(doc, "span", "kit-section__count", String(entry.items.length)));
        const asked = state.blame[entry.id];
        if (state.by === "file" && !(asked && asked.byLine)) {
          const holder = make(doc, "div", "kit-section__actions");
          const ask = button(asked && asked.loading ? t("authorLoading") : t("author"), () => actions.blame(entry.id), "kit-btn--small kit-btn--ghost");
          ask.id = `blame-${entry.id}`;
          ask.setAttribute("aria-label", t("authorLabel", { file: entry.id }));
          if (asked && asked.loading) ask.disabled = true;
          holder.append(ask);
          head.append(holder);
        }
        section.append(head);
        if (asked && asked.denied) section.append(banner("error", t("blameDenied"), "alert"));
        if (asked && asked.status && asked.status !== "ok") section.append(banner("warning", t(`blameStatus_${asked.status.replace("-", "_")}`)));
        const ul = make(doc, "ul", "kit-rows kit-rows--group");
        for (const item of shown) ul.append(row(item, state.by !== "file"));
        section.append(ul);
        stack.append(section);
      }
      const shownCount = Math.min(state.limit, total);
      if (shownCount < total) {
        const holder = make(doc, "div", "kit-actions");
        const more = button(t("showMore", { n: total - shownCount }), actions.showMore, "kit-btn--small kit-btn--ghost");
        more.id = "more";
        holder.append(more);
        stack.append(holder);
      }
      return stack;
    }

    function header() {
      const head = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      main.append(make(doc, "h1", "kit-header__title", t("tab")));
      if (state.view !== "loading") {
        const line = make(doc, "p", "kit-header__state");
        if (state.scanning) line.append(make(doc, "span", "kit-state kit-state--running", t("scanning")));
        else if (scan && scan.total) line.append(make(doc, "span", "kit-state kit-state--neutral", t("stateFound", { n: scan.total })));
        else if (scan) line.append(make(doc, "span", "kit-state kit-state--success", t("stateNone")));
        main.append(line);
        if (scan && !state.scanning) main.append(live(make(doc, "p", "kit-small kit-dim kit-wrap"), () => t("lastScan", { time: since(scan.at) })));
      }
      head.append(main);
      if (state.view === "ready") {
        const box = make(doc, "div", "kit-header__actions");
        const run = button(scan ? t("scanAgain") : t("scan"), actions.scan, "kit-btn--small");
        run.id = "scan";
        if (state.scanning) run.disabled = true;
        box.append(run);
        head.append(box);
      }
      return head;
    }

    const page = make(doc, "main", "kit-page");

    if (state.view === "loading") {
      const box = make(doc, "div", "kit-empty");
      box.append(make(doc, "span", "kit-state kit-state--running", t("scanning")));
      page.append(box);
    } else if (state.view === "denied") {
      page.append(empty("lock", t("deniedTitle"), t("deniedText"), button(t("retry"), actions.scan)));
    } else {
      if (state.scanning) {
        const bar = make(doc, "div", "kit-progress kit-progress--indeterminate");
        bar.setAttribute("role", "progressbar");
        bar.setAttribute("aria-label", t("scanning"));
        bar.append(make(doc, "div", "kit-progress__fill"));
        page.append(bar);
        if (!scan) page.append(banner("", t("scanningText"), "status"));
      }
      if (state.error) {
        const code = ["timeout", "not_found", "busy", "unreadable"].includes(state.error.code) ? state.error.code : "failed";
        page.append(empty("alert", t("errorTitle"), t(`error.${code}`, { detail: state.error.detail }), state.scanning ? null : button(t("retry"), actions.scan, "kit-btn--small kit-btn--primary")));
      }
      if (scan && !scan.total) {
        if (scan.incomplete) page.append(banner("warning", t("incomplete")));
        page.append(empty("check", t("noneTitle"), t("noneText")));
      } else if (scan) {
        const stack = make(doc, "div", "kit-stack");
        if (scan.truncated || scan.total > scan.items.length) stack.append(banner("warning", `${t("tooManyTitle")}. ${t("tooMany", { total: scan.total, shown: scan.items.length })}`));
        if (scan.incomplete) stack.append(banner("warning", t("incomplete")));
        stack.append(controls(), agentPicker());
        const matching = logic.filterItems(scan.items, { query: state.query, tags: state.tags });
        if (!matching.length) {
          stack.append(empty("search", t("nothingMatches"), t("nothingMatchesText"), button(t("clearFilters"), actions.clearFilters)));
        } else {
          stack.append(make(doc, "p", "kit-small kit-dim", t("matches", { n: matching.length, total: scan.items.length })));
          stack.append(list(logic.group(matching, state.by), matching.length));
        }
        page.append(stack);
      }
    }

    // What had the keyboard keeps it, caret included: the page is drawn again, the person did not move.
    const active = doc.activeElement;
    const focused = active && active.id;
    const selection = active && active.id === "query" ? [active.selectionStart, active.selectionEnd] : null;
    container.replaceChildren(header(), page);
    if (focused && doc.getElementById) {
      const again = doc.getElementById(focused);
      if (again) {
        again.focus();
        if (selection && typeof again.setSelectionRange === "function" && selection[0] !== null) again.setSelectionRange(selection[0], selection[1]);
      }
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
