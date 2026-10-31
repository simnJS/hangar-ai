"use strict";
// Pure readers for what each system says about listening TCP ports. No process is
// started here: tools/ports.js runs the tools and hands their text to these, and the
// tests hand them recorded text (test/fixtures/).

/** `127.0.0.1:3000`, `[::]:25`, `*:80`, `127.0.0.53%lo:53` → { address, port }, or null. */
function splitEndpoint(text) {
  const match = /^(.*):(\d{1,5})$/.exec(String(text).trim());
  if (!match) return null;
  const port = Number(match[2]);
  if (port < 1 || port > 65535) return null;
  let address = match[1].replace(/^\[|\]$/g, "").replace(/%.*$/, "");
  if (address === "*" || address === "") address = "0.0.0.0";
  return { address, port };
}

/** Linux `ss -H -ltnp`. A process is only listed for sockets the user may see. */
function parseSs(text) {
  const rows = [];
  for (const line of String(text).split(/\r?\n/)) {
    const fields = line.trim().split(/\s+/);
    if (fields[0] !== "LISTEN" || fields.length < 5) continue;
    const local = splitEndpoint(fields[3]);
    if (!local) continue;
    const users = /users:\(\(("(?:[^"\\]|\\.)*"),pid=(\d+)/.exec(line);
    rows.push({ ...local, pid: users ? Number(users[2]) : null, name: users ? JSON.parse(users[1]) : null });
  }
  return rows;
}

/** `lsof -nP -iTCP -sTCP:LISTEN -F pcn`: p<pid>, c<command>, then one n<address:port> per socket. */
function parseLsof(text) {
  const rows = [];
  let pid = null;
  let name = null;
  for (const line of String(text).split(/\r?\n/)) {
    const tag = line[0];
    const value = line.slice(1);
    if (tag === "p") {
      pid = Number(value);
      name = null;
    } else if (tag === "c") {
      name = value.replace(/\\x([0-9a-f]{2})/gi, (_, hex) => String.fromCharCode(parseInt(hex, 16)));
    } else if (tag === "n" && pid !== null) {
      const local = splitEndpoint(value.replace(/\s*\(LISTEN\)$/, ""));
      if (local) rows.push({ ...local, pid, name });
    }
  }
  return rows;
}

/**
 * Windows: the JSON of the script in tools/ports.js, { listen: [{ LocalAddress, LocalPort,
 * OwningProcess }], procs: [{ ProcessId, Name, Start }] } (Start in ms since the epoch).
 * PowerShell writes a lone item as an object, not a list.
 */
function parseWindows(text) {
  const data = JSON.parse(String(text).replace(/^﻿/, ""));
  const list = (value) => (value === null || value === undefined ? [] : Array.isArray(value) ? value : [value]);
  const procs = new Map(list(data.procs).map((proc) => [Number(proc.ProcessId), proc]));
  const rows = [];
  for (const socket of list(data.listen)) {
    const port = Number(socket.LocalPort);
    if (!Number.isInteger(port) || port < 1 || port > 65535) continue;
    const pid = Number(socket.OwningProcess);
    const proc = procs.get(pid);
    rows.push({
      address: String(socket.LocalAddress || "0.0.0.0").replace(/%.*$/, ""),
      port,
      pid,
      name: proc && proc.Name ? String(proc.Name) : null,
      startedAt: proc && Number.isFinite(Number(proc.Start)) ? new Date(Number(proc.Start)).toISOString() : null,
    });
  }
  return rows;
}

const MONTHS = { Jan: 0, Feb: 1, Mar: 2, Apr: 3, May: 4, Jun: 5, Jul: 6, Aug: 7, Sep: 8, Oct: 9, Nov: 10, Dec: 11 };

/** `ps -o pid=,lstart=` under LC_ALL=C: `  812 Sat Oct 31 23:44:23 2026` (local time) → Map pid → ISO time. */
function parsePsStart(text) {
  const starts = new Map();
  for (const line of String(text).split(/\r?\n/)) {
    const match = /^\s*(\d+)\s+\w{3}\s+(\w{3})\s+(\d{1,2})\s+(\d{2}):(\d{2}):(\d{2})\s+(\d{4})\s*$/.exec(line);
    if (!match || !(match[2] in MONTHS)) continue;
    const [, pid, month, day, hour, minute, second, year] = match;
    const at = new Date(Number(year), MONTHS[month], Number(day), Number(hour), Number(minute), Number(second));
    if (!Number.isNaN(at.getTime())) starts.set(Number(pid), at.toISOString());
  }
  return starts;
}

/**
 * Which worker's pane a process was started in, from the two variables Hangar gives
 * its panes. Only these two names are looked for, in `/proc/<pid>/environ` (NUL
 * separated) or in what `ps eww` prints (space separated): nothing else of an
 * environment is kept, so no value of a secret passes through here.
 */
function readOwner(text) {
  const pick = (name) => {
    const match = new RegExp(`(?:^|[\\0\\s])${name}=([^\\0\\r\\n]*)`).exec(String(text));
    return match ? match[1] : "";
  };
  const base = pick("HANGAR_PORT_BASE").split(/\s/)[0];
  // `ps eww` runs the variables together with spaces: a name stops at the first one.
  const pane = pick("HANGAR_PANE_NAME").split(/\s/)[0].slice(0, 60);
  return { portBase: /^\d{1,5}$/.test(base) ? Number(base) : null, pane: pane || null };
}

/** One entry per port and process: IPv4 and IPv6 listeners of the same program count once. */
function merge(rows) {
  const byKey = new Map();
  for (const row of rows) {
    const key = `${row.port}:${row.pid === null ? "?" : row.pid}`;
    const known = byKey.get(key);
    if (known) {
      if (!known.addresses.includes(row.address)) known.addresses.push(row.address);
      known.name = known.name || row.name;
      known.startedAt = known.startedAt || row.startedAt || null;
    } else {
      byKey.set(key, { port: row.port, pid: row.pid, name: row.name || null, startedAt: row.startedAt || null, addresses: [row.address] });
    }
  }
  return [...byKey.values()].sort((a, b) => a.port - b.port || (a.pid || 0) - (b.pid || 0));
}

const isLoopback = (address) => address === "::1" || /^127\./.test(address);

module.exports = { splitEndpoint, parseSs, parseLsof, parseWindows, parsePsStart, readOwner, merge, isLoopback };
