import type { ActionState, ChatItem, ChatQuestion, Pending } from "./api";

/**
 * The fake PC's conversations, as the real one reads them from Claude Code's
 * transcripts (task C0): what the person and Hangar wrote, Claude's answers,
 * its tool calls one line each, the questions it asked and what it waits for.
 *
 * Ada's is long, three pages of it, and works on; Ben waits for a
 * permission, Cora asks three questions, one of several choices; Max, the
 * project manager, answers in the manager's tab.
 */

export interface FakeChat {
  sessionId: string;
  /** Oldest first. */
  items: ChatItem[];
  pending: Pending | null;
}

const SECOND = 1000;

type Action = Omit<Extract<ChatItem, { kind: "action" }>, "id" | "at" | "kind">;

/** A conversation written forward, then moved so its last item is `endsAt`. */
function transcript(prefix: string) {
  const items: ChatItem[] = [];
  let at = 0;
  let n = 0;
  const next = (seconds: number) => ({ id: `${prefix}-${++n}`, at: (at += seconds * SECOND) });
  const action = (seconds: number, fields: Action) => items.push({ ...next(seconds), kind: "action", ...fields });

  return {
    person: (text: string, seconds = 40, images = 0) => items.push({ ...next(seconds), kind: "user", text, from: "person", images }),
    hangar: (text: string, seconds = 4) => items.push({ ...next(seconds), kind: "user", text: `[Hangar] ${text}`, from: "hangar", images: 0 }),
    claude: (text: string, seconds = 9) => items.push({ ...next(seconds), kind: "assistant", text }),
    notice: (text: string, seconds = 2) => items.push({ ...next(seconds), kind: "notice", text }),
    read: (path: string, lines = 120, seconds = 2) =>
      action(seconds, { tool: "Read", title: `Read ${path}`, detail: path, result: { ok: true, text: `${lines} lines` }, state: "done" }),
    readMany: (paths: string[], seconds = 3) =>
      action(seconds, { tool: "Read", title: `Read ${paths.length} files`, detail: paths.join("\n"), state: "done" }),
    search: (pattern: string, found: string, seconds = 2, tool = "Grep") =>
      action(seconds, { tool, title: `Searched for ${pattern}`, detail: pattern, result: { ok: true, text: found }, state: "done" }),
    edit: (path: string, diff: string, seconds = 7) =>
      action(seconds, { tool: "Edit", title: `Edited ${path}`, detail: diff, result: { ok: true, text: `Updated ${path}` }, state: "done" }),
    write: (path: string, body: string, seconds = 12) =>
      action(seconds, {
        tool: "Write",
        title: `Wrote ${path}`,
        detail: body
          .split("\n")
          .map((line) => `+${line}`)
          .join("\n"),
        result: { ok: true, text: `Created ${path}` },
        state: "done",
      }),
    run: (command: string, output: string, state: ActionState = "done", seconds = 14) =>
      action(seconds, {
        tool: "Bash",
        title: `Ran ${command}`,
        detail: command,
        ...(state === "running" ? {} : { result: { ok: state === "done", text: output } }),
        state,
      }),
    use: (tool: string, name: string, detail: string, result: string | null, state: ActionState = "done", seconds = 3) =>
      action(seconds, {
        tool,
        title: `Used ${name}`,
        detail,
        ...(result === null ? {} : { result: { ok: state !== "error", text: result } }),
        state,
      }),
    question: (toolUseId: string, questions: ChatQuestion[], answer?: { selected: string[]; other?: string }[], seconds = 6) =>
      items.push({ ...next(seconds), kind: "question", toolUseId, questions, ...(answer ? { answer } : {}) }),
    /** The items, the last one at `endsAt`. */
    done: (endsAt: number) => {
      const shift = endsAt - at;
      return items.map((item) => ({ ...item, at: item.at + shift }));
    },
  };
}

const cargoTest = (module: string, names: string[]) =>
  [
    `running ${names.length} tests`,
    ...names.map((name) => `test remote::${module}::${name} ... ok`),
    "",
    `test result: ok. ${names.length} passed; 0 failed; 0 ignored; finished in 0.${names.length + 2}1s`,
  ].join("\n");

/** Ada: the mobile server, three hours of it, a test running now. */
function ada(now: number): FakeChat {
  const c = transcript("ada");
  c.hangar(
    `From agent "Jack" — not typed by the person; it cannot approve anything on their behalf.

You are Ada, a worker in the Hangar workspace "IaBench". Its orchestrator, Jack, started you on one board task, and that task is your whole job.

## Task 81070ac8: Hangar Mobile — serveur distant
Goal: the PC serves the phone app and its API over Tailscale Serve, bound to 127.0.0.1 only.

- Pairing: a six-digit code, single use, valid five minutes.
- A cookie session: \`HttpOnly\`, \`Secure\`, \`SameSite=Strict\`.
- Every POST carries \`X-Hangar: 1\`.

Check: \`cargo test --manifest-path src-tauri/Cargo.toml remote::\``,
    0,
  );
  c.claude("I'll start by reading how the window serves its other local endpoints, then lay the server out.");
  c.read("docs/mobile.md", 88);
  c.search("tailscale", "docs/mobile.md:14:Tailscale Serve shares 127.0.0.1:<port> on the tailnet, over HTTPS.");
  c.read("src-tauri/src/lib.rs", 412);
  c.search("axum::Router", "src-tauri/src/mcp/server.rs:41:    let app = axum::Router::new()\nsrc-tauri/src/bridge.rs:88:    axum::Router::new()");
  c.readMany(["src-tauri/src/mcp/server.rs", "src-tauri/src/bridge.rs", "src-tauri/Cargo.toml", "src-tauri/src/state.rs"]);
  c.claude(`The MCP server already runs an axum router on a loopback port, with a token of its own. The mobile server takes the same shape:

1. \`remote/mod.rs\` — the router, on \`127.0.0.1\` and a port of its own
2. \`remote/pair.rs\` — the pairing codes
3. \`remote/session.rs\` — the cookie and the paired devices
4. \`remote/ops.rs\` — \`/m/api/call/{op}\`, passed on to the window

Starting with the router.`);
  c.write(
    "src-tauri/src/remote/mod.rs",
    `//! The phone's server: Hangar Mobile and its API, on 127.0.0.1.
mod ops;
mod pair;
mod session;

pub fn router(state: RemoteState) -> axum::Router {
    axum::Router::new()
        .route("/m/api/health", get(health))
        .route("/m/api/pair", post(pair::pair))
        .route("/m/api/call/:op", post(ops::call))
        .with_state(state)
}`,
  );
  c.run("cargo check --manifest-path src-tauri/Cargo.toml", "    Checking hangar v1.1.0 (src-tauri)\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 41.27s");

  // The work of the first two hours: one part after another, read, changed, tested.
  const parts: { what: string; file: string; module: string; diff: string; tests: string[] }[] = [
    {
      what: "the pairing codes: six digits, single use, five minutes",
      file: "src-tauri/src/remote/pair.rs",
      module: "pair",
      diff: "@@ -12,6 +12,14 @@ pub struct Codes {\n-    code: Option<String>,\n+    code: Option<Code>,\n+}\n+\n+struct Code {\n+    digits: String,\n+    expires: Instant,\n+    tries: u8,\n }",
      tests: ["a_code_is_single_use", "a_code_expires_after_five_minutes", "five_wrong_tries_kill_the_code"],
    },
    {
      what: "the paired devices, kept on disk",
      file: "src-tauri/src/remote/devices.rs",
      module: "devices",
      diff: "@@ -1,4 +1,9 @@\n+/// The phones this PC paired, by the hash of their session.\n+pub struct Devices {\n+    path: PathBuf,\n+    known: Vec<Device>,\n+}",
      tests: ["a_device_survives_a_restart", "forgetting_a_device_ends_its_session"],
    },
    {
      what: "the origin check: a POST without `X-Hangar: 1` is refused",
      file: "src-tauri/src/remote/guard.rs",
      module: "guard",
      diff: "@@ -20,3 +20,8 @@ pub async fn guard(req: Request, next: Next) -> Response {\n+    if req.method() == Method::POST && req.headers().get(\"x-hangar\") != Some(&ONE) {\n+        return refuse(StatusCode::FORBIDDEN, \"missing-header\");\n+    }",
      tests: ["a_post_without_the_header_is_refused", "a_get_needs_no_header"],
    },
    {
      what: "the ops, passed on to the window with a ten-second timeout",
      file: "src-tauri/src/remote/ops.rs",
      module: "ops",
      diff: "@@ -8,6 +8,12 @@ const OPS: &[&str] = &[\n     \"state\",\n     \"screen\",\n+    \"send\",\n+    \"permission\",\n+    \"manager\",\n+    \"history\",\n ];",
      tests: ["an_unknown_op_is_a_404", "a_slow_window_is_a_503", "the_ops_reach_the_window"],
    },
    {
      what: "the uploads, written into the workspace's .hangar folder",
      file: "src-tauri/src/remote/upload.rs",
      module: "upload",
      diff: "@@ -30,4 +30,7 @@ pub async fn upload(\n+    let name = safe_name(&query.name);\n+    let path = workspace.join(\".hangar/uploads\").join(format!(\"{stamp}-{name}\"));\n+    tokio::fs::write(&path, &body).await?;",
      tests: ["an_upload_lands_in_the_workspace", "a_name_cannot_climb_out"],
    },
    {
      what: "the files agents share with the phone",
      file: "src-tauri/src/remote/shares.rs",
      module: "shares",
      diff: "@@ -1,3 +1,8 @@\n+/// What phone_share copied for the phone, newest first.\n+pub fn list(since: Option<u64>) -> Vec<Share> {\n+    let mut shares = read_index();\n+    shares.retain(|share| since.map_or(true, |since| share.at > since));",
      tests: ["shares_come_newest_first", "since_keeps_only_the_new_ones"],
    },
    {
      what: "rate limits on pairing, so six digits cannot be guessed",
      file: "src-tauri/src/remote/pair.rs",
      module: "pair",
      diff: "@@ -48,6 +48,10 @@ impl Codes {\n+        if self.attempts.recent(Duration::from_secs(60)) >= 10 {\n+            return Err(PairError::TooMany);\n+        }",
      tests: ["ten_tries_a_minute_at_most", "a_good_code_still_works_after_a_pause"],
    },
    {
      what: "the history the phone reads, since its last visit",
      file: "src-tauri/src/remote/history.rs",
      module: "history",
      diff: "@@ -5,3 +5,8 @@\n+/// The activity log's entries after `since`, the newest last.\n+pub fn since(log: &ActivityLog, since: Option<u64>) -> Vec<Entry> {\n+    log.entries().filter(|entry| since.map_or(true, |since| entry.at > since)).collect()\n+}",
      tests: ["since_keeps_only_the_new_entries", "the_history_survives_a_restart"],
    },
    {
      what: "the health route, with the PC's name",
      file: "src-tauri/src/remote/health.rs",
      module: "health",
      diff: "@@ -1,2 +1,9 @@\n+pub async fn health(State(state): State<RemoteState>, jar: CookieJar) -> Json<Health> {\n+    Json(Health {\n+        ok: true,\n+        pc: state.pc_name.clone(),\n+        paired: state.sessions.knows(&jar),\n+    })",
      tests: ["health_needs_no_session", "health_says_whether_the_phone_is_paired"],
    },
    {
      what: "the push keys, made once and kept",
      file: "src-tauri/src/remote/push.rs",
      module: "push",
      diff: "@@ -1,2 +1,7 @@\n+/// The VAPID key pair, made the first time and kept in the app's data.\n+pub fn keys(dir: &Path) -> Result<Keys> {\n+    if let Some(keys) = Keys::read(dir)? {\n+        return Ok(keys);\n+    }",
      tests: ["the_key_is_made_once", "the_public_key_is_base64url"],
    },
  ];
  parts.forEach((part, index) => {
    c.claude(`Next: ${part.what}.`, 30);
    c.search(`mod ${part.module}`, `src-tauri/src/remote/mod.rs:${3 + index}:mod ${part.module};`);
    c.read(part.file, 80 + part.tests.length * 23);
    c.edit(part.file, part.diff);
    c.edit(
      `src-tauri/src/remote/${part.module}_tests.rs`,
      part.tests.map((name) => `+#[tokio::test]\n+async fn ${name}() {\n+    let pc = TestPc::new().await;\n+    …\n+}`).join("\n+\n"),
      9,
    );
    c.run(`cargo test --manifest-path src-tauri/Cargo.toml remote::${part.module}`, cargoTest(part.module, part.tests));
  });
  c.claude(
    "Every part has its tests and they pass: **22 tests** under `remote::`. Committing, then the session cookie, the last piece.",
    20,
  );
  c.run('git commit -m "Hangar Mobile: the server, its pairing and its ops"', "[hangar/hangar-mobile-serveur-distant-81070a 4e1c2b9] Hangar Mobile: the server, its pairing and its ops\n 21 files changed, 1904 insertions(+)");
  c.hangar(
    `Delivery review for « Hangar Mobile — serveur distant »: changes requested.

- Every refusal says why, in a code the phone can read: \`missing-header\`, \`bad-code\`, \`too-big\`.
- Nothing a cookie or a code could be read from goes into the logs.`,
    60 * 9,
  );
  c.claude("Two changes asked. I'll go through each module: its refusals get a code, its logs lose anything secret.");
  for (const part of parts) {
    c.read(part.file, 90 + part.tests.length * 25);
    c.edit(
      part.file,
      `@@ -61,4 +61,4 @@\n-        return Err(StatusCode::BAD_REQUEST);\n+        return Err(refuse(StatusCode::BAD_REQUEST, "bad-${part.module}"));\n-    log::info!("{:?}", req.headers());\n+    log::info!("{} {}", req.method(), req.uri().path());`,
      8,
    );
    c.run(`cargo test --manifest-path src-tauri/Cargo.toml remote::${part.module}`, cargoTest(part.module, part.tests), "done", 11);
  }
  c.claude("Every refusal now carries its code, and the logs only ever show a method and a path.", 10);
  c.run(
    "cargo test --manifest-path src-tauri/Cargo.toml remote::",
    cargoTest("all", parts.flatMap((part) => part.tests)),
    "done",
    40,
  );
  c.run(
    'git commit -m "Hangar Mobile: every refusal says why, the logs keep no secret"',
    "[hangar/hangar-mobile-serveur-distant-81070a 9a03d1e] Hangar Mobile: every refusal says why, the logs keep no secret\n 10 files changed, 96 insertions(+), 41 deletions(-)",
  );
  c.person("Can the phone reach it without Tailscale, from a café's wifi?", 60 * 6);
  c.claude(
    `Not by design: the server only listens on \`127.0.0.1\`, and only Tailscale Serve carries it to the phone, over HTTPS, inside your tailnet. On a café's wifi the phone reaches it all the same **through Tailscale**, as long as the app is on.

Opening it to the internet (Tailscale Funnel) is out of this task's scope; I left a question for you on the board.`,
    18,
  );
  c.notice("Conversation compacted", 60 * 12);

  c.person("How far along are you? I'd like the phone to open straight from a notification without pairing again.", 60 * 20);
  c.claude(`## Where it stands

The server answers on \`127.0.0.1:47950\`, and Tailscale Serve puts it on the tailnet.

| Part | State |
|---|---|
| Pairing codes | done, 9 tests |
| Paired devices | done |
| Ops \`/m/api/call/{op}\` | done |
| Cookie session | in progress |

A notification opens the app with the **same cookie**, so it never pairs again: the cookie lasts 30 days and every answer renews it. [Tailscale Serve](https://tailscale.com/kb/1312/serve) explains how the address is shared.`, 25);
  c.person("Good. Make sure the cookie is SameSite=Strict.");
  c.claude("Checking the cookie's flags where it is built.", 6);
  c.search("SameSite", "src-tauri/src/remote/session.rs:57:        .same_site(SameSite::Lax)\nsrc-tauri/src/remote/session.rs:112:        .same_site(SameSite::Lax)");
  c.read("src-tauri/src/remote/session.rs", 164);
  c.edit(
    "src-tauri/src/remote/session.rs",
    "@@ -54,7 +54,7 @@ fn cookie(token: &str) -> Cookie<'static> {\n     Cookie::build((\"hangar\", token.to_owned()))\n         .http_only(true)\n         .secure(true)\n-        .same_site(SameSite::Lax)\n+        .same_site(SameSite::Strict)\n         .path(\"/m\")\n         .max_age(Duration::days(30))",
  );
  c.run(
    "cargo test --manifest-path src-tauri/Cargo.toml remote::session",
    `running 6 tests
test remote::session::a_session_lasts_thirty_days ... ok
test remote::session::every_answer_renews_it ... ok
test remote::session::the_cookie_is_strict ... FAILED
test remote::session::logout_expires_the_cookie ... ok

failures:

---- remote::session::the_cookie_is_strict stdout ----
thread 'remote::session::the_cookie_is_strict' panicked at src-tauri/src/remote/session.rs:203:9:
assertion \`left == right\` failed
  left: "hangar=; HttpOnly; SameSite=Lax; Secure; Path=/m; Max-Age=0"
 right: "hangar=; HttpOnly; SameSite=Strict; Secure; Path=/m; Max-Age=0"

test result: FAILED. 5 passed; 1 failed; 0 ignored; finished in 0.08s`,
    "error",
    21,
  );
  c.claude("One test still sees `Lax`: the logout route builds its own expired cookie. Fixing it there too.", 7);
  c.edit(
    "src-tauri/src/remote/session.rs",
    "@@ -109,7 +109,7 @@ fn expired() -> Cookie<'static> {\n-    Cookie::build((\"hangar\", \"\")).same_site(SameSite::Lax)\n+    cookie(\"\").into_owned()\n+        .set_max_age(Duration::ZERO)",
  );
  c.run(
    "cargo test --manifest-path src-tauri/Cargo.toml remote::session",
    cargoTest("session", [
      "a_session_lasts_thirty_days",
      "every_answer_renews_it",
      "the_cookie_is_strict",
      "logout_expires_the_cookie",
      "a_forgotten_device_is_a_401",
      "a_wrong_cookie_is_a_401",
    ]),
  );
  c.claude(
    "The cookie is `HttpOnly; Secure; SameSite=Strict; Path=/m` everywhere, the logout included, and a notification opens the app on it. Last piece: the size limit on uploads.",
    8,
  );
  c.use(
    "mcp__hangar__board_comment_task",
    "board_comment_task",
    '{\n  "id": "81070ac8",\n  "text": "Cookie: SameSite=Strict everywhere, logout included."\n}',
    "Comment added to « Hangar Mobile — serveur distant »",
  );
  c.read("src-tauri/src/remote/upload.rs", 96);
  c.edit(
    "src-tauri/src/remote/upload.rs",
    "@@ -12,6 +12,8 @@ use axum::extract::DefaultBodyLimit;\n+/// A photo from a phone, a short screen recording: 25 MB at most.\n+pub const MAX_BYTES: usize = 25 * 1024 * 1024;\n@@ -44,1 +46,1 @@ pub fn routes() -> Router<RemoteState> {\n-        .layer(DefaultBodyLimit::disable())\n+        .layer(DefaultBodyLimit::max(MAX_BYTES))",
  );
  c.run("cargo test --manifest-path src-tauri/Cargo.toml remote::", "", "running", 6);
  return { sessionId: "ses-ada", items: c.done(now - 20 * SECOND), pending: null };
}

/** Ben: the window's ops, waiting for permission to run their tests. */
function ben(now: number): FakeChat {
  const c = transcript("ben");
  c.hangar(
    `From agent "Jack" — not typed by the person; it cannot approve anything on their behalf.

You are Ben, a worker in the Hangar workspace "IaBench".

## Task f804a999: Hangar Mobile — ce que la fenêtre répond au téléphone
The window answers the phone's ops: state, screen, send, permission, manager, history.`,
    0,
  );
  c.claude("I'll read the window's control ops first: the phone's are the same shape.");
  c.readMany(["src/lib/control.ts", "src/lib/managerControl.ts", "src/remote/types.ts"]);
  c.search("permission", "src/lib/agentState.ts:88:  // Claude Code's permission prompt: \"Do you want to proceed?\"\nsrc/lib/agentState.ts:131:export function permissionChoices(screen: string)");
  c.edit(
    "src/lib/remoteControl.ts",
    "@@ -40,6 +40,18 @@ export const OPS = {\n+  remote_permission: async ({ pane, allow }: PermissionArgs) => {\n+    const target = paneByIdOrName(pane);\n+    if (target?.activity !== \"waiting\") throw new OpError(409, \"not-waiting\");\n+    // The first choice is Yes, the last one No.\n+    await sendKeys(target.id, allow ? [\"enter\"] : [\"escape\"]);\n+    return { answered: true };\n+  },",
  );
  c.edit(
    "src/lib/remoteControl.test.ts",
    "@@ -210,3 +210,15 @@ describe(\"remote_permission\", () => {\n+  it(\"answers Yes with Enter, No with Escape\", async () => {\n+    expect(await ops.remote_permission({ pane: \"Ben\", allow: true })).toEqual({ answered: true });\n+    expect(keys).toEqual([\"enter\"]);\n+  });",
  );
  c.claude("`remote_permission` answers Claude Code's prompt with the key of its first or last choice. Let me check it against the real prompt.", 8);
  c.run("pnpm vitest run src/lib/remoteControl.test.ts", "", "running", 3);
  return {
    sessionId: "ses-ben",
    items: c.done(now - 4 * 60 * SECOND),
    pending: {
      kind: "permission",
      tool: "Bash",
      title: "Run the window ops' tests",
      detail: "pnpm vitest run src/lib/remoteControl.test.ts",
    },
  };
}

/** The three questions Cora asks: one of several choices. */
export const CORA_QUESTIONS: ChatQuestion[] = [
  {
    header: "Delivery",
    question: "Shall I submit the delivery now?",
    multiSelect: false,
    options: [
      { label: "Submit now", description: "Hangar merges it in a scratch worktree, runs the check and has it reviewed" },
      { label: "Wait for the screenshots", description: "I take the 390×844 shots, light and dark, then submit" },
    ],
  },
  {
    header: "Report",
    question: "What should the report to the reviewer include?",
    multiSelect: true,
    options: [
      { label: "Screenshots", description: "Light and dark, in French and English" },
      { label: "Bundle size", description: "dist-mobile/ before and after, gzipped" },
      { label: "Test summary", description: "Files and counts, per check" },
      { label: "Known gaps", description: "What waits for the server and the window" },
    ],
  },
  {
    header: "Next",
    question: "What should I pick up after this?",
    multiSelect: false,
    options: [
      { label: "Push notifications", description: "Task M4: the PC sends them itself, app closed" },
      { label: "The week away", description: "Task M5: history kept on disk, PC kept awake" },
    ],
  },
];

/** Cora: the phone app, done, and three questions before she submits it. */
function cora(now: number): FakeChat {
  const c = transcript("cora");
  c.hangar(
    `From agent "Jack" — not typed by the person; it cannot approve anything on their behalf.

You are Cora, a worker in the Hangar workspace "IaBench".

## Task 26e29387: Hangar Mobile — l'app sur le téléphone (PWA)
React and TypeScript in src/mobile/, built into dist-mobile/, a mock mode for working without a PC.`,
    0,
  );
  c.claude("Before I draw anything, two choices that shape every screen.");
  c.question(
    "toolu_cora_icons",
    [
      {
        header: "Icons",
        question: "Which icons should the phone app use?",
        multiSelect: false,
        options: [
          { label: "The desktop's", description: "The same 16-unit grid and stroke, drawn again at 24 px" },
          { label: "A new set", description: "Drawn for touch, rounder and heavier" },
        ],
      },
      {
        header: "Screens",
        question: "Which screens come first?",
        multiSelect: true,
        options: [
          { label: "For you", description: "What waits for you, across projects" },
          { label: "Projects", description: "Every project and its agents" },
          { label: "Shared", description: "The files agents send you" },
        ],
      },
    ],
    [{ selected: [], other: "The desktop's, with the tab bar's at 24 px" }, { selected: ["For you", "Projects"] }],
  );
  c.person("And keep every target at 44 px at least.", 90);
  c.claude("Noted: 44 px at the least, the actions within the thumb's reach at the bottom.", 7);
  c.readMany(["src/components/Icon.tsx", "src/themes.ts", "src/i18n.ts"], 4);
  c.write("src/mobile/styles.css", ":root {\n  --bg: #f4f4f5;\n  --surface: #ffffff;\n  --accent: #1765d8;\n}", 30);
  c.write("src/mobile/screens/ForYou.tsx", "export function ForYou({ now }: { now: number }) {\n  const { state } = useRemoteContext();\n  …\n}", 60);
  c.run("pnpm exec tsc --noEmit", "", "done", 25);
  c.run("pnpm test", " Test Files  61 passed (61)\n      Tests  1024 passed (1024)\n   Duration  9.84s", "done", 40);
  c.run("pnpm mobile:build", "vite v7.0.4 building for production...\n✓ 92 modules transformed.\ndist-mobile/index.html                 1.21 kB │ gzip:  0.58 kB\ndist-mobile/assets/index-Cq3f.js     201.43 kB │ gzip: 63.02 kB\n✓ built in 1.94s", "done", 12);
  c.claude(`The phone app builds into \`dist-mobile/\` and every screen works in mock mode, light and dark, in French and English.

Checks:
- \`pnpm exec tsc --noEmit\`: clean
- \`pnpm test\`: 61 files, 1 024 tests passed
- \`pnpm mobile:build\`: \`dist-mobile/\` in 1.9 s

A few things to settle before I submit it.`, 10);
  c.question("toolu_cora_ship", CORA_QUESTIONS, undefined, 4);
  return {
    sessionId: "ses-cora",
    items: c.done(now - 2 * 60 * SECOND),
    pending: { kind: "question", toolUseId: "toolu_cora_ship", questions: CORA_QUESTIONS },
  };
}

/** Jack: the orchestrator, handing the work out. */
function jack(now: number): FakeChat {
  const c = transcript("jack");
  c.notice("Session resumed", 0);
  c.person("Plan the Hangar Mobile work and hand it out");
  c.readMany(["docs/mobile.md", "src-tauri/src/mcp/server.rs", "src/lib/control.ts"]);
  const tasks = ["the server (M0a)", "the window's ops (M1)", "the phone app (M2)", "files both ways (M0b)", "settings (M3)", "push (M4)", "the week away (M5)"];
  for (const task of tasks) {
    c.use("mcp__hangar__board_create_task", "board_create_task", `{ "title": "Hangar Mobile — ${task}" }`, `Created « Hangar Mobile — ${task} »`, "done", 4);
  }
  c.claude("I split it into seven tasks on the board: the server (M0a), the window's ops (M1), the phone app (M2), files both ways (M0b), settings (M3), push (M4) and the week away (M5). M0a, M1 and M2 run side by side.");
  for (const [name, task] of [["Ada", "81070ac8"], ["Ben", "f804a999"], ["Cora", "26e29387"]]) {
    c.use("mcp__hangar__worker_spawn", "worker_spawn", `{ "task": "${task}", "name": "${name}" }`, `${name} started`, "done", 6);
  }
  c.claude("Ada, Ben and Cora are on M0a, M1 and M2. I'll watch them, and land what is ready.");
  c.use("mcp__hangar__worker_list", "worker_list", "{}", null, "running", 50);
  return { sessionId: "ses-jack", items: c.done(now - 30 * SECOND), pending: null };
}

/** Mia: the storefront, done. */
function mia(now: number): FakeChat {
  const c = transcript("mia");
  c.person("Run Lighthouse on mobile and send me the report.", 0);
  c.run("pnpm lighthouse --preset=mobile --output=html --output-path=docs/lighthouse.html", "Performance: 98\nAccessibility: 100\nBest practices: 100\nSEO: 100");
  c.use("mcp__hangar__phone_share", "phone_share", '{ "path": "docs/lighthouse.html", "caption": "Lighthouse report, mobile" }', "Shared lighthouse.html with the phone");
  c.claude(`All twelve tasks are done and landed on main. The storefront passes Lighthouse at **98** on mobile; the report is in \`docs/lighthouse.html\`, and on your phone under Shared.

Anything else for this sprint?`);
  return { sessionId: "ses-mia", items: c.done(now - 50 * 60 * SECOND), pending: null };
}

/** Max: the project manager. */
function max(now: number): FakeChat {
  const c = transcript("max");
  c.person("Where does everything stand?", 0);
  c.use("mcp__hangar__project_list", "project_list", "{}", '[\n  { "name": "IaBench", "open": true, "doing": 3 },\n  { "name": "storefront", "open": true, "doing": 0 },\n  { "name": "billing-api", "asleep": true },\n  { "name": "recipes-app", "open": true }\n]');
  c.claude(`Four projects:
- **IaBench**: Jack orchestrates three workers on Hangar Mobile. Ben waits for a permission, Cora for your answers; one delivery is ready to merge.
- **storefront**: all twelve tasks done, Mia waits for what's next.
- **billing-api**: asleep since yesterday evening, two tasks to do.
- **recipes-app**: Leo (Codex) is adding the shopping list export.`);
  c.person("Wake billing-api tomorrow at 9 and start its two tasks.", 120);
  c.claude(`Noted: I'll wake **billing-api** tomorrow at 9:00 and hand Noé its two tasks:

1. \`Invoices: retry the webhooks that failed\`
2. \`Export the October invoices as CSV\`

I'll write here when they start.`);
  return { sessionId: "ses-max", items: c.done(now - 6 * 60 * SECOND), pending: null };
}

/** Every pane's conversation the fake PC can read, by pane. */
export function conversations(now: number): Record<string, FakeChat> {
  return {
    "p-ada": ada(now),
    "p-ben": ben(now),
    "p-cora": cora(now),
    "p-jack": jack(now),
    "p-mia": mia(now),
    "p-max": max(now),
  };
}

/**
 * A page of a conversation, as the PC cuts them: the `limit` items before
 * `before` (an item's id), or the last ones; the cursor is the first item's
 * id while there are more before it.
 */
export function chatPage(items: ChatItem[], before: string | undefined, limit = 60) {
  const end = before === undefined ? items.length : items.findIndex((item) => item.id === before);
  if (end === -1) return null;
  const start = Math.max(0, end - Math.max(1, limit));
  return { items: items.slice(start, end), older: start > 0, cursor: start > 0 ? items[start].id : null };
}
