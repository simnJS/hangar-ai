"use strict";
// Draws the controller's state with Hangar UI's classes (docs/kit-ui.md). Only
// elements made by hand and text: nothing a file or a caller said is parsed as
// markup. `render` answers a function that refreshes what typing changes, so a
// keystroke never rebuilds the field being typed in.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.EditorView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const MASK = "••••••••";
  const VALUE_LINES_MAX = 8;

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

  function dirName(path) {
    const slash = path.lastIndexOf("/");
    return slash < 0 ? "" : path.slice(0, slash + 1);
  }

  function focusedId(doc) {
    const active = doc.activeElement;
    return active && typeof active.getAttribute === "function" ? active.getAttribute("data-focus") : null;
  }

  function isSaveKey(event) {
    return (event.ctrlKey || event.metaKey) && !event.altKey && String(event.key).toLowerCase() === "s";
  }

  /** @param {{ doc, t, logic, actions, confirm?: Function }} ctx */
  function render(container, state, ctx) {
    const { doc, t, logic, actions } = ctx;
    const memory = ctx.memory || (ctx.memory = {});
    const keepFocus = focusedId(doc);
    const updaters = [];
    const afterAttach = [];

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

    /** The part that scrolls, and the padded column inside it. */
    function scroller() {
      const outer = make(doc, "main", "kit-grow");
      const page = make(doc, "div", "kit-page");
      outer.append(page);
      return { outer, page };
    }

    function stateWord(kind, word) {
      return make(doc, "span", `kit-state kit-state--${kind}`, word);
    }

    function empty({ icon, title, text, detail, action }) {
      const box = make(doc, "div", "kit-empty");
      if (icon) box.append(make(doc, "span", `kit-icon kit-icon--${icon} kit-empty__icon`));
      box.append(make(doc, "p", "kit-empty__title", title));
      if (text) box.append(make(doc, "p", "kit-empty__text", text));
      if (detail) box.append(make(doc, "p", "kit-empty__text kit-mono", detail));
      if (action) {
        const holder = make(doc, "div", "kit-empty__actions");
        holder.append(action);
        box.append(holder);
      }
      return box;
    }

    function section(title, count, ...content) {
      const element = make(doc, "section", "kit-section");
      const head = make(doc, "div", "kit-section__head");
      head.append(make(doc, "h2", "kit-section__title", title));
      if (count !== null && count !== undefined) head.append(make(doc, "span", "kit-section__count", count));
      element.append(head, ...content);
      return element;
    }

    /* ── the parts every file shows ───────────────────────────────── */

    function goBack(event) {
      if (!actions.isDirty()) return actions.back();
      const asked = ctx.confirm
        ? ctx.confirm(event && event.currentTarget, { message: t("leaveMessage"), confirm: t("leaveConfirm"), cancel: t("leaveCancel") })
        : Promise.resolve(false);
      return asked.then((leave) => (leave ? actions.back() : undefined));
    }

    function header(stateOf) {
      const head = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      const title = make(doc, "h1", "kit-header__title", logic.baseName(state.path));
      title.title = state.path;
      const line = make(doc, "p", "kit-header__state");
      const draw = () => {
        const parts = stateOf(state);
        const dir = dirName(state.path);
        if (dir) parts.push(make(doc, "span", "kit-mono", dir));
        const spaced = [];
        parts.forEach((part, index) => spaced.push(...(index ? [" ", part] : [part])));
        line.replaceChildren(...spaced);
        line.title = state.path;
      };
      draw();
      updaters.push(draw);
      main.append(title, line);
      head.append(main);
      if (state.fromPicker) {
        const actionsBox = make(doc, "div", "kit-header__actions");
        actionsBox.append(small(t("back"), "kit-btn--ghost", goBack, "back"));
        head.append(actionsBox);
      }
      return head;
    }

    function requestBanner() {
      if (!state.why) return null;
      const pane = state.caller ? t("askedBy", { pane: state.caller.pane }) : t("askedAnon");
      const element = make(doc, "p", "kit-banner");
      const text = make(doc, "span", "kit-banner__text");
      text.append(make(doc, "strong", "", pane), " ", state.why);
      element.append(text);
      return element;
    }

    function saveProblem() {
      if (!state.problem) return null;
      return banner("error", t(`write.${state.problem.code}`));
    }

    function tellProblem() {
      if (!state.told || state.told.status !== "failed") return null;
      const reasonKey = `tell.${state.told.code}`;
      const reason = logic.STRINGS.en[reasonKey] ? t(reasonKey) : t("tell.other");
      return banner(
        "warning",
        t("notTold", { pane: state.told.pane, reason }),
        small(t("tellAgain", { pane: state.told.pane }), "", actions.retryTell, "tell-again"),
      );
    }

    /** What is true right now, and the one thing to do: a line of its own at the foot of the page. */
    function foot(buildStatus, buildButtons) {
      const bar = make(doc, "footer", "kit-bar kit-bar--foot editor-foot");
      const draw = () => {
        const status = make(doc, "span", "editor-foot__status", buildStatus());
        status.setAttribute("role", "status");
        const buttons = make(doc, "span", "kit-actions");
        buttons.append(...buildButtons());
        bar.replaceChildren(status, buttons);
      };
      draw();
      updaters.push(draw);
      return bar;
    }

    function savedStatus() {
      if (state.busy) return t("saving");
      if (!state.told) return state.saved ? t("savedLocal") : "";
      if (state.told.status === "sent") {
        return t(state.view === "text" ? "toldText" : "told", { pane: state.told.pane });
      }
      if (state.told.status === "checked") return t("toldNoChange", { pane: state.told.pane });
      return state.saved ? t("savedLocal") : "";
    }

    const canTellNoChange = () =>
      Boolean(state.caller) && !actions.isDirty() && !state.saved && !(state.told && state.told.status !== "failed");

    /* ── the form ─────────────────────────────────────────────────── */

    function valueInput(row) {
      const id = `value:${row.key}`;
      const multiline = row.value.includes("\n");
      if (multiline && !row.revealed) {
        // A password field would drop the line breaks: there is nothing to edit until it shows.
        const hidden = make(doc, "input", "kit-input kit-mono");
        hidden.type = "password";
        hidden.disabled = true;
        hidden.placeholder = t("valueLines");
        hidden.setAttribute("aria-label", row.key);
        return hidden;
      }
      const field = multiline ? make(doc, "textarea", "kit-input kit-mono") : make(doc, "input", "kit-input kit-mono");
      if (multiline) field.rows = Math.min(VALUE_LINES_MAX, row.value.split("\n").length);
      else field.type = row.revealed ? "text" : "password";
      field.value = row.value;
      field.placeholder = t("valueEmpty");
      field.spellcheck = false;
      field.setAttribute("autocapitalize", "off");
      field.setAttribute("autocomplete", "new-password"); // no password manager offers to fill this in
      field.setAttribute("aria-label", row.key);
      field.setAttribute("data-focus", id);
      field.oninput = () => actions.editRow(row.key, field.value);
      field.onkeydown = (event) => {
        if (!isSaveKey(event)) return;
        event.preventDefault();
        actions.saveForm();
      };
      return field;
    }

    function rowMark(row) {
      if (row.asked && row.value === "") return stateWord("waiting", t("keyMissing"));
      if (row.present ? row.value !== row.original : row.value !== "") return make(doc, "span", "kit-chip", t("keyChanged"));
      return row.asked ? stateWord("success", t("keySet")) : null;
    }

    function keyRow(row) {
      const item = make(doc, "li", "kit-row kit-row--wrap editor-key");
      const main = make(doc, "div", "kit-row__main editor-key__main");
      const name = make(doc, "div", "editor-key__name");
      const mark = make(doc, "span", "editor-key__mark");
      const drawMark = () => {
        const now = rowMark(row);
        mark.replaceChildren(...(now ? [now] : []));
      };
      drawMark();
      updaters.push(drawMark);
      const title = make(doc, "span", "kit-mono kit-row__title", row.key);
      title.title = row.key;
      name.append(title, mark);
      const value = make(doc, "div", "editor-key__value");
      value.append(
        valueInput(row),
        small(row.revealed ? t("hide") : t("show"), "kit-btn--ghost", () => actions.toggleReveal(row.key), `reveal:${row.key}`),
      );
      main.append(name, value);
      if (row.hint) main.append(make(doc, "span", "kit-field__hint", row.hint));
      if (row.count > 1) main.append(make(doc, "span", "kit-field__hint", t("keyTwice", { n: row.count })));
      item.append(main);
      return item;
    }

    function ignoreBanner() {
      const ignore = state.ignore;
      if (!ignore || ignore.status === "ignored") return null;
      const file = logic.baseName(state.path);
      if (ignore.status === "unknown") return banner("", t("ignoreUnknown", { file }));
      const text = t(ignore.status === "no-file" ? "ignoreNoFile" : "ignoreWarning", { file });
      const add = small(
        ignore.busy ? t("ignoreAdding") : t("ignoreAdd", { pattern: ctx.gitignore.patternFor(state.path) }),
        "",
        actions.addToGitignore,
        "ignore-add",
      );
      add.disabled = Boolean(ignore.busy);
      const element = banner("warning", text, add);
      if (ignore.problem) {
        const code = logic.STRINGS.en[`write.${ignore.problem}`] ? ignore.problem : "other";
        element.append(make(doc, "span", "kit-field__error editor-wide", t("ignoreFailed", { reason: t(`write.${code}`) })));
      }
      return element;
    }

    function offeredSection(form) {
      if (!form.offered.length) return null;
      const list = make(doc, "ul", "kit-rows kit-rows--group");
      for (const offer of form.offered) {
        const item = make(doc, "li", "kit-row kit-row--wrap");
        const main = make(doc, "span", "kit-row__main");
        main.append(make(doc, "span", "kit-mono kit-row__title", offer.key));
        if (offer.hint) main.append(make(doc, "span", "kit-row__sub", offer.hint));
        item.append(main, small(t("add"), "", () => actions.addOffered(offer.key), `offer:${offer.key}`));
        list.append(item);
      }
      const part = section(t("offeredTitle", { file: form.exampleName }), form.offered.length, list);
      if (form.offered.length > 1) {
        const holder = make(doc, "div", "kit-section__actions");
        holder.append(small(t("addAll"), "kit-btn--ghost", actions.addAllOffered, "offer-all"));
        part.children[0].append(holder);
      }
      return part;
    }

    function newKeyField(form) {
      const field = make(doc, "div", "kit-field");
      const input = make(doc, "input", "kit-input kit-mono");
      input.type = "text";
      input.value = form.newKey;
      input.placeholder = t("newKeyPlaceholder");
      input.spellcheck = false;
      input.setAttribute("aria-label", t("newKeyLabel"));
      input.setAttribute("autocapitalize", "off");
      input.setAttribute("data-focus", "new-key");
      if (form.newKeyProblem) input.setAttribute("aria-invalid", "true");
      input.oninput = () => actions.setNewKey(input.value);
      input.onkeydown = (event) => {
        if (event.key !== "Enter") return;
        event.preventDefault();
        actions.addNewKey();
      };
      const line = make(doc, "div", "editor-key__value");
      line.append(input, small(t("add"), "", actions.addNewKey, "new-key-add"));
      field.append(line);
      if (form.newKeyProblem) field.append(make(doc, "span", "kit-field__error", t(form.newKeyProblem)));
      return field;
    }

    function dotenvView() {
      const form = state.form;
      const head = header(() => {
        const missing = form.rows.filter((row) => row.asked && row.value === "").length;
        if (missing) return [stateWord("waiting", t("stateMissing", { n: missing }))];
        if (state.saved) return [stateWord("success", t("stateSaved"))];
        if (!state.exists) return [stateWord("neutral", t("stateNew"))];
        return [];
      });
      const { outer, page } = scroller();
      for (const part of [requestBanner(), ignoreBanner(), saveProblem(), tellProblem()]) if (part) page.append(part);
      if (!state.exists && !state.why) page.append(banner("", t("newFile")));

      const asked = form.rows.filter((row) => row.asked);
      const rest = form.rows.filter((row) => !row.asked);
      const list = (rows) => {
        const element = make(doc, "ul", "kit-rows kit-rows--group");
        element.append(...rows.map(keyRow));
        return element;
      };
      if (asked.length) page.append(section(t("sectionAsked"), asked.length, list(asked)));
      if (rest.length) page.append(section(t("sectionFile"), rest.length, list(rest)));
      if (!form.rows.length) page.append(empty({ title: t("noKeys") }));
      const offered = offeredSection(form);
      if (offered) page.append(offered);
      page.append(section(t("newKeyLabel"), null, newKeyField(form)));
      if (form.otherLines) page.append(make(doc, "p", "kit-muted kit-small editor-note", t("otherLines", { n: form.otherLines })));

      const bar = foot(savedStatus, () => {
        const out = [];
        // With keys still to fill in, saying there is nothing to change is the lesser choice.
        const missing = form.rows.some((row) => row.asked && row.value === "");
        const offerTell = canTellNoChange();
        if (offerTell) {
          const tell = small(t("tellNoChange", { pane: state.caller.pane }), missing ? "" : "kit-btn--primary", actions.tellNoChange, "tell");
          tell.disabled = state.busy;
          out.push(tell);
        }
        if (!offerTell || missing) {
          const save = small(t("save"), "kit-btn--primary", actions.saveForm, "save");
          save.disabled = state.busy || !actions.isDirty();
          save.title = save.disabled && !state.busy ? t("nothingToSave") : "";
          out.push(save);
        }
        return out;
      });
      return [head, outer, bar];
    }

    /* ── diffs ────────────────────────────────────────────────────── */

    function diffBlock(hunks) {
      const box = make(doc, "div", "kit-log editor-diff");
      box.setAttribute("tabindex", "0");
      for (const hunk of hunks) {
        const line = make(doc, "div", "kit-log__line");
        if (hunk.op === "skip") {
          line.className = "kit-log__line kit-log__line--head";
          line.append(make(doc, "span", "kit-log__text", t("diffSkip", { n: hunk.count })));
        } else {
          if (hunk.op === "del") line.className = "kit-log__line kit-log__line--error";
          if (hunk.op === "add") line.className = "kit-log__line editor-add";
          const number = hunk.op === "add" ? hunk.b : hunk.a;
          const sign = hunk.op === "add" ? "+ " : hunk.op === "del" ? "− " : "  ";
          line.append(make(doc, "span", "kit-log__num", number), make(doc, "span", "kit-log__text", sign + hunk.text));
        }
        box.append(line);
      }
      return box;
    }

    /* ── the conflict ─────────────────────────────────────────────── */

    function conflictHead() {
      return header(() => [stateWord("failure", t("stateConflict"))]);
    }

    function conflictIntro(conflict) {
      return banner("error", conflict.gone ? t("conflictGone") : `${t("conflictTitle")}. ${t("conflictHint")}`);
    }

    function textConflictView(conflict) {
      const { outer, page } = scroller();
      page.append(conflictIntro(conflict), make(doc, "p", "kit-mono kit-small kit-dim editor-note", t("conflictDiff")), diffBlock(conflict.hunks));
      const bar = foot(
        () => "",
        () => [
          small(conflict.gone ? t("conflictWriteMine") : t("conflictKeepMine"), "", actions.conflictKeepMine, "conflict-mine"),
          small(t("conflictTakeTheirs"), "", actions.conflictTakeTheirs, "conflict-theirs"),
          small(t("conflictEdit"), "kit-btn--ghost", actions.conflictBack, "conflict-edit"),
        ],
      );
      return [conflictHead(), outer, bar];
    }

    function conflictChoice(row, side) {
      const label = make(doc, "label", "editor-choice");
      const radio = make(doc, "input");
      radio.type = "radio";
      radio.name = `choice-${row.key}`;
      radio.checked = row.choice === side;
      radio.setAttribute("data-focus", `choice:${side}:${row.key}`);
      radio.onchange = () => actions.conflictChoose(row.key, side);
      const value = row[side];
      let shown;
      if (value === null) shown = make(doc, "span", "kit-muted", t("absent"));
      else if (value === "") shown = make(doc, "span", "kit-muted", t("valueEmpty"));
      else if (row.revealed) shown = make(doc, "span", "kit-mono kit-wrap editor-value", value);
      else shown = make(doc, "span", "kit-mono kit-muted", MASK);
      label.append(radio, make(doc, "span", "kit-dim editor-choice__side", t(side)), shown);
      return label;
    }

    function formConflictView(conflict) {
      const { outer, page } = scroller();
      page.append(conflictIntro(conflict));
      if (conflict.rows.length) {
        const list = make(doc, "ul", "kit-rows kit-rows--group");
        for (const row of conflict.rows) {
          const item = make(doc, "li", "kit-row kit-row--wrap editor-key");
          const main = make(doc, "div", "kit-row__main editor-key__main");
          const name = make(doc, "div", "editor-key__name");
          const mark = make(doc, "span", "editor-key__mark");
          mark.append(small(row.revealed ? t("hide") : t("show"), "kit-btn--ghost", () => actions.conflictReveal(row.key), `conflict-reveal:${row.key}`));
          name.append(make(doc, "span", "kit-mono kit-row__title", row.key), mark);
          main.append(name, conflictChoice(row, "mine"), conflictChoice(row, "theirs"));
          item.append(main);
          list.append(item);
        }
        page.append(section(t("conflictRows"), conflict.rows.length, list));
      } else {
        page.append(banner("", t("conflictNone")));
      }
      if (conflict.changedOnDisk) page.append(make(doc, "p", "kit-small kit-dim editor-note", t("conflictTheirsChanged", { n: conflict.changedOnDisk })));
      const bar = foot(
        () => "",
        () => [
          small(t("conflictApply"), "kit-btn--primary", actions.conflictKeepMine, "conflict-apply"),
          small(t("conflictTakeTheirs"), "", actions.conflictTakeTheirs, "conflict-theirs"),
        ],
      );
      return [conflictHead(), outer, bar];
    }

    /* ── the editor ───────────────────────────────────────────────── */

    function checkWord(check) {
      if (!check || !check.kind) return null;
      const name = check.kind === "jsonc" ? "JSONC" : check.kind.toUpperCase();
      if (check.ok) return stateWord(check.partial ? "neutral" : "success", check.partial ? t("checkBasic", { kind: name }) : t("checkOk", { kind: name }));
      return stateWord("failure", check.line ? t("checkBrokenAt", { kind: name, line: check.line }) : t("checkBroken", { kind: name }));
    }

    function textHeader() {
      return header(() => {
        const word = checkWord(state.text.check);
        if (state.saved) return [stateWord("success", t("stateSaved"))];
        if (word) return [word];
        return state.exists ? [] : [stateWord("neutral", t("stateNew"))];
      });
    }

    function checkReason(check) {
      if (!check || !check.kind) return "";
      const name = check.kind === "jsonc" ? "JSONC" : check.kind.toUpperCase();
      if (check.ok) return check.partial ? t("checkPartial", { kind: name }) : "";
      const reason = t(`check.${check.code}`);
      return check.line ? t("checkBadAt", { kind: name, line: check.line, reason }) : t("checkBad", { kind: name, reason });
    }

    function gutterMarks() {
      const marks = new Set();
      const text = state.text;
      if (text.target) marks.add(text.target);
      if (text.check && !text.check.ok && text.check.line) marks.add(text.check.line);
      return marks;
    }

    function drawGutter(gutter, count, marks) {
      const parts = [];
      let run = "";
      for (let line = 1; line <= count; line++) {
        if (marks.has(line)) {
          if (run) parts.push(run);
          run = "";
          parts.push(make(doc, "span", "editor-gutter__mark", String(line)));
          run = "\n";
        } else {
          run += `${line}\n`;
        }
      }
      if (run) parts.push(run);
      gutter.replaceChildren(...parts);
    }

    function textEditView() {
      const text = state.text;
      const page = make(doc, "main", "editor-column kit-grow");
      const notes = make(doc, "div", "editor-notes");
      const fixed = [requestBanner(), saveProblem(), tellProblem()];
      if (text.far) fixed.push(banner("", t("lineJumpFar", { line: text.far.line, total: text.far.total })));
      if (!state.exists) fixed.push(banner("", t("newFile")));
      for (const part of fixed) if (part) notes.append(part);
      const reason = make(doc, "p", "editor-reason kit-small");
      const held = fixed.some(Boolean);
      const drawReason = () => {
        const check = state.text.check;
        let words = checkReason(check);
        if (check && !check.ok) {
          const bare = t(`check.${check.code}`);
          words = bare[0].toUpperCase() + bare.slice(1);
        }
        reason.textContent = words;
        reason.hidden = !words;
        reason.className = check && !check.ok ? "editor-reason kit-small editor-reason--bad" : "editor-reason kit-small kit-dim";
        notes.hidden = !held && !words;
      };
      drawReason();
      updaters.push(drawReason);
      notes.append(reason);

      const wrap = make(doc, "div", "editor-code kit-grow");
      const gutter = make(doc, "pre", "editor-gutter");
      gutter.setAttribute("aria-hidden", "true");
      const area = make(doc, "textarea", "editor-area");
      area.value = text.value;
      area.wrap = "off";
      area.spellcheck = false;
      area.setAttribute("autocapitalize", "off");
      area.setAttribute("aria-label", t("editorLabel"));
      area.setAttribute("data-focus", "editor");

      let drawnCount = -1;
      let drawnMarks = "";
      const syncGutter = () => {
        const count = state.text.value.split("\n").length;
        const marks = gutterMarks();
        const key = [...marks].join(",");
        if (count === drawnCount && key === drawnMarks) return;
        drawnCount = count;
        drawnMarks = key;
        drawGutter(gutter, count, marks);
        gutter.scrollTop = area.scrollTop;
      };
      syncGutter();
      updaters.push(syncGutter);

      area.oninput = () => actions.editText(area.value);
      area.onscroll = () => {
        gutter.scrollTop = area.scrollTop;
        memory.scrollTop = area.scrollTop;
        memory.scrollLeft = area.scrollLeft;
      };
      area.onkeydown = (event) => {
        if (!isSaveKey(event)) return;
        event.preventDefault();
        actions.review();
      };
      wrap.append(gutter, area);
      page.append(notes, wrap);

      afterAttach.push(() => {
        const jump = text.jump;
        if (jump) {
          // The caret moves there without taking the keyboard: a page never grabs it unasked.
          const lines = text.value.split("\n");
          const start = lines.slice(0, jump - 1).reduce((sum, line) => sum + line.length + 1, 0);
          if (typeof area.setSelectionRange === "function") area.setSelectionRange(start, start + lines[jump - 1].length);
          const view = doc.defaultView;
          const height = (view && parseFloat(view.getComputedStyle(area).lineHeight)) || 18;
          area.scrollTop = Math.max(0, (jump - 4) * height);
          gutter.scrollTop = area.scrollTop;
          actions.consumeJump();
        } else if (memory.scrollTop) {
          area.scrollTop = memory.scrollTop;
          area.scrollLeft = memory.scrollLeft || 0;
          gutter.scrollTop = area.scrollTop;
        }
      });

      const bar = foot(savedStatus, () => {
        if (canTellNoChange()) {
          const tell = small(t("tellNoChange", { pane: state.caller.pane }), "kit-btn--primary", actions.tellNoChange, "tell");
          tell.disabled = state.busy;
          return [tell];
        }
        const go = small(t("review"), "kit-btn--primary", actions.review, "review");
        go.disabled = state.busy || (!actions.isDirty() && state.exists);
        go.title = go.disabled && !state.busy ? t("nothingToSave") : "";
        return [go];
      });
      return [textHeader(), page, bar];
    }

    function textReviewView() {
      const text = state.text;
      const { outer, page } = scroller();
      for (const part of [saveProblem()]) if (part) page.append(part);
      const broken = text.check && !text.check.ok;
      if (broken) {
        const box = banner("error", checkReason(text.check));
        const say = make(doc, "label", "kit-switch editor-wide");
        const toggle = make(doc, "input");
        toggle.type = "checkbox";
        toggle.setAttribute("role", "switch");
        toggle.checked = text.ack;
        toggle.setAttribute("data-focus", "ack");
        toggle.onchange = () => actions.setAck(toggle.checked);
        say.append(toggle, make(doc, "span", "kit-switch__track"), t("ackBroken"));
        box.append(say);
        page.append(box);
      } else if (text.check && text.check.ok && text.check.partial) {
        page.append(make(doc, "p", "kit-small kit-dim editor-note", checkReason(text.check)));
      }
      const changed = text.hunks && text.hunks.some((hunk) => hunk.op === "add" || hunk.op === "del");
      page.append(section(t("diffTitle"), null, changed ? diffBlock(text.hunks) : make(doc, "p", "kit-small kit-dim editor-note", t("diffNone"))));
      const bar = foot(
        () => (state.busy ? t("saving") : ""),
        () => {
          const save = small(t("save"), "kit-btn--primary", actions.saveText, "save");
          save.disabled = state.busy || (broken && !text.ack) || (!changed && state.exists);
          return [small(t("backToEdit"), "kit-btn--ghost", actions.backToEdit, "edit"), save];
        },
      );
      return [textHeader(), outer, bar];
    }

    /* ── the picker ───────────────────────────────────────────────── */

    function pickerView() {
      const picker = state.picker;
      const head = make(doc, "header", "kit-header");
      const main = make(doc, "div", "kit-header__main");
      main.append(make(doc, "h1", "kit-header__title", t("pickTitle")));
      head.append(main);

      const tools = make(doc, "div", "kit-bar");
      const search = make(doc, "div", "kit-search");
      const input = make(doc, "input");
      input.type = "search";
      input.value = picker.filter;
      input.placeholder = t("pickFilter");
      input.setAttribute("aria-label", t("pickFilter"));
      input.setAttribute("data-focus", "filter");
      input.oninput = () => actions.setFilter(input.value);
      search.append(input);
      tools.append(search);

      const { outer, page } = scroller();
      const notes = make(doc, "div", "kit-stack");
      if (picker.truncated) notes.append(banner("warning", t("pickTruncated")));
      const list = make(doc, "div", "kit-stack");
      const titles = { env: "groupEnv", config: "groupConfig", other: "groupOther" };
      const drawList = () => {
        const groups = logic.filterGroups(state.picker.groups, state.picker.filter).filter((group) => group.files.length);
        if (!groups.length) {
          list.replaceChildren(empty({ title: t("pickEmpty") }));
          return;
        }
        list.replaceChildren(
          ...groups.map((group) => {
            const rows = make(doc, "ul", "kit-rows");
            for (const file of group.files) {
              const item = make(doc, "li");
              const row = make(doc, "button", "kit-row");
              row.type = "button";
              row.title = file.path;
              row.onclick = () => actions.pick(file.path);
              const text = make(doc, "span", "kit-row__main");
              text.append(make(doc, "span", "kit-row__title kit-mono", logic.baseName(file.path)));
              const dir = dirName(file.path);
              if (dir) text.append(make(doc, "span", "kit-row__sub kit-mono", dir));
              row.append(text);
              if (file.isNew) {
                const end = make(doc, "span", "kit-row__end");
                end.append(make(doc, "span", "kit-chip", t("pickNew")));
                row.append(end);
              }
              item.append(row);
              rows.append(item);
            }
            const part = section(t(titles[group.id]), group.files.length + group.more, rows);
            if (group.more) part.append(make(doc, "p", "kit-small kit-dim editor-note", t("pickMore", { n: group.more })));
            return part;
          }),
        );
      };
      drawList();
      updaters.push(drawList);
      if (picker.truncated) page.append(notes);
      page.append(list);
      return [head, tools, outer];
    }

    /* ── the rest ─────────────────────────────────────────────────── */

    function errorView() {
      const kind = (state.error && state.error.kind) || "error";
      const { outer, page } = scroller();
      const retry = kind === "bad-path"
        ? small(t("pickTitle"), "kit-btn--primary", actions.back, "pick")
        : small(t("retry"), "kit-btn--primary", actions.retry, "retry");
      page.append(
        empty({
          icon: kind === "denied" ? "lock" : "alert",
          title: t(`err.${kind}.title`),
          text: t(`err.${kind}.hint`),
          detail: kind === "denied" ? null : state.error && state.error.detail,
          action: retry,
        }),
      );
      return [outer];
    }

    function loadingView() {
      const { outer, page } = scroller();
      const inner = make(doc, "div", "kit-empty");
      inner.append(stateWord("running", t("loading")));
      page.append(inner);
      return [outer];
    }

    let parts;
    if (state.view === "picker") parts = pickerView();
    else if (state.view === "error") parts = errorView();
    else if (state.view === "loading") parts = loadingView();
    else if (state.conflict) parts = state.conflict.kind === "text" ? textConflictView(state.conflict) : formConflictView(state.conflict);
    else if (state.view === "dotenv") parts = dotenvView();
    else parts = state.text.stage === "review" ? textReviewView() : textEditView();

    container.replaceChildren(...parts);

    if (keepFocus) {
      const found = container.find
        ? container.find((node) => node.getAttribute && node.getAttribute("data-focus") === keepFocus)[0]
        : [...container.querySelectorAll("[data-focus]")].find((node) => node.getAttribute("data-focus") === keepFocus);
      if (found) found.focus();
    }
    for (const run of afterAttach) run();

    return () => updaters.forEach((update) => update());
  }

  return { render, MASK };
});
