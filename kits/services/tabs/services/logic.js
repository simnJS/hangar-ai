"use strict";
// What the Services tab knows, apart from any DOM and from Hangar: the tools' answers
// read into rows, which worker a port belongs to, what an error means, the brief an agent
// is sent, and the tab's words in English and French (the glossary of src/i18n.ts: tu,
// panneau, workspace, Settings).
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.ServicesLogic = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  // The pool every worker's block comes from (src/lib/workerControl.ts): 21000, 21020…
  const POOL_START = 21000;
  const POOL_END = 26999;
  const BLOCK_SIZE = 20;
  const LOG_LINES = 200;
  /** What `docker logs` is asked for: the id `docker ps` prints, in full or short. Never an option. */
  const ID_PATTERN = /^[0-9a-f]{12,64}$/;

  const STRINGS = {
    en: {
      tab: "Services",
      refresh: "Refresh",
      refreshing: "Reading…",
      updated: "Updated {time}",
      auto: "Refreshes every 10 s while this tab is shown.",
      ports: "Ports",
      containers: "Containers",
      onlyBlocks: "Worker ports",
      allPorts: "All",
      loading: "Reading ports and containers…",
      noPorts: "Nothing is listening",
      noPortsHint: "No program is waiting for a connection on this computer. A server started later shows here.",
      noContainers: "No container",
      noContainersHint: "Docker answers, and has no container, running or stopped.",
      pid: "pid {pid}",
      started: "started {since}",
      loopback: "local only",
      exposed: "reachable from the network",
      unknownProgram: "Unknown program",
      hiddenNote: "Some programs belong to another account: their name is not shown.",
      blockOf: "Port block of {name}",
      blockGone: "Port block of {name}, whose pane is closed",
      blockFree: "Worker port block {from}–{to}",
      dockerPort: "Container {name}",
      ask: "Ask an agent to stop this",
      askTitle: "Ask an agent to stop {what}",
      askTo: "Send to",
      askNone: "No agent can be reached. Open a Claude Code pane in this workspace, then try again.",
      askSend: "Send",
      askCancel: "Cancel",
      askSent: "Sent to {pane}. It acts when it is free.",
      askFailed: "Not sent: {detail}",
      askDenied: "This tab may not send to a pane. Accept panes.send in Settings › Kits.",
      askNoPanes: "The panes could not be listed.",
      portWhat: "port {port}",
      containerWhat: "container {name}",
      briefPort:
        "Please stop what listens on port {port}: {program}, pid {pid}{since}. {owner}Check that it is yours and that nothing needs it, then stop it cleanly (not by killing other programs) and tell me what you stopped. If it is not yours, leave it and tell me.",
      briefContainer:
        "Please stop the Docker container {name} (id {id}, image {image}, {status}). Check that it is yours and that nothing needs it, then stop it with docker, and tell me what you stopped. If it is not yours, leave it and tell me.",
      briefOwnerWorker: "It was started in the pane of {name}. ",
      briefOwnerBlock: "It sits in the worker port block {from}–{to}. ",
      logs: "Logs",
      hideLogs: "Hide logs",
      logsLoading: "Reading the logs…",
      logsLabel: "Logs of {name}",
      logsEmpty: "This container has written nothing.",
      logsFailed: "The logs could not be read: {detail}",
      logsTail: "The last {n} lines.",
      noPublished: "No published port",
      state_running: "Running",
      state_paused: "Paused",
      state_restarting: "Restarting",
      state_created: "Created",
      state_exited: "Stopped",
      state_exited_code: "Stopped ({code})",
      state_dead: "Dead",
      state_other: "{state}",
      errPortsTitle: "The ports could not be listed",
      errPortsHint: "The listing tool did not answer. Try again.",
      errNoNodeTitle: "Node is not installed",
      errNoNodeHint: "This tab lists the ports with a small Node script. Install Node, then try again.",
      errNoToolTitle: "No tool lists the ports here",
      errNoToolHint: "It needs PowerShell on Windows, lsof on macOS, ss or lsof on Linux. Install one, then try again.",
      errTimeoutTitle: "It took too long",
      errTimeoutHint: "The system did not answer in time. Try again.",
      dockerAbsentTitle: "Docker is not installed",
      dockerAbsentHint: "Containers show here once the docker command is on your PATH. The ports above do not need it.",
      dockerStoppedTitle: "Docker is not running",
      dockerStoppedHint: "Start Docker Desktop or the Docker service, then refresh. The ports above do not need it.",
      dockerFailedTitle: "Docker answered with an error",
      deniedTitle: "This tab may not read this",
      deniedHint: "Accept the commands of Services in Settings › Kits, then reload the tab.",
      tryAgain: "Try again",
    },
    fr: {
      tab: "Services",
      refresh: "Actualiser",
      refreshing: "Lecture…",
      updated: "Actualisé {time}",
      auto: "S'actualise toutes les 10 s tant que cet onglet est affiché.",
      ports: "Ports",
      containers: "Conteneurs",
      onlyBlocks: "Ports des workers",
      allPorts: "Tous",
      loading: "Lecture des ports et des conteneurs…",
      noPorts: "Rien n'écoute",
      noPortsHint: "Aucun programme n'attend de connexion sur cet ordinateur. Un serveur lancé plus tard apparaît ici.",
      noContainers: "Aucun conteneur",
      noContainersHint: "Docker répond, et n'a aucun conteneur, démarré ou arrêté.",
      pid: "pid {pid}",
      started: "lancé {since}",
      loopback: "local uniquement",
      exposed: "joignable depuis le réseau",
      unknownProgram: "Programme inconnu",
      hiddenNote: "Certains programmes appartiennent à un autre compte : leur nom n'est pas affiché.",
      blockOf: "Bloc de ports de {name}",
      blockGone: "Bloc de ports de {name}, dont le panneau est fermé",
      blockFree: "Bloc de ports d'un worker {from}–{to}",
      dockerPort: "Conteneur {name}",
      ask: "Demander à un agent d'arrêter ça",
      askTitle: "Demander à un agent d'arrêter {what}",
      askTo: "Envoyer à",
      askNone: "Aucun agent n'est joignable. Ouvre un panneau Claude Code dans ce workspace, puis réessaie.",
      askSend: "Envoyer",
      askCancel: "Annuler",
      askSent: "Envoyé à {pane}. Il agit quand il est libre.",
      askFailed: "Non envoyé : {detail}",
      askDenied: "Cet onglet ne peut pas écrire à un panneau. Accepte panes.send dans Réglages › Kits.",
      askNoPanes: "Les panneaux n'ont pas pu être listés.",
      portWhat: "le port {port}",
      containerWhat: "le conteneur {name}",
      briefPort:
        "Arrête ce qui écoute sur le port {port} : {program}, pid {pid}{since}. {owner}Vérifie que c'est à toi et que rien n'en a besoin, puis arrête-le proprement (sans tuer d'autres programmes) et dis-moi ce que tu as arrêté. Si ce n'est pas à toi, n'y touche pas et dis-le-moi.",
      briefContainer:
        "Arrête le conteneur Docker {name} (id {id}, image {image}, {status}). Vérifie que c'est à toi et que rien n'en a besoin, puis arrête-le avec docker et dis-moi ce que tu as arrêté. Si ce n'est pas à toi, n'y touche pas et dis-le-moi.",
      briefOwnerWorker: "Il a été lancé dans le panneau de {name}. ",
      briefOwnerBlock: "Il est dans le bloc de ports de worker {from}–{to}. ",
      logs: "Journal",
      hideLogs: "Masquer le journal",
      logsLoading: "Lecture du journal…",
      logsLabel: "Journal de {name}",
      logsEmpty: "Ce conteneur n'a rien écrit.",
      logsFailed: "Le journal n'a pas pu être lu : {detail}",
      logsTail: "Les {n} dernières lignes.",
      noPublished: "Aucun port publié",
      state_running: "En marche",
      state_paused: "En pause",
      state_restarting: "Redémarre",
      state_created: "Créé",
      state_exited: "Arrêté",
      state_exited_code: "Arrêté ({code})",
      state_dead: "Mort",
      state_other: "{state}",
      errPortsTitle: "Les ports n'ont pas pu être listés",
      errPortsHint: "L'outil de liste n'a pas répondu. Réessaie.",
      errNoNodeTitle: "Node n'est pas installé",
      errNoNodeHint: "Cet onglet liste les ports avec un petit script Node. Installe Node, puis réessaie.",
      errNoToolTitle: "Aucun outil ne liste les ports ici",
      errNoToolHint: "Il faut PowerShell sous Windows, lsof sous macOS, ss ou lsof sous Linux. Installe-en un, puis réessaie.",
      errTimeoutTitle: "C'était trop long",
      errTimeoutHint: "Le système n'a pas répondu à temps. Réessaie.",
      dockerAbsentTitle: "Docker n'est pas installé",
      dockerAbsentHint: "Les conteneurs apparaissent ici dès que la commande docker est dans ton PATH. Les ports ci-dessus n'en ont pas besoin.",
      dockerStoppedTitle: "Docker n'est pas démarré",
      dockerStoppedHint: "Démarre Docker Desktop ou le service Docker, puis actualise. Les ports ci-dessus n'en ont pas besoin.",
      dockerFailedTitle: "Docker a répondu par une erreur",
      deniedTitle: "Cet onglet n'a pas le droit de lire ça",
      deniedHint: "Accepte les commandes de Services dans Réglages › Kits, puis recharge l'onglet.",
      tryAgain: "Réessayer",
    },
  };

  const stripAnsi = (text) => String(text).replace(/\u001b\[[0-9;?]*[ -/]*[@-~]/g, "").replace(/\u001b\][^\u0007]*\u0007/g, "");

  /** The first port of the worker block `port` falls in, or null for a port outside the pool. */
  function blockOf(port) {
    if (!Number.isInteger(port) || port < POOL_START || port > POOL_END) return null;
    return POOL_START + Math.floor((port - POOL_START) / BLOCK_SIZE) * BLOCK_SIZE;
  }

  /** What `node tools/ports.js` printed → { ports, notes } or { error }. */
  function parsePorts(stdout) {
    let data;
    try {
      data = JSON.parse(String(stdout).trim());
    } catch {
      return { error: "unreadable" };
    }
    if (!data || typeof data !== "object") return { error: "unreadable" };
    if (data.error) return { error: String(data.error) };
    if (!Array.isArray(data.ports)) return { error: "unreadable" };
    const ports = data.ports
      .filter((entry) => entry && Number.isInteger(entry.port))
      .map((entry) => ({
        port: entry.port,
        pid: Number.isInteger(entry.pid) ? entry.pid : null,
        name: typeof entry.name === "string" && entry.name ? entry.name : null,
        startedAt: typeof entry.startedAt === "string" && !Number.isNaN(Date.parse(entry.startedAt)) ? entry.startedAt : null,
        loopback: entry.loopback === true,
        portBase: Number.isInteger(entry.portBase) ? entry.portBase : null,
        pane: typeof entry.pane === "string" && entry.pane ? entry.pane : null,
      }))
      .sort((a, b) => a.port - b.port || (a.pid || 0) - (b.pid || 0));
    return { ports, notes: Array.isArray(data.notes) ? data.notes.filter((note) => typeof note === "string") : [] };
  }

  /** `0.0.0.0:5432->5432/tcp, :::5432->5432/tcp` → [{ host, container, proto }] without the IPv6 twins. */
  function publishedPorts(text) {
    const seen = new Set();
    const out = [];
    for (const part of String(text || "").split(",")) {
      const match = /(\d{1,5})(?:-(\d{1,5}))?->(\d{1,5})(?:-\d{1,5})?\/(\w+)/.exec(part);
      if (!match) continue;
      const first = Number(match[1]);
      const last = match[2] ? Number(match[2]) : first;
      for (let host = first; host <= Math.min(last, first + 99); host++) {
        const key = `${host}/${match[4]}`;
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({ host, container: Number(match[3]) + (host - first), proto: match[4] });
      }
    }
    return out;
  }

  /** `docker ps -a --format json`: one object per line (a JSON array is read as well). */
  function parseDocker(stdout) {
    const text = String(stdout).trim();
    if (!text) return [];
    let items;
    if (text.startsWith("[")) {
      try {
        items = JSON.parse(text);
      } catch {
        return null;
      }
    } else {
      items = [];
      for (const line of text.split(/\r?\n/)) {
        if (!line.trim()) continue;
        try {
          items.push(JSON.parse(line));
        } catch {
          return null;
        }
      }
    }
    return items
      .filter((item) => item && typeof item === "object" && ID_PATTERN.test(String(item.ID || "")))
      .map((item) => {
        const status = String(item.Status || "");
        const code = /^Exited \((-?\d+)\)/.exec(status);
        return {
          id: String(item.ID),
          name: String(item.Names || item.ID).split(",")[0],
          image: String(item.Image || ""),
          state: String(item.State || "").toLowerCase(),
          status,
          exitCode: code ? Number(code[1]) : null,
          ports: publishedPorts(item.Ports),
          portsText: String(item.Ports || ""),
        };
      })
      .sort((a, b) => stateRank(a) - stateRank(b) || a.name.localeCompare(b.name));
  }

  const stateRank = (container) => ({ running: 0, restarting: 1, dead: 2, paused: 3, created: 4, exited: container.exitCode ? 2 : 5 }[container.state] ?? 6);

  /** The glyph and the word of a container's state. */
  function containerState(container) {
    switch (container.state) {
      case "running":
        return { glyph: "success", key: "state_running" };
      case "restarting":
        return { glyph: "running", key: "state_restarting" };
      case "paused":
        return { glyph: "waiting", key: "state_paused" };
      case "created":
        return { glyph: "waiting", key: "state_created" };
      case "exited":
        return container.exitCode ? { glyph: "failure", key: "state_exited_code", vars: { code: container.exitCode } } : { glyph: "neutral", key: "state_exited" };
      case "dead":
        return { glyph: "failure", key: "state_dead" };
      default:
        return { glyph: "neutral", key: "state_other", vars: { state: container.state || "?" } };
    }
  }

  /**
   * Whose a port is. A program that carries its pane's variables names it (Linux, macOS);
   * any other port in the pool belongs to a block, and a worker's panel can be gone.
   * `panes` is `panes.list` or null when it could not be read.
   */
  function ownerOf(port, panes) {
    const paneNames = panes ? new Map(panes.map((pane) => [String(pane.name).toLowerCase(), pane])) : null;
    if (port.pane) {
      const known = paneNames ? paneNames.get(port.pane.toLowerCase()) : null;
      return { kind: "worker", name: known ? known.name : port.pane, gone: paneNames ? !known : false, base: port.portBase };
    }
    const base = blockOf(port.port);
    if (base !== null) return { kind: "block", base };
    return null;
  }

  /** The rows of the port list, each with its owner and the container publishing it, if any. */
  function portRows(ports, panes, containers) {
    const byHost = new Map();
    for (const container of containers || []) for (const published of container.ports) byHost.set(published.host, container);
    return ports.map((port) => ({ ...port, owner: ownerOf(port, panes), container: byHost.get(port.port) || null }));
  }

  /** The brief an agent is sent to stop a port or a container: it checks first, and says what it did. */
  function briefFor(target, t, since) {
    if (target.type === "container") {
      const c = target.container;
      return t("briefContainer", { name: c.name, id: c.id.slice(0, 12), image: c.image || "?", status: c.status || c.state });
    }
    const row = target.row;
    let owner = "";
    if (row.owner && row.owner.kind === "worker") owner = t("briefOwnerWorker", { name: row.owner.name });
    else if (row.owner && row.owner.kind === "block") owner = t("briefOwnerBlock", { from: row.owner.base, to: row.owner.base + BLOCK_SIZE - 1 });
    const started = row.startedAt ? `, ${t("started", { since: since(row.startedAt) })}` : "";
    return t("briefPort", { port: row.port, program: row.name || t("unknownProgram"), pid: row.pid === null ? "?" : row.pid, since: started, owner });
  }

  /** What a command's refusal or exit means to the tab. */
  function classify(fault) {
    if (fault.code === "denied") return "denied";
    if (fault.code === "not_found") return "missing";
    if (fault.code === "busy") return "busy";
    if (fault.timedOut || fault.code === "timeout") return "timeout";
    return "failed";
  }

  /** `docker ps` failing: is the engine off, or is it another trouble? */
  function dockerFault(text) {
    return /cannot connect to the docker daemon|failed to connect to the docker api|error during connect|is the docker daemon running|docker daemon is not running|pipe\/docker|docker\.sock/i.test(text) ? "stopped" : "failed";
  }

  /** A container's log → lines for hangar.ui.log, the last LOG_LINES, with docker's stderr folded in. */
  function logLines(text) {
    const lines = stripAnsi(text).split(/\r?\n/);
    while (lines.length && lines[lines.length - 1] === "") lines.pop();
    return lines.slice(-LOG_LINES);
  }

  return { POOL_START, POOL_END, BLOCK_SIZE, LOG_LINES, ID_PATTERN, STRINGS, stripAnsi, blockOf, parsePorts, publishedPorts, parseDocker, containerState, ownerOf, portRows, briefFor, classify, dockerFault, logLines };
});
