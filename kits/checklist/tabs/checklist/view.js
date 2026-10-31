"use strict";
// Draws the controller's state with Hangar UI's classes (docs/kit-ui.md). Only elements
// made by hand and text: nothing the list or an answer said is parsed as markup. A press
// redraws the page, so the scroll and the focus are put back where they were.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.ChecklistView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const GLYPH = { ok: "success", fail: "failure", skip: "neutral", redo: "waiting", todo: "neutral" };
  const STATE_LABEL = { ok: "ok", fail: "problem", skip: "skip", redo: "redo", todo: null };

  function make(doc, tag, className, text) {
    const element = doc.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined && text !== null) element.textContent = text;
    return element;
  }

  function find(container, id) {
    return typeof container.querySelector === "function" ? container.querySelector(`[data-focus="${id}"]`) : null;
  }

  /** @param {{ doc, t, actions, summary: () => string, copyButton?: Function }} ctx */
  function render(container, state, ctx) {
    const { doc, t, actions } = ctx;
    const memory = ctx.memory || (ctx.memory = {});
    const active = doc.activeElement;
    const keepFocus = memory.focusNext || (active && typeof active.getAttribute === "function" ? active.getAttribute("data-focus") : null);
    memory.focusNext = null;
    let scroller = null;

    const button = (label, className, handler, focusId) => {
      const element = make(doc, "button", `kit-btn ${className || ""}`.trim(), label);
      element.type = "button";
      element.onclick = handler;
      if (focusId) element.setAttribute("data-focus", focusId);
      return element;
    };

    const banner = (kind, text) => {
      const element = make(doc, "p", kind ? `kit-banner kit-banner--${kind}` : "kit-banner");
      if (kind === "error") element.setAttribute("role", "alert");
      element.append(make(doc, "span", "kit-banner__text", text));
      return element;
    };

    const frame = make(doc, "div", `chk-column view-${state.view}`);

    if (state.view === "loading") {
      const main = make(doc, "main", "kit-grow");
      const empty = make(doc, "div", "kit-empty");
      empty.append(make(doc, "span", "kit-state kit-state--running", t("loading")));
      main.append(empty);
      frame.append(main);
    } else if (state.view === "error") {
      frame.append(errorView());
    } else {
      listView();
    }

    function errorView() {
      const error = state.error || { kind: "error" };
      const kind = error.kind;
      const known = ["missing", "path", "not_found", "denied", "json", "empty", "shape", "error"].includes(kind) ? kind : "error";
      const detail = known === "shape" ? t(`shape.${error.code || "notObject"}`, error.vars) : error.detail || "";
      const main = make(doc, "main", "kit-grow");
      const empty = make(doc, "div", "kit-empty");
      empty.setAttribute("role", "alert");
      empty.append(make(doc, "span", `kit-icon kit-icon--${known === "denied" ? "lock" : known === "missing" ? "info" : "alert"} kit-empty__icon`));
      empty.append(make(doc, "p", "kit-empty__title", t(`err.${known}.title`)));
      empty.append(make(doc, "p", "kit-empty__text", t(`err.${known}.text`, { path: error.path || "", detail })));
      if (known !== "missing" && known !== "path") {
        const row = make(doc, "div", "kit-empty__actions");
        row.append(button(t("retry"), "kit-btn--small kit-btn--primary", () => actions.retry(), "retry"));
        empty.append(row);
      }
      main.append(empty);
      return main;
    }

    function listView() {
      const { list, keys, saved, counts } = state;

      const header = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      main.append(make(doc, "h1", "kit-header__title", list.title || t("untitled")));
      const line = make(doc, "p", "kit-header__state");
      if (counts.state === "running") {
        line.append(make(doc, "span", "kit-state kit-state--running", t("stateRunning", { done: counts.answered, total: counts.total })));
      } else if (counts.state === "failure") {
        line.append(make(doc, "span", "kit-state kit-state--failure", t("stateFailure", { n: counts.fail })));
      } else {
        line.append(make(doc, "span", "kit-state kit-state--success", t(counts.skip ? "stateSuccessSkipped" : "stateSuccess")));
      }
      main.append(line);
      header.append(main);
      frame.append(header);

      const progress = make(doc, "div", "chk-progress");
      const bar = make(doc, "div", `kit-progress${counts.done ? ` kit-progress--${counts.state}` : ""}`);
      bar.setAttribute("role", "progressbar");
      bar.setAttribute("aria-label", t("steps"));
      bar.setAttribute("aria-valuemin", "0");
      bar.setAttribute("aria-valuemax", String(counts.total));
      bar.setAttribute("aria-valuenow", String(counts.answered));
      const fill = make(doc, "div", "kit-progress__fill");
      fill.style.width = `${Math.round((counts.answered / counts.total) * 100)}%`;
      bar.append(fill);
      progress.append(bar);
      frame.append(progress);

      scroller = make(doc, "main", "kit-grow");
      scroller.setAttribute("tabindex", "-1");
      const page = make(doc, "div", "kit-page");
      const notes = make(doc, "div", "chk-notes");
      if (!state.persist) notes.append(banner("warning", t("persistOff")));
      if (state.send.status === "sent") notes.append(banner("", t("sentTo", { pane: state.send.pane })));
      if (state.send.status === "error") {
        const code = state.send.error.code;
        notes.append(banner("error", t(`send.${code}`, { detail: state.send.error.detail })));
      }
      page.append(notes);

      const section = make(doc, "section", "kit-section");
      const head = make(doc, "div", "kit-section__head");
      head.append(make(doc, "h2", "kit-section__title", t("steps")), make(doc, "span", "kit-section__count", String(counts.total)));
      section.append(head);
      const rows = make(doc, "ul", "kit-rows kit-rows--group");
      list.steps.forEach((step, index) => rows.append(stepRow(step, index, keys[index], saved)));
      section.append(rows);
      page.append(section);
      scroller.append(page);
      frame.append(scroller);
      frame.append(footer(counts));
    }

    function stepRow(step, index, key, saved) {
      const status = ctx.logic.statusOf(saved, key);
      const answer = saved.answers[key];
      const item = make(doc, "li", `kit-row kit-row--wrap chk-step chk-step--${status}`);
      const lead = make(doc, "span", "kit-row__lead");
      const glyph = make(doc, "span", `kit-glyph kit-glyph--${GLYPH[status]}`);
      glyph.setAttribute("role", "img");
      glyph.setAttribute("aria-label", STATE_LABEL[status] ? t(STATE_LABEL[status]) : t("todo"));
      lead.append(glyph);
      const body = make(doc, "div", "chk-main");
      body.append(make(doc, "span", "kit-row__title", `${index + 1}. ${step.text}`));
      if (step.expect) body.append(make(doc, "span", "kit-row__sub", t("expectedLine", { text: step.expect })));
      if (status === "redo") {
        const was = saved.redo[key];
        const redo = make(doc, "span", "chk-redo");
        redo.append(
          make(doc, "span", "kit-state kit-state--waiting", t("redo")),
          make(doc, "span", "kit-row__sub", was ? t("redoWas", { note: was }) : t("redoWasBare")),
        );
        body.append(redo);
      }

      const choices = make(doc, "div", "chk-choices");
      choices.setAttribute("role", "group");
      choices.setAttribute("aria-label", `${index + 1}`);
      for (const [name, label] of [["ok", "ok"], ["fail", "problem"], ["skip", "skip"]]) {
        const pressed = status === name;
        const choice = button(t(label), "kit-filter", () => {
          if (pressed) return actions.clear(index);
          if (name === "fail") memory.focusNext = `note-${index}`;
          else memory.focusNext = `s${index}-${name}`;
          return actions.answer(index, name);
        }, `s${index}-${name}`);
        choice.className = "kit-filter";
        choice.setAttribute("aria-pressed", pressed ? "true" : "false");
        choices.append(choice);
      }
      body.append(choices);

      if (status === "fail") {
        const field = make(doc, "label", "kit-field chk-note");
        field.append(make(doc, "span", "kit-field__label", t("noteLabel")));
        const input = make(doc, "textarea", "kit-input");
        input.value = answer.note || "";
        input.setAttribute("placeholder", t("notePlaceholder"));
        input.setAttribute("data-focus", `note-${index}`);
        input.setAttribute("maxlength", "2000");
        input.oninput = () => actions.setNote(index, input.value);
        field.append(input);
        body.append(field);
      }
      item.append(lead, body);
      return item;
    }

    function footer(counts) {
      const foot = make(doc, "div", "kit-bar kit-bar--foot chk-foot");
      const hint = make(doc, "span", "kit-small kit-dim chk-foot__hint");
      const actionsBox = make(doc, "div", "chk-foot__actions");
      const sending = state.send.status === "sending";
      const sent = state.send.status === "sent";

      if (!state.caller) {
        hint.textContent = t("noCaller");
        const copy = button(t("copy"), "kit-btn--small kit-btn--primary", undefined, "copy");
        copy.disabled = counts.answered === 0;
        if (typeof ctx.copyButton === "function") ctx.copyButton(copy, () => ctx.summary());
        actionsBox.append(copy);
      } else {
        hint.textContent = !counts.answered ? t("needAnswer") : counts.pending ? t("pending", { n: counts.pending }) : "";
        const label = sending ? t("sending") : sent ? t("sendAgain") : t("send");
        const send = button(label, `kit-btn--small${counts.done && !sent ? " kit-btn--primary" : ""}`, () => actions.send(), "send");
        send.disabled = counts.answered === 0 || sending;
        actionsBox.append(send);
      }
      foot.append(hint, actionsBox);
      return foot;
    }

    // Put the page back as it was: where it scrolled, and what had the focus.
    const previousScroll = memory.scrollTop || 0;
    container.replaceChildren(frame);
    if (scroller && state.view === "list") {
      scroller.scrollTop = previousScroll;
      scroller.onscroll = () => {
        memory.scrollTop = scroller.scrollTop;
      };
    } else {
      memory.scrollTop = 0;
    }
    if (keepFocus) {
      const target = find(container, keepFocus);
      if (target && typeof target.focus === "function") target.focus({ preventScroll: true });
    }
  }

  return { render };
}
);
