"use strict";
// Draws the controller's state with Hangar UI's classes (docs/kit-ui.md). Only
// elements made by hand and text: nothing a file or a caller said is parsed as markup.
// `render` answers a function that refreshes what typing changes, so a keystroke never
// rebuilds the field being typed in, and gives `layout` for when the panel is resized.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.AnnotateView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const TOOL_KEYS = { arrow: "tool_arrow", rect: "tool_rect", free: "tool_free", note: "tool_note" };
  const TIP_KEYS = { arrow: "tip_arrow", rect: "tip_rect", free: "tip_free", note: "tip_note" };

  function make(doc, tag, className, text) {
    const element = doc.createElement(tag);
    if (className) element.className = className;
    if (text !== undefined && text !== null) element.textContent = text;
    return element;
  }

  function button(doc, label, className, handler, focusId) {
    const element = make(doc, "button", className ? `kit-btn ${className}` : "kit-btn", label);
    element.type = "button";
    element.onclick = handler;
    if (focusId) element.setAttribute("data-focus", focusId);
    return element;
  }

  function focusedId(doc) {
    const active = doc.activeElement;
    return active && typeof active.getAttribute === "function" ? active.getAttribute("data-focus") : null;
  }

  /** @param {{ doc, t, logic, model, actions, images, win?, confirm?, pickFile? }} ctx */
  function render(container, state, ctx) {
    const { doc, t, logic, model, actions } = ctx;
    const win = ctx.win || {};
    const keepFocus = focusedId(doc);
    const updaters = [];
    const afterAttach = [];
    const ready = state.phase === "ready" && state.image;
    let stage = null;
    let canvas = null;
    let draft = null;

    const small = (label, kind, handler, focusId) => button(doc, label, `kit-btn--small ${kind || ""}`.trim(), handler, focusId);

    function banner(kind, text, action) {
      const element = make(doc, "p", kind ? `kit-banner kit-banner--${kind}` : "kit-banner");
      if (kind === "error") element.setAttribute("role", "alert");
      element.append(make(doc, "span", "kit-banner__text", text));
      if (action) {
        const holder = make(doc, "span", "kit-banner__action");
        holder.append(action);
        element.append(holder);
      }
      return element;
    }

    function empty({ icon, title, text, action }) {
      const box = make(doc, "div", "kit-empty");
      if (icon) box.append(make(doc, "span", `kit-icon kit-icon--${icon} kit-empty__icon`));
      box.append(make(doc, "p", "kit-empty__title", title));
      if (text) box.append(make(doc, "p", "kit-empty__text", text));
      if (action) {
        const holder = make(doc, "div", "kit-empty__actions");
        holder.append(action);
        box.append(holder);
      }
      return box;
    }

    function section(title, count) {
      const head = make(doc, "div", "kit-section__head");
      head.append(make(doc, "h2", "kit-section__title", title));
      if (count !== undefined) head.append(make(doc, "span", "kit-section__count", String(count)));
      const element = make(doc, "section", "kit-section");
      element.append(head);
      return element;
    }

    const choose = (className) => small(t("choose"), className, () => ctx.pickFile && ctx.pickFile(), "choose");

    /* ── the canvas ───────────────────────────────────────────────── */

    function effectiveZoom() {
      if (state.zoom !== null) return state.zoom;
      const box = stage && stage.clientWidth ? stage.clientWidth : 0;
      const high = win.innerHeight ? Math.max(240, win.innerHeight - 260) : 0;
      return logic.fitZoom(state.image.width, state.image.height, box, high);
    }

    function paint() {
      if (!canvas || !ready) return;
      const zoom = effectiveZoom();
      const dpr = win.devicePixelRatio || 1;
      const { width, height } = state.image;
      canvas.style.width = `${Math.round(width * zoom)}px`;
      canvas.style.height = `${Math.round(height * zoom)}px`;
      canvas.width = Math.max(1, Math.round(width * zoom * dpr));
      canvas.height = Math.max(1, Math.round(height * zoom * dpr));
      const context = canvas.getContext && canvas.getContext("2d");
      if (!context) return;
      model.draw(context, { image: ctx.images(state.image.url), width, height, shapes: state.shapes, draft }, zoom * dpr);
      if (zoomLabel) zoomLabel.textContent = t("zoom_value", { n: Math.round(zoom * 100) });
    }

    function pointOf(event) {
      const rect = canvas.getBoundingClientRect ? canvas.getBoundingClientRect() : { left: 0, top: 0, width: 1, height: 1 };
      return model.toImagePoint(event.clientX, event.clientY, rect, state.image.width, state.image.height);
    }

    function wireCanvas() {
      let drag = null;
      canvas.onpointerdown = (event) => {
        if (event.button !== undefined && event.button !== 0) return;
        const point = pointOf(event);
        if (state.tool === "note") {
          actions.addNote(point.x, point.y);
          return;
        }
        drag = { start: point, end: point, points: [point] };
        if (canvas.setPointerCapture && event.pointerId !== undefined) canvas.setPointerCapture(event.pointerId);
        if (event.preventDefault) event.preventDefault();
      };
      canvas.onpointermove = (event) => {
        if (!drag) return;
        const point = pointOf(event);
        drag.end = point;
        drag.points = model.extend(drag.points, point, state.image.width, state.image.height);
        draft = model.fromDrag(state.tool, drag.start, drag.end, drag.points, state.color, state.image.width, state.image.height);
        paint();
      };
      const finish = (event) => {
        if (!drag) return;
        const point = pointOf(event);
        const shape = model.fromDrag(state.tool, drag.start, point, drag.points, state.color, state.image.width, state.image.height);
        drag = null;
        draft = null;
        if (shape) actions.addMark(shape);
        else paint();
      };
      canvas.onpointerup = finish;
      canvas.onpointercancel = () => {
        drag = null;
        draft = null;
        paint();
      };
    }

    /* ── the toolbar ──────────────────────────────────────────────── */

    function tools() {
      const group = make(doc, "div", "kit-chips");
      group.setAttribute("role", "group");
      group.setAttribute("aria-label", t("tools"));
      for (const tool of model.TOOLS) {
        const chip = make(doc, "button", "kit-filter", t(TOOL_KEYS[tool]));
        chip.type = "button";
        chip.setAttribute("aria-pressed", String(state.tool === tool));
        chip.setAttribute("data-focus", `tool:${tool}`);
        chip.onclick = () => actions.setTool(tool);
        group.append(chip);
      }
      return group;
    }

    function colors() {
      const group = make(doc, "div", "annotate-colors");
      group.setAttribute("role", "group");
      group.setAttribute("aria-label", t("color"));
      for (const color of model.COLORS) {
        const label = t(`color_${color.id}`);
        const swatch = make(doc, "button", "annotate-swatch");
        swatch.type = "button";
        swatch.title = label;
        swatch.setAttribute("aria-label", label);
        swatch.setAttribute("aria-pressed", String(state.color === color.id));
        swatch.setAttribute("data-focus", `color:${color.id}`);
        swatch.style.background = color.value;
        swatch.onclick = () => actions.setColor(color.id);
        group.append(swatch);
      }
      return group;
    }

    let zoomLabel = null;
    function zoomControls() {
      const group = make(doc, "div", "annotate-zoom");
      group.setAttribute("role", "group");
      group.setAttribute("aria-label", t("zoom"));
      const step = (direction) => () => actions.setZoom(logic.stepZoom(effectiveZoom(), direction));
      const out = small(t("zoom_out"), "kit-btn--icon", step(-1), "zoom-out");
      out.textContent = "−";
      out.title = t("zoom_out");
      out.setAttribute("aria-label", t("zoom_out"));
      const into = small(t("zoom_in"), "kit-btn--icon", step(1), "zoom-in");
      into.textContent = "+";
      into.title = t("zoom_in");
      into.setAttribute("aria-label", t("zoom_in"));
      zoomLabel = make(doc, "span", "kit-num kit-dim annotate-zoom__value", "");
      const fit = small(t("zoom_fit"), "kit-btn--ghost", () => actions.setZoom(null), "zoom-fit");
      fit.setAttribute("aria-pressed", String(state.zoom === null));
      group.append(out, zoomLabel, into, fit);
      return group;
    }

    /* ── the notes, comment and target ────────────────────────────── */

    function notesList() {
      const notes = model.notesOf(state.shapes);
      const part = section(t("notes_title"), notes.length);
      if (!notes.length) {
        part.append(make(doc, "p", "kit-small kit-dim", t("no_notes")));
        return part;
      }
      const list = make(doc, "ul", "kit-rows");
      for (const note of notes) {
        const item = make(doc, "li", "annotate-note");
        const badge = make(doc, "span", "annotate-note__n kit-num", String(note.n));
        const field = make(doc, "input", "kit-input");
        field.type = "text";
        field.value = note.text;
        field.placeholder = t("note_placeholder");
        field.maxLength = logic.NOTE_MAX;
        field.setAttribute("aria-label", t("note_label", { n: note.n }));
        field.setAttribute("data-focus", `note:${note.n}`);
        field.oninput = () => actions.setNoteText(note.n, field.value);
        item.append(badge, field);
        list.append(item);
        if (state.focusNote === note.n) afterAttach.push(() => field.focus && field.focus());
      }
      part.append(list);
      return part;
    }

    function fields() {
      const wrap = make(doc, "div", "kit-stack");
      const comment = make(doc, "label", "kit-field");
      comment.append(make(doc, "span", "kit-field__label", t("comment_label")));
      const area = make(doc, "textarea", "kit-input annotate-comment");
      area.value = state.comment;
      area.rows = 2;
      area.placeholder = t("comment_placeholder");
      area.maxLength = logic.COMMENT_MAX;
      area.setAttribute("aria-label", t("comment_label"));
      area.setAttribute("data-focus", "comment");
      area.oninput = () => actions.setComment(area.value);
      comment.append(area);

      const target = make(doc, "label", "kit-field");
      target.append(make(doc, "span", "kit-field__label", t("target_label")));
      const input = make(doc, "input", "kit-input");
      input.type = "text";
      input.value = state.target;
      input.placeholder = t("target_placeholder");
      input.maxLength = logic.PANE_MAX;
      input.setAttribute("autocomplete", "off");
      input.setAttribute("aria-label", t("target_label"));
      input.setAttribute("data-focus", "target");
      input.oninput = () => actions.setTarget(input.value);
      const hint = make(doc, "span", "kit-field__hint", "");
      const drawHint = () => {
        hint.textContent = state.caller && state.target === state.caller ? t("target_caller", { pane: state.caller }) : state.target ? "" : t("target_none");
      };
      drawHint();
      updaters.push(drawHint);
      target.append(input, hint);
      wrap.append(comment, target);
      return wrap;
    }

    /* ── the parts of the page ────────────────────────────────────── */

    function header() {
      const head = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      main.append(make(doc, "h1", "kit-header__title", t("tab")));
      if (ready) {
        const size = t("image_size", { w: state.image.width, h: state.image.height });
        const line = make(doc, "p", "kit-header__state kit-truncate", state.image.name ? `${state.image.name} · ${size}` : size);
        line.title = state.image.name || size;
        main.append(line);
      }
      head.append(main);
      if (ready) {
        const actionsBox = make(doc, "div", "kit-header__actions");
        const another = small(t("another"), "", async (event) => {
          if (state.shapes.length && ctx.confirm) {
            const yes = await ctx.confirm(another, { message: t("another_ask"), confirm: t("another_yes"), cancel: t("another_no"), danger: true });
            if (!yes) return;
          }
          actions.close();
        }, "another");
        actionsBox.append(another);
        head.append(actionsBox);
      }
      return head;
    }

    function noticeBox() {
      const notice = state.notice;
      if (!notice) return null;
      if (notice.kind === "denied") return empty({ icon: "lock", title: notice.title, text: notice.text, action: choose("kit-btn--primary") });
      if (notice.kind === "info") {
        const box = make(doc, "div", "kit-empty");
        box.append(make(doc, "span", "kit-state kit-state--running", notice.text));
        return box;
      }
      const retry = notice.retry ? small(t("retry"), "", () => actions.loadPath(notice.retry), "retry") : null;
      return banner("error", notice.text, retry);
    }

    function foot() {
      const bar = make(doc, "div", "kit-bar kit-bar--foot annotate-foot");
      const status = make(doc, "span", "annotate-foot__status kit-small");
      status.setAttribute("role", "status");
      if (state.status) {
        status.textContent = state.status.text;
        if (state.status.kind === "error") status.className += " annotate-foot__status--bad";
      }
      const send = button(doc, "", "kit-btn--primary kit-btn--small", () => actions.send(), "send");
      const label = () => {
        send.textContent = state.busy ? t("sending") : state.target ? t("send") : t("save");
      };
      label();
      send.disabled = Boolean(state.busy);
      updaters.push(label);
      bar.append(status, send);
      return bar;
    }

    const page = make(doc, "div", "kit-page");
    const scrolling = make(doc, "main", "kit-grow");
    scrolling.append(page);
    const parts = [header(), scrolling];

    if (state.notice && !(ready && state.notice.kind === "info")) {
      const box = noticeBox();
      if (box) page.append(box);
    }

    if (!ready) {
      if (state.phase !== "loading" && !(state.notice && state.notice.kind === "denied")) {
        page.append(empty({ title: t("empty_title"), text: t("empty_text"), action: choose("kit-btn--primary") }));
      }
    } else {
      const bar = make(doc, "div", "annotate-tools");
      bar.append(tools(), colors());
      const second = make(doc, "div", "annotate-tools");
      const undo = small(t("undo"), "", () => actions.undo(), "undo");
      undo.disabled = state.shapes.length === 0;
      second.append(undo, zoomControls());
      const tip = make(doc, "p", "kit-small kit-dim annotate-tip", t(TIP_KEYS[state.tool]));

      stage = make(doc, "div", "annotate-stage");
      stage.setAttribute("tabindex", "0");
      stage.setAttribute("role", "region");
      stage.setAttribute("aria-label", t("canvas"));
      canvas = make(doc, "canvas", "annotate-canvas");
      canvas.setAttribute("role", "img");
      canvas.setAttribute("aria-label", t("canvas"));
      wireCanvas();
      stage.append(canvas);

      page.append(bar, second, tip, stage, notesList(), fields());
      updaters.push(paint);
      afterAttach.push(paint);
    }
    parts.push(...(ready ? [foot()] : []));

    container.replaceChildren(...parts);

    if (keepFocus) {
      const found = container.find
        ? container.find((node) => node.getAttribute && node.getAttribute("data-focus") === keepFocus)[0]
        : [...container.querySelectorAll("[data-focus]")].find((node) => node.getAttribute("data-focus") === keepFocus);
      if (found && found.focus) found.focus();
    }
    for (const run of afterAttach) run();

    const refresh = () => updaters.forEach((update) => update());
    refresh.layout = paint;
    return refresh;
  }

  return { render };
});
