"use strict";
// The tab's behaviour against a fake window.hangar: what it reads, when it writes,
// what it refuses to write, and what it says when Hangar answers no.
const test = require("node:test");
const assert = require("node:assert/strict");
const { fakeHangar, opened, row, settle } = require("./helpers.js");

const CALLER = { pane: "Asker" };
const IGNORED = { ".gitignore": ".env\n" };
const writes = (hangar) => hangar.callsTo("files.write");
const writesTo = (hangar, path) => writes(hangar).filter((call) => call.args.path === path);
const lastBadge = (hangar) => hangar.callsTo("tab.setBadge").at(-1).args;

/* ── the dotenv form ──────────────────────────────────────────────── */

test("a dotenv file opens as a form: one row per key, hidden, its comments and order kept on save", async () => {
  const text = "# Database\nDB_URL=old\n\n# Payments\nexport STRIPE_KEY='x' # test mode\nLOG=debug\nDB_URL=later\n";
  const { hangar, controller, state } = await opened({ params: { path: ".env" }, files: { ".env": text, ...IGNORED } });
  assert.equal(state.view, "dotenv");
  assert.deepEqual(state.form.rows.map((each) => each.key), ["DB_URL", "STRIPE_KEY", "LOG"]);
  assert.ok(state.form.rows.every((each) => each.revealed === false), "hidden until revealed");
  assert.equal(row(state, "DB_URL").value, "later");
  assert.equal(row(state, "DB_URL").count, 2);
  assert.equal(row(state, "STRIPE_KEY").hint, "Payments");

  controller.editRow("STRIPE_KEY", "sk_new");
  await controller.saveForm();
  await settle();
  assert.equal(hangar.store.get(".env"), "# Database\nDB_URL=old\n\n# Payments\nexport STRIPE_KEY='sk_new' # test mode\nLOG=debug\nDB_URL=later\n");
  assert.equal(writes(hangar)[0].args.expected, text, "with what it read");
  assert.equal(state.saved, true);
  assert.equal(lastBadge(hangar).state, "success");
});

test("a value is revealed row by row, and hidden again after saving", async () => {
  const { controller, state } = await opened({ params: { path: ".env" }, files: { ".env": "A=1\nB=2\n", ...IGNORED } });
  controller.toggleReveal("A");
  assert.equal(row(state, "A").revealed, true);
  assert.equal(row(state, "B").revealed, false);
  controller.editRow("A", "9");
  await controller.saveForm();
  await settle();
  assert.equal(row(state, "A").revealed, false);
});

test("the keys an agent asks for come first, marked missing, with why at the top", async () => {
  const { hangar, state, controller } = await opened({
    params: { path: ".env", keys: "NEW_ONE, PRESENT,EMPTY", why: "For the payment tests." },
    caller: CALLER,
    files: { ".env": "OTHER=1\nPRESENT=x\nEMPTY=\n", ...IGNORED },
  });
  assert.equal(state.why, "For the payment tests.");
  assert.deepEqual(state.form.rows.map((each) => each.key), ["NEW_ONE", "PRESENT", "EMPTY", "OTHER"]);
  assert.deepEqual(state.form.rows.map((each) => each.asked), [true, true, true, false]);
  assert.equal(lastBadge(hangar).text, "2 missing");
  controller.editRow("NEW_ONE", "v");
  assert.equal(lastBadge(hangar).text, "1 missing");
  controller.editRow("EMPTY", "v");
  assert.equal(lastBadge(hangar).state, null);
});

test("a key asked and left empty is not written; one filled in goes at the end", async () => {
  const { hangar, controller, state } = await opened({
    params: { path: ".env", keys: "FILLED,LEFT" }, caller: CALLER, files: { ".env": "OTHER=1\n", ...IGNORED },
  });
  controller.editRow("FILLED", "v");
  await controller.saveForm();
  await settle();
  assert.equal(hangar.store.get(".env"), "OTHER=1\nFILLED=v\n");
  assert.equal(
    hangar.callsTo("panes.send")[0].args.text,
    "The person saved `.env`. Set: FILLED. Still empty: LEFT. The values stay in the file: do not read them back, use them through the program that loads it.",
  );
  assert.deepEqual(hangar.callsTo("panes.send")[0].args.pane, "Asker");
  assert.equal(row(state, "LEFT").asked, true, "still asked for");
});

test("a file that does not exist yet opens empty, and saving creates it", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "config/.env", keys: "A" }, files: { ".gitignore": "*.env\n.env\n" } });
  assert.equal(state.view, "dotenv");
  assert.equal(state.exists, false);
  controller.editRow("A", "1");
  await controller.saveForm();
  await settle();
  assert.equal(hangar.store.get("config/.env"), "A=1\n");
  assert.equal(writes(hangar)[0].args.expected, undefined, "there was nothing to expect");
  assert.equal(state.exists, true);
});

test("keys of .env.example missing here are offered, not added", async () => {
  const { hangar, controller, state } = await opened({
    params: { path: ".env", keys: "FROM_EXAMPLE" },
    files: { ".env": "HERE=1\n", ".env.example": "HERE=\n# Where to find it\nFROM_EXAMPLE=placeholder\n# Second\nEXTRA=placeholder\nANOTHER=\n", ...IGNORED },
  });
  assert.deepEqual(state.form.offered.map((offer) => offer.key), ["EXTRA", "ANOTHER"]);
  assert.equal(state.form.offered[0].hint, "Second");
  assert.equal(state.form.exampleName, ".env.example");
  assert.equal(row(state, "FROM_EXAMPLE").hint, "Where to find it");
  assert.equal(row(state, "FROM_EXAMPLE").value, "", "the example's value is not taken for a real one");
  assert.equal(controller.isDirty(), false, "nothing is added until asked");

  controller.addOffered("EXTRA");
  assert.equal(row(state, "EXTRA").value, "");
  assert.deepEqual(state.form.offered.map((offer) => offer.key), ["ANOTHER"]);
  controller.editRow("EXTRA", "mine");
  await controller.saveForm();
  await settle();
  assert.equal(hangar.store.get(".env"), "HERE=1\nEXTRA=mine\n");
  assert.deepEqual(state.form.offered.map((offer) => offer.key), ["ANOTHER"], "what is still missing stays offered");
  assert.equal(row(state, "FROM_EXAMPLE").hint, "Where to find it", "and the template still explains what is asked");

  controller.addAllOffered();
  await controller.saveForm();
  await settle();
  assert.equal(hangar.store.get(".env"), "HERE=1\nEXTRA=mine\nANOTHER=\n", "an explicit add is written, even empty");
  assert.equal(state.form.offered.length, 0);
});

test("a template offers nothing, and needs no warning", async () => {
  const { state, hangar } = await opened({ params: { path: ".env.example" }, files: { ".env.example": "A=\n" } });
  assert.equal(state.form.offered.length, 0);
  assert.equal(state.ignore.status, "ignored");
  assert.equal(hangar.callsTo("files.read").filter((call) => call.args.path.endsWith(".gitignore")).length, 0);
});

test("a key the person adds is checked, and a bad one says why", async () => {
  const { controller, state } = await opened({ params: { path: ".env" }, files: { ".env": "A=1\n", ...IGNORED } });
  controller.setNewKey("not a key");
  controller.addNewKey();
  assert.equal(state.form.newKeyProblem, "newKeyBad");
  controller.setNewKey("A");
  controller.addNewKey();
  assert.equal(state.form.newKeyProblem, "newKeyTaken");
  controller.setNewKey(" B_KEY ");
  controller.addNewKey();
  assert.equal(state.form.newKeyProblem, null);
  assert.equal(state.form.newKey, "");
  assert.equal(row(state, "B_KEY").added, true);
});

/* ── git ──────────────────────────────────────────────────────────── */

test("not ignored: it says so, and .gitignore changes only on a click", async () => {
  const { hangar, controller, state } = await opened({ params: { path: ".env" }, files: { ".env": "A=1\n", ".gitignore": "node_modules/\n" } });
  assert.equal(state.ignore.status, "not-ignored");
  assert.equal(writes(hangar).length, 0, "opening writes nothing");
  controller.editRow("A", "2");
  await controller.saveForm();
  await settle();
  assert.equal(writesTo(hangar, ".gitignore").length, 0, "saving the file does not touch it either");

  await controller.addToGitignore();
  assert.equal(hangar.store.get(".gitignore"), "node_modules/\n.env\n");
  assert.equal(writesTo(hangar, ".gitignore")[0].args.expected, "node_modules/\n");
  assert.equal(state.ignore.status, "ignored");
});

test("no .gitignore at all: said plainly, and the click creates one", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "sub/.env.local" }, files: { "sub/.env.local": "A=1\n" } });
  assert.equal(state.ignore.status, "no-file");
  await controller.addToGitignore();
  assert.equal(hangar.store.get(".gitignore"), "sub/.env.local\n");
  assert.equal(writesTo(hangar, ".gitignore")[0].args.expected, undefined);
  assert.equal(state.ignore.status, "ignored");
});

test("a .gitignore that moved under the click is not overwritten, and a denied one says so", async () => {
  const { hangar, controller, state } = await opened({ params: { path: ".env" }, files: { ".env": "A=1\n", ".gitignore": "old\n" } });
  hangar.writeError = "rate_limited";
  await controller.addToGitignore();
  assert.equal(state.ignore.problem, "rate_limited");
  assert.equal(state.ignore.status, "not-ignored");
  assert.equal(hangar.store.get(".gitignore"), "old\n");
  hangar.writeError = null;
  await controller.addToGitignore();
  assert.equal(hangar.store.get(".gitignore"), "old\n.env\n");
});

test("a .gitignore that cannot be read leaves the question open, and the form works", async () => {
  const hangar = fakeHangar({ params: { path: ".env" }, files: { ".env": "A=1\n" } });
  const read = hangar.call;
  hangar.call = async (method, args) => {
    if (method === "files.read" && args.path === ".gitignore") throw Object.assign(new Error("no"), { code: "denied" });
    return read(method, args);
  };
  const { state } = await opened({ hangar });
  assert.equal(state.ignore.status, "unknown");
  assert.equal(state.view, "dotenv");
});

/* ── any other text file ──────────────────────────────────────────── */

test("a text file opens in the editor at the line asked for", async () => {
  const lines = Array.from({ length: 9 }, (_, i) => `line ${i + 1}`).join("\n");
  const { state } = await opened({ params: { path: "notes.txt", line: "7" }, files: { "notes.txt": lines } });
  assert.equal(state.view, "text");
  assert.equal(state.text.jump, 7);
  assert.equal(state.text.target, 7);
  assert.equal(state.text.check.kind, null);
  const far = await opened({ params: { path: "notes.txt", line: "70" }, files: { "notes.txt": lines } });
  assert.equal(far.state.text.jump, 9, "as far as the file goes");
  assert.deepEqual(far.state.text.far, { line: 70, total: 9 });
});

test("a diff comes before saving, and saving sends what was reviewed", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, files: { "a.txt": "one\ntwo\nthree\n" } });
  controller.editText("one\nTWO\nthree\n");
  assert.equal(writes(hangar).length, 0, "typing writes nothing");
  controller.review();
  assert.equal(state.text.stage, "review");
  assert.deepEqual(state.text.hunks.filter((hunk) => hunk.op !== "same").map((hunk) => [hunk.op, hunk.text]), [["del", "two"], ["add", "TWO"]]);
  assert.equal(writes(hangar).length, 0, "reviewing writes nothing");
  await controller.saveText();
  await settle();
  assert.equal(hangar.store.get("a.txt"), "one\nTWO\nthree\n");
  assert.equal(writes(hangar)[0].args.expected, "one\ntwo\nthree\n");
  assert.equal(state.text.stage, "edit");
  assert.equal(state.saved, true);
});

test("line endings survive the textarea", async () => {
  const crlf = await opened({ params: { path: "a.txt" }, files: { "a.txt": "one\r\ntwo\r\n" } });
  assert.equal(crlf.state.text.value, "one\ntwo\n", "as a textarea would hold it");
  crlf.controller.editText("one\ntwo\nthree\n");
  crlf.controller.review();
  assert.deepEqual(crlf.state.text.hunks.filter((hunk) => hunk.op !== "same").map((hunk) => hunk.text), ["three"], "the line endings are no change");
  await crlf.controller.saveText();
  await settle();
  assert.equal(crlf.hangar.store.get("a.txt"), "one\r\ntwo\r\nthree\r\n");

  const mixed = await opened({ params: { path: "a.txt" }, files: { "a.txt": "a\r\nb\nc\r\nd\r\n" } });
  mixed.controller.editText("a\nB\nc\nd\n");
  mixed.controller.review();
  await mixed.controller.saveText();
  await settle();
  assert.equal(mixed.hangar.store.get("a.txt"), "a\r\nB\r\nc\r\nd\r\n", "only the changed line takes the common one");
});

test("a broken file is saved only after saying so", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.json" }, files: { "a.json": '{"a": 1}\n' } });
  assert.equal(state.text.check.ok, true);
  controller.editText('{"a": 1,}\n');
  assert.equal(state.text.check.ok, false);
  controller.review();
  await controller.saveText();
  await settle();
  assert.equal(writes(hangar).length, 0, "not without saying so");
  assert.equal(hangar.store.get("a.json"), '{"a": 1}\n');

  controller.setAck(true);
  controller.editText('{"a": 1,,}\n');
  assert.equal(state.text.ack, false, "changing the text takes back what was said");
  controller.review();
  assert.equal(state.text.ack, false);
  await controller.saveText();
  await settle();
  assert.equal(writes(hangar).length, 0);

  controller.setAck(true);
  await controller.saveText();
  await settle();
  assert.equal(hangar.store.get("a.json"), '{"a": 1,,}\n');
});

test("a file already broken when opened follows the same rule", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.toml" }, files: { "a.toml": "a = \n" } });
  assert.equal(state.text.check.ok, false);
  controller.editText("a = \nb = 2\n");
  controller.review();
  await controller.saveText();
  assert.equal(writes(hangar).length, 0);
});

test("a new text file: the whole of it is the diff, and it is created", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "conf/new.yaml" }, files: {} });
  assert.equal(state.view, "text");
  assert.equal(state.exists, false);
  controller.editText("a: 1\n");
  controller.review();
  await controller.saveText();
  await settle();
  assert.equal(hangar.store.get("conf/new.yaml"), "a: 1\n");
  assert.equal(writes(hangar)[0].args.expected, undefined);
});

/* ── conflicts ────────────────────────────────────────────────────── */

test("text conflict: nothing is written until the person chooses, and both sides show", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, caller: CALLER, files: { "a.txt": "base\n" } });
  controller.editText("mine\n");
  controller.review();
  hangar.store.set("a.txt", "theirs\n");
  await controller.saveText();
  await settle();

  assert.equal(hangar.store.get("a.txt"), "theirs\n", "never overwritten silently");
  assert.equal(state.conflict.kind, "text");
  assert.equal(state.conflict.theirs, "theirs\n");
  assert.deepEqual(state.conflict.hunks.map((hunk) => [hunk.op, hunk.text]).filter(([op]) => op !== "same"), [["del", "theirs"], ["add", "mine"]]);
  assert.equal(hangar.callsTo("panes.send").length, 0, "no answer for what was not written");

  await controller.conflictKeepMine();
  await settle();
  assert.equal(hangar.store.get("a.txt"), "mine\n");
  assert.equal(writes(hangar).at(-1).args.expected, "theirs\n", "measured against what the person just saw");
  assert.equal(state.conflict, null);
  assert.equal(hangar.callsTo("panes.send").length, 1);
});

test("text conflict: taking theirs drops mine and writes nothing", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, files: { "a.txt": "base\n" } });
  controller.editText("mine\n");
  controller.review();
  hangar.store.set("a.txt", "theirs\n");
  await controller.saveText();
  await settle();
  const before = writes(hangar).length;
  await controller.conflictTakeTheirs();
  await settle();
  assert.equal(state.text.value, "theirs\n");
  assert.equal(controller.isDirty(), false);
  assert.equal(writes(hangar).length, before);
  assert.equal(hangar.store.get("a.txt"), "theirs\n");
});

test("text conflict: back to editing keeps mine, against what the disk holds now", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, files: { "a.txt": "base\n" } });
  controller.editText("mine\n");
  controller.review();
  hangar.store.set("a.txt", "theirs\n");
  await controller.saveText();
  await settle();
  controller.conflictBack();
  assert.equal(state.text.value, "mine\n");
  assert.equal(state.text.original, "theirs\n");
  assert.equal(state.text.stage, "edit");
  controller.review();
  await controller.saveText();
  await settle();
  assert.equal(hangar.store.get("a.txt"), "mine\n");
});

test("the file moved twice: a second conflict is a conflict again", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, files: { "a.txt": "base\n" } });
  controller.editText("mine\n");
  controller.review();
  hangar.store.set("a.txt", "theirs\n");
  await controller.saveText();
  await settle();
  hangar.store.set("a.txt", "theirs again\n");
  await controller.conflictKeepMine();
  await settle();
  assert.equal(hangar.store.get("a.txt"), "theirs again\n");
  assert.equal(state.conflict.theirs, "theirs again\n");
});

test("a file deleted meanwhile is a conflict, and writing it back is a choice", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, files: { "a.txt": "base\n" } });
  controller.editText("mine\n");
  controller.review();
  hangar.store.delete("a.txt");
  await controller.saveText();
  await settle();
  assert.equal(hangar.store.has("a.txt"), false);
  assert.equal(state.conflict.gone, true);
  await controller.conflictKeepMine();
  await settle();
  assert.equal(hangar.store.get("a.txt"), "mine\n");
});

test("a new file that appeared meanwhile is a conflict too", async () => {
  const { hangar, controller, state } = await opened({ params: { path: ".env", keys: "A" }, files: { ...IGNORED } });
  controller.editRow("A", "mine");
  hangar.store.set(".env", "A=theirs\n");
  await controller.saveForm();
  await settle();
  assert.equal(hangar.store.get(".env"), "A=theirs\n", "not overwritten though nothing was expected");
  assert.equal(state.conflict.kind, "dotenv");
});

test("dotenv conflict: key by key, hidden, and my keys go on top of what the disk holds", async () => {
  const { hangar, controller, state } = await opened({
    params: { path: ".env" }, caller: CALLER, files: { ".env": "# base\nA=1\nB=2\nC=3\n", ...IGNORED },
  });
  controller.editRow("A", "mine-a");
  controller.editRow("B", "mine-b");
  hangar.store.set(".env", "# moved\nA=theirs-a\nB=2\nC=3\nD=new\n");
  await controller.saveForm();
  await settle();

  assert.equal(hangar.store.get(".env"), "# moved\nA=theirs-a\nB=2\nC=3\nD=new\n");
  assert.equal(state.conflict.kind, "dotenv");
  assert.deepEqual(state.conflict.rows.map((each) => [each.key, each.mine, each.theirs, each.choice, each.revealed]), [
    ["A", "mine-a", "theirs-a", "mine", false],
    ["B", "mine-b", "2", "mine", false],
  ]);
  assert.equal(state.conflict.changedOnDisk, 1, "D");
  assert.equal(hangar.callsTo("panes.send").length, 0);

  controller.conflictReveal("A");
  assert.equal(state.conflict.rows[0].revealed, true);
  controller.conflictChoose("A", "theirs");
  await controller.conflictKeepMine();
  await settle();
  assert.equal(hangar.store.get(".env"), "# moved\nA=theirs-a\nB=mine-b\nC=3\nD=new\n", "their comment and their key stay");
  assert.equal(writes(hangar).at(-1).args.expected, "# moved\nA=theirs-a\nB=2\nC=3\nD=new\n");
  assert.match(hangar.callsTo("panes.send")[0].args.text, /Set: B\./);
  assert.equal(state.conflict, null);
  assert.equal(row(state, "D").value, "new", "the form now reads the new file");
});

test("dotenv conflict: when nothing collides it still asks", async () => {
  const { hangar, controller, state } = await opened({ params: { path: ".env" }, files: { ".env": "A=1\nB=2\n", ...IGNORED } });
  controller.editRow("A", "mine");
  hangar.store.set(".env", "A=1\nB=theirs\n");
  await controller.saveForm();
  await settle();
  assert.equal(state.conflict.rows.length, 1, "A: mine is not what the disk holds");
  assert.equal(hangar.store.get(".env"), "A=1\nB=theirs\n");
  await controller.conflictTakeTheirs();
  await settle();
  assert.equal(row(state, "B").value, "theirs");
  assert.equal(row(state, "A").value, "1");
  assert.equal(controller.isDirty(), false);
});

/* ── who asked ────────────────────────────────────────────────────── */

test("the agent is answered on save, and only when one asked", async () => {
  const text = await opened({ params: { path: "a.txt" }, caller: CALLER, files: { "a.txt": "x\n" } });
  text.controller.editText("y\n");
  text.controller.review();
  await text.controller.saveText();
  await settle();
  assert.deepEqual(text.hangar.callsTo("panes.send").map((call) => call.args), [{ pane: "Asker", text: "The person saved `a.txt`: the file changed." }]);
  assert.equal(text.state.told.status, "sent");

  const nobody = await opened({ params: { path: "a.txt" }, files: { "a.txt": "x\n" } });
  nobody.controller.editText("y\n");
  nobody.controller.review();
  await nobody.controller.saveText();
  await settle();
  assert.equal(nobody.hangar.callsTo("panes.send").length, 0);
  assert.equal(nobody.state.told, null);
});

test("nothing to change: the waiting agent can still be told, with names only", async () => {
  const { hangar, controller, state } = await opened({
    params: { path: ".env", keys: "SET_KEY,EMPTY_KEY" }, caller: CALLER, files: { ".env": "SET_KEY=abc\nEMPTY_KEY=\n", ...IGNORED },
  });
  await controller.tellNoChange();
  assert.equal(
    hangar.callsTo("panes.send")[0].args.text,
    "The person checked `.env` and changed nothing. Already set: SET_KEY. Still empty: EMPTY_KEY. The values stay in the file: do not read them back, use them through the program that loads it.",
  );
  assert.equal(state.told.status, "checked");
  assert.equal(writes(hangar).length, 0);

  const text = await opened({ params: { path: "a.txt" }, caller: CALLER, files: { "a.txt": "x\n" } });
  await text.controller.tellNoChange();
  assert.equal(text.hangar.callsTo("panes.send")[0].args.text, "The person checked `a.txt` and changed nothing.");
  text.controller.editText("y\n");
  await text.controller.tellNoChange();
  assert.equal(text.hangar.callsTo("panes.send").length, 1, "not while something is unsaved");
});

test("an agent that cannot be told: saved all the same, it says so, and it can try again", async () => {
  const { hangar, controller, state } = await opened({
    params: { path: ".env" }, caller: CALLER, files: { ".env": "A=1\n", ...IGNORED }, sendError: "not_found",
  });
  controller.editRow("A", "2");
  await controller.saveForm();
  await settle();
  assert.equal(hangar.store.get(".env"), "A=2\n");
  assert.equal(state.saved, true);
  assert.deepEqual(state.told, { status: "failed", pane: "Asker", code: "not_found" });
  hangar.sendError = null;
  await controller.retryTell();
  assert.equal(state.told.status, "sent");
});

test("asked again: the latest to ask is answered, and what was typed stays", async () => {
  const { hangar, controller, state } = await opened({
    params: { path: ".env", keys: "A" }, caller: CALLER, files: { ".env": "A=\nB=2\n", ...IGNORED },
  });
  controller.editRow("A", "typing");
  hangar.emit("tab.params", { params: { path: ".env", keys: "A,NEW", why: "Now this too." }, caller: { pane: "Other" } });
  await settle();
  assert.equal(state.caller.pane, "Other");
  assert.equal(row(state, "A").value, "typing");
  assert.equal(state.why, "Now this too.");
  assert.deepEqual(state.form.rows.map((each) => [each.key, each.asked]), [["A", true], ["NEW", true], ["B", false]]);
  assert.equal(hangar.callsTo("files.read").filter((call) => call.args.path === ".env").length, 1, "not read again");
  await controller.saveForm();
  await settle();
  assert.equal(hangar.callsTo("panes.send")[0].args.pane, "Other");
});

test("asked again after a save: the new asker can be told there is nothing more to change", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, caller: CALLER, files: { "a.txt": "x\n" } });
  controller.editText("y\n");
  controller.review();
  await controller.saveText();
  await settle();
  assert.equal(state.saved, true);
  hangar.emit("tab.params", { params: { path: "a.txt" }, caller: { pane: "Other" } });
  await settle();
  assert.equal(state.saved, false);
  assert.equal(state.told, null);
  await controller.tellNoChange();
  assert.deepEqual(hangar.callsTo("panes.send").at(-1).args, { pane: "Other", text: "The person checked `a.txt` and changed nothing." });
});

test("the person opening it again takes the caller away, and nobody is told", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, caller: CALLER, files: { "a.txt": "x\n" } });
  hangar.emit("tab.params", { params: { path: "a.txt" }, caller: null });
  await settle();
  assert.equal(state.caller, null);
  controller.editText("y\n");
  controller.review();
  await controller.saveText();
  await settle();
  assert.equal(hangar.callsTo("panes.send").length, 0);
});

/* ── what Hangar refuses ──────────────────────────────────────────── */

test("reading denied: a page that says what to do, with nothing written", async () => {
  const { state, hangar } = await opened({ params: { path: ".env" }, files: { ".env": "A=1\n" }, deny: ["files.read"] });
  assert.equal(state.view, "error");
  assert.equal(state.error.kind, "denied");
  assert.equal(writes(hangar).length, 0);
});

test("listing denied: the picker says so too", async () => {
  const { state } = await opened({ deny: ["files.list"] });
  assert.equal(state.view, "error");
  assert.equal(state.error.kind, "denied");
});

test("writing denied or over the limit: the edits stay, and it says so", async () => {
  for (const code of ["denied", "rate_limited", "invalid", "failed"]) {
    const { hangar, controller, state } = await opened({ params: { path: ".env" }, files: { ".env": "A=1\n", ...IGNORED } });
    hangar.writeError = code;
    controller.editRow("A", "typed");
    await controller.saveForm();
    await settle();
    assert.equal(state.problem.code, code === "failed" ? "other" : code);
    assert.equal(row(state, "A").value, "typed", code);
    assert.equal(controller.isDirty(), true);
    assert.equal(state.saved, false);
    assert.equal(state.busy, false);
    hangar.writeError = null;
    await controller.saveForm();
    await settle();
    assert.equal(hangar.store.get(".env"), "A=typed\n");
    assert.equal(state.problem, null);
  }
});

test("panes.send denied: saved, and it says the agent was not told", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.txt" }, caller: CALLER, files: { "a.txt": "x\n" }, deny: ["panes.send"] });
  controller.editText("y\n");
  controller.review();
  await controller.saveText();
  await settle();
  assert.equal(hangar.store.get("a.txt"), "y\n");
  assert.equal(state.told.code, "denied");
});

test("a title or a badge refused changes nothing", async () => {
  const { state } = await opened({ params: { path: ".env", keys: "A" }, files: { ".env": "", ...IGNORED }, deny: ["tab.setTitle", "tab.setBadge"] });
  assert.equal(state.view, "dotenv");
});

test("a path that can never open: said as such, and Hangar is not asked", async () => {
  for (const path of ["../outside", "/etc/passwd", "C:/x", ".git/config", "a/../b", "sub\\.GIT\\hooks", "a. "]) {
    const { state, hangar } = await opened({ params: { path } });
    assert.equal(state.view, "error", path);
    assert.equal(state.error.kind, "bad-path", path);
    assert.equal(hangar.callsTo("files.read").length, 0, path);
  }
});

test("a file Hangar cannot read shows why", async () => {
  const hangar = fakeHangar({ params: { path: "big.txt" } });
  const read = hangar.call;
  hangar.call = async (method, args) => {
    if (method === "files.read") throw Object.assign(new Error("big.txt is larger than 1 MB"), { code: "failed" });
    return read(method, args);
  };
  const { state } = await opened({ hangar });
  assert.equal(state.error.kind, "failed");
  assert.equal(state.error.detail, "big.txt is larger than 1 MB");
});

/* ── the picker ───────────────────────────────────────────────────── */

test("no params: a picker, env files first, then config, what is not text left out", async () => {
  const { state, hangar, controller } = await opened({
    files: {
      "README.md": "", "src/app.ts": "", "logo.png": "", "package.json": "{}", "pnpm-lock.yaml": "", "config/app.yaml": "", ".env.local": "A=1", "sub/.env": "", ".npmrc": "", "tsconfig.json": "{}",
    },
  });
  assert.equal(state.view, "picker");
  assert.equal(hangar.callsTo("files.list").length, 1);
  assert.deepEqual(state.picker.groups.env.map((file) => [file.path, file.isNew]), [[".env", true], [".env.local", false], ["sub/.env", false]]);
  assert.deepEqual(state.picker.groups.config.map((file) => file.path), ["package.json", "config/app.yaml", ".npmrc", "tsconfig.json"]);
  assert.deepEqual(state.picker.groups.other.map((file) => file.path), ["README.md", "src/app.ts", "pnpm-lock.yaml"]);

  await controller.pick(".env.local");
  await settle();
  assert.equal(state.view, "dotenv");
  assert.equal(state.fromPicker, true);
  assert.equal(state.path, ".env.local");
  assert.equal(hangar.callsTo("tab.setTitle").at(-1).args.title, ".env.local");

  await controller.back();
  await settle();
  assert.equal(state.view, "picker");
  assert.equal(state.path, null);
});

test("the picker's filter, and a listing that was cut", async () => {
  const { lib: { logic } } = require("./helpers.js");
  const groups = logic.rankFiles([{ path: "a.yaml" }, { path: "b/.env" }, ...Array.from({ length: 450 }, (_, i) => ({ path: `src/f${i}.ts` }))]);
  assert.deepEqual(logic.filterGroups(groups, "YAML").map((group) => group.files.length), [0, 1, 0], "any case");
  const all = logic.filterGroups(groups, "");
  assert.equal(all[2].files.length, 200);
  assert.equal(all[2].more, 250);
  const { state } = await opened({ files: { "a.txt": "" }, truncated: true });
  assert.equal(state.picker.truncated, true);
});

/* ── params, read like form input ─────────────────────────────────── */

test("params", () => {
  const { lib: { logic } } = require("./helpers.js");
  assert.deepEqual(logic.parseKeys("A, B ,A,not a key,1BAD,C.D"), ["A", "B", "C.D"]);
  assert.deepEqual(logic.parseKeys(undefined), []);
  assert.equal(logic.parseLine("12"), 12);
  for (const bad of ["0", "-1", "1e3", "abc", "", undefined]) assert.equal(logic.parseLine(bad), null, String(bad));
  assert.equal(logic.checkPath("a\\b/./c.txt"), "a/b/c.txt");
  assert.equal(logic.checkPath(".env"), ".env");
  assert.equal(logic.checkPath(".github/x.yml"), ".github/x.yml");
  const read = logic.readParams({ params: { path: ".env", why: "  why  ", keys: "A" }, caller: { pane: "P" } });
  assert.deepEqual(read, { path: ".env", badPath: false, keys: ["A"], why: "why", line: null, caller: { pane: "P" } });
  assert.equal(logic.readParams({ params: {}, caller: { pane: "" } }).caller, null);
  assert.equal(logic.tabTitle(`a/${"x".repeat(60)}`).length, 40);
});
