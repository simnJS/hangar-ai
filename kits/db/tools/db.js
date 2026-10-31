"use strict";
// Hangar kit « Database »: a read-only look at a SQLite file of the workspace.
//   node db.js list
//   node db.js tables PATH
//   node db.js schema PATH TABLE
//   node db.js rows PATH TABLE SORT DIRECTION OFFSET LIMIT CHARS
// Prints one JSON document. Known conditions (locked, no sqlite, not a database…) are answers
// with `ok: false` and exit 0; only a crash of the script itself ends otherwise.
const { run } = require("./core");

let answer;
try {
  answer = run(process.argv.slice(2));
} catch (error) {
  answer = { ok: false, error: "failed", detail: String((error && error.message) || error).slice(0, 300) };
}
process.stdout.write(`${JSON.stringify(answer)}\n`);
