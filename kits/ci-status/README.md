# CI status

A Hangar kit: a **CI** tab that follows a GitHub Actions run, and opens by itself
after an agent runs `git push`. It is also the example to copy when you make a
kit of your own: every part of a kit is in it, in the smallest form that works.

## What it does

- **The CI tab.** Given a run's number it shows the workflow's name as the tab's
  title, a badge (running, passed, failed), the jobs with their progress and the
  step running now, and the time elapsed. It polls `gh` every 5 seconds while the
  run is going, stops when it ends, and raises one desktop notification. When the
  run failed it shows the end of the log of the failed steps. Without a number it
  lists the 15 most recent runs to choose from. One tab per run.
- **A hook** (Claude Code `PostToolUse` on `Bash`). When an agent in a pane runs
  `git push`, it looks for the run that push started — a run takes a few seconds
  to appear, so it retries for about half a minute — and opens the tab for it. The
  search runs detached: the agent is never kept waiting, and the hook always exits
  0, silently, whatever is missing (Node, `gh`, a run, Hangar).
- **A skill** telling agents to open the tab with `kit_tab_open` when the hook did
  not, and to read a run's outcome with `gh` rather than sleeping.

If `gh` is missing or not signed in, the tab says so and how to fix it. English
and French, from the browser's language; light and dark; readable at the side
panel's narrowest.

## What it runs

Only these three, declared in `hangar-kit.json` and shown to you verbatim before
you enable the kit. The tab runs them with your `gh` login; only `{id}` changes,
and it must be digits.

| Name      | Command                                                                                                  |
| --------- | -------------------------------------------------------------------------------------------------------- |
| `ci-run`  | `gh run view {id} --json status,conclusion,displayTitle,headBranch,workflowName,url,jobs,startedAt,updatedAt` |
| `ci-list` | `gh run list --limit 15 --json databaseId,displayTitle,workflowName,headBranch,status,conclusion,createdAt` |
| `ci-log`  | `gh run view {id} --log-failed`                                                                          |

They run in the workspace's folder, so `gh` finds the repository from its git
remote. The hook runs `git rev-parse HEAD` and `gh run list --commit <sha>` in
the folder the agent pushed from, and talks to Hangar on `127.0.0.1` only. Nothing
else leaves the machine, and the tab loads no external resource.

## Install

You need Hangar 1.9.0 or later, [`gh`](https://cli.github.com) signed in
(`gh auth login`), and `node` on your PATH for the hook (without it, the hook does nothing).

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/simnJS/hangar-ai/tree/main/kits/ci-status`
   (or link a local copy of this folder).
2. Enable it on a workspace, and read what you accept: the three commands, the
   `notify` permission, and the hook.
3. Restart the workspace's Claude Code panes so they load the plugin.

## How it is built

```
ci-status/
├── hangar-kit.json            the kit: id, hangar >=1.9.0, commands, the tab and its params
├── icon.svg
├── .claude-plugin/plugin.json the Claude Code plugin (name = the kit's id)
├── hooks/
│   ├── hooks.json             PostToolUse on Bash → open-ci.js
│   ├── open-ci.js             reads the event, spots a push, spawns find-run.js detached, exits 0
│   ├── find-run.js            the detached search: git + gh, retries, then asks Hangar for the tab
│   └── lib.js                 what counts as a push, which runs belong to it, the call to Hangar
├── skills/ci-status/SKILL.md
├── tabs/ci/
│   ├── index.html             loads /_hangar/kit.js (the SDK), then the four scripts below
│   ├── logic.js               pure: gh's JSON → what to show; messages in en and fr
│   ├── controller.js          polling, title, badge, notification — takes window.hangar as an argument
│   ├── view.js                draws a state with createElement + textContent, never markup
│   ├── app.js                 the three lines that wire them in the page
│   └── style.css              one column, system colours, light and dark
├── test/                      node --test, no dependency; fixtures/ are real gh output
└── package.json, run-tests.js let `node --test kits/ci-status/` find the tests on any Node
```

Choices worth copying:

- **Logic out of the page.** `logic.js` and `controller.js` touch neither the DOM
  nor Hangar except through arguments, so the tests drive them with a fake
  `window.hangar` and a fake clock (`test/helpers.js`).
- **Params, one tab per run.** The tab declares `params.id` with the pattern
  `[0-9]+`. Hangar checks a caller's value against it, and again before any
  command runs; the page re-checks, because a page treats params like form input.
- **Declared commands, not a shell.** Each is an argv; `{id}` is its only
  placeholder and carries a rule. A value starting with `-` is refused by Hangar,
  so an id can never become an option.
- **Badge and title are set on load**, as a stale « running » is worse than none,
  and taken away (`state: null`) when the tab can follow nothing.
- **A hook fails quietly.** `uncaughtException` → exit 0, a two-second timeout on
  the call, work done detached.

## Tests

```
node --test kits/ci-status/
```

No install step. They cover the page's logic on recorded `gh` output (running,
success, failure, not signed in, `gh` absent), the controller's polling and its
single notification, what the view draws, the hook's recognition of `git push`
(and not `git push --help` or `echo git push`), and the manifest against the rules
in [docs/kits.md](../../docs/kits.md) — Hangar's own validator is Rust, so the Node
test restates its rules. `test/fixtures/run-running.json` is the recorded
success run with its statuses turned to « in progress », the one state that cannot
be recorded after the fact.
