"use strict";
// `node --test kits/editor/` hands the folder to Node as a module on Node 24 (older
// versions search it for test files): loading the tests from here makes both work.
// Nothing else uses this file.
const fs = require("node:fs");
const path = require("node:path");

const folder = path.join(__dirname, "test");
for (const name of fs.readdirSync(folder).sort()) {
  if (name.endsWith(".test.js")) require(path.join(folder, name));
}
