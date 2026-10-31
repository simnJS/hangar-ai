"use strict";
// The tab's behaviour, apart from any DOM. `hangar` is `window.hangar`, or a fake of
// it. It reports a change of what is on screen through `onChange(state)`, and a
// keystroke's consequences through `onTyping(state)`, so that typing never redraws
// the field being typed in. Values go to `files.write` and nowhere else.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.EditorController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const VALIDATE_WHILE_TYPING_MAX = 100000; // larger files are checked when reviewing

  /**
   * @param {object} options
   * @param {{call(method: string, params?: object): Promise<any>, on?(event: string, fn: Function): Function}} options.hangar
   * @param {{logic, dotenv, validate, diff, gitignore, answer}} options.lib
   * @param {string} [options.lang] Used only when no `t` is given.
   * @param {(key: string, vars?: object) => string} [options.t] The page's words.
   * @param {(state: object) => void} [options.onChange]
   * @param {(state: object) => void} [options.onTyping]
   */
  function createController(options) {
    const { hangar, lib } = options;
    const { logic, dotenv, validate, diff, gitignore, answer } = lib;
    const t = options.t || ((key, vars) => logic.t(options.lang || "en", key, vars));
    const onChange = options.onChange || (() => undefined);
    const onTyping = options.onTyping || (() => undefined);

    const state = {
      view: "loading", // loading | picker | dotenv | text | error
      path: null,
      caller: null,
      why: null,
      asked: [],
      fromPicker: false,
      exists: false,
      busy: false,
      problem: null, // what a write said: { code, detail }
      saved: false,
      told: null, // { status: "sent" | "failed" | "checked", pane, code? }
      error: null, // { kind, detail }: the whole page
      picker: null, // { groups, filter, truncated }
      form: null, // { rows, offered, exampleName, otherLines }
      ignore: null, // { status, busy, problem }
      text: null, // { value, original, stage, check, ack, hunks, jump, far }
      conflict: null,
    };

    let generation = 0; // a later file makes the earlier answers stale
    let disposed = false;
    let offParams = null;
    let lastTitle = null;
    let lastBadge = null;
    let doc = null; // the dotenv file as last read or written
    let lastRead = null; // the exact text the file held when last read, null when absent
    let exampleRows = [];
    let exampleName = null;
    let exampleValues = [];
    let unsent = null; // an answer that could not be sent, to try again as it is

    const emit = () => {
      if (!disposed) onChange(state);
    };
    const typed = () => {
      if (!disposed) onTyping(state);
    };

    async function setTitle(title) {
      if (title === lastTitle) return;
      lastTitle = title;
      try {
        await hangar.call("tab.setTitle", { title });
      } catch {
        /* a refused title leaves the tab's own */
      }
    }

    function missingCount() {
      if (state.view !== "dotenv" || !state.form) return 0;
      return state.form.rows.filter((row) => row.asked && row.value === "").length;
    }

    async function setBadge() {
      const missing = missingCount();
      let badge = null;
      if (missing) badge = { state: "neutral", text: t("badgeMissing", { n: missing }).slice(0, 12) };
      else if (state.saved) badge = { state: "success", text: t("badgeSaved").slice(0, 12) };
      const key = badge ? `${badge.state}|${badge.text}` : "none";
      if (key === lastBadge) return;
      lastBadge = key;
      try {
        await hangar.call("tab.setBadge", badge || { state: null });
      } catch {
        /* idem */
      }
    }

    async function readOrNull(path) {
      try {
        return await hangar.call("files.read", { path });
      } catch (error) {
        if (error && error.code === "not_found") return null;
        throw error;
      }
    }

    const KNOWN_ERRORS = new Set(["denied", "invalid", "failed", "unavailable", "timeout"]);

    function showError(error, kind) {
      state.view = "error";
      const code = error && error.code;
      state.error = { kind: kind || (KNOWN_ERRORS.has(code) ? code : "error"), detail: String((error && error.message) || "") };
      state.busy = false;
      setBadge();
      emit();
    }

    /* ── the agent's answer ───────────────────────────────────────── */

    /** The one place that speaks to an agent. `message` comes from answer.js, which is handed no value. */
    async function send(message, status) {
      if (!state.caller) {
        state.told = null;
        unsent = null;
        return;
      }
      const { pane } = state.caller;
      unsent = { message, status };
      try {
        await hangar.call("panes.send", { pane, text: message });
        state.told = { status, pane };
        unsent = null;
      } catch (error) {
        state.told = { status: "failed", pane, code: (error && error.code) || "other" };
      }
    }

    async function retryTell() {
      if (!unsent || state.busy) return;
      state.busy = true;
      emit();
      await send(unsent.message, unsent.status);
      state.busy = false;
      emit();
    }

    /* ── dotenv: the form ─────────────────────────────────────────── */

    function buildForm(fileDoc) {
      const present = dotenv.rows(fileDoc);
      const byKey = new Map(present.map((row) => [row.key, row]));
      const hints = new Map(exampleRows.map((row) => [row.key, row.hint]));
      const makeRow = (key, asked) => {
        const found = byKey.get(key);
        return {
          key,
          asked,
          present: Boolean(found),
          original: found ? found.value : null,
          value: found ? found.value : "",
          hint: (found && found.hint) || hints.get(key) || "",
          count: found ? found.count : 0,
          revealed: false,
          added: false,
          typed: false,
        };
      };
      const rows = state.asked.map((key) => makeRow(key, true));
      for (const row of present) if (!state.asked.includes(row.key)) rows.push(makeRow(row.key, false));
      const shown = new Set(rows.map((row) => row.key));
      const offered = exampleRows.filter((row) => !shown.has(row.key)).map((row) => ({ key: row.key, hint: row.hint }));
      return { rows, offered, exampleName, otherLines: dotenv.otherLines(fileDoc), newKey: "", newKeyProblem: null };
    }

    /** What the form changes: key → value. */
    function formEdits() {
      const edits = new Map();
      for (const row of state.form.rows) {
        if (row.present ? row.value !== row.original : row.added || row.value !== "") edits.set(row.key, row.value);
      }
      return edits;
    }

    async function readExample(path) {
      if (dotenv.isExamplePath(path)) return null;
      for (const candidate of dotenv.exampleCandidates(path)) {
        let text = null;
        try {
          text = await readOrNull(candidate);
        } catch {
          return null; // no template to offer: the form works without
        }
        if (text !== null) return { name: logic.baseName(candidate), doc: dotenv.parse(text) };
      }
      return null;
    }

    async function checkIgnore(path) {
      if (dotenv.isExamplePath(path)) return { status: "ignored" }; // a template is meant to be committed
      const sources = [];
      for (const source of gitignore.sourcesFor(path)) {
        try {
          const text = await readOrNull(source.path);
          if (text !== null) sources.push({ dir: source.dir, text });
        } catch {
          return { status: "unknown" };
        }
      }
      if (!sources.length) return { status: "no-file" };
      return { status: gitignore.isIgnored(sources, path) ? "ignored" : "not-ignored" };
    }

    async function openDotenv(path, text, token) {
      const example = await readExample(path);
      const ignore = await checkIgnore(path);
      if (token !== generation || disposed) return;
      doc = dotenv.parse(text === null ? "" : text);
      exampleRows = example ? dotenv.rows(example.doc) : [];
      exampleName = example ? example.name : null;
      exampleValues = example ? dotenv.allValues(example.doc) : [];
      Object.assign(state, {
        view: "dotenv",
        form: buildForm(doc),
        ignore,
        text: null,
        busy: false,
      });
      setBadge();
      emit();
    }

    function editRow(key, value) {
      const row = state.form && state.form.rows.find((other) => other.key === key);
      if (!row) return;
      row.value = value;
      state.saved = false;
      setBadge();
      typed();
    }

    function toggleReveal(key) {
      const row = state.form && state.form.rows.find((other) => other.key === key);
      if (!row) return;
      row.revealed = !row.revealed;
      emit();
    }

    function addOffered(key) {
      const form = state.form;
      const offer = form && form.offered.find((other) => other.key === key);
      if (!offer) return;
      form.offered = form.offered.filter((other) => other !== offer);
      form.rows.push({
        key: offer.key, asked: false, present: false, original: null, value: "", hint: offer.hint,
        count: 0, revealed: false, added: true, typed: false,
      });
      emit();
    }

    function addAllOffered() {
      if (!state.form) return;
      for (const offer of [...state.form.offered]) addOffered(offer.key);
    }

    function setNewKey(name) {
      if (!state.form) return;
      state.form.newKey = name;
      state.form.newKeyProblem = null;
      typed();
    }

    /** A key the person types. It may be a secret pasted in the wrong field, so it is never named to the agent. */
    function addNewKey() {
      const form = state.form;
      if (!form) return;
      const name = form.newKey.trim();
      if (!dotenv.isKey(name)) form.newKeyProblem = "newKeyBad";
      else if (form.rows.some((row) => row.key === name)) form.newKeyProblem = "newKeyTaken";
      else {
        form.offered = form.offered.filter((other) => other.key !== name);
        form.rows.push({
          key: name, asked: false, present: false, original: null, value: "", hint: "",
          count: 0, revealed: false, added: true, typed: true,
        });
        form.newKey = "";
        form.newKeyProblem = null;
      }
      emit();
    }

    /** Names and counts for the agent, read from what the file holds now. */
    function dotenvFacts(before, after) {
      const rows = dotenv.rows(after);
      const value = new Map(rows.map((row) => [row.key, row.value]));
      const typedNames = new Set(state.form.rows.filter((row) => row.typed).map((row) => row.key));
      const known = new Set([...state.asked, ...exampleRows.map((row) => row.key), ...dotenv.rows(before).map((row) => row.key)]);
      for (const name of typedNames) known.delete(name);
      return {
        path: state.path,
        known,
        values: [...dotenv.allValues(before), ...dotenv.allValues(after), ...exampleValues],
        stillEmpty: state.asked.filter((key) => !value.get(key)),
        alreadySet: state.asked.filter((key) => value.get(key)),
      };
    }

    async function afterFormSave(written) {
      const before = doc;
      const after = dotenv.parse(written);
      const changed = dotenv.changedKeys(before, after);
      const facts = dotenvFacts(before, after);
      const message = answer.dotenvSaved({ ...facts, set: changed.set, emptied: changed.emptied });
      doc = after;
      state.exists = true;
      state.form = buildForm(after);
      state.saved = true;
      await send(message, "sent");
    }

    function saveForm() {
      if (state.view !== "dotenv" || state.busy || state.conflict) return undefined;
      const edits = formEdits();
      if (!edits.size) return undefined;
      return persist(dotenv.serialize(dotenv.withValues(doc, edits)), afterFormSave);
    }

    async function addToGitignore() {
      if (!state.ignore || state.ignore.busy || !state.path) return;
      const path = state.path;
      state.ignore = { ...state.ignore, busy: true, problem: null };
      emit();
      try {
        const current = await readOrNull(".gitignore");
        const text = gitignore.withPattern(current === null ? "" : current, gitignore.patternFor(path));
        await hangar.call("files.write", current === null ? { path: ".gitignore", text } : { path: ".gitignore", text, expected: current });
        state.ignore = await checkIgnore(path);
      } catch (error) {
        state.ignore = { ...state.ignore, busy: false, problem: (error && error.code) || "other" };
      }
      emit();
    }

    /* ── any other text file: the editor ──────────────────────────── */

    function openText(path, text, line) {
      const original = diff.normalize(text === null ? "" : text);
      const total = original.split("\n").length;
      Object.assign(state, {
        view: "text",
        form: null,
        ignore: null,
        busy: false,
        text: {
          value: original,
          original,
          stage: "edit",
          check: validate.validate(path, original),
          ack: false,
          hunks: null,
          jump: line === null ? null : Math.min(line, total),
          target: line === null ? null : Math.min(line, total),
          far: line !== null && line > total ? { line, total } : null,
        },
      });
      setBadge();
      emit();
    }

    function editText(value) {
      const text = state.text;
      if (!text) return;
      text.value = value;
      state.saved = false;
      if (value.length <= VALIDATE_WHILE_TYPING_MAX) {
        text.check = validate.validate(state.path, value);
        text.ack = false;
      }
      setBadge();
      typed();
    }

    function consumeJump() {
      if (state.text) state.text.jump = null;
    }

    function review() {
      const text = state.text;
      if (!text || state.view !== "text") return;
      text.check = validate.validate(state.path, text.value);
      text.ack = false;
      text.hunks = diff.hunks(diff.diffLines(text.original, text.value));
      text.stage = "review";
      state.problem = null;
      emit();
    }

    function backToEdit() {
      if (!state.text) return;
      state.text.stage = "edit";
      emit();
    }

    function setAck(checked) {
      if (!state.text) return;
      state.text.ack = Boolean(checked);
      emit();
    }

    async function afterTextSave(written) {
      const text = state.text;
      text.original = diff.normalize(written);
      text.value = text.original;
      text.stage = "edit";
      text.hunks = null;
      text.ack = false;
      state.exists = true;
      state.saved = true;
      await send(answer.textSaved({ path: state.path }), "sent");
    }

    /** A broken file goes through only once the person has said they know. */
    function saveText() {
      const text = state.text;
      if (!text || state.view !== "text" || state.busy || state.conflict) return undefined;
      text.check = validate.validate(state.path, text.value);
      if (!text.check.ok && !text.ack) return undefined;
      if (text.value === text.original && state.exists) return undefined;
      return persist(diff.restoreEols(lastRead === null ? "" : lastRead, text.value), afterTextSave);
    }

    /* ── writing, and what happens when the file moved ────────────── */

    /** `expected` cannot say "still absent", so a new file is looked for again just before. */
    async function write(path, text) {
      if (lastRead === null) {
        const now = await readOrNull(path);
        if (now !== null) return { conflict: true, theirs: now };
        await hangar.call("files.write", { path, text });
        return { ok: true };
      }
      try {
        await hangar.call("files.write", { path, text, expected: lastRead });
        return { ok: true };
      } catch (error) {
        if (error && error.code === "conflict") return { conflict: true, theirs: await readOrNull(path) };
        throw error;
      }
    }

    async function persist(text, done) {
      const token = generation;
      Object.assign(state, { busy: true, problem: null, saved: false, told: null });
      emit();
      let outcome;
      try {
        outcome = await write(state.path, text);
      } catch (error) {
        outcome = { error };
      }
      if (token !== generation || disposed) return;
      if (outcome.error) {
        const code = outcome.error.code;
        state.busy = false;
        state.problem = { code: code === "denied" || code === "rate_limited" || code === "invalid" ? code : "other", detail: String(outcome.error.message || "") };
        emit();
        return;
      }
      if (outcome.conflict) {
        state.busy = false;
        state.conflict = state.view === "dotenv" ? formConflict(outcome.theirs) : textConflict(outcome.theirs);
        emit();
        return;
      }
      lastRead = text;
      await done(text);
      if (token !== generation || disposed) return;
      state.busy = false;
      setBadge();
      emit();
    }

    function textConflict(theirs) {
      const mine = state.text.value;
      const disk = diff.normalize(theirs === null ? "" : theirs);
      return { kind: "text", theirs, gone: theirs === null, hunks: diff.hunks(diff.diffLines(disk, mine)) };
    }

    function formConflict(theirs) {
      const edits = formEdits();
      const theirDoc = dotenv.parse(theirs === null ? "" : theirs);
      const disk = new Map(dotenv.rows(theirDoc).map((row) => [row.key, row.value]));
      const rows = [];
      for (const [key, mine] of edits) {
        if (disk.get(key) !== mine) rows.push({ key, mine, theirs: disk.has(key) ? disk.get(key) : null, choice: "mine", revealed: false });
      }
      const was = new Map(dotenv.rows(doc).map((row) => [row.key, row.value]));
      let changedOnDisk = 0;
      for (const key of new Set([...was.keys(), ...disk.keys()])) {
        if (!edits.has(key) && was.get(key) !== disk.get(key)) changedOnDisk++;
      }
      return { kind: "dotenv", theirs, gone: theirs === null, rows, edits, changedOnDisk };
    }

    function conflictChoose(key, choice) {
      const row = state.conflict && state.conflict.rows && state.conflict.rows.find((other) => other.key === key);
      if (!row || (choice !== "mine" && choice !== "theirs")) return;
      row.choice = choice;
      emit();
    }

    function conflictReveal(key) {
      const row = state.conflict && state.conflict.rows && state.conflict.rows.find((other) => other.key === key);
      if (!row) return;
      row.revealed = !row.revealed;
      emit();
    }

    /** Mine wins, on top of what the disk holds now. Only ever on the person's click. */
    function conflictKeepMine() {
      const conflict = state.conflict;
      if (!conflict || state.busy) return undefined;
      state.conflict = null;
      lastRead = conflict.theirs;
      if (conflict.kind === "text") {
        const text = state.text;
        text.original = diff.normalize(conflict.theirs === null ? "" : conflict.theirs);
        return persist(diff.restoreEols(conflict.theirs === null ? "" : conflict.theirs, text.value), afterTextSave);
      }
      const edits = new Map(conflict.edits);
      for (const row of conflict.rows) if (row.choice === "theirs") edits.delete(row.key);
      doc = dotenv.parse(conflict.theirs === null ? "" : conflict.theirs);
      return persist(dotenv.serialize(dotenv.withValues(doc, edits)), afterFormSave);
    }

    /** What is on disk wins, and what the person typed is dropped: only on their click too. */
    async function conflictTakeTheirs() {
      const conflict = state.conflict;
      if (!conflict || state.busy) return;
      state.conflict = null;
      lastRead = conflict.theirs;
      state.exists = conflict.theirs !== null;
      Object.assign(state, { saved: false, told: null, problem: null });
      await loadInto(state.path, conflict.theirs, null);
    }

    /** Back to the text with mine, measured against what the disk holds now. */
    function conflictBack() {
      const conflict = state.conflict;
      if (!conflict || conflict.kind !== "text") return;
      state.conflict = null;
      lastRead = conflict.theirs;
      state.exists = conflict.theirs !== null;
      state.text.original = diff.normalize(conflict.theirs === null ? "" : conflict.theirs);
      state.text.stage = "edit";
      emit();
    }

    /** The person looked and has nothing to change: the waiting agent still hears of it. */
    async function tellNoChange() {
      if (!state.caller || state.busy) return;
      let message = null;
      if (state.view === "dotenv") {
        if (formEdits().size) return;
        const facts = dotenvFacts(doc, doc);
        message = answer.dotenvChecked(facts);
      } else if (state.view === "text") {
        if (state.text.value !== state.text.original) return;
        message = answer.textChecked({ path: state.path });
      }
      if (!message) return;
      state.busy = true;
      emit();
      await send(message, "checked");
      state.busy = false;
      emit();
    }

    /* ── opening ──────────────────────────────────────────────────── */

    async function loadInto(path, text, line) {
      const token = generation;
      if (dotenv.isDotenvPath(path)) await openDotenv(path, text, token);
      else if (token === generation && !disposed) openText(path, text, line);
    }

    async function open(path, line) {
      const token = ++generation;
      Object.assign(state, {
        view: "loading", path, busy: false, problem: null, saved: false, told: null, error: null,
        conflict: null, form: null, text: null, ignore: null,
      });
      unsent = null;
      emit();
      await setTitle(logic.tabTitle(path));
      let text;
      try {
        text = await readOrNull(path);
      } catch (error) {
        if (token === generation && !disposed) showError(error);
        return;
      }
      if (token !== generation || disposed) return;
      lastRead = text;
      state.exists = text !== null;
      await loadInto(path, text, line);
    }

    async function showPicker() {
      const token = ++generation;
      Object.assign(state, {
        view: "loading", path: null, busy: false, problem: null, saved: false, told: null, error: null,
        conflict: null, form: null, text: null, ignore: null, fromPicker: false,
      });
      unsent = null;
      emit();
      await setTitle(t("tab"));
      setBadge();
      let listing;
      try {
        listing = await hangar.call("files.list");
      } catch (error) {
        if (token === generation && !disposed) showError(error);
        return;
      }
      if (token !== generation || disposed) return;
      const groups = logic.rankFiles(listing && listing.files);
      state.picker = { groups, filter: "", truncated: Boolean(listing && listing.truncated) };
      state.view = "picker";
      emit();
    }

    function pick(path) {
      const checked = logic.checkPath(path);
      if (!checked) return undefined;
      state.fromPicker = true;
      return open(checked, null);
    }

    function setFilter(filter) {
      if (!state.picker) return;
      state.picker.filter = filter;
      typed();
    }

    function isDirty() {
      if (state.view === "dotenv" && state.form) return formEdits().size > 0;
      if (state.view === "text" && state.text) return state.text.value !== state.text.original;
      return false;
    }

    /** Asked for again: the latest to ask is the one to answer, and what was typed stays. */
    function handleParams(message) {
      const read = logic.readParams(message);
      state.caller = read.caller;
      unsent = null;
      const editing = (state.view === "dotenv" || state.view === "text") && state.path;
      if (editing && (!read.path || read.path === state.path)) {
        state.why = read.why;
        if (state.view === "dotenv" && read.keys.length) {
          const fresh = read.keys.filter((key) => !state.asked.includes(key));
          state.asked = [...state.asked, ...fresh];
          for (const key of fresh) {
            const row = state.form.rows.find((other) => other.key === key);
            if (row) row.asked = true;
            else {
              state.form.offered = state.form.offered.filter((other) => other.key !== key);
              state.form.rows.push({
                key, asked: true, present: false, original: null, value: "", hint: "",
                count: 0, revealed: false, added: false, typed: false,
              });
            }
          }
          state.form.rows.sort((a, b) => Number(b.asked) - Number(a.asked));
        }
        state.told = null;
        state.saved = false;
        setBadge();
        emit();
        return undefined;
      }
      if (read.badPath) {
        state.path = null;
        showError(null, "bad-path");
        return undefined;
      }
      state.why = read.why;
      state.asked = read.keys;
      state.fromPicker = false;
      return read.path ? open(read.path, read.line) : showPicker();
    }

    return {
      state,
      async start() {
        if (typeof hangar.on === "function") offParams = hangar.on("tab.params", (data) => handleParams(data));
        let message = { params: {}, caller: null };
        try {
          message = await hangar.call("tab.params");
        } catch {
          /* no params: the picker */
        }
        return handleParams(message);
      },
      isDirty,
      pick, setFilter,
      /** From a file opened in the picker, back to the picker. The view asks first when there are edits. */
      back: () => showPicker(),
      retry() {
        if (state.path) return open(state.path, null);
        return showPicker();
      },
      editRow, toggleReveal, addOffered, addAllOffered, setNewKey, addNewKey, addToGitignore, saveForm,
      editText, consumeJump, review, backToEdit, setAck, saveText,
      conflictChoose, conflictReveal, conflictKeepMine, conflictTakeTheirs, conflictBack,
      tellNoChange, retryTell,
      dispose() {
        disposed = true;
        generation++;
        if (typeof offParams === "function") offParams();
      },
    };
  }

  return { createController };
});
