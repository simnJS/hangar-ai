// Rebuilds kits/index.json from the kits/<id>/hangar-kit.json files.
//
//   node scripts/kits-index.mjs           write kits/index.json
//   node scripts/kits-index.mjs --check   exit 1 when the file differs
//
// An entry takes id, name, description and version from the kit's manifest, and
// `permissionsSummary` and `tags` from its `gallery` block:
//
//   "gallery": { "permissionsSummary": "Runs gh to read CI runs.", "tags": ["ci"] }
//
// No dependency: Node alone.

import { readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const kitsDir = join(root, "kits");
const indexFile = join(kitsDir, "index.json");

function fail(message) {
  console.error(`kits-index: ${message}`);
  process.exit(1);
}

function readJson(file) {
  try {
    return JSON.parse(readFileSync(file, "utf8").replace(/^﻿/, ""));
  } catch (error) {
    return fail(`${file} is not readable JSON (${error.message})`);
  }
}

function text(value, where) {
  if (typeof value !== "string" || value.trim() === "") {
    fail(`${where} must be a non-empty string`);
  }
  return value.trim();
}

function entryOf(folder) {
  const manifestFile = join(kitsDir, folder, "hangar-kit.json");
  const manifest = readJson(manifestFile);
  const at = `kits/${folder}/hangar-kit.json`;
  const id = text(manifest.id, `${at}: id`);
  if (id !== folder) fail(`${at}: id "${id}" must be the folder's name`);
  const gallery = manifest.gallery;
  if (gallery === null || typeof gallery !== "object" || Array.isArray(gallery)) {
    fail(`${at}: "gallery" is missing (it needs permissionsSummary and tags)`);
  }
  const tags = gallery.tags ?? [];
  if (!Array.isArray(tags) || tags.some((tag) => typeof tag !== "string" || tag.trim() === "")) {
    fail(`${at}: gallery.tags must be a list of non-empty strings`);
  }
  return {
    id,
    name: text(manifest.name, `${at}: name`),
    description: text(manifest.description, `${at}: description`),
    path: `kits/${id}`,
    version: text(manifest.version, `${at}: version`),
    permissionsSummary: text(gallery.permissionsSummary, `${at}: gallery.permissionsSummary`),
    tags: tags.map((tag) => tag.trim()),
  };
}

function render(entries) {
  const lines = entries.map((entry) => {
    const fields = Object.entries(entry).map(([key, value]) => {
      const json = Array.isArray(value)
        ? `[${value.map((item) => JSON.stringify(item)).join(", ")}]`
        : JSON.stringify(value);
      return `      ${JSON.stringify(key)}: ${json}`;
    });
    return `    {\n${fields.join(",\n")}\n    }`;
  });
  return `{\n  "schema": 1,\n  "kits": [\n${lines.join(",\n")}\n  ]\n}\n`;
}

const folders = readdirSync(kitsDir, { withFileTypes: true })
  .filter((dir) => dir.isDirectory() && existsSync(join(kitsDir, dir.name, "hangar-kit.json")))
  .map((dir) => dir.name)
  .sort();

const built = render(folders.map(entryOf));

if (process.argv.includes("--check")) {
  const current = existsSync(indexFile) ? readFileSync(indexFile, "utf8") : "";
  if (current.replace(/\r\n/g, "\n") !== built) {
    fail("kits/index.json is stale: run `node scripts/kits-index.mjs` and commit it");
  }
  console.log("kits-index: kits/index.json is up to date");
} else {
  writeFileSync(indexFile, built);
  console.log(`kits-index: kits/index.json written (${folders.length} kit(s))`);
}
