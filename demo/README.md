# demo

The app, built for a browser tab instead of a window, for the live demo on the
site.

```bash
pnpm demo:dev      # http://localhost:4300, with HMR
pnpm demo:build    # → site/demo/index.html, one self-contained file
```

`demo:build` runs vite, then `scripts/demo-singlefile.mjs` folds the CSS, the
bundle and every image into the HTML and deletes the assets folder. That is what
makes the iframe immune to how a host spells its URLs. See site/README.md.

## How it works

`src/` is compiled **unmodified**. `vite.demo.config.ts` points every
`@tauri-apps/*` import at a stand-in in `mock/`, and those talk to `backend/`:

```
mock/core.ts          invoke()  → backend/index.ts
mock/event.ts         listen()  ← backend, an in-process emitter
mock/window.ts        no-ops
mock/dialog.ts        the folder picker returns a plausible path
mock/updater.ts       always up to date, so the banner never shows
mock/process.ts       relaunch reloads the frame
mock/app.ts           the version string
mock/notification.ts  stays silent rather than prompting a visitor
```

```
backend/index.ts            one switch over every command src-tauri exposes
backend/state.ts            the workspaces, folders, shells and sessions a machine reported
backend/scene.ts            what the agents off screen are busy with when the demo opens
backend/checkout.ts         the checkout-v2 story: its tasks, workers, plan and briefs
backend/checkoutReplies.ts  what checkout-v2's orchestrator and workers say
backend/board.ts            the task board, in memory, raising board:changed
backend/memory.ts           the global memory, seeded, raising memory:changed
backend/shell.ts            the pseudo-terminal: a shell, and an agent mode
backend/wrap.ts             wraps a transcript at a word, as Claude Code does
backend/replies.ts          what the agents answer
backend/model.ts            the 3D preview's file stat, and its URL scheme over fetch()
backend/stats.ts            four months of made-up usage statistics, seeded
```

Because `src/` is untouched, a command that changes name in Rust breaks a panel
here rather than a type check. That is the trade for having the demo *be* the
app instead of a drawing of it.

## The 3D model

`models/sample.glb` is the one model on the demo's pretend disk, at
`C:\dev\hangar-ai\demo\models\sample.glb` in the hangar-ai workspace: a drone
hangar and a quadcopter flying a loop around it. It is generated from three.js
primitives by `node scripts/demo-model.mjs` — no third-party asset — so it can
be changed and rebuilt at will. `backend/model.ts` answers `model_stat` for it
and serves it wherever the app fetches the `hangar-model` scheme.

The demo build inlines the 3D preview's engine into the page, three.js
included, which the desktop app keeps in a chunk of its own: the page has no
sub-resources to load it from. The Draco and Basis decoders stay out; the
sample needs neither.

## The terminals

`backend/shell.ts` is a small state machine per pane. It starts in `shell` mode
(`ls`, `pwd`, `git status`, `git log`, `pnpm tauri build`, `clear`, `help`,
`exit`, arrow-key history, Ctrl+C) and drops into an agent when you run
`claude`, `codex`, `gemini` or `opencode`. The app types those itself on launch,
resume flag included, which is why panes come up already inside an agent.

An agent starts the way Claude Code does: the screen cleared, its header at the
top. A session the demo has a past for (Iris's and Kai's, in checkout-v2) is
printed back on resume, as Claude Code prints a conversation it resumes.

In agent mode a line goes to `replies.ts`, which picks a transcript by keyword
and plays it out on a timer. Some of them really do move the board, so a pane
that says it claimed a task has claimed it, and the Board view updates while you
watch. Transcripts are wrapped at a word for the pane's width, and drawn again
when the pane is resized, as Claude Code redraws itself.

A message an agent or Hangar queued for a pane reaches it under the label the
hangar-bridge mod gives it (`[Hangar] Event — not typed by the person.`), and
`replies.ts` answers by who sent it: an orchestrator reacts to Hangar's events, a
worker to its brief and to its delivery's outcome.

**No model runs here, and nothing is written to disk.** Every reply is a string
in `replies.ts` and `checkoutReplies.ts`. The demo says so once: in the bar above
the frame on the site, and in a line above the app when it is opened on its own
page (`main.tsx`). Not in every pane, where it would be noise in every capture.
If you extend this, keep it saying so.

## checkout-v2

The workspace the site's hero is captured on, and the one place the
orchestration plays out for real. `backend/checkout.ts` holds its story, and the
board, the memory, the panes and the transcripts all read it:

- Iris, the orchestrator, comes back with her session: the objective, the plan
  written to memory, five tasks with the files they own, two workers started,
  then Lou's delivery landing and the cart totals handed to Jade.
- Jade starts on her brief (`workerBrief` from `src/lib/workerControl.ts`, sent
  as Iris's message) and does her task live, for about two minutes, down to
  `delivery_submit`, which goes through the app: the delivery passes (the
  stand-in in `backend/index.ts` answers as `delivery.rs` would), Needs you
  lists it, and Iris hears of it.
- Kai's delivery passed its check and its review and waits for you: **Merge** in
  Needs you lands it, Kai's pane closes, and Iris tries to start the task it
  unblocked. `worker_spawn` then fails for real, because the browser has no git
  for a worktree, and Iris says so.

Branch names, worktree folders and ports follow `src/lib/workerControl.ts`;
keep them in step if that changes.

## Adding a reply

Add a branch to `replyFor` in `replies.ts`. A step is `{ after, text, fx }`:
`after` is milliseconds since the previous step, `text` is written to the
terminal (a function if it needs to name something an earlier step found), `fx`
is where a board call goes.
