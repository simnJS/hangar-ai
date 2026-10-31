"use strict";
// The proof the kit stands on: whatever the file holds, whatever the person types,
// however the save goes, no value reaches panes.send. Every test drives the real
// controller against a fake Hangar, marks every value with a sentinel, and looks at
// what left the page: panes.send above all, and every other call but the file ones.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { TAB, answer, dotenv, fakeHangar, opened, row, settle } = require("./helpers.js");

const CALLER = { pane: "Asker" };

/** A deterministic generator: a failure here can be run again as it is. */
function randomizer(seed) {
  let state = seed >>> 0 || 1;
  const next = () => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    state >>>= 0;
    return state;
  };
  return { int: (max) => next() % max, pick: (list) => list[next() % list.length] };
}

/** What every message must look like: fixed sentences, a path, names, counts. Nothing else fits. */
const SHAPE = new RegExp(
  "^The person (?:saved|checked) `[^`\\n]+`(?:" +
    ": the file changed\\.|" +
    " and changed nothing\\.(?: (?:Already set|Still empty): [A-Za-z_][\\w.-]*(?:, [A-Za-z_][\\w.-]*)*\\.)*(?: \\d+ other keys? set or empty\\.)?(?: The values stay in the file: do not read them back, use them through the program that loads it\\.)?|" +
    "\\.(?: (?:Set|Emptied|Still empty): [A-Za-z_][\\w.-]*(?:, [A-Za-z_][\\w.-]*)*\\.)*(?: \\d+ other keys? changed or empty\\.)? The values stay in the file: do not read them back, use them through the program that loads it\\." +
    ")$",
);

function assertClean(hangar, secrets, label) {
  const sent = hangar.callsTo("panes.send");
  for (const call of sent) {
    assert.match(call.args.text, SHAPE, `${label}: not one of the fixed sentences: ${call.args.text}`);
    for (const secret of secrets) {
      assert.ok(!call.args.text.includes(secret), `${label}: a value reached panes.send: ${secret} in ${call.args.text}`);
    }
  }
  // Only the file calls may carry a value, and only to the file itself.
  for (const call of hangar.calls) {
    if (call.method === "files.write" || call.method === "files.read") continue;
    const seen = JSON.stringify(call.args === undefined ? null : call.args);
    for (const secret of secrets) assert.ok(!seen.includes(secret), `${label}: ${secret} left in ${call.method}: ${seen}`);
  }
  return sent;
}

const S = (n) => `s3cr3t-${n}-XqZ9`;

/** Every way a value can sit in a dotenv file, and the values to look for. */
const HOSTILE_LINES = [
  (v) => `PLAIN=${v}`,
  (v) => `export EXPORTED=${v}`,
  (v) => `SPACED = ${v}`,
  (v) => `DOUBLE="${v}"`,
  (v) => `SINGLE='${v}'`,
  (v) => `TICK=\`${v}\``,
  (v) => `INLINE=${v} # also ${v}`,
  (v) => `# COMMENTED=${v}`,
  (v) => `#${v}`,
  (v) => `${v}`, // a token alone on its line
  (v) => `MULTI="first ${v}\nINNER_KEY=${v}\nlast"`,
  (v) => `MULTISINGLE='${v}\nOTHER=${v}'`,
  (v) => `UNCLOSED="${v}`,
  (v) => `DUP=${v}`,
  (v) => `DUP=${v}x`,
  (v) => `COLON: ${v}`,
  (v) => `EQUALS=${v}=${v}==`,
  (v) => `A${v.replace(/-/g, "_")}=${v}`, // the value is also in the key's name
  (v) => `${v.replace(/-/g, "_")}=${v.replace(/-/g, "_")}`, // a key that is its own value
  (v) => `NOTE=see ${v.replace(/-/g, "_")}`,
];

test("every hostile line, one by one: the answer holds no value", async () => {
  for (const [index, line] of HOSTILE_LINES.entries()) {
    const secret = S(index);
    const text = `# head\n${line(secret)}\nTARGET=old\n`;
    const { hangar, controller, state } = await opened({
      params: { path: ".env", keys: "TARGET,NEW_KEY" }, caller: CALLER, files: { ".env": text, ".gitignore": ".env\n" },
    });
    controller.editRow("TARGET", `${secret}-typed`);
    controller.editRow("NEW_KEY", `${secret}-new`);
    for (const each of state.form.rows) controller.editRow(each.key, `${each.value}-${secret}`);
    await controller.saveForm();
    await settle();
    const sent = assertClean(hangar, [secret, secret.replace(/-/g, "_")], `line ${index}`);
    assert.equal(sent.length, 1, `line ${index}: it answered once`);
  }
});

test("random files, random edits, 400 of them: the answer holds no value", async () => {
  const random = randomizer(20261031);
  const eols = ["\n", "\r\n"];
  const keys = ["API_KEY", "DB_URL", "TOKEN", "PORT", "A.B", "x-y", "_z", "DEBUG"];
  for (let run = 0; run < 400; run++) {
    const secrets = [];
    const secret = () => {
      const value = `v${run}-${secrets.length}-${random.int(1e9).toString(36)}Q`;
      secrets.push(value);
      return value;
    };
    const eol = random.pick(eols);
    const lines = [];
    for (let i = random.int(12); i >= 0; i--) lines.push(random.pick(HOSTILE_LINES)(secret()));
    for (const key of keys) if (random.int(2)) lines.splice(random.int(lines.length + 1), 0, `${key}=${secret()}`);
    const text = lines.join(eol) + (random.int(2) ? eol : "");
    const example = random.int(2) ? { ".env.example": `# say what\nFROM_EXAMPLE=${secret()}\nAPI_KEY=${secret()}\n` } : {};
    const ignore = random.int(2) ? { ".gitignore": ".env\n" } : {};
    const asked = keys.filter(() => random.int(3) === 0).join(",");
    const { hangar, controller, state } = await opened({
      params: { path: ".env", ...(asked ? { keys: asked } : {}), why: "for the test" },
      caller: CALLER,
      files: { ".env": text, ...example, ...ignore },
    });

    assert.equal(state.view, "dotenv", `run ${run}`);
    for (const each of [...state.form.rows]) {
      const dice = random.int(4);
      if (dice === 0) controller.editRow(each.key, secret());
      else if (dice === 1) controller.editRow(each.key, "");
      else if (dice === 2) controller.editRow(each.key, `${secret()}\n${secret()}=x`);
    }
    if (state.form.offered.length && random.int(2)) controller.addAllOffered();
    if (random.int(4) === 0) {
      // The person pastes a secret where the name of a new key goes.
      const pasted = `Pasted_${secret().replace(/-/g, "_")}`;
      secrets.push(pasted);
      controller.setNewKey(pasted);
      controller.addNewKey();
      controller.editRow(pasted, secret());
    }
    // The file may have moved in the meantime, and the person chooses.
    if (random.int(4) === 0) hangar.store.set(".env", `# moved\nMOVED=${secret()}\n${text}`);
    if (controller.isDirty()) await controller.saveForm();
    else await controller.tellNoChange();
    await settle();
    if (state.conflict) {
      if (random.int(2)) await controller.conflictKeepMine();
      else await controller.conflictTakeTheirs();
      await settle();
      await controller.tellNoChange();
      await settle();
    }
    assertClean(hangar, secrets, `run ${run} of ${JSON.stringify(text)}`);
  }
});

test("a text file: only that it changed, whatever it holds", async () => {
  const secret = S("text");
  const files = {
    "config.json": `{ "password": "${secret}" }\n`,
    "values.yaml": `password: ${secret}\n`,
    "app.toml": `password = "${secret}"\n`,
    "notes.txt": `${secret}\n`,
  };
  for (const file of Object.keys(files)) {
    const { hangar, controller, state } = await opened({ params: { path: file }, caller: CALLER, files });
    controller.editText(`${state.text.value}# ${secret}-typed\n`);
    controller.review();
    controller.setAck(true);
    await controller.saveText();
    await settle();
    const [sent] = assertClean(hangar, [secret], file);
    assert.equal(sent.args.text, `The person saved \`${file}\`: the file changed.`);
  }
});

test("failures and retries carry no value either", async () => {
  const secret = S("retry");
  for (const code of ["not_found", "denied", "unavailable", "rate_limited"]) {
    const { hangar, controller, state } = await opened({
      params: { path: ".env", keys: "A" }, caller: CALLER, files: { ".env": `A=${secret}\n`, ".gitignore": ".env\n" }, sendError: code,
    });
    controller.editRow("A", `${secret}2`);
    await controller.saveForm();
    await settle();
    assert.equal(state.told.status, "failed");
    hangar.sendError = null;
    await controller.retryTell();
    await settle();
    assert.equal(state.told.status, "sent");
    const sent = assertClean(hangar, [secret], code);
    assert.equal(sent.length, 2);
    assert.equal(sent[0].args.text, sent[1].args.text, "the same message, sent again");
  }
});

test("with nobody to answer, nothing is sent", async () => {
  const { hangar, controller } = await opened({ params: { path: ".env" }, files: { ".env": "A=1\n", ".gitignore": ".env\n" } });
  controller.editRow("A", "2");
  await controller.saveForm();
  await settle();
  assert.equal(hangar.callsTo("panes.send").length, 0);
  assert.equal(hangar.store.get(".env"), "A=2\n");
});

test("the message is made of names: what is not a name, or is a value, stays unsaid", () => {
  const known = new Set(["API_KEY", "hunter2hunter2", "HAS_hunter2hunter2_IN", "SHORT", "Bad Name", "a=b"]);
  const text = answer.dotenvSaved({
    path: ".env",
    set: ["API_KEY", "hunter2hunter2", "HAS_hunter2hunter2_IN", "NEVER_SEEN", "Bad Name", "a=b", "line\nbreak", 42],
    emptied: [],
    stillEmpty: ["SHORT"],
    known,
    values: ["hunter2hunter2", "x"],
  });
  assert.equal(
    text,
    "The person saved `.env`. Set: API_KEY. Still empty: SHORT. 7 other keys changed or empty. The values stay in the file: do not read them back, use them through the program that loads it.",
  );
  assert.ok(answer.mayName("A1", new Set(["A1"]), ["1"]), "a short value does not silence a name by chance");
  assert.ok(!answer.mayName("A1", new Set(["A1"]), ["A1"]), "but a name that is a value does");
  assert.ok(!answer.mayName("x".repeat(65), new Set(["x".repeat(65)]), []), "a name this long reads as a token");
});

test("a key the person typed is counted, never named", async () => {
  const { hangar, controller } = await opened({ params: { path: ".env", keys: "A" }, caller: CALLER, files: { ".env": "A=\n", ".gitignore": ".env\n" } });
  controller.setNewKey("sk_live_PASTED_BY_MISTAKE");
  controller.addNewKey();
  controller.editRow("sk_live_PASTED_BY_MISTAKE", "whatever");
  controller.editRow("A", "filled");
  await controller.saveForm();
  await settle();
  const [{ args }] = hangar.callsTo("panes.send");
  assert.match(args.text, /Set: A\. 1 other key changed or empty\./);
  assert.ok(!args.text.includes("sk_live"));
});

test("a long list of keys stays a list of names, and within what panes.send carries", async () => {
  const lines = Array.from({ length: 300 }, (_, i) => `KEY_${i}=old${i}`).join("\n");
  const { hangar, controller, state } = await opened({ params: { path: ".env" }, caller: CALLER, files: { ".env": lines, ".gitignore": ".env\n" } });
  for (const each of state.form.rows) controller.editRow(each.key, `new-${each.key}`);
  await controller.saveForm();
  await settle();
  const [{ args }] = assertClean(hangar, ["new-KEY", "old1"], "long");
  assert.match(args.text, /260 other keys changed or empty\./);
  assert.ok(args.text.length < 20000);
});

test("in the sources: one panes.send, fed by answer.js alone", () => {
  const sources = fs.readdirSync(TAB).filter((name) => name.endsWith(".js"));
  const uses = sources.flatMap((name) => {
    const text = fs.readFileSync(path.join(TAB, name), "utf8");
    return (text.match(/["']panes\.send["']/g) || []).map(() => name);
  });
  assert.deepEqual(uses, ["controller.js"], "panes.send is called from one place");
  const controller = fs.readFileSync(path.join(TAB, "controller.js"), "utf8");
  assert.match(controller, /hangar\.call\("panes\.send", \{ pane, text: message \}\)/);
  // Each `send(` is given what answer.js made, or the message already made for it.
  const senders = controller.match(/\bsend\([^)]*\)/g).filter((call) => !call.startsWith("send(message, status)"));
  for (const call of senders) assert.match(call, /^send\((message|answer\.\w+|unsent\.message)/, call);
  // And answer.js takes no field that could carry a value but `values`, used only to stay silent.
  const speech = fs.readFileSync(path.join(TAB, "answer.js"), "utf8");
  assert.doesNotMatch(speech, /\$\{\s*values/);
  assert.doesNotMatch(speech, /\.value\b/);
});

test("the fuzz would catch a leak: a value in the message fails the checks", () => {
  // Guards the checks themselves: a broken answer.js must not slip past `assertClean`.
  const fake = fakeHangar();
  fake.calls.push({ method: "panes.send", args: { pane: "Asker", text: `The person saved \`.env\`. Set: API_KEY=${S("leak")}. The values stay in the file: do not read them back, use them through the program that loads it.` } });
  assert.throws(() => assertClean(fake, [S("leak")], "planted"));
  const named = fakeHangar();
  named.calls.push({ method: "panes.send", args: { pane: "Asker", text: `The person saved \`.env\`. Set: ${S("leak").replace(/-/g, "_")}. The values stay in the file: do not read them back, use them through the program that loads it.` } });
  assert.throws(() => assertClean(named, [S("leak").replace(/-/g, "_")], "planted name"));
  const title = fakeHangar();
  title.calls.push({ method: "tab.setBadge", args: { state: "neutral", text: S("leak") } });
  assert.throws(() => assertClean(title, [S("leak")], "planted badge"));
  assert.equal(dotenv.isKey(S("leak")), false, "the sentinel with dashes starts with a letter yet holds no dot: a name only if asked");
});
