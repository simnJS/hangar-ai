"use strict";
// Draws the controller's state. Only text nodes and elements made by hand:
// nothing `gh` said is ever parsed as markup.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.CiView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const GLYPH = { queued: "◌", running: "●", success: "✓", failure: "✕", neutral: "–" };

  function make(doc, tag, className, text) {
    const element = doc.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined && text !== null) element.textContent = text;
    return element;
  }

  /** Renders `state` into `container`.
   *  @param {{ doc, logic, lang, now: () => number, actions: { open(id), back(), retry() } }} ctx */
  function render(container, state, ctx) {
    const { doc, logic, lang, actions } = ctx;
    const t = (key, vars) => logic.t(lang, key, vars);
    const kindLabel = (kind) => t(kind === "queued" ? "queued" : kind);

    function pill(kind) {
      const element = make(doc, "span", `pill ${kind}`);
      element.append(make(doc, "span", "glyph", GLYPH[kind] || GLYPH.neutral), " ", kindLabel(kind));
      return element;
    }

    function button(label, handler) {
      const element = make(doc, "button", "", label);
      element.type = "button";
      element.onclick = handler;
      return element;
    }

    const page = make(doc, "main", `view-${state.view}`);

    if (state.view === "loading") {
      page.append(make(doc, "p", "muted", t("loading")));
    } else if (state.view === "error") {
      const kind = (state.error && state.error.kind) || "error";
      const box = make(doc, "section", "problem");
      box.setAttribute("role", "alert");
      box.append(make(doc, "h2", "", t(`err.${kind}.title`)), make(doc, "p", "", t(`err.${kind}.hint`)));
      if (kind === "error" && state.error && state.error.detail) box.append(make(doc, "pre", "", state.error.detail));
      box.append(button(t("retry"), actions.retry));
      page.append(box);
    } else if (state.view === "picker") {
      const head = make(doc, "header", "bar");
      head.append(make(doc, "h1", "", t("runs")), button(t("refresh"), actions.retry));
      page.append(head);
      const runs = state.runs || [];
      if (!runs.length) page.append(make(doc, "p", "muted", t("noRuns")));
      const list = make(doc, "ul", "runs");
      for (const run of runs) {
        const item = make(doc, "li");
        const row = make(doc, "button", `run ${run.kind}`);
        row.type = "button";
        row.onclick = () => actions.open(run.id);
        const top = make(doc, "span", "run-top");
        top.append(pill(run.kind), make(doc, "span", "when", logic.relativeTime(run.createdAt, ctx.now(), lang)));
        row.append(
          top,
          make(doc, "span", "run-name", run.name),
          make(doc, "span", "run-title", run.title),
          make(doc, "span", "run-branch", `${run.branch} · #${run.id}`),
        );
        item.append(row);
        list.append(item);
      }
      page.append(list);
    } else if (state.run) {
      const view = logic.runView(state.run, ctx.now());
      const head = make(doc, "header", "bar");
      if (state.fromPicker) head.append(button(t("back"), actions.back));
      head.append(make(doc, "h1", "", view.name || t("run", { id: state.id })));
      page.append(head);
      const summary = make(doc, "section", "summary");
      summary.append(pill(view.kind), make(doc, "p", "title", view.title));
      const facts = make(doc, "dl", "facts");
      facts.append(
        make(doc, "dt", "", t("branch")), make(doc, "dd", "", view.branch || "–"),
        make(doc, "dt", "", t("elapsed")), make(doc, "dd", "", logic.formatDuration(view.elapsed)),
      );
      summary.append(facts);
      if (view.url) summary.append(make(doc, "p", "url", view.url));
      if (state.warning) summary.append(make(doc, "p", "warning", t("warning")));
      page.append(summary);

      page.append(make(doc, "h2", "", t("jobs")));
      if (!view.jobs.length) page.append(make(doc, "p", "muted", t("noJobs")));
      const jobs = make(doc, "ul", "jobs");
      for (const job of view.jobs) {
        const item = make(doc, "li", `job ${job.kind}`);
        const line = make(doc, "div", "job-line");
        line.append(make(doc, "span", `glyph ${job.kind}`, GLYPH[job.kind]), " ", make(doc, "span", "job-name", job.name));
        item.append(line);
        if (job.total) {
          const bar = make(doc, "progress");
          bar.max = job.total;
          bar.value = job.done;
          item.append(bar, make(doc, "span", "muted", " " + t("steps", { done: job.done, total: job.total })));
        }
        if (job.step) item.append(make(doc, "div", "step", "▸ " + job.step));
        jobs.append(item);
      }
      page.append(jobs);

      if (view.kind === "failure" && state.log) {
        const section = make(doc, "section", "log");
        section.append(make(doc, "h2", "", t("failedLog")));
        if (state.log.error) {
          section.append(make(doc, "p", "muted", t("logError", { reason: t(`err.${state.log.error}.title`) })));
        } else if (state.log.empty) {
          section.append(make(doc, "p", "muted", t("logEmpty")));
        } else {
          if (state.log.total > state.log.shown) {
            section.append(make(doc, "p", "muted", t("logTail", { shown: state.log.shown, total: state.log.total })));
          }
          const pre = make(doc, "pre", "log-text");
          for (const line of state.log.lines) {
            pre.append(make(doc, "span", `line ${line.kind}`, line.text), "\n");
          }
          section.append(pre);
        }
        page.append(section);
      }
    }

    container.replaceChildren(page);
  }

  return { render, GLYPH };
});
