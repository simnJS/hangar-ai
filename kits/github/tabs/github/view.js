"use strict";
// Draws the controller's state with Hangar UI's components (docs/kit-ui.md), and no
// colour of its own. Only text nodes and elements made by hand: what GitHub's users
// wrote is never parsed as markup.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.GithubView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const BODY_SHOWN = 4000;
  const FILES_SHOWN = 30;

  function make(doc, tag, className, text) {
    const element = doc.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined && text !== null) element.textContent = text;
    return element;
  }

  /**
   * Renders `state` into `container`.
   * @param {{ doc, logic, lang, kind: "pulls"|"issues", now: () => number,
   *           actions: { refresh(), open(n), back(), filters(patch), clearFilters(), hand(anchor, n), copy?(button, text) },
   *           t?: (key, vars) => string, format?: { since(at, now) } }} ctx
   */
  function render(container, state, ctx) {
    const { doc, logic, lang, kind, actions } = ctx;
    const isPull = kind === "pulls";
    const t = ctx.t || ((key, vars) => logic.t(lang, key, vars));
    const since = (at) => (ctx.format ? ctx.format.since(at, ctx.now()) : logic.relativeTime(at, ctx.now(), lang));

    const focused = doc.activeElement && doc.activeElement.id === "filter-query" ? doc.activeElement : null;
    const caret = focused && typeof focused.selectionStart === "number" ? [focused.selectionStart, focused.selectionEnd] : null;

    function button(label, handler, className = "kit-btn--small") {
      const element = make(doc, "button", `kit-btn ${className}`, label);
      element.type = "button";
      element.onclick = handler;
      return element;
    }

    function glyph(stateName, label) {
      const element = make(doc, "span", `kit-glyph kit-glyph--${stateName}`);
      element.setAttribute("role", "img");
      element.setAttribute("aria-label", label);
      return element;
    }

    const stateWord = (name) => ({ failure: t("stateFailed"), running: t("stateRunning"), success: t("statePassed") })[name] || t("stateSkipped");

    function banner(kindName, text, role, actionButton) {
      const box = make(doc, "p", `kit-banner${kindName ? ` kit-banner--${kindName}` : ""}`);
      if (role) box.setAttribute("role", role);
      box.append(make(doc, "span", "kit-banner__text", text));
      if (actionButton) {
        const holder = make(doc, "span", "kit-banner__action");
        holder.append(actionButton);
        box.append(holder);
      }
      return box;
    }

    function empty(icon, title, text, actionButton, extra) {
      const box = make(doc, "div", "kit-empty");
      if (icon) box.append(make(doc, "span", `kit-icon kit-icon--${icon} kit-empty__icon`));
      box.append(make(doc, "p", "kit-empty__title", title));
      if (text) box.append(make(doc, "p", "kit-empty__text", text));
      if (extra) box.append(extra);
      if (actionButton) {
        const holder = make(doc, "div", "kit-empty__actions");
        holder.append(actionButton);
        box.append(holder);
      }
      return box;
    }

    function problem(error) {
      const info = logic.problem(error.kind, lang, error.detail);
      let extra = null;
      if (info.command || info.detail) {
        extra = make(doc, "div", "kit-stack");
        if (info.command) extra.append(make(doc, "code", "kit-code", info.command));
        if (info.detail) extra.append(make(doc, "p", "kit-small kit-dim kit-mono kit-wrap", info.detail));
      }
      const retry = info.kind === "denied" ? null : button(t("retry"), () => actions.refresh(), "kit-btn--small kit-btn--primary");
      return empty(info.icon, info.title, info.text, retry, extra);
    }

    function section(title, count, children) {
      const box = make(doc, "section", "kit-section");
      const head = make(doc, "div", "kit-section__head");
      head.append(make(doc, "h2", "kit-section__title", title));
      if (count !== null && count !== undefined) head.append(make(doc, "span", "kit-section__count", String(count)));
      box.append(head, ...children);
      return box;
    }

    function handButton(number, className = "kit-btn--small") {
      const element = button(t("hand"), () => actions.hand(element, number), className);
      if (state.busy !== null) element.disabled = true;
      if (state.busy === number) element.textContent = t("sending");
      return element;
    }

    // -------------------------------------------------------------------- list

    function checksWord(checks) {
      if (checks.state === "failure") return t("checkFailing", { n: checks.failing.length });
      if (checks.state === "running") return t("checksPending", { n: checks.pending });
      if (checks.state === "success") return t("checksPassing");
      return t("checksNone");
    }

    const reviewWord = (review) =>
      ({ approved: t("reviewApproved"), changes: t("reviewChanges"), required: t("reviewRequired"), requested: t("reviewRequested") })[review] || "";

    function itemRow(row) {
      const item = make(doc, "li", "kit-stack");
      const open = make(doc, "button", "kit-row");
      open.type = "button";
      open.onclick = () => actions.open(row.number);
      const lead = make(doc, "span", "kit-row__lead");
      if (isPull) lead.append(glyph(row.checks.state === "none" ? "neutral" : row.checks.state, checksWord(row.checks)));
      else lead.append(glyph("neutral", `#${row.number}`));
      const main = make(doc, "span", "kit-row__main");
      const title = make(doc, "span", "kit-row__title", row.title);
      title.setAttribute("title", row.title);
      const meta = [`#${row.number}`, row.author, isPull ? row.branch : "", row.updatedAt ? since(row.updatedAt) : ""].filter(Boolean).join(" · ");
      const sub = make(doc, "span", "kit-row__sub", meta);
      sub.setAttribute("title", meta);
      main.append(title, sub);
      const second = [];
      if (isPull) {
        if (row.draft) second.push(t("draft"));
        second.push(checksWord(row.checks));
        const review = reviewWord(row.review);
        if (review) second.push(review);
      } else {
        if (row.assignees.length) second.push(`${t("assignees")}${lang === "fr" ? " : " : ": "}${row.assignees.join(", ")}`);
        if (row.labels.length) second.push(row.labels.join(", "));
      }
      open.append(lead, main);
      item.append(open);
      if (second.length) {
        const line = make(doc, "span", "kit-row__sub kit-small", second.join(" · "));
        line.setAttribute("title", second.join(" · "));
        main.append(line);
      }
      const actionsLine = make(doc, "div", "kit-actions gh-item-actions");
      actionsLine.append(handButton(row.number));
      item.append(actionsLine);
      return item;
    }

    function filterChip(label, key, count, danger) {
      const chip = make(doc, "button", "kit-filter", label);
      chip.type = "button";
      chip.setAttribute("aria-pressed", state.filters[key] ? "true" : "false");
      if (count !== null && count !== undefined) {
        chip.append(" ", make(doc, "span", `kit-filter__count${danger && count ? " kit-filter__count--danger" : ""}`, String(count)));
      }
      chip.onclick = () => actions.filters({ [key]: !state.filters[key] });
      return chip;
    }

    function filterBar() {
      const bar = make(doc, "div", "kit-stack");
      const search = make(doc, "div", "kit-search");
      const input = make(doc, "input");
      input.id = "filter-query";
      input.type = "search";
      input.value = state.filters.query;
      input.placeholder = t(isPull ? "searchPulls" : "searchIssues");
      input.setAttribute("aria-label", t(isPull ? "searchPulls" : "searchIssues"));
      input.oninput = () => actions.filters({ query: input.value });
      search.append(input);
      const chips = make(doc, "div", "kit-chips");
      if (isPull) {
        chips.append(
          filterChip(t("fFailing"), "failing", state.rows.filter((row) => row.checks.state === "failure").length, true),
          filterChip(t("fReview"), "review", state.rows.filter((row) => row.needsReview).length, false),
          filterChip(t("fDrafts"), "hideDrafts", null, false),
        );
      } else {
        chips.append(filterChip(t("fUnassigned"), "unassigned", state.rows.filter((row) => !row.assignees.length).length, false));
      }
      bar.append(search, chips);
      return bar;
    }

    function listView() {
      const rows = logic.filterRows(kind, state.rows, state.filters);
      const failing = isPull ? state.rows.filter((row) => row.checks.state === "failure").length : 0;
      const header = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      const heading = state.repo ? state.repo.nameWithOwner : t(isPull ? "tabPulls" : "tabIssues");
      const title = make(doc, "h1", "kit-header__title", heading);
      title.setAttribute("title", heading);
      const line = make(doc, "p", "kit-header__state");
      if (failing) line.append(make(doc, "span", "kit-state kit-state--failure", t("checkFailing", { n: failing })), " · ");
      line.append(t("open", { n: state.rows.length }));
      main.append(title, line);
      const side = make(doc, "div", "kit-header__actions");
      const refresh = button(state.refreshing ? t("refreshing") : t("refresh"), () => actions.refresh());
      refresh.disabled = state.refreshing;
      if (state.refreshedAt) refresh.setAttribute("title", t("updated", { when: since(state.refreshedAt) }));
      side.append(refresh);
      header.append(main, side);

      const page = make(doc, "main", "kit-page");
      page.append(header);
      if (state.stale) page.append(banner("warning", t("stale"), "status"));
      page.append(filterBar());
      if (!state.rows.length) {
        page.append(empty(null, t(isPull ? "emptyPulls" : "emptyIssues"), t(isPull ? "emptyPullsText" : "emptyIssuesText")));
      } else if (!rows.length) {
        page.append(empty(null, t("emptyFiltered"), t("emptyFilteredText", { n: state.rows.length }), button(t("clearFilters"), () => actions.clearFilters())));
      } else {
        const list = make(doc, "ul", "kit-rows kit-rows--group");
        for (const row of rows) list.append(itemRow(row));
        page.append(list);
      }
      return page;
    }

    // ------------------------------------------------------------------ detail

    function kv(pairs) {
      const list = make(doc, "dl", "kit-kv");
      for (const [name, value] of pairs) {
        if (value === "" || value === null || value === undefined) continue;
        list.append(make(doc, "dt", "", name), make(doc, "dd", "kit-wrap", value));
      }
      return list;
    }

    function textRow(glyphName, glyphLabel, title, sub) {
      const item = make(doc, "li", "kit-row kit-row--wrap");
      if (glyphName) {
        const lead = make(doc, "span", "kit-row__lead");
        lead.append(glyph(glyphName, glyphLabel));
        item.append(lead);
      }
      const main = make(doc, "span", "kit-row__main");
      main.append(make(doc, "span", "kit-row__title kit-wrap", title));
      if (sub) main.append(make(doc, "span", "kit-row__sub kit-wrap", sub));
      item.append(main);
      return item;
    }

    const excerpt = (text, max) => {
      const flat = String(text || "").replace(/\s+/g, " ").trim();
      return flat.length > max ? `${flat.slice(0, max)}…` : flat;
    };

    function detailBody(data, bar) {
      const page = make(doc, "main", "kit-page");
      page.append(bar);
      const header = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      const title = make(doc, "h1", "kit-header__title kit-wrap", `#${data.number} ${data.title}`);
      title.setAttribute("title", data.title);
      const line = make(doc, "p", "kit-header__state");
      if (isPull) {
        const checks = data.checks;
        if (checks.state !== "none") line.append(make(doc, "span", `kit-state kit-state--${checks.state === "neutral" ? "neutral" : checks.state}`, checksWord(checks)));
        const review = reviewWord(data.review);
        if (review) line.append(checks.state !== "none" ? " · " : "", review);
        if (data.draft) line.append(" · ", t("draft"));
      } else {
        line.append(data.state.toLowerCase());
      }
      main.append(title, line);
      header.append(main);
      page.append(header);

      const facts = [[t("author"), data.author]];
      if (isPull) {
        facts.push([t("branch"), `${data.branch} → ${data.base}`]);
        facts.push([t("files"), `${t("fileCount", { n: data.changedFiles })}, +${data.additions} −${data.deletions}`]);
      } else {
        facts.push([t("assignees"), data.assignees.join(", ")]);
      }
      facts.push([t("labels"), data.labels.join(", ")]);
      const about = make(doc, "section", "kit-section");
      about.append(kv(facts));
      const link = make(doc, "div", "kit-actions");
      link.append(handButton(data.number, "kit-btn--small kit-btn--primary"));
      const copy = button(t("copyLink"), () => actions.copy && actions.copy(copy, data.url), "kit-btn--small");
      link.append(copy);
      about.append(link, make(doc, "p", "kit-small kit-dim kit-wrap", data.url));
      page.append(about);

      if (isPull) {
        const ordered = [...data.checkList].sort((a, b) => ["failure", "running", "success", "neutral"].indexOf(a.state) - ["failure", "running", "success", "neutral"].indexOf(b.state));
        if (ordered.length) {
          const list = make(doc, "ul", "kit-rows");
          for (const check of ordered) list.append(textRow(check.state, stateWord(check.state), check.name, null));
          page.append(section(t("checks"), ordered.length, [list]));
        }
        if (data.reviews.length) {
          const list = make(doc, "ul", "kit-rows");
          for (const review of data.reviews) {
            const word = { APPROVED: t("reviewApproved"), CHANGES_REQUESTED: t("reviewChanges") }[review.state] || "";
            list.append(textRow(null, "", `${review.author || "?"}${word ? ` · ${word}` : ""}`, excerpt(review.body, 300)));
          }
          page.append(section(t("reviews"), data.reviews.length, [list]));
        }
        if (!data.threads) {
          page.append(section(t("threads"), null, [banner("warning", t("threadsUnavailable"))]));
        } else {
          const count = data.threads.unresolved.length;
          const list = make(doc, "ul", "kit-rows");
          for (const thread of data.threads.unresolved) {
            const where = thread.path ? `${thread.path}${thread.line ? `:${thread.line}` : ""}` : "";
            list.append(textRow(null, "", `${thread.author || "?"}${where ? ` · ${where}` : ""}`, excerpt(thread.body, 300)));
          }
          page.append(section(t("threads"), count, [count ? list : make(doc, "p", "kit-dim kit-small", t("noThreads"))]));
        }
        if (data.files.length) {
          const list = make(doc, "ul", "kit-rows");
          for (const file of data.files.slice(0, FILES_SHOWN)) {
            const item = textRow(null, "", file.path, `+${file.additions} −${file.deletions}`);
            list.append(item);
          }
          if (data.files.length > FILES_SHOWN) list.append(textRow(null, "", t("moreFiles", { n: data.files.length - FILES_SHOWN }), null));
          page.append(section(t("files"), data.files.length, [list]));
        }
      }

      const body = data.body.trim();
      const shown = body.length > BODY_SHOWN ? `${body.slice(0, BODY_SHOWN)}…` : body;
      const text = make(doc, "pre", "kit-pre", shown);
      text.setAttribute("tabindex", "0");
      page.append(section(t("description"), null, [body ? text : make(doc, "p", "kit-dim kit-small", t("noDescription"))]));

      if (!isPull && data.comments.length) {
        const list = make(doc, "ul", "kit-rows");
        for (const comment of data.comments.slice(-5)) list.append(textRow(null, "", comment.author || "?", excerpt(comment.body, 400)));
        page.append(section(t("comments"), data.comments.length, [list]));
      }
      return page;
    }

    function detailView() {
      const detail = state.detail;
      const wrap = make(doc, "main", "kit-page");
      const bar = make(doc, "div", "kit-actions");
      bar.append(button(`← ${t("back")}`, () => actions.back(), "kit-btn--small kit-btn--ghost"));
      if (detail.loading) {
        wrap.append(bar, empty(null, t("loading"), ""));
        return wrap;
      }
      if (detail.error) {
        wrap.append(bar, problem(detail.error));
        return wrap;
      }
      return detailBody(detail.data, bar);
    }

    // -------------------------------------------------------------------- page

    let page;
    if (state.view === "loading") {
      page = make(doc, "main", "kit-page");
      page.append(empty(null, t("loading"), ""));
    } else if (state.view === "error") {
      page = make(doc, "main", "kit-page");
      page.append(problem(state.error));
    } else if (state.view === "detail" && state.detail) {
      page = detailView();
    } else {
      page = listView();
    }
    container.replaceChildren(page);

    if (focused && doc.getElementById) {
      const input = doc.getElementById("filter-query");
      if (input && typeof input.focus === "function") {
        input.focus();
        if (caret && typeof input.setSelectionRange === "function") input.setSelectionRange(caret[0], caret[1]);
      }
    }
  }

  return { render };
});
