---
name: ci-status
description: After pushing to GitHub, show the CI run in Hangar's CI tab and read its outcome through gh instead of waiting. Use right after a git push, or when asked how the CI is doing.
---

# Following the CI after a push

A hook of this kit looks for the GitHub Actions run a `git push` started and
opens the **CI** tab for it by itself. It works in the background and may take
half a minute to find the run.

## After you push

1. Give the hook a moment. If you need the tab in front now, or the hook did
   not open one (no run appeared, `gh` was absent, Hangar was not asked), open
   it yourself with the `hangar` MCP tool `kit_tab_open`:

   ```json
   { "kit": "ci-status", "tab": "ci", "params": { "id": "<run id>" } }
   ```

   Find the run id with `gh run list --commit "$(git rev-parse HEAD)" --json databaseId,status,workflowName`.
   `id` is digits only. `kit_tab_open` on a tab already open for that run just
   brings it forward.
2. If `kit_tab_open` is refused (the kit is not enabled on this workspace, or
   waits for the person to accept something), say so in one line and go on:
   the tab is a convenience, not a step you depend on.

## Reading the outcome

Do not sleep or loop to wait for a run. The tab polls for the person, and
raises a notification when the run ends. You read the outcome with `gh`:

- `gh run view <id> --json status,conclusion,jobs` — `status` is `completed`
  once it is over, then `conclusion` says `success` or `failure`.
- `gh run view <id> --log-failed` — the log of the steps that failed.

If the run is still going when you need its result, carry on with other work
and look again later; if you have nothing else to do, tell the person the run is
in progress and where to see it (the CI tab), rather than waiting on it.
