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
backend/index.ts      one switch over every command src-tauri exposes
backend/state.ts      the workspaces, shells and sessions a machine reported
backend/board.ts      the task board, in memory, raising board:changed
backend/shell.ts      the pseudo-terminal: a shell, and an agent mode
backend/replies.ts    what the agents answer
```

Because `src/` is untouched, a command that changes name in Rust breaks a panel
here rather than a type check. That is the trade for having the demo *be* the
app instead of a drawing of it.

## The terminals

`backend/shell.ts` is a small state machine per pane. It starts in `shell` mode
(`ls`, `pwd`, `git status`, `git log`, `pnpm tauri build`, `clear`, `help`,
`exit`, arrow-key history, Ctrl+C) and drops into an agent when you run
`claude`, `codex`, `gemini` or `opencode`. The app types those itself on launch,
resume flag included, which is why panes come up already inside an agent.

In agent mode a line goes to `replies.ts`, which picks a transcript by keyword
and plays it out on a timer. Some of them really do move the board, so a pane
that says it claimed a task has claimed it, and the Board view updates while you
watch.

**No model runs here, and nothing is written to disk.** Every reply is a string
in `replies.ts`. The banner each agent prints says so, and so does the page
around the frame. If you extend this, keep it saying so.

## Adding a reply

Add a branch to `replyFor` in `replies.ts`. A step is `{ after, text, fx }`:
`after` is milliseconds since the previous step, `text` is written to the
terminal (a function if it needs to name something an earlier step found), `fx`
is where a board call goes.
