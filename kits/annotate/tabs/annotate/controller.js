"use strict";
// The behaviour of the tab, with no DOM: what is open, the marks, and the sending.
// It takes `window.hangar` as an argument, and the two things only a browser can do
// (reading an image's size, painting the PNG) as `deps`, so a test drives it all.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.AnnotateController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  /**
   * @param {{
   *   hangar, t: (key: string, vars?: object) => string,
   *   lib: { logic, model, answer },
   *   deps: {
   *     decode: (url: string) => Promise<{ width: number, height: number }>,
   *     readFile: (file: any) => Promise<string>,
   *     exportPng: (state: object, scale: number) => Promise<string>,
   *     now?: () => Date,
   *   },
   *   onChange?: (state: object) => void,
   *   onTyping?: (state: object) => void,
   * }} options
   */
  function createController({ hangar, t, lib, deps, onChange, onTyping }) {
    const { logic, model, answer } = lib;
    const now = deps.now || (() => new Date());
    const state = {
      /** "empty", "loading" or "ready". */
      phase: "empty",
      /** What is wrong with opening, or null: { kind: "denied" | "info" | "error", title?, text, retry? }. */
      notice: null,
      image: null,
      source: null,
      shapes: [],
      tool: "arrow",
      color: model.COLORS[0].id,
      /** null: the whole image fits the panel. */
      zoom: null,
      comment: "",
      target: "",
      caller: null,
      busy: false,
      /** What the last send came to: { kind: "success" | "warning" | "error", text }. */
      status: null,
      /** The note a click just placed: its field takes the focus once. */
      focusNote: null,
    };
    let opening = 0;

    const changed = () => onChange && onChange(state);
    const reason = (error) => (error && error.message ? String(error.message) : "unknown error");
    const codeOf = (error) => (error && typeof error.code === "string" ? error.code : "");

    async function start() {
      let params = {};
      try {
        const answered = await hangar.call("tab.params");
        params = (answered && answered.params) || {};
        const pane = answered && answered.caller && answered.caller.pane;
        if (typeof pane === "string" && pane) {
          state.caller = pane;
          state.target = pane;
        }
      } catch (error) {
        // Not a reason to refuse: the tab still takes a pasted image.
      }
      const path = logic.cleanPath(params.path);
      if (path) await loadPath(path);
      else changed();
    }

    /**
     * The tab was asked for again (`tab.params` event): the latest to ask is the one to
     * answer. The same image keeps its marks, and so does any image that has some.
     */
    async function receive(data) {
      const pane = data && data.caller && data.caller.pane;
      if (typeof pane === "string" && pane) {
        state.caller = pane;
        state.target = pane;
      }
      const path = logic.cleanPath(data && data.params && data.params.path);
      if (path && path !== state.source && !state.shapes.length) await loadPath(path);
      else changed();
    }

    /** An image of the workspace, by its path. */
    async function loadPath(path) {
      const mine = ++opening;
      state.phase = "loading";
      state.notice = { kind: "info", text: t("loading", { path }) };
      changed();
      let file;
      try {
        file = await hangar.call("files.readBinary", { path });
      } catch (error) {
        if (mine !== opening) return;
        const code = codeOf(error);
        if (code === "denied") {
          state.notice = { kind: "denied", title: t("denied_title"), text: t("denied_text") };
        } else if (code === "not_found") {
          state.notice = { kind: "error", text: t("not_found", { path }) };
        } else if (code === "unsupported") {
          state.notice = { kind: "error", text: t("unsupported", { path }) };
        } else {
          state.notice = { kind: "error", text: t("read_failed", { path, reason: reason(error) }), retry: path };
        }
        state.phase = state.image ? "ready" : "empty";
        return changed();
      }
      if (mine !== opening) return;
      if (!file || !logic.openableMime(file.mime)) {
        state.notice = { kind: "error", text: t("unsupported", { path }) };
        state.phase = state.image ? "ready" : "empty";
        return changed();
      }
      await show(logic.dataUrl(file.mime, file.base64), logic.baseName(path), path, mine);
    }

    /** An image the person pasted, dropped or picked. */
    async function openFile(file) {
      if (!file) return;
      if (state.shapes.length) {
        state.status = { kind: "warning", text: t("keep_marks") };
        return changed();
      }
      if (!logic.openableMime(file.type)) {
        state.notice = { kind: "error", text: t("bad_image") };
        return changed();
      }
      if (file.size > logic.INPUT_MAX_BYTES) {
        state.notice = { kind: "error", text: t("too_large") };
        return changed();
      }
      const mine = ++opening;
      let url;
      try {
        url = await deps.readFile(file);
      } catch (error) {
        if (mine !== opening) return;
        state.notice = { kind: "error", text: t("bad_image") };
        return changed();
      }
      await show(url, file.name || "", null, mine);
    }

    async function show(url, name, source, mine) {
      let size;
      try {
        size = await deps.decode(url);
      } catch (error) {
        if (mine !== opening) return;
        state.notice = { kind: "error", text: t("bad_image") };
        state.phase = state.image ? "ready" : "empty";
        return changed();
      }
      if (mine !== opening) return;
      state.image = { url, name, width: size.width, height: size.height };
      state.source = source;
      state.shapes = [];
      state.zoom = null;
      state.status = null;
      state.notice = null;
      state.focusNote = null;
      state.phase = "ready";
      changed();
    }

    /** Back to nothing open: what "Another image" confirms. */
    function close() {
      opening += 1;
      state.image = null;
      state.source = null;
      state.shapes = [];
      state.zoom = null;
      state.status = null;
      state.notice = null;
      state.phase = "empty";
      changed();
    }

    function setTool(tool) {
      if (!model.TOOLS.includes(tool)) return;
      state.tool = tool;
      changed();
    }

    function setColor(id) {
      if (!model.COLORS.some((color) => color.id === id)) return;
      state.color = id;
      changed();
    }

    function setZoom(zoom) {
      state.zoom = zoom === null ? null : Math.min(4, Math.max(0.1, Number(zoom) || 1));
      changed();
    }

    /** A finished drag: an arrow, a rectangle or a stroke. Too small a one is dropped. */
    function addMark(shape) {
      if (!shape || state.phase !== "ready") return;
      state.shapes = [...state.shapes, shape];
      state.status = null;
      changed();
    }

    /** A click with the note tool: a numbered mark whose word is written in the list. */
    function addNote(x, y) {
      if (state.phase !== "ready" || !state.image) return;
      state.shapes = [...state.shapes, { type: "note", x, y, text: "", color: state.color }];
      state.status = null;
      state.focusNote = model.notesOf(state.shapes).length;
      changed();
    }

    /** The word of the note numbered `n`. Typing does not rebuild the field being typed in. */
    function setNoteText(n, text) {
      const note = model.notesOf(state.shapes).find((entry) => entry.n === n);
      if (!note) return;
      state.shapes = state.shapes.map((shape, i) => (i === note.index ? { ...shape, text: logic.clip(text, logic.NOTE_MAX) } : shape));
      if (onTyping) onTyping(state);
    }

    function undo() {
      if (!state.shapes.length) return;
      state.shapes = state.shapes.slice(0, -1);
      state.focusNote = null;
      changed();
    }

    function setComment(text) {
      state.comment = logic.clip(text, logic.COMMENT_MAX, true);
      if (onTyping) onTyping(state);
    }

    function setTarget(text) {
      state.target = logic.clip(text, logic.PANE_MAX).trim();
      if (onTyping) onTyping(state);
    }

    /** The PNG of the image with its marks, at the first scale whose base64 fits the limit. */
    async function paint() {
      for (const scale of logic.EXPORT_SCALES) {
        const base64 = await deps.exportPng(state, scale);
        if (base64.length <= logic.BASE64_MAX) return { base64, scale };
      }
      return null;
    }

    /** Writes the PNG then the notes, under a name no file has, and tells the agent. */
    async function send() {
      if (state.phase !== "ready" || !state.image || state.busy) return;
      state.busy = true;
      state.status = null;
      changed();
      try {
        state.status = await deliver();
      } catch (error) {
        state.status = { kind: "error", text: t("write_failed", { reason: reason(error) }) };
      }
      state.busy = false;
      changed();
    }

    async function deliver() {
      const image = state.image;
      const painted = await paint();
      if (!painted) return { kind: "error", text: t("too_heavy") };
      const { base64, scale } = painted;
      const date = now();
      const base = logic.stamp(date);
      let pngPath = null;
      let used = null;
      for (const name of logic.names(base)) {
        const path = `${logic.FOLDER}/${name}.png`;
        try {
          await hangar.call("files.writeBinary", { path, base64 });
          pngPath = path;
          used = name;
          break;
        } catch (error) {
          const code = codeOf(error);
          if (code === "conflict") continue;
          if (code === "denied") return { kind: "error", text: t("write_denied") };
          if (code === "unknown") return { kind: "error", text: t("write_unknown") };
          return { kind: "error", text: t("write_failed", { reason: reason(error) }) };
        }
      }
      if (!pngPath) return { kind: "error", text: t("write_failed", { reason: "name taken" }) };

      const width = Math.round(image.width * scale);
      const height = Math.round(image.height * scale);
      const px = (value) => Math.round(value * scale);
      const notes = model.notesOf(state.shapes).map((note) => ({ n: note.n, x: px(note.x), y: px(note.y), text: note.text }));
      const counts = model.counts(state.shapes);
      const jsonPath = `${logic.FOLDER}/${used}.json`;
      let jsonSaved = true;
      let jsonProblem = null;
      const body = answer.record({
        png: `${used}.png`,
        width,
        height,
        source: state.source,
        comment: state.comment.trim(),
        notes,
        marks: model.describe(state.shapes, scale),
        createdAt: date.toISOString(),
      });
      try {
        await hangar.call("files.write", { path: jsonPath, text: `${JSON.stringify(body, null, 2)}\n` });
      } catch (error) {
        jsonSaved = false;
        jsonProblem = reason(error);
      }

      const pane = state.target.trim();
      if (!pane) {
        const text = t("saved", { png: pngPath });
        return jsonSaved ? { kind: "success", text, png: pngPath } : { kind: "warning", text: t("json_failed", { reason: jsonProblem }), png: pngPath };
      }
      const text = answer.message({
        pngPath,
        jsonPath,
        jsonSaved,
        width,
        height,
        source: state.source,
        comment: state.comment.trim(),
        notes,
        counts,
        reduced: scale < 1,
      });
      try {
        await hangar.call("panes.send", { pane, text });
      } catch (error) {
        const code = codeOf(error);
        if (code === "denied") return { kind: "warning", text: t("send_denied", { png: pngPath }), png: pngPath };
        if (code === "not_found") return { kind: "warning", text: t("send_not_found", { png: pngPath, pane }), png: pngPath };
        if (code === "unavailable") return { kind: "warning", text: t("send_unavailable", { png: pngPath, pane }), png: pngPath };
        return { kind: "warning", text: t("send_failed", { png: pngPath, pane, reason: reason(error) }), png: pngPath };
      }
      if (!jsonSaved) return { kind: "warning", text: t("json_failed", { reason: jsonProblem }), png: pngPath };
      return { kind: "success", text: t("sent", { pane, png: pngPath }), png: pngPath };
    }

    return {
      state,
      start,
      receive,
      loadPath,
      openFile,
      close,
      setTool,
      setColor,
      setZoom,
      addMark,
      addNote,
      setNoteText,
      undo,
      setComment,
      setTarget,
      send,
    };
  }

  return { createController };
});
