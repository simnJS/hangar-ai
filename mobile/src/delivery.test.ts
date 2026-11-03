import { describe, expect, it } from "vitest";
import { ApiError, type Delivery, type DeliveryFile, type Landed } from "./api";
import {
  checkVerdict,
  fileRows,
  landBlocked,
  landUntilDone,
  outcomeLabel,
  patchLines,
  remember,
  rememberedFile,
  reviewVerdict,
  reworkText,
  seconds,
  stageText,
  totals,
} from "./delivery";
import { translator } from "./i18n";

const en = translator("en");
const fr = translator("fr");

const file = (path: string, status: DeliveryFile["status"], additions: number, deletions: number, extra: Partial<DeliveryFile> = {}): DeliveryFile => ({
  path,
  status,
  additions,
  deletions,
  ...extra,
});

const delivery = (patch: Partial<Delivery> = {}): Delivery => ({
  taskId: "t1",
  title: "Pair a phone by QR code",
  workspace: "IaBench",
  workspaceId: "ws",
  worker: "Dana",
  branch: "hangar/qr",
  target: "main",
  summary: null,
  check: { command: "pnpm test", passed: true, seconds: 96 },
  review: { approved: true, notes: "Clean.", blocking: [] },
  files: [file("src/qr.ts", "added", 88, 0), file("assets/frame.png", "added", 0, 0, { binary: true })],
  waiting: true,
  stage: null,
  merge: "4f9c2ab",
  passedAt: 1,
  moved: false,
  inTheWay: null,
  earlier: [],
  ...patch,
});

describe("the check and the review", () => {
  it("say how long the check took, the number kept with its unit", () => {
    expect(seconds(48)).toBe("48 s");
    expect(seconds(96)).toBe("1 min 36 s");
    expect(seconds(120)).toBe("2 min");
    expect(seconds(3_900)).toBe("1 h 05");
  });

  it("read as one line each, in the phone's language", () => {
    expect(checkVerdict(delivery().check, fr)).toEqual({ tone: "ok", title: "Réussie en 1 min 36 s", detail: "pnpm test" });
    expect(checkVerdict({ command: "pnpm test", passed: false, seconds: 0 }, en)).toMatchObject({ tone: "failed", title: "Failed" });
    expect(checkVerdict(null, en)).toEqual({ tone: "none", title: "No check is set for this project", detail: null });
    expect(reviewVerdict(delivery().review, fr)).toMatchObject({ tone: "ok", title: "Approuvée", notes: "Clean." });
    expect(reviewVerdict({ approved: true, notes: " ", blocking: [{ file: "a.ts", problem: "crashes" }] }, en)).toMatchObject({
      tone: "failed",
      title: "Changes requested",
      notes: null,
    });
    expect(reviewVerdict(null, en)).toMatchObject({ tone: "none", title: "Not reviewed" });
  });

  it("name what an earlier delivery ran into, and what runs now", () => {
    expect(outcomeLabel("checkFailed", fr)).toBe("Vérification échouée");
    expect(outcomeLabel("somethingNew", en)).toBe("Did not pass");
    expect(stageText("reviewing", en)).toBe("being reviewed");
    expect(stageText(null, fr)).toBe("en attente de son tour");
  });
});

describe("the files", () => {
  it("show each name over its folder, its letter and its counts", () => {
    const rows = fileRows(
      [file("src/mobile/qr.ts", "modified", 24, 3), file("logo.png", "added", 0, 0, { binary: true }), file("b.ts", "renamed", 1, 1, { oldPath: "a.ts" })],
      fr,
    );
    expect(rows.map(({ dir, name, mark, label, counts }) => ({ dir, name, mark, label, counts }))).toEqual([
      { dir: "src/mobile/", name: "qr.ts", mark: "M", label: "Modifié", counts: "+24 −3" },
      { dir: "", name: "logo.png", mark: "A", label: "Ajouté", counts: null },
      { dir: "", name: "b.ts", mark: "R", label: "Renommé", counts: "+1 −1" },
    ]);
    expect(totals([file("a", "added", 3, 0), file("b", "modified", 2, 5)])).toEqual({ additions: 5, deletions: 5 });
  });

  it("draw a patch's hunks and lines, without git's header", () => {
    const lines = patchLines(
      "diff --git a/a.ts b/a.ts\nindex 1..2 100644\n--- a/a.ts\n+++ b/a.ts\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n\\ No newline at end of file\n",
    );
    expect(lines.map((line) => [line.kind, line.text])).toEqual([
      ["hunk", "@@ -1,2 +1,2 @@"],
      ["ctx", "keep"],
      ["del", "old"],
      ["add", "new"],
      ["note", "\\ No newline at end of file"],
    ]);
    expect(patchLines("")).toEqual([]);
  });

  it("are remembered for the diff screen's title", () => {
    remember(delivery({ taskId: "t-remembered" }));
    expect(rememberedFile("t-remembered", "src/qr.ts")).toMatchObject({ status: "added", additions: 88 });
    expect(rememberedFile("t-remembered", "nowhere.ts")).toBeNull();
    expect(rememberedFile("t-unknown", "src/qr.ts")).toBeNull();
  });
});

describe("« Fusionner »", () => {
  it("waits while something runs for the delivery, and once the worker moved past it", () => {
    expect(landBlocked(delivery())).toBeNull();
    expect(landBlocked(delivery({ waiting: false, stage: "checking" }))).toBe("running");
    expect(landBlocked(delivery({ moved: true }))).toBe("moved");
    // Its own changes in the way on the PC may be set aside since: it can be tried again.
    expect(landBlocked(delivery({ inTheWay: ["src/a.ts"] }))).toBeNull();
  });

  const fake = (answers: (Landed | Error)[]) => {
    const asked: [string, string | null][] = [];
    return {
      asked,
      client: {
        land: async (task: string, merge: string | null) => {
          asked.push([task, merge]);
          const next = answers.shift();
          if (!next) throw new Error("asked once too often");
          if (next instanceof Error) throw next;
          return next;
        },
      },
    };
  };
  const sleep = async () => undefined;

  it("lands, asking again while a landing tested again runs on the PC", async () => {
    const { client, asked } = fake([
      { landed: false, stage: "checking" },
      { landed: false, stage: "landing" },
      { landed: true, target: "main" },
    ]);
    const stages: (string | null)[] = [];
    expect(await landUntilDone(client, "t1", "4f9c2ab", { onStage: (stage) => stages.push(stage), sleep })).toEqual({
      kind: "landed",
      target: "main",
    });
    expect(stages).toEqual(["checking", "landing"]);
    // The same call each time: the PC waits for the landing it started.
    expect(asked).toEqual([
      ["t1", "4f9c2ab"],
      ["t1", "4f9c2ab"],
      ["t1", "4f9c2ab"],
    ]);
  });

  it("says why the PC did not land it, in the PC's words", async () => {
    const refusal = new ApiError("refused", "refused", 400, "Local changes in main's checkout are in the way (src/a.ts).");
    expect(await landUntilDone(fake([refusal]).client, "t1", null, { sleep })).toEqual({
      kind: "refused",
      reason: "Local changes in main's checkout are in the way (src/a.ts).",
    });
  });

  it("tells a PC gone quiet from a refusal: the landing may go on there", async () => {
    const offline = new ApiError("offline", "timeout");
    expect(await landUntilDone(fake([{ landed: false, stage: "checking" }, offline]).client, "t1", null, { sleep })).toEqual({
      kind: "lost",
      error: offline,
    });
  });

  it("stops asking once the screen is left", async () => {
    let left = false;
    const { client, asked } = fake([{ landed: false, stage: "checking" }, { landed: true, target: "main" }]);
    const outcome = await landUntilDone(client, "t1", null, {
      sleep: async () => {
        left = true;
      },
      stopped: () => left,
    });
    expect(outcome).toEqual({ kind: "landing", stage: "checking" });
    expect(asked).toHaveLength(1);
  });
});

describe("« Renvoyer »", () => {
  it("sends what to fix, not blanks", () => {
    expect(reworkText("  The QR code is too small.\n")).toBe("The QR code is too small.");
    expect(reworkText(" \n ")).toBe("");
  });
});
