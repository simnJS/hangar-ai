# Changelog

What changed in each Hangar.AI release, newest first. The format loosely follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[Semantic Versioning](https://semver.org/); before 1.0.0, a minor version could
change behaviour as well as add to it. The 0.x history is grouped by minor version.

Every release, with its installers, is on the
[releases page](https://github.com/simnJS/hangar-ai/releases).

## [1.0.1] - 2026-10-27

- **Put a workspace to sleep by hand.** Right-click a workspace in the navigator, or run
  **Put to sleep** from the command palette. It is the same sleep Hangar applies on its
  own, without the idle delay, and with the same guards: the entry is greyed out, with
  the reason, while an agent works or waits for you, a shell runs a process, an agent
  could not resume its session, or a worker or a delivery is running. The workspace on
  screen hands over to the one you used last.
- **A delivery waiting for you can be redone.** When a worker fixes its branch after its
  delivery passed and waits to be merged, `delivery_submit` withdraws the waiting one and
  runs the whole chain again. **Merge** never lands a version the worker changed since it
  was tested, and the reason a merge was refused stays on screen.
- **No personal path on the launch line.** Orchestrator and worker panes read their MCP
  configuration from `HANGAR_MCP_CONFIG` instead of typing its full path, so a screen
  share or a recording no longer shows your user folder.

## [1.0.0] - 2026-10-27

The first stable release. It adds orchestrator mode on top of what the 0.12 to 0.15
releases brought the day before: the Pro interface, the 3D preview, usage
statistics, workspace sleep and per-workspace MCP servers.

- **Orchestrator mode.** Pin one Claude Code pane per workspace as its orchestrator
  (Opus by default). It plans on the board, with owned files, labels and dependencies
  per task, and delegates the work instead of writing code.
- **Workers.** `worker_spawn` claims a task, checks out a git worktree on a branch named
  after it, and opens a Claude Code worker there (Sonnet by default) with a brief, its
  own port range (`HANGAR_PORT_BASE`, `HANGAR_PORT_COUNT`) and its model.
  `worker_list` and `worker_stop` complete the set. Two tasks that own the same files
  cannot be in progress at once.
- **Delivery queue.** `delivery_submit` merges a worker's branch onto the target's
  current head in a temporary worktree, runs the workspace's check command if one is
  set, has a fresh `claude -p` session with read-only tools review it, then lands it
  locally. Hangar never pushes. A failure goes back to the worker with the conflicting files
  or the failing line; repeated failures are handed to you.
- **Event-driven orchestrator.** Board moves, comments, workers that wait for you, stop
  or exit, and delivery outcomes reach the orchestrator as batched messages, so it
  never polls.
  Orchestrators and workers get their role and the workspace's state when a session
  starts, resumes or is compacted.
- **"Needs you" inbox.** Agents waiting for an answer or a permission, tasks handed back
  with a question, and deliveries waiting to land, across every workspace.
- Per-workspace orchestration settings: maximum workers, attempts, automatic landing (on
  by default),
  check command, models, target branch and permission mode.
- A prompt queued by an agent or by Hangar is labelled with its sender, and the
  inspector shows who queued it.
- A workspace stays awake while its workers run or one of its deliveries lands.
- PowerShell panes start without the PowerShell banner.
- The workspace board (`.hangar/`) is kept out of `git status` through the repository's
  local `.git/info/exclude`.

## [0.15] - 2026-10-26

- Each workspace has its own 3D preview; a model shown in a workspace you are not
  looking at waits there, and the agent is told whether you can see it.
- Sidebar folders read as headings, and each workspace row sums up its agents.
- Changes agents make to the layout show as news rather than as a warning.
- The workspace path left the toolbar.
- A Dockerfile to self-host the website.

## [0.14] - 2026-10-26

- **Usage statistics**: a view built from Claude Code and Codex transcripts on disk,
  with tokens, API-equivalent cost, rankings by project, agent and model, and an
  activity heatmap. Prices are API list prices, not a plan's price.
- **Workspace sleep**: an idle workspace has its processes ended after a delay you set,
  and its agents resume their sessions when it is opened again. Agents acting on a
  sleeping workspace over MCP wake it.
- **MCP servers per workspace**: a panel lists the servers Claude Code would load in the
  workspace and lets you pick which ones its Claude Code panes start with.
- hangar-bridge no longer holds an agent on a file another pane just edited: the edit
  goes through and the model is told who edited it before.
- Closing a pane can end every process its shell started.

## [0.13] - 2026-10-26

- **3D preview over MCP**: `model_show`, `model_snapshot` and `model_close` let an agent
  open a model in a panel beside the terminals and render it to a PNG to check its work.
- Reads glTF/GLB, OBJ with MTL, STL, PLY, FBX and USDZ, only from the workspace's
  folders and its git worktrees, and reloads when the file changes.
- Captures of the preview can be saved where you choose.

## [0.12] - 2026-10-26

- **The Pro interface**: a three-column window with a navigator, the work area and an
  inspector, on a new default theme, Graphite (33 themes in all).
- The navigator shows workspaces as a tree with their agents, plan meters and a filter
  for the ones waiting on you.
- The inspector shows the focused pane's session, context and cost, board task, prompt
  queue, changed files and recent activity.
- A command palette (`Mod+Shift+P`) finds panes, commands and workspaces, in English or
  French.
- Keyboard access throughout: F6 walks the regions, pane boundaries resize from the
  keyboard, and focus returns to where a dialog was opened from.
- Contrast held to 4.5:1 for secondary text, light themes included; one icon set
  across the window.
- Side panels without room become drawers; a layout preset asks before closing busy
  panes.

## [0.11] - 2026-10-07

0.11.0 (2026-10-06), 0.11.1 (2026-10-07).

- Every pane has a first name unique across all workspaces, and Claude Code sessions
  run as "Ava (Shop)" so neighbouring agents know which project they work on.
- A pane resumes only a conversation that was actually written, and starts fresh
  otherwise, instead of failing with "No conversation found".
- A Tidy button lays the open panes out as an even grid without restarting anything.

## [0.10] - 2026-10-05

- **Pane tools over MCP**: `workspace_list`, `pane_list`, `pane_create`, `pane_close`,
  `pane_restart`, `pane_send`, `pane_read`, `pane_focus`, which a setting can turn off.
- A workspace can move to a window of its own, on another screen, with its agents
  still running; `workspace_window` does the same over MCP.
- Agents sign the board with their pane's name.
- `HANGAR_ENDPOINT_FILE` lets a development build run beside an installed Hangar.

## [0.9] - 2026-10-03

- **hangar-bridge**, a Claude Code mod loaded into Hangar's panes: it reports turns,
  permission dialogs, context, plan usage and edits, and submits prompts held until
  the agent is free.
- Agent state (working, waiting for you, done) in each pane header and per workspace
  in the sidebar; plan usage at the bottom of the sidebar.
- Reset every agent of a workspace to a new conversation.
- Zoom a pane, search a terminal, drop a file or image onto a pane to paste its path.
- WebGL rendering in the terminals.
- Board cards with assignee, blockers and filters; the board as a side panel; a diff
  view of what changed in a pane's folder or worktree.
- The Groq key moved to the system keychain.

## [0.8] - 2026-10-03

- A gauge of how full each agent's context window is.
- Saved commands, run from any pane.
- Folders to organise workspaces in the sidebar.

## [0.7] - 2026-08-11

- **Git worktrees**: create, open and remove linked worktrees from a panel.
- **Global memory over MCP**: `memory_*` tools give every agent a store of facts shared
  across workspaces, with a view to browse it.
- Clicking a desktop notification brings Hangar back on the right pane (Windows).
- The website and its browser demo joined the repository.

## [0.6] - 2026-08-10

0.6.0 and 0.6.1, both 2026-08-10.

- Dictation transcribes in seconds rather than minutes, on a smaller local Whisper
  checkpoint.
- whisper.cpp is built for AVX2 instead of the build machine's CPU, so releases run on
  CPUs without AVX-512.
- The custom Windows installer's buttons work.
- MCP registrations pointing at a missing executable are reported as stale and can be
  repaired; internal identifiers moved from `iabench` to `hangar` with fallbacks.

## [0.5] - 2026-08-09

0.5.0 (2026-08-08), 0.5.1 and 0.5.2 (2026-08-09).

- **Dictation**: hold a key, speak, and the text is typed into the focused pane. It
  runs locally on whisper.cpp, with Groq's hosted Whisper as an optional fallback.
- Dictation has its own section in the shortcut settings.
- One Windows installer, the per-user `.exe`, which needs no administrator rights.

## [0.4] - 2026-08-07

0.4.0 and 0.4.1, both 2026-08-07.

- macOS (universal `.dmg`) and Linux (`.AppImage`, `.deb`, `.rpm`) builds.
- Open a VS Code `.code-workspace` file; its extra folders are passed to Claude Code.
- A launch command of your own per agent, for wrappers and aliases.
- A new pane starts with the agent of the pane it was split from.
- Closing a pane no longer freezes the window.
- Shift+Enter inserts a newline in agent panes and still submits in a plain shell.

## [0.3] - 2026-08-05

- Renamed **Hangar.AI**, after shipping as the misspelt Hangar.IA in 0.2.0; updating
  replaces the old install and keeps its data.
- A logo, for the app icon, the installer and the sidebar.
- Twenty more themes ported from popular VS Code palettes, 32 in total.
- Settings rebuilt around a searchable list, with rebindable shortcuts.
- Discord Rich Presence, off by default.

## [0.2] - 2026-08-05

- Renamed from IaBench to Hangar.IA.
- The pane grid rewritten so a layout change no longer restarts every terminal: split,
  close, drag to move or resize, up to 16 panes, each with a first name.
- A desktop notification when a pane falls quiet after real work.
- Switching workspace no longer kills their shells.
- A custom Windows installer.

## [0.1] - 2026-08-05

0.1.0 and 0.1.1, released as IaBench, Windows only.

- Workspaces with saved pane layouts and a per-pane agent or shell.
- Real terminals in a resizable split, with 12 themes.
- Session resume for Claude Code and Codex.
- A shared task board per workspace, served to agents over MCP by the same binary run
  with `--mcp`, with atomic claims.
- One-click MCP registration for Claude Code, Codex, Gemini CLI, Cursor and VS Code.
- Automatic updates from signed GitHub releases.
- An interface in English and French (0.1.1).

[1.0.1]: https://github.com/simnJS/hangar-ai/releases/tag/v1.0.1
[1.0.0]: https://github.com/simnJS/hangar-ai/releases/tag/v1.0.0
[0.15]: https://github.com/simnJS/hangar-ai/releases/tag/v0.15.0
[0.14]: https://github.com/simnJS/hangar-ai/releases/tag/v0.14.0
[0.13]: https://github.com/simnJS/hangar-ai/releases/tag/v0.13.0
[0.12]: https://github.com/simnJS/hangar-ai/releases/tag/v0.12.0
[0.11]: https://github.com/simnJS/hangar-ai/releases/tag/v0.11.1
[0.10]: https://github.com/simnJS/hangar-ai/releases/tag/v0.10.0
[0.9]: https://github.com/simnJS/hangar-ai/releases/tag/v0.9.0
[0.8]: https://github.com/simnJS/hangar-ai/releases/tag/v0.8.0
[0.7]: https://github.com/simnJS/hangar-ai/releases/tag/v0.7.0
[0.6]: https://github.com/simnJS/hangar-ai/releases/tag/v0.6.1
[0.5]: https://github.com/simnJS/hangar-ai/releases/tag/v0.5.2
[0.4]: https://github.com/simnJS/hangar-ai/releases/tag/v0.4.1
[0.3]: https://github.com/simnJS/hangar-ai/releases/tag/v0.3.0
[0.2]: https://github.com/simnJS/hangar-ai/releases/tag/v0.2.0
[0.1]: https://github.com/simnJS/hangar-ai/releases/tag/v0.1.1
