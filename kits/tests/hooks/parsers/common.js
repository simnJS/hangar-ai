"use strict";
// What every parser shares. A parser takes the output of a test run, already cleaned of
// colours (see `clean`), and answers one of:
//
//   null                                  it does not recognise this framework's summary
//   { framework, passed, failed, skipped, duration, failures, partial }
//
// `passed` and `duration` are null when the output does not say (a plain `go test` does not
// count tests). `partial` is true when the summary was missing and the numbers come from
// what could be seen: a run cut short. A parser never guesses "passed" from the absence of
// a failure: it needs the framework's own closing summary for that.

const FAILURES_MAX = 50;
const MESSAGE_MAX = 2000;
const NAME_MAX = 300;

// CSI sequences (colours, cursor moves) and OSC sequences (hyperlinks, titles).
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]|\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)|\u001b[()][A-Z0-9]|\u001b[=>]/g;

/** Output without colours, with `\n` line ends, and without what a progress bar overwrote. */
function clean(text) {
  return String(text || "")
    .replace(ANSI, "")
    .replace(/\r\n/g, "\n")
    .replace(/[^\n\r]*\r(?!\n)/g, "");
}

const splitLines = (text) => text.split("\n");

/** A block of lines as one message: dedented, no blank edges, cut at MESSAGE_MAX. */
function messageOf(lines) {
  const kept = lines.map((line) => line.replace(/\s+$/, ""));
  while (kept.length && !kept[0].trim()) kept.shift();
  while (kept.length && !kept[kept.length - 1].trim()) kept.pop();
  const indent = Math.min(...kept.filter((line) => line.trim()).map((line) => line.match(/^ */)[0].length), Infinity);
  const text = kept.map((line) => (Number.isFinite(indent) ? line.slice(Math.min(indent, line.match(/^ */)[0].length)) : line)).join("\n");
  return text.length > MESSAGE_MAX ? `${text.slice(0, MESSAGE_MAX)}\n…` : text;
}

const clip = (text, max = NAME_MAX) => (String(text).length > max ? `${String(text).slice(0, max - 1)}…` : String(text));

/** { name, file?, line?, message }, with what is missing left out. */
function failure(name, message, file, line) {
  const out = { name: clip(name), message: message || "" };
  if (file) out.file = clip(file, 400);
  if (Number.isFinite(line) && line > 0) out.line = line;
  return out;
}

/** "12.5s" / "274ms" / "1m 28s" / "0.545 s" → milliseconds, or null. */
function toMs(text) {
  const source = String(text || "").trim();
  let total = 0;
  let found = false;
  for (const match of source.matchAll(/(\d+(?:[.,]\d+)?)\s*(ms|µs|us|ns|s|sec|secs|seconds?|m|min|mins|minutes?|h)\b/gi)) {
    const value = Number(match[1].replace(",", "."));
    const unit = match[2].toLowerCase();
    found = true;
    if (unit === "ms") total += value;
    else if (unit === "µs" || unit === "us") total += value / 1000;
    else if (unit === "ns") total += value / 1e6;
    else if (unit.startsWith("s")) total += value * 1000;
    else if (unit.startsWith("m")) total += value * 60000;
    else if (unit === "h") total += value * 3600000;
  }
  return found ? Math.round(total) : null;
}

/** `path:line` or `path:line:col` at the end of a string → { file, line }. */
function locate(text) {
  const match = /([^\s()'"<>]+?\.[A-Za-z0-9]+):(\d+)(?::\d+)?/.exec(String(text || ""));
  return match ? { file: match[1], line: Number(match[2]) } : null;
}

/** Keeps the first FAILURES_MAX, and says nothing else: the count is `failed`. */
const capFailures = (failures) => failures.slice(0, FAILURES_MAX);

module.exports = { clean, splitLines, messageOf, clip, failure, toMs, locate, capFailures, FAILURES_MAX, MESSAGE_MAX };
