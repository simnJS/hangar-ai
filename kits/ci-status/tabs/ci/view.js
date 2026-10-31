"use strict";
// Draws the controller's state with Hangar UI's classes (docs/kit-ui.md). Only
// text nodes and elements made by hand: nothing `gh` said is ever parsed as markup.
//
// The controller calls this every second while a run goes. What is on screen is
// only rebuilt when it changed, and the clocks are written in place: a redraw
// must not take the focus from a button, nor scroll a log back.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.CiView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  /** The library's word for each state. */
  const GLYPH = { queued: "waiting", running: "running", success: "success", failure: "failure", neutral: "neutral" };

  /** Fills `host` from `build`, unless what it shows is still `key`. */
  function part(host, key, build) {
    if (host.ciKey === key) return;
    host.ciKey = key;
    host.replaceChildren(...build());
  }

  /** Writes a line only when it changed: one written again would lose a selection in it. */
  function write(node, text) {
    if (node.ciText === text) return;
    node.ciText = text;
    node.textContent = text;
    node.title = text;
  }

  function createPage(ctx, id, fromPicker) {
    const { doc, logic, ui, t, actions } = ctx;
    /** What ticks, written again at each call. */
    let clocks = [];

    function el(tag, className, text) {
      const element = doc.createElement(tag);
      if (className) element.className = className;
      if (text !== undefined && text !== null) element.textContent = text;
      return element;
    }

    function button(label, className, handler) {
      const element = el("button", `kit-btn kit-btn--small ${className}`.trim(), label);
      element.type = "button";
      element.onclick = handler;
      return element;
    }

    function iconButton(label, icon, handler) {
      const element = button(undefined, "kit-btn--ghost kit-btn--icon", handler);
      element.title = label;
      element.setAttribute("aria-label", label);
      element.append(el("span", `kit-icon kit-icon--${icon}`));
      return element;
    }

    function truncated(tag, className, text) {
      const element = el(tag, className, text);
      element.title = text;
      return element;
    }

    function glyph(kind, said) {
      const element = el("span", `kit-glyph kit-glyph--${GLYPH[kind] || GLYPH.neutral}`);
      element.setAttribute("role", "img");
      element.setAttribute("aria-label", said);
      return element;
    }

    function loading(word) {
      const box = el("div", "kit-empty");
      box.append(el("span", "kit-state kit-state--running", t(word)));
      return box;
    }

    function empty(title, hint) {
      const box = el("div", "kit-empty");
      box.append(el("p", "kit-empty__title", title), el("p", "kit-empty__text", hint));
      return box;
    }

    /** Something to copy, on a line of its own: a sandboxed page can open no link. */
    function toCopy(text) {
      const line = el("div", "ci-copy");
      const copy = iconButton(t("copy"), "copy");
      ui.copyButton(copy, text);
      line.append(el("code", "kit-code", text), copy);
      return line;
    }

    function problem(error) {
      const kind = (error && error.kind) || "error";
      const box = el("div", "kit-empty");
      box.setAttribute("role", "alert");
      box.append(
        el("span", `kit-icon kit-icon--${kind === "denied" ? "lock" : "alert"} kit-empty__icon`),
        el("p", "kit-empty__title", t(`err.${kind}.title`)),
        el("p", "kit-empty__text", t(`err.${kind}.hint`)),
      );
      if (logic.ERROR_COPY[kind]) box.append(toCopy(logic.ERROR_COPY[kind]));
      if (kind === "error" && error && error.detail) {
        const said = el("pre", "kit-pre ci-said", error.detail);
        said.tabIndex = 0;
        box.append(said);
      }
      const again = el("div", "kit-empty__actions");
      again.append(button(t("retry"), "kit-btn--primary", actions.retry));
      box.append(again);
      return [box];
    }

    function section(title, count) {
      const box = el("section", "kit-section");
      const head = el("div", "kit-section__head");
      head.append(el("h2", "kit-section__title", title));
      if (count) head.append(el("span", "kit-section__count", String(count)));
      box.append(head);
      return box;
    }

    const where = (name, branch) => (name && branch ? t("onBranch", { name, branch }) : name || branch);

    /** A job: its state's glyph, its name, the step it is at, how long it took. */
    function jobRow(job) {
      // It wraps: the step a job is at is what its row is read for, at any width.
      const row = el("li", "kit-row kit-row--wrap");
      const said = t(`job.${job.word}`);
      const lead = el("span", "kit-row__lead");
      lead.append(glyph(job.kind, said));
      const main = el("span", "kit-row__main");
      main.append(truncated("span", "kit-row__title", job.name));
      const sub = job.step
        ? t("stepRunning", { step: job.step, at: job.at, total: job.total })
        : job.kind === "failure" && job.failedStep
          ? t("stepFailed", { step: job.failedStep })
          : null;
      if (sub) main.append(truncated("span", "kit-row__sub", sub));
      const end = el("span", "kit-row__end");
      row.append(lead, main, end);
      // A job that did not run has no time to give: its state's word instead.
      if (job.startedAt === null || job.kind === "neutral") end.textContent = said;
      else clocks.push(() => (end.textContent = ui.format.duration(logic.jobDuration(job, ctx.now()))));
      return row;
    }

    function jobRows(jobs) {
      const list = el("ul", "kit-rows");
      list.append(...jobs.map(jobRow));
      return list;
    }

    function logPart(host, log) {
      const box = section(t("failedLog"));
      host.replaceChildren(box);
      if (!log) return box.append(loading("logLoading"));
      if (log.empty) return box.append(empty(t("logEmpty.title"), t("logEmpty.hint")));
      if (log.error) {
        const said = el("p", "kit-banner kit-banner--error");
        const action = el("span", "kit-banner__action");
        action.append(button(t("retry"), "", actions.retry));
        said.append(el("span", "kit-banner__text", t("logError", { reason: t(`err.${log.error}.title`) })), action);
        return box.append(said);
      }
      if (log.truncated) {
        const cut = el("p", "kit-banner kit-banner--warning");
        cut.append(el("span", "kit-banner__text", t("logCut")));
        box.append(cut);
      }
      const lines = el("div", "ci-log");
      box.append(lines);
      // It opens on the first error by itself: the kit only says which lines those are.
      ui.log(lines, { lines: log.lines, label: t("failedLog"), isError: logic.isErrorLine, numbers: false });
    }

    // What every state shares: a head that does not jump, a body, a foot.
    const head = el("header", "kit-header");
    if (id && fromPicker) {
      const back = iconButton(t("back"), "chevron-down", actions.back);
      back.className += " ci-back";
      head.append(back);
    }
    const main = el("div", "kit-header__main");
    const headline = el("div", "ci-headline");
    const title = el("h1", "kit-header__title");
    const word = el("span");
    word.hidden = true;
    headline.append(title, word);
    const meta = el("p", "kit-header__state");
    meta.hidden = true;
    main.append(headline, meta);
    head.append(main);

    // Busy, it refuses by itself rather than being disabled: a disabled button would
    // lose the keyboard, and fade the ring that says it is working.
    let refresh = null;
    let busy = false;
    if (!id) {
      const tools = el("div", "kit-header__actions");
      refresh = iconButton(t("refresh"), "restart", () => {
        if (!busy) actions.retry();
      });
      tools.append(refresh);
      head.append(tools);
    }

    const body = el("main", "kit-grow");
    body.tabIndex = 0;
    const content = el("div", "kit-page ci-page");
    body.append(content);

    const foot = el("div", "kit-bar kit-bar--foot ci-foot");
    foot.hidden = true;

    write(title, id ? t("run", { id }) : t("runs"));

    let slots = null;

    function showRun(state) {
      const view = logic.runView(state.run, ctx.now());
      write(title, view.title || view.name || t("run", { id }));

      word.hidden = false;
      word.className = `kit-state kit-state--${GLYPH[view.kind] || GLYPH.neutral}`;
      write(word, t(view.word));

      const facts = [];
      if (view.elapsed !== null) facts.push(ui.format.duration(view.elapsed));
      if (view.endedAt !== null) facts.push(ui.format.since(view.endedAt, ctx.now()));
      if (view.name || view.branch) facts.push(where(view.name, view.branch));
      const line = facts.join(", ");
      meta.hidden = !line;
      write(meta, line);

      part(content, "run", () => {
        slots = { warning: el("p", "kit-banner kit-banner--warning"), jobs: el("div"), log: el("div"), others: el("div") };
        slots.warning.append(el("span", "kit-banner__text", t("warning")));
        return [slots.warning, slots.jobs, slots.log, slots.others];
      });
      slots.warning.hidden = !state.warning;

      // On a failure, what failed comes first, with its log right under it.
      const failed = view.kind === "failure" && view.failedJobs.length > 0;
      const first = failed ? view.failedJobs : view.jobs;
      const others = failed ? view.otherJobs : [];
      part(slots.jobs, JSON.stringify([view.kind, view.jobs]), () => {
        clocks = [];
        const jobs = section(t(failed ? "failedJobs" : "jobs"), first.length);
        jobs.append(first.length ? jobRows(first) : el("p", "ci-none kit-small kit-dim", t(view.done ? "noJobsRan" : "noJobs")));
        slots.others.replaceChildren();
        if (others.length) {
          const rest = section(t("otherJobs"), others.length);
          rest.append(jobRows(others));
          slots.others.append(rest);
        }
        return [jobs];
      });
      slots.others.hidden = others.length === 0;

      slots.log.hidden = view.kind !== "failure";
      const log = view.kind === "failure" ? state.log || "reading" : "none";
      if (slots.log.ciKey !== log) {
        slots.log.ciKey = log;
        if (log !== "none") logPart(slots.log, state.log);
      }

      part(foot, view.url, () => {
        if (!view.url) return [];
        const copy = iconButton(t("copyLink"), "copy");
        ui.copyButton(copy, view.url);
        return [truncated("span", "kit-truncate", view.url), copy];
      });
      foot.hidden = !view.url;
    }

    function showRuns(state) {
      const runs = state.runs || [];
      part(content, JSON.stringify(runs), () => {
        clocks = [];
        if (!runs.length) return [empty(t("noRuns.title"), t("noRuns.hint"))];
        const list = el("ul", "kit-rows");
        for (const run of runs) {
          const row = el("button", "kit-row");
          row.type = "button";
          row.onclick = () => actions.open(run.id);
          const lead = el("span", "kit-row__lead");
          lead.append(glyph(run.kind, t(run.word)));
          // The age goes under the title rather than beside it: « yesterday at 9:13 PM » would
          // leave a title little room at the panel's narrowest, and the title says which run.
          const sub = el("span", "kit-row__sub ci-run-sub");
          const age = el("span", "ci-age kit-num");
          sub.append(truncated("span", "kit-truncate", where(run.name, run.branch)), age);
          if (run.createdAt !== null) clocks.push(() => (age.textContent = ui.format.since(run.createdAt, ctx.now())));
          const text = el("span", "kit-row__main");
          text.append(truncated("span", "kit-row__title", run.title || run.name), sub);
          row.append(lead, text);
          const item = el("li");
          item.append(row);
          list.append(item);
        }
        return [list];
      });
    }

    /** Loading, or what stopped it: nothing about a run stands beside it. */
    function showNoContent(state) {
      word.hidden = true;
      foot.hidden = true;
      clocks = [];
      // Loading a run, its second line is kept empty, so the head does not grow once it is read.
      meta.hidden = state.view === "error" || !id;
      write(meta, " ");
      if (state.view === "error") part(content, `error:${JSON.stringify(state.error)}`, () => problem(state.error));
      else part(content, "loading", () => [loading(id ? "loadingRun" : "loadingRuns")]);
    }

    return {
      nodes: [head, body, foot],
      update(state) {
        if (refresh) {
          busy = Boolean(state.refreshing) || state.view === "loading";
          refresh.setAttribute("aria-busy", String(busy));
          refresh.setAttribute("aria-disabled", String(busy));
          // A first read says so in the page already: one ring is enough.
          refresh.children[0].className = state.refreshing ? "kit-glyph kit-glyph--running" : "kit-icon kit-icon--restart";
        }
        if (state.view === "run" && state.run) showRun(state);
        else if (state.view === "picker") showRuns(state);
        else showNoContent(state);
        for (const tick of clocks) tick();
      },
    };
  }

  /** Renders `state` into `container`.
   *  @param {{ doc, logic, ui, t, now: () => number, actions: { open(id), back(), retry() } }} ctx */
  function render(container, state, ctx) {
    const key = `${state.id || ""}|${state.fromPicker}|${ctx.ui.locale()}`;
    if (container.ciPageKey !== key) {
      container.ciPageKey = key;
      container.ciPage = createPage(ctx, state.id, state.fromPicker);
      container.replaceChildren(...container.ciPage.nodes);
    }
    container.ciPage.update(state);
  }

  return { render, GLYPH };
});
