"use strict";
// The tab's behaviour, apart from any DOM: reads the ports, the containers and the panes,
// refreshes every 10 s while the tab is shown, reads a container's logs, and sends the
// brief of "ask an agent to stop it". It stops nothing itself: its commands only read.
// `hangar` is `window.hangar`, or a fake of it; every change goes to `onChange(state)`.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.ServicesController = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const REFRESH_MS = 10000;

  function createController(options) {
    const { hangar, logic, t } = options;
    const now = options.now || Date.now;
    const setTimer = options.setTimer || ((fn, ms) => setTimeout(fn, ms));
    const clearTimer = options.clearTimer || ((handle) => clearTimeout(handle));
    const onChange = options.onChange || (() => undefined);

    const state = {
      loaded: false,
      refreshing: false,
      updatedAt: null,
      // Each of status: ok | error | denied | absent | stopped, with `kind` and `detail` for a failure.
      ports: { status: "ok", items: [], notes: [], kind: null, detail: "" },
      docker: { status: "ok", items: [], kind: null, detail: "" },
      panes: null, // panes.list, or null when it could not be read
      panesDenied: false,
      filter: "all", // all | blocks
      logs: null, // { id, status: loading | ok | error, lines, truncated, detail }
      ask: null, // { key, target, pane, status: idle | sending | sent | error, detail }
    };

    let generation = 0;
    let timer = null;
    let visible = true;
    let disposed = false;

    const emit = () => {
      if (!disposed) onChange(state);
    };

    /** Runs a declared command: { ok, stdout } or { ok: false, kind, detail }. */
    async function command(name, args) {
      let answer;
      try {
        answer = await hangar.call("command.run", args ? { name, args } : { name });
      } catch (error) {
        return { ok: false, kind: logic.classify({ code: error && error.code }), detail: String((error && error.message) || "") };
      }
      if (answer && answer.timedOut) return { ok: false, kind: "timeout", detail: "" };
      const text = logic.stripAnsi((answer && (answer.stderr || answer.stdout)) || "").trim().slice(0, 500);
      if (answer && answer.exitCode === 0) return { ok: true, stdout: String(answer.stdout || ""), stderr: String(answer.stderr || "") };
      return { ok: false, kind: "failed", detail: text, exitCode: answer ? answer.exitCode : null, stdout: String((answer && answer.stdout) || ""), stderr: String((answer && answer.stderr) || "") };
    }

    async function readPorts() {
      const answer = await command("ports");
      if (answer.ok) {
        const parsed = logic.parsePorts(answer.stdout);
        if (parsed.error === "no-tool") return { status: "error", items: [], notes: [], kind: "no-tool", detail: "" };
        if (parsed.error) return { status: "error", items: [], notes: [], kind: "failed", detail: "" };
        return { status: "ok", items: parsed.ports, notes: parsed.notes, kind: null, detail: "" };
      }
      if (answer.kind === "denied") return { status: "denied", items: [], notes: [], kind: "denied", detail: "" };
      if (answer.kind === "missing") return { status: "error", items: [], notes: [], kind: "no-node", detail: "" };
      if (answer.exitCode === 2 && /no-tool/.test(answer.stdout || "")) return { status: "error", items: [], notes: [], kind: "no-tool", detail: "" };
      return { status: "error", items: [], notes: [], kind: answer.kind, detail: answer.detail };
    }

    async function readDocker() {
      const answer = await command("docker-list");
      if (answer.ok) {
        const items = logic.parseDocker(answer.stdout);
        if (items === null) return { status: "error", items: [], kind: "failed", detail: "" };
        return { status: "ok", items, kind: null, detail: "" };
      }
      if (answer.kind === "denied") return { status: "denied", items: [], kind: "denied", detail: "" };
      if (answer.kind === "missing") return { status: "absent", items: [], kind: "absent", detail: "" };
      if (answer.kind === "failed") {
        const fault = logic.dockerFault(`${answer.stderr} ${answer.stdout}`);
        return { status: fault === "stopped" ? "stopped" : "error", items: [], kind: fault, detail: answer.detail };
      }
      return { status: "error", items: [], kind: answer.kind, detail: answer.detail };
    }

    async function readPanes() {
      try {
        return { panes: await hangar.call("panes.list"), denied: false };
      } catch (error) {
        return { panes: null, denied: Boolean(error && error.code === "denied") };
      }
    }

    function schedule() {
      if (timer !== null) clearTimer(timer);
      timer = null;
      if (disposed || !visible) return;
      timer = setTimer(() => {
        timer = null;
        refresh();
      }, REFRESH_MS);
    }

    /** Reads everything again. A refresh already going is not doubled. */
    async function refresh() {
      if (state.refreshing || disposed) return;
      state.refreshing = true;
      const mine = ++generation;
      emit();
      const [ports, docker, panes] = await Promise.all([readPorts(), readDocker(), readPanes()]);
      if (disposed || mine !== generation) return;
      state.ports = ports;
      state.docker = docker;
      state.panes = panes.panes;
      state.panesDenied = panes.denied;
      state.loaded = true;
      state.refreshing = false;
      state.updatedAt = now();
      // A container that is gone takes its open logs and its question with it.
      if (state.logs && docker.status === "ok" && !docker.items.some((c) => c.id === state.logs.id)) state.logs = null;
      if (state.ask && state.ask.status === "idle" && !stillThere(state.ask.target)) state.ask = null;
      emit();
      schedule();
    }

    function stillThere(target) {
      if (target.type === "container") return state.docker.items.some((c) => c.id === target.container.id);
      return state.ports.items.some((p) => p.port === target.row.port && p.pid === target.row.pid);
    }

    /** The tab is shown or hidden: a hidden one does not read. */
    function setVisible(isVisible) {
      if (visible === isVisible) return;
      visible = isVisible;
      if (!visible) {
        if (timer !== null) clearTimer(timer);
        timer = null;
      } else if (state.loaded && !state.refreshing) {
        refresh();
      }
    }

    function setFilter(filter) {
      state.filter = filter === "blocks" ? "blocks" : "all";
      emit();
    }

    async function showLogs(id) {
      if (!logic.ID_PATTERN.test(String(id))) return;
      if (state.logs && state.logs.id === id) {
        state.logs = null;
        emit();
        return;
      }
      state.logs = { id, status: "loading", lines: [], detail: "" };
      emit();
      const answer = await command("docker-logs", { id });
      if (disposed || !state.logs || state.logs.id !== id) return;
      if (answer.ok) {
        // A container writes its errors to stderr: both are the log, stderr after stdout.
        state.logs = { id, status: "ok", lines: logic.logLines([answer.stdout, answer.stderr].map((part) => part.replace(/\s+$/, "")).filter(Boolean).join("\n")), detail: "" };
      } else {
        state.logs = { id, status: "error", lines: [], detail: answer.detail || answer.kind };
      }
      emit();
    }

    /** Opens the question under a port or a container; asking again for the same one closes it. */
    function openAsk(key, target) {
      if (state.ask && state.ask.key === key) {
        state.ask = null;
      } else {
        const sendable = (state.panes || []).filter((pane) => pane.canSend);
        const owner = target.type === "port" && target.row.owner && target.row.owner.kind === "worker" ? target.row.owner.name : null;
        const first = sendable.find((pane) => owner && pane.name.toLowerCase() === owner.toLowerCase()) || null;
        state.ask = { key, target, pane: first ? first.name : "", status: "idle", detail: "" };
      }
      emit();
    }

    /** The person picked a pane: remembered, nothing redrawn under their hand. */
    function pickPane(name) {
      if (state.ask && state.ask.status !== "sending") state.ask.pane = name;
    }

    function closeAsk() {
      state.ask = null;
      emit();
    }

    async function sendAsk(sinceWords) {
      const ask = state.ask;
      if (!ask || ask.status === "sending" || !ask.pane) return;
      ask.status = "sending";
      emit();
      const text = logic.briefFor(ask.target, t, sinceWords || (() => ""));
      try {
        await hangar.call("panes.send", { pane: ask.pane, text });
        if (state.ask === ask) ask.status = "sent";
      } catch (error) {
        if (state.ask === ask) {
          ask.status = "error";
          ask.detail = error && error.code === "denied" ? "denied" : String((error && error.message) || "");
        }
      }
      emit();
    }

    let stopPanes = null;

    function start() {
      if (hangar.on) {
        stopPanes = hangar.on("panes.changed", async () => {
          const panes = await readPanes();
          if (disposed) return;
          state.panes = panes.panes;
          state.panesDenied = panes.denied;
          emit();
        });
      }
      return refresh();
    }

    function dispose() {
      disposed = true;
      if (timer !== null) clearTimer(timer);
      timer = null;
      if (stopPanes) stopPanes();
    }

    return { state, start, refresh, setVisible, setFilter, showLogs, openAsk, pickPane, closeAsk, sendAsk, dispose, REFRESH_MS };
  }

  return { createController, REFRESH_MS };
});
