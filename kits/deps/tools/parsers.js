"use strict";
// Turns what each package manager prints into one shape. Pure functions: a string in, a list
// out, nothing read from disk and nothing run. A parser throws when the output is not what its
// tool prints, so the caller can say "failed" instead of "clean".
//
//   vulnerability: { package, installed, severity, title, id, url, fixed, vulnerable }
//   outdated:      { package, current, latest, kind: "major" | "minor" | "patch", dev }

const SEVERITIES = ["critical", "high", "moderate", "low", "unknown"];

function severity(value) {
  const word = String(value || "").toLowerCase();
  if (word === "medium") return "moderate";
  return SEVERITIES.includes(word) ? word : "unknown";
}

const clip = (value, max) => (typeof value === "string" ? value.replace(/\s+/g, " ").trim().slice(0, max) : "");
const clean = (value) => (typeof value === "string" && value.trim() ? value.trim() : null);

/** Every JSON document in a text: one, several lines of them (ndjson), or several side by side. */
function jsonDocuments(text) {
  const source = String(text || "").replace(/^﻿/, "");
  const found = [];
  let depth = 0;
  let start = -1;
  let inString = false;
  let escaped = false;
  for (let i = 0; i < source.length; i++) {
    const c = source[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') inString = false;
    } else if (c === '"') {
      if (depth > 0) inString = true;
    } else if (c === "{" || c === "[") {
      if (depth === 0) start = i;
      depth++;
    } else if ((c === "}" || c === "]") && depth > 0) {
      depth--;
      if (depth === 0) found.push(JSON.parse(source.slice(start, i + 1)));
    }
  }
  if (!found.length) throw new Error("no JSON in the output");
  return found;
}

/** The version a range of patched versions starts at: ">=4.17.21" → "4.17.21"; "<0.0.0" (none) → null. */
function fixedFromPatched(range) {
  const text = clean(range);
  if (!text || /^<\s*0\.0\.0$/.test(text)) return null;
  const first = text.split("||")[0].trim();
  const match = /^>=?\s*(\d[\w.+-]*)/.exec(first) || /^(\d[\w.+-]*)$/.exec(first);
  return match ? match[1] : null;
}

/** The version a vulnerable range ends below: "<4.17.21" or ">=3.7.0 <4.17.19" → the last bound. */
function fixedFromVulnerable(range) {
  const match = /<\s*(\d[\w.+-]*)\s*$/.exec(String(range || "").split("||").pop().trim());
  return match ? match[1] : null;
}

const parts = (version) => {
  const match = /^v?(\d+)(?:\.(\d+))?(?:\.(\d+))?/.exec(String(version || "").trim());
  return match ? [Number(match[1]), Number(match[2] || 0), Number(match[3] || 0)] : null;
};

/** major, minor or patch for current → latest; null when they are the same, ahead, or not versions. */
function kindOf(current, latest) {
  const a = parts(current);
  const b = parts(latest);
  if (!a || !b) return null;
  if (b[0] !== a[0]) return b[0] > a[0] ? "major" : null;
  if (b[1] !== a[1]) return b[1] > a[1] ? "minor" : null;
  if (b[2] !== a[2]) return b[2] > a[2] ? "patch" : null;
  return null;
}

function outdatedItem(name, current, latest, dev) {
  const kind = kindOf(current, latest);
  return kind ? { package: clip(name, 214), current: clean(current), latest: clean(latest), kind, dev: Boolean(dev) } : null;
}

const compact = (list) => list.filter(Boolean);

// ---- npm ------------------------------------------------------------------------------------

/** `npm audit --json`. `installed` maps a package name to the version the lockfile holds. */
function npmAudit(text, installed = {}) {
  const [report] = jsonDocuments(text);
  if (report.error) throw new Error(clip(report.error.summary || report.error.code || "npm audit failed", 300));
  if (!report.vulnerabilities || typeof report.vulnerabilities !== "object") throw new Error("not an npm audit report");
  const list = [];
  for (const [name, entry] of Object.entries(report.vulnerabilities)) {
    const fix = entry.fixAvailable && typeof entry.fixAvailable === "object" ? entry.fixAvailable : null;
    for (const via of Array.isArray(entry.via) ? entry.via : []) {
      if (!via || typeof via !== "object") continue; // a string names another package of the chain
      const own = via.name === name;
      list.push({
        package: clip(via.name || name, 214),
        installed: installed[via.name || name] || null,
        severity: severity(via.severity),
        title: clip(via.title, 200),
        id: via.url ? String(via.url).split("/").pop() : String(via.source || ""),
        url: clean(via.url),
        fixed: fixedFromVulnerable(via.range) || (own && fix && fix.name === name ? clean(fix.version) : null),
        vulnerable: clean(via.range),
      });
    }
  }
  return list;
}

/** `npm outdated --json`: { name: { current?, wanted, latest } }. */
function npmOutdated(text) {
  const [report] = jsonDocuments(text);
  if (report.error) throw new Error(clip(report.error.summary || report.error.code || "npm outdated failed", 300));
  return compact(
    Object.entries(report).map(([name, entry]) => {
      const row = Array.isArray(entry) ? entry[0] : entry;
      return row ? outdatedItem(name, row.current || row.wanted, row.latest, false) : null;
    }),
  );
}

// ---- pnpm -----------------------------------------------------------------------------------

/** `pnpm audit --json`: { advisories: { id: { module_name, severity, title, url, patched_versions, findings } } }. */
function pnpmAudit(text) {
  const [report] = jsonDocuments(text);
  if (report.error) throw new Error(clip(report.error.message || report.error.code || "pnpm audit failed", 300));
  if (!report.advisories || typeof report.advisories !== "object") throw new Error("not a pnpm audit report");
  const list = [];
  for (const advisory of Object.values(report.advisories)) {
    const versions = [...new Set((advisory.findings || []).map((finding) => finding.version).filter(Boolean))];
    for (const version of versions.length ? versions : [null]) {
      list.push({
        package: clip(advisory.module_name, 214),
        installed: clean(version),
        severity: severity(advisory.severity),
        title: clip(advisory.title, 200),
        id: clip(advisory.github_advisory_id || String(advisory.id || ""), 60),
        url: clean(advisory.url),
        fixed: fixedFromPatched(advisory.patched_versions),
        vulnerable: clean(advisory.vulnerable_versions),
      });
    }
  }
  return list;
}

/** `pnpm outdated --format json`: { name: { current?, wanted, latest, dependencyType } }. */
function pnpmOutdated(text) {
  const [report] = jsonDocuments(text);
  return compact(
    Object.entries(report).map(([name, row]) =>
      row && typeof row === "object" ? outdatedItem(name, row.current || row.wanted, row.latest, row.dependencyType === "devDependencies") : null,
    ),
  );
}

// ---- yarn -----------------------------------------------------------------------------------

/** `yarn audit --json` (classic): one JSON line per advisory, `{ type: "auditAdvisory", data: { advisory } }`. */
function yarnAudit(text) {
  const documents = jsonDocuments(text);
  const errors = documents.filter((doc) => doc && doc.type === "error");
  const advisories = documents.filter((doc) => doc && doc.type === "auditAdvisory" && doc.data && doc.data.advisory);
  if (errors.length && !advisories.length) throw new Error(clip(String(errors[0].data), 300));
  if (!advisories.length && !documents.some((doc) => doc && doc.type === "auditSummary")) throw new Error("not a yarn audit report");
  return advisories.map(({ data }) => {
    const advisory = data.advisory;
    const finding = (advisory.findings || [])[0] || {};
    return {
      package: clip(advisory.module_name, 214),
      installed: clean(finding.version),
      severity: severity(advisory.severity),
      title: clip(advisory.title, 200),
      id: clip(advisory.github_advisory_id || String(advisory.id || ""), 60),
      url: clean(advisory.url),
      fixed: fixedFromPatched(advisory.patched_versions),
      vulnerable: clean(advisory.vulnerable_versions),
    };
  });
}

/** `yarn npm audit --json` (Berry): one line per package, `{ value, children: { ID, Issue, URL, Severity, … } }`. */
function yarnBerryAudit(text) {
  return compact(
    jsonDocuments(text).map((doc) => {
      const child = doc && doc.children;
      if (!child || typeof child !== "object") return null;
      const trees = Array.isArray(child["Tree Versions"]) ? child["Tree Versions"] : [];
      return {
        package: clip(doc.value, 214),
        installed: clean(trees[0]),
        severity: severity(child.Severity),
        title: clip(child.Issue, 200),
        id: child.URL ? String(child.URL).split("/").pop() : String(child.ID || ""),
        url: clean(child.URL),
        fixed: fixedFromVulnerable(child["Vulnerable Versions"]),
        vulnerable: clean(child["Vulnerable Versions"]),
      };
    }),
  );
}

/** `yarn outdated --json` (classic): a `table` line whose head names Package, Current, Wanted, Latest. */
function yarnOutdated(text) {
  const table = jsonDocuments(text).find((doc) => doc && doc.type === "table" && doc.data && Array.isArray(doc.data.body));
  if (!table) return [];
  const head = table.data.head || [];
  const at = (name) => head.indexOf(name);
  return compact(
    table.data.body.map((row) =>
      outdatedItem(row[at("Package")], row[at("Current")], row[at("Latest")], /dev/i.test(String(row[at("Package Type")] || ""))),
    ),
  );
}

// ---- cargo ----------------------------------------------------------------------------------

/** `cargo audit --json`: { vulnerabilities: { list: [{ advisory, versions: { patched }, package }] } }. */
function cargoAudit(text) {
  const [report] = jsonDocuments(text);
  if (!report.vulnerabilities || !Array.isArray(report.vulnerabilities.list)) throw new Error("not a cargo audit report");
  return report.vulnerabilities.list.map((item) => {
    const advisory = item.advisory || {};
    const patched = (item.versions && item.versions.patched) || [];
    return {
      package: clip((item.package || {}).name || advisory.package, 214),
      installed: clean((item.package || {}).version),
      severity: severity(advisory.severity), // RustSec rarely grades: "unknown" says so
      title: clip(advisory.title, 200),
      id: clip(advisory.id, 60),
      url: clean(advisory.url),
      fixed: fixedFromPatched(patched[0]),
      vulnerable: null,
    };
  });
}

/** `cargo outdated --format json`: { dependencies: [{ name, project, compat, latest, kind }] }, perhaps one document per crate. */
function cargoOutdated(text) {
  const rows = [];
  for (const doc of jsonDocuments(text)) {
    if (!doc || !Array.isArray(doc.dependencies)) throw new Error("not a cargo outdated report");
    rows.push(...doc.dependencies);
  }
  const seen = new Set();
  return compact(
    rows.map((row) => {
      const key = `${row.name}@${row.project}`;
      if (seen.has(key)) return null;
      seen.add(key);
      return outdatedItem(row.name, row.project, row.latest, /dev/i.test(String(row.kind || "")));
    }),
  );
}

// ---- pip ------------------------------------------------------------------------------------

/** `pip-audit -f json`: { dependencies: [{ name, version, vulns: [{ id, fix_versions, description, aliases }] }] }. */
function pipAudit(text) {
  const [report] = jsonDocuments(text);
  if (!Array.isArray(report.dependencies)) throw new Error("not a pip-audit report");
  const list = [];
  for (const dependency of report.dependencies) {
    for (const vuln of dependency.vulns || []) {
      list.push({
        package: clip(dependency.name, 214),
        installed: clean(dependency.version),
        severity: "unknown", // pip-audit does not grade its findings
        title: clip(vuln.description, 200) || clip(vuln.id, 60),
        id: clip(vuln.id, 60),
        url: /^GHSA-/.test(vuln.id || "") ? `https://github.com/advisories/${vuln.id}` : null,
        fixed: clean((vuln.fix_versions || [])[0]),
        vulnerable: null,
      });
    }
  }
  return list;
}

/** `pip list --outdated --format json`: [{ name, version, latest_version }]. Only the names in `only`, when given. */
function pipOutdated(text, only = null) {
  const [report] = jsonDocuments(text);
  if (!Array.isArray(report)) throw new Error("not a pip list report");
  const norm = (name) => String(name).toLowerCase().replace(/[-_.]+/g, "-");
  const wanted = only ? new Set(only.map(norm)) : null;
  return compact(report.map((row) => (wanted && !wanted.has(norm(row.name)) ? null : outdatedItem(row.name, row.version, row.latest_version, false))));
}

/** The package names a requirements.txt pins or asks for. */
function requirementNames(text) {
  return String(text || "")
    .split(/\r?\n/)
    .map((line) => line.split("#")[0].trim())
    .filter((line) => line && !line.startsWith("-"))
    .map((line) => /^([A-Za-z0-9][A-Za-z0-9._-]*)/.exec(line))
    .filter(Boolean)
    .map((match) => match[1]);
}

module.exports = {
  SEVERITIES, severity, jsonDocuments, fixedFromPatched, fixedFromVulnerable, kindOf,
  npmAudit, npmOutdated, pnpmAudit, pnpmOutdated, yarnAudit, yarnBerryAudit, yarnOutdated,
  cargoAudit, cargoOutdated, pipAudit, pipOutdated, requirementNames,
};
