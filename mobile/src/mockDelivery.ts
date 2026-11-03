import type { Delivery, DeliveryFile, DeliveryPatch, DeliveryStage, Landed } from "./api";

/**
 * The fake PC's delivery ready to merge (mock.ts): Dana's QR-code pairing on
 * IaBench, which failed its check once, then passed it and its review. Its
 * files' counts are read from their patches, as git counts them; one of them,
 * a generated table, is too big for the phone and comes cut at 200 KB.
 *
 * Landing it takes a few seconds, its target having moved: merged again,
 * checked, then landed. `blocked` lands nothing: the person's own changes on
 * the PC are in the way, as the PC says.
 */

const MIN = 60_000;
const PATCH_MAX_BYTES = 200 * 1024;
const LAND_MS = { landing: 2_400, done: 4_200 };

const TASK = "t-settings";
const BRANCH = "hangar/mobile-settings-qr-3e1f0a";
const CHECK = "pnpm exec tsc --noEmit && pnpm test && pnpm mobile:build";

/** Each file as git prints its diff against main. */
const PATCHES: Record<string, { status: DeliveryFile["status"]; oldPath?: string; patch: string | null }> = {
  "src/components/settings/MobileSettings.tsx": {
    status: "added",
    patch: `@@ -0,0 +1,38 @@
+import { useEffect, useState } from "react";
+import { useT } from "../../i18n";
+import { drawQr } from "../../lib/qr";
+import { remotePairingCode, type PairingCode } from "../../lib/remote";
+import { PairingCodeView } from "./PairingCode";
+
+/** Settings › Mobile: a QR code that pairs a phone in one scan, and the phones paired. */
+export function MobileSettings({ address }: { address: string | null }) {
+  const t = useT();
+  const [code, setCode] = useState<PairingCode | null>(null);
+
+  useEffect(() => {
+    if (!address) return;
+    let current = true;
+    void remotePairingCode().then((next) => current && setCode(next));
+    return () => {
+      current = false;
+    };
+  }, [address]);
+
+  if (!address) return <p className="settings__hint">{t("mobile.offTailscale")}</p>;
+  const link = code ? \`\${address}/m/?code=\${code.code}\` : null;
+
+  return (
+    <section className="settings__section">
+      <h2>{t("mobile.pairTitle")}</h2>
+      <p className="settings__hint">{t("mobile.pairHint")}</p>
+      {link && (
+        <figure className="qr">
+          <canvas ref={(canvas) => canvas && drawQr(canvas, link, { margin: 2 })} aria-label={t("mobile.qrLabel")} />
+          <figcaption>{t("mobile.qrExpires", { minutes: 5 })}</figcaption>
+        </figure>
+      )}
+      {code && <PairingCodeView code={code} onRenew={() => void remotePairingCode().then(setCode)} />}
+    </section>
+  );
+}
`,
  },
  "src/lib/qr.ts": {
    status: "added",
    patch: `@@ -0,0 +1,27 @@
+import { encodeQr, type QrMatrix } from "./qr-tables";
+
+/**
+ * A QR code drawn on a canvas, crisp at any pixel ratio: one square per
+ * module, the quiet zone around it in modules too, as the scanners expect.
+ */
+export function drawQr(canvas: HTMLCanvasElement, text: string, { margin = 4 } = {}): QrMatrix {
+  const matrix = encodeQr(text, "M");
+  const ratio = window.devicePixelRatio || 1;
+  const size = matrix.size + margin * 2;
+  const css = canvas.clientWidth || 220;
+  const scale = Math.floor((css * ratio) / size);
+  canvas.width = canvas.height = scale * size;
+  const context = canvas.getContext("2d");
+  if (!context) return matrix;
+  context.fillStyle = "#ffffff";
+  context.fillRect(0, 0, canvas.width, canvas.height);
+  context.fillStyle = "#0f1114";
+  for (let y = 0; y < matrix.size; y++) {
+    for (let x = 0; x < matrix.size; x++) {
+      if (matrix.dark(x, y)) context.fillRect((x + margin) * scale, (y + margin) * scale, scale, scale);
+    }
+  }
+  return matrix;
+}
+
+export const QR_EXPIRES_MS = 5 * 60_000;
`,
  },
  "src/lib/qr.test.ts": {
    status: "added",
    patch: `@@ -0,0 +1,19 @@
+import { describe, expect, it } from "vitest";
+import { encodeQr } from "./qr-tables";
+
+describe("the pairing QR code", () => {
+  it("holds the whole link, at medium correction", () => {
+    const matrix = encodeQr("https://pc-simon.tail1234.ts.net/m/?code=482913", "M");
+    expect(matrix.version).toBe(4);
+    expect(matrix.size).toBe(33);
+  });
+
+  it("draws the three finder patterns in their corners", () => {
+    const matrix = encodeQr("x", "M");
+    for (const [x, y] of [[0, 0], [matrix.size - 7, 0], [0, matrix.size - 7]]) {
+      expect(matrix.dark(x, y)).toBe(true);
+      expect(matrix.dark(x + 1, y + 1)).toBe(false);
+      expect(matrix.dark(x + 3, y + 3)).toBe(true);
+    }
+  });
+});
`,
  },
  "src/lib/qr-tables.ts": { status: "added", patch: null },
  "src/i18n.ts": {
    status: "modified",
    patch: `@@ -1412,7 +1412,13 @@ const fr = {
   "mobile.title": "Hangar Mobile",
-  "mobile.pairHint": "Ouvrez l'adresse ci-dessous sur le téléphone, puis saisissez le code.",
+  "mobile.pairTitle": "Associer un téléphone",
+  "mobile.pairHint": "Scannez ce code avec l'appareil photo du téléphone : l'app s'ouvre déjà associée. Sinon, ouvrez l'adresse ci-dessous sur le téléphone et saisissez le code à six chiffres.",
+  "mobile.qrLabel": "Code QR d'association",
+  "mobile.qrExpires": "Valable {minutes} min, une seule fois",
+  "mobile.renew": "Nouveau code",
+  "mobile.offTailscale": "Ce PC n'est pas sur Tailscale : connectez-le pour associer un téléphone.",
   "mobile.devices": "Téléphones associés",
   "mobile.forget": "Oublier",
   "mobile.lastSeen": "Vu {time}",
`,
  },
  "src-tauri/src/remote/pairing.rs": {
    status: "modified",
    patch: `@@ -38,16 +38,23 @@ pub struct Pairing {
 /// A code a phone pairs with: six digits, once, for a few minutes.
 pub struct Code {
     pub code: String,
-    pub issued: Instant,
+    pub issued_at: Instant,
+    /// Wrong tries left before the code dies.
+    pub tries_left: u8,
 }

-const CODE_TTL: Duration = Duration::from_secs(10 * 60);
+const CODE_TTL: Duration = Duration::from_secs(5 * 60);
+const CODE_TRIES: u8 = 5;

 impl Pairing {
     pub fn issue(&mut self) -> &Code {
-        self.code = Some(Code { code: six_digits(), issued: Instant::now() });
+        self.code = Some(Code {
+            code: six_digits(),
+            issued_at: Instant::now(),
+            tries_left: CODE_TRIES,
+        });
         self.code.as_ref().unwrap()
     }

@@ -71,7 +78,9 @@ impl Pairing {
         let Some(code) = self.code.as_mut() else { return Err(PairError::Expired) };
-        if code.issued.elapsed() > CODE_TTL {
+        if code.issued_at.elapsed() > CODE_TTL || code.tries_left == 0 {
+            self.code = None;
             return Err(PairError::Expired);
         }
+        code.tries_left -= 1;
         if code.code != given {
`,
  },
  "src/styles/settings.css": {
    status: "modified",
    patch: `@@ -212,6 +212,26 @@
 .settings__hint {
   color: var(--fg-2);
 }
+
+/* The pairing QR code: on white whatever the theme, as scanners want it. */
+.qr {
+  display: grid;
+  justify-items: center;
+  gap: 8px;
+  margin: 16px 0 0;
+}
+
+.qr canvas {
+  width: 220px;
+  height: 220px;
+  border-radius: 12px;
+  box-shadow: 0 0 0 1px var(--line);
+}
+
+.qr figcaption {
+  color: var(--fg-3);
+  font-size: 13px;
+}

 .settings__devices {
-  margin-top: 12px;
+  margin-top: 20px;
`,
  },
  "src/components/settings/PairingCode.tsx": {
    status: "renamed",
    oldPath: "src/components/settings/Pairing.tsx",
    patch: `@@ -4,7 +4,7 @@ import type { PairingCode } from "../../lib/remote";

-/** The six digits to type on the phone. */
-export function Pairing({ code, onRenew }: { code: PairingCode; onRenew: () => void }) {
+/** The six digits to type on the phone, for a phone without a camera at hand. */
+export function PairingCodeView({ code, onRenew }: { code: PairingCode; onRenew: () => void }) {
   const t = useT();
   return (
     <div className="pairing">
`,
  },
  "assets/qr-frame.png": { status: "added", patch: null },
};

/** The generated table: thousands of lines, past what the phone is sent. */
function tables(): string {
  const lines = ["@@ -0,0 +1,6000 @@", "+// Generated by scripts/qr-tables.mjs: do not edit.", "+export const ALPHA_EXP = ["];
  for (let at = 0; at < 5997; at++) lines.push(`+  0x${((at * 37 + 11) % 256).toString(16).padStart(2, "0")}, // ${at}: alpha^${at} in GF(256)`);
  lines.push("+];");
  return `${lines.join("\n")}\n`;
}

function patchOf(path: string): { text: string; binary: boolean } {
  const file = PATCHES[path];
  if (path === "src/lib/qr-tables.ts") return { text: tables(), binary: false };
  return file?.patch === null || !file ? { text: "", binary: true } : { text: file.patch, binary: false };
}

const count = (text: string, sign: "+" | "-") =>
  text.split("\n").filter((line) => line.startsWith(sign) && !line.startsWith(`${sign}${sign}${sign} `)).length;

function files(): DeliveryFile[] {
  return Object.entries(PATCHES)
    .map(([path, file]) => {
      const { text, binary } = patchOf(path);
      return {
        path,
        ...(file.oldPath ? { oldPath: file.oldPath } : {}),
        status: file.status,
        additions: count(text, "+"),
        deletions: count(text, "-"),
        ...(binary ? { binary: true as const } : {}),
      };
    })
    .sort((a, b) => a.path.localeCompare(b.path));
}

/** The answers, and what landing or sending back does to the fake PC. */
export interface FakeDeliveries {
  /** null: not a delivery that waits. */
  delivery: (task: string) => Delivery | null;
  file: (task: string, path: string) => DeliveryPatch | null;
  /** Throws the PC's refusal, in its words. */
  land: (task: string, merge: string | undefined, now: number) => Landed;
  rework: (task: string, text: string, merge: string | undefined) => { sent: true };
}

export class Refusal extends Error {}

export function fakeDeliveries(
  start: number,
  { blocked = false, onLanded, onRework }: { blocked?: boolean; onLanded?: (task: string) => void; onRework?: (task: string, text: string) => void } = {},
): FakeDeliveries {
  const waiting: Delivery = {
    taskId: TASK,
    title: "Mobile settings: pair a phone by QR code",
    workspace: "IaBench",
    workspaceId: "ws-iabench",
    worker: "Dana",
    branch: BRANCH,
    target: "main",
    summary:
      "Settings › Mobile now shows a QR code that pairs a phone in one scan; the six digits stay for a phone without a camera.\n\n" +
      "- The code works once and for five minutes, and dies after five wrong tries.\n" +
      "- Tested in mock mode, light and dark, in French and English.",
    check: { command: CHECK, passed: true, seconds: 96 },
    review: {
      approved: true,
      notes:
        "Clean split between drawing the code and the pairing flow, and the expiry is tested both ways. Not blocking: " +
        "the canvas is drawn again on every render; memoizing it would spare slow phones a few milliseconds.",
      blocking: [],
    },
    files: files(),
    waiting: true,
    stage: null,
    merge: "4f9c2ab",
    passedAt: start - 70 * MIN,
    moved: false,
    inTheWay: null,
    earlier: [
      {
        at: start - 160 * MIN,
        outcome: "checkFailed",
        text: `The check \`${CHECK}\` failed (exit 1) on ${BRANCH} merged onto main.`,
      },
    ],
  };
  const deliveries = new Map<string, Delivery>([[TASK, waiting]]);
  /** When each landing started. */
  const landings = new Map<string, number>();

  const same = (delivery: Delivery, merge: string | undefined) => {
    if (merge !== undefined && merge !== delivery.merge) {
      throw new Refusal(
        `"${delivery.title}" was delivered again since you opened it (now ${delivery.merge}, you saw ${merge}): look at ` +
          "what waits now first. Nothing was done.",
      );
    }
  };
  const stageAt = (since: number, now: number): DeliveryStage => (now - since >= LAND_MS.landing ? "landing" : "checking");

  return {
    delivery: (task) => {
      const delivery = deliveries.get(task);
      if (!delivery) return null;
      const since = landings.get(task);
      return since === undefined ? delivery : { ...delivery, waiting: false, stage: stageAt(since, Date.now()) };
    },
    file: (task, path) => {
      const delivery = deliveries.get(task);
      if (!delivery?.files.some((file) => file.path === path)) return null;
      const { text, binary } = patchOf(path);
      if (text.length <= PATCH_MAX_BYTES) return { path, patch: text, binary, truncated: false };
      return { path, patch: text.slice(0, text.lastIndexOf("\n", PATCH_MAX_BYTES - 1) + 1), binary, truncated: true };
    },
    land: (task, merge, now) => {
      const delivery = deliveries.get(task);
      if (!delivery) throw new Refusal("Nothing of this task waits to land any more: the board shows where it is now.");
      const since = landings.get(task);
      if (since === undefined) {
        same(delivery, merge);
        landings.set(task, now);
        return { landed: false, stage: "checking" };
      }
      if (now - since < LAND_MS.done) return { landed: false, stage: stageAt(since, now) };
      landings.delete(task);
      if (blocked) {
        deliveries.set(task, { ...delivery, inTheWay: ["src/i18n.ts"] });
        throw new Refusal(
          "Local changes in main's checkout are in the way (src/i18n.ts): commit them or set them aside, then " +
            "land it again. Nothing was touched.",
        );
      }
      deliveries.delete(task);
      onLanded?.(task);
      return { landed: true, target: delivery.target };
    },
    rework: (task, text, merge) => {
      const delivery = deliveries.get(task);
      if (!delivery) throw new Refusal("Nothing of this task waits to land any more: the board shows where it is now.");
      if (!text.trim()) throw new Refusal("Say what to fix: give text.");
      same(delivery, merge);
      if (landings.has(task)) throw new Refusal(`"${delivery.title}" is being landed or tested right now: wait for its outcome. Nothing was sent.`);
      onRework?.(task, text.trim());
      return { sent: true };
    },
  };
}
