# Changelog

What changed in each Hangar.AI release, newest first. The format loosely follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[Semantic Versioning](https://semver.org/); before 1.0.0, a minor version could
change behaviour as well as add to it. The 0.x history is grouped by minor version.

Every release, with its installers, is on the
[releases page](https://github.com/simnJS/hangar-ai/releases).

## [1.8.0] - 2026-10-31

- **Kits can do more, and show more before you accept them.**
  - A kit's tabs can keep their own data, in a folder of the kit outside your projects.
    A kit can never reach another kit's folder.
  - A kit's tab can talk to a local server on a port it declares, such as a game editor's
    bridge. You accept each port when you enable the kit. Hangar makes the call: the
    tab's page still has no network of its own.
  - Before you enable a kit, you now see the hooks of its Claude Code plugin, exactly as
    they will run, since Claude Code runs them by itself. A kit from GitHub whose hooks
    change in an update stays out of your agents until you accept it again. A kit linked
    from a folder, which you are writing yourself, is not asked again.
  - The details for kit makers are in [docs/kits.md](docs/kits.md).

## [1.7.0] - 2026-10-31

- **Claude accounts.** Use several Claude subscriptions from one Hangar — say a work Max
  and a personal one — and give each workspace its own.
  - **Settings → Claude accounts** adds an account: name it, and a pane opens for you to
    log in through your browser. Rename, remove, and pick the default account there.
  - Each workspace can have its own account, from its right-click menu in the sidebar
    or when you create it. A workspace that is not on the default shows a small badge
    with the account's name.
  - The toolbar's account menu shows the account of the workspace on screen and each
    account's 5-hour and weekly usage. One click moves this workspace to another
    account, or every workspace that follows the default. Hangar then offers to
    restart the workspace's agents now, with their conversations kept, or at their
    next launch.
  - A single pane can run on another account too, from its menu.
  - Every account has the same setup: conversations, settings, skills, plugins and MCP
    servers come from your main Claude Code configuration. Only the login differs.
    Change the setup from a pane on the main account: the other accounts copy it again
    at each launch.
  - Hangar never switches account by itself, not even when a limit is reached.
  - On macOS, each account keeps its own login in the Keychain.

## [1.6.0] - 2026-10-31

- **Kits.** A kit brings a domain — a game engine, a framework, a studio's own tools —
  into the workspaces you link it to, without Hangar knowing anything about that
  domain. A kit can bring a Claude Code plugin (skills, commands, agents and MCP
  servers), tabs of its own, and setup commands.
  - **Settings → Kits** adds a kit from a folder, linked and read live while you make
    it, or from a GitHub link (a whole repository or one of its folders), checks for
    updates, updates and removes kits.
  - Enable a kit on a workspace from its right-click menu in the sidebar (**Kits…**) or
    when you create it. Before anything is enabled you see what the kit brings, the
    permissions its tabs ask for in plain words, and its setup commands exactly as
    they will run.
  - Setup commands run only when you click **Run**, in a visible pane, once per
    machine. A command that changes in an update is asked again.
  - Every Claude Code pane of the workspace, workers included, gets the kit's skills
    and MCP servers, also when the workspace picks its own MCP servers. Nothing is
    written into your project, and other workspaces are untouched.
  - A kit's tabs open in the side panel, from the toolbar's **Kits** menu or the command
    palette. They run isolated, with no network, and talk to Hangar only through a
    small API, with the permissions you accepted: see the workspace and its panes,
    write to its Claude Code agents, read its text files, open a file in the preview,
    raise a notification.
  - How to make a kit: [docs/kits.md](docs/kits.md).

## [1.5.0] - 2026-10-29

- **See the app an agent is building, on the phone.** In Hangar Mobile, a project whose
  worker runs a dev server shows **See the app**: it opens full screen, live, with hot
  reload, and a button to open it in Safari. Workers' servers are found by themselves
  on their own ports. Under **Settings → Mobile** you can add ports of your own for
  each workspace, such as your own `pnpm dev`.
  - Each app gets an HTTPS port of its own on your Tailscale address (8443 to 8448).
    Only your paired phones get through, and the dev server never sees the phone's
    session.
  - The first time an app is opened, Hangar adds that port to Tailscale Serve, which
    keeps it after a restart.
- **Merge from the phone.** A delivery ready to merge opens on the phone with the files
  it changes, their diff, the check's result and the review's verdict. **Merge** lands
  it, or says what stands in the way. **Send back** gives the worker an instruction to
  fix before it delivers again.

## [1.4.0] - 2026-10-29

- **Stats on the phone.** Hangar Mobile has a **Stats** tab with your agents' usage. Your
  plan's limits come first, when Claude Code reports them: how much of the 5-hour and
  weekly windows is used, and when each resets. Below them are the figures of the
  desktop's statistics page, computed the same way: a period (7, 30 or 90 days, or all),
  tokens or the API-equivalent cost, a chart per day, and rankings by project, agent and
  model.

## [1.3.0] - 2026-10-29

- **Preview any file.** The 3D panel becomes **Preview**: one tab per open file, whatever
  its kind. Agents show you a render, a screenshot, a recording, a report or a page with
  the new `preview_show` and `preview_close` MCP tools.
  - Images zoom and pan, and video and audio play with loop and speed.
  - Code and text get syntax highlighting, Markdown is rendered with a source view, and
    PDFs open in the panel or in your default app.
  - HTML pages run in an isolated sandbox that reaches neither Hangar nor your other files.
    Their scripts and styles must be inline.
  - 3D models work as before.
- **Shares open in place on the phone.** In Hangar Mobile, a file an agent shares opens
  right in **Shared**, whatever its kind: video and audio play, text and code are
  highlighted, Markdown renders, a PDF opens in the phone's viewer, and an HTML page
  renders isolated. Images open in a zoomable view.
- **Wake a sleeping project from the phone.** An asleep project has a **Wake up** button.
  A message, an answer or a permission sent to one of its agents now waits until that
  agent's session has resumed, instead of being lost while it started.
- **The phone layout holds on iPhone.** The app no longer shifts down, under the Dynamic
  Island, and pushes its bottom bar and composer off screen after the keyboard or a
  return to the app.
- Agents' instructions now mention `preview_show` and `phone_share`.

## [1.2.0] - 2026-10-29

- **A real chat on the phone.** In Hangar Mobile, a Claude Code pane (and the project
  manager) now opens as a conversation instead of a copy of its terminal: your messages,
  Claude's answers with their formatting, and between them one line per action ("Edited
  src/a.ts", "Ran pnpm test") that opens its detail. Older messages load as you scroll up.
  Codex, Gemini CLI and shells keep the terminal view, and a **Terminal** button still
  shows any pane's raw screen.
- **Claude's questions as buttons.** When Claude asks you something, the phone shows each
  question with its options as large buttons, several choices when it allows them,
  "Other" with a text field, and Next / Send across several questions. Your answer
  reaches the session on the PC as if you had typed it there.
- **Permissions say what they are for.** A permission shows the tool and what it will do
  (the command, the file) with Allow and Deny.

## [1.1.0] - 2026-10-28

- **Hangar Mobile.** Follow and steer your projects from your phone while Hangar keeps
  running on your PC. It is a web app that Hangar itself serves and that you add to the
  phone's Home Screen: no app store, no account, no server of ours in between. Set it up
  in about a minute under **Settings → Mobile** (see the README).
  - **For you** lists what waits for you in every project: an agent asking for a
    permission, a question for you, a delivery ready to land.
  - Read any pane's screen as it updates, answer an agent, allow or deny a permission,
    press `Esc` or `Enter`, attach a photo or a file, and talk to the project manager.
  - Agents can share images and files with your phone through the new `phone_share`
    MCP tool; images show in **Shares**.
  - Push notifications reach the phone even with the app closed, sent by the PC itself.
  - **Since your last visit** shows what happened while you were away, kept across
    restarts for 30 days. For a few days away, two options: keep the PC from sleeping
    while Hangar Mobile is on, and start Hangar when you log in.
  - Security: it listens on `127.0.0.1` only and is reached through Tailscale Serve over
    HTTPS; each phone pairs with a one-time code and can be revoked. Off by default.

## [1.0.3] - 2026-10-28

- **Project manager.** One agent to talk to instead of every project's agents. Open it
  from the top of the sidebar or the command palette: Hangar creates an HQ workspace
  with its own folder and one Claude Code agent (Opus by default) that follows every
  workspace, with or without an orchestrator.
  - Ask it where your projects stand: agents, boards, what waits for you, deliveries
    ready to land, each plan's status.
  - Ask it to pass an instruction on: it is queued for that project's orchestrator,
    and a sleeping workspace wakes up.
  - It speaks up by itself only for what waits for you: an agent waiting for an answer
    or a permission, a task marked needs-you, a delivery ready to land, a board whose
    tasks are all done.
  - It keeps a tracker of what you want from each project, handed back after every
    compaction. It never writes code, never acts on its own, and the HQ never sleeps.
- **New MCP tools for it:** `project_list`, `project_status` and `project_send`, which
  only the project manager can call.

## [1.0.2] - 2026-10-28

- **Orchestrator mode without git.** A workspace that is not a git repository, such as
  a UEFN island, can now run workers. They start in the workspace's own folder, side by
  side, with no worktree and no branch. When a worker delivers, nothing is merged,
  checked or reviewed: its task is done and its pane closes. Since nothing keeps two
  workers' changes apart there, a task that does not list the files it owns never runs
  beside another one. The orchestrator no longer asks you to make the folder a
  repository.
- **A pane that just started is no longer shown as working.** A Claude Code pane stayed
  "working" until its first turn, in the navigator, in `pane_list` and for sleep, so a
  workspace of fresh sessions never went to sleep. It now shows idle once its session
  is ready.

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
