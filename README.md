<p align="center">
  <img src="assets/logo.png" alt="" width="104" height="104">
</p>

<h1 align="center">Hangar.AI</h1>

<p align="center">
  A terminal multiplexer for CLI coding agents.<br>
  Built with Tauri 2 — around 30 MB of RAM at rest, not 300.
</p>

Run Claude Code, Codex and friends side by side in one window, resume their
sessions where you left off, and give them a shared task board they can read and
edit themselves.

## What it does

**Workspaces.** A project directory plus a pane arrangement, saved to disk. Reopen a
workspace and every pane comes back with its agent and its previous conversation.

**Panes.** 1, 2, 4 or 8 real pseudo-terminals in a tiling layout. Every split is
independently resizable — dragging one boundary leaves the others alone. Pick the shell
per pane from whatever is installed: PowerShell, cmd, Git Bash, WSL, MSYS2, Nushell.

**Session resume.** CLI agents do not expose their session id, so Hangar.AI derives it:
it snapshots the transcript directory before launching, then watches for the new file
that appears. On the next launch the pane runs `claude --resume <id>` or
`codex resume <id>` automatically. A global claim prevents two panes in the same
directory from latching onto the same transcript.

**Shared task board.** A kanban board per workspace, stored in `.iabench/board.json`.
Agents reach it over MCP: they can list work, claim a task, report progress and file
new tasks for each other.

**Broadcast.** Send the same instruction to every pane at once.

**Discord Rich Presence.** Off until you ask for it. Turned on, your Discord profile
shows the workspace you have open and the agents running in it, with a link back to
this repository.

**Themes.** Thirty-two palettes ported from the most-installed VS Code themes (Dark+,
One Dark, Night Owl, Dracula, Monokai Pro, Tokyo Night, Catppuccin, Nord, Gruvbox,
SynthWave '84, …) that restyle the whole window, not just the terminals. Font, size, line height, cursor and
padding are all live-adjustable.

**Languages.** English and French, following the OS language unless you pick one.
Translations live in `src/i18n.ts`, where English is the reference: the French table is
typed as `Record<Key, string>`, so a missing or misspelled key fails the build rather
than silently falling back at runtime.

## Why the board is not just a JSON file

Eight agents doing read-modify-write on a shared file would all claim the same task.
Every mutation goes through a single lock inside the app, shared by the UI and the
HTTP API, so `claim_task` is genuinely atomic. An agent that loses the race gets a
`409 Conflict` and moves on to different work instead of duplicating effort.

## MCP tools

| Tool | Purpose |
|---|---|
| `board_list_tasks` | See the current state of the work |
| `board_next_task` | Highest-priority free task whose dependencies are done |
| `board_claim_task` | Take ownership; fails if already claimed |
| `board_create_task` | File work for other agents, with priorities and dependencies |
| `board_update_task` | Move between columns, edit, or release |
| `board_comment_task` | Leave notes — the channel agents use to talk to each other |
| `board_delete_task` | Remove a task |

The MCP server is the same executable run with `--mcp`. That means agent configs only
ever reference one absolute path, with no sidecar to bundle or locate.

The API port is ephemeral and its token is regenerated on every launch, so neither is
ever written into an agent config. The app publishes them to `~/.iabench/endpoint.json`
and the MCP process resolves them at startup — configs stay valid across restarts.

## Connecting agents

The MCP panel registers the server in whichever tools it finds. There is no single
standard, so each target is patched in its own format:

| Target | File | Shape |
|---|---|---|
| Claude Code | `.mcp.json` (project) | `mcpServers` |
| Codex | `~/.codex/config.toml` | `[mcp_servers.x]` |
| Gemini CLI | `~/.gemini/settings.json` | `mcpServers` |
| Cursor | `.cursor/mcp.json` | `mcpServers` |
| VS Code | `.vscode/mcp.json` | `servers` |

Patches are non-destructive: existing JSON is merged, Codex TOML is edited with
`toml_edit` to preserve comments and formatting, and a `.iabench-bak` copy is kept on
first write.

For Claude Code the panel also whitelists the server in `~/.claude/settings.json`.
Without that, project-scoped MCP servers must be re-approved on every session unless
the workspace is explicitly trusted.

It can also write an agent playbook into `AGENTS.md` and `CLAUDE.md`, inside a
delimited block so your own content is untouched. That is where agents learn to claim
before working and to report through comments.

## Keyboard

| Shortcut | Action |
|---|---|
| `Ctrl` + `1`…`8` | Focus pane N |
| `Ctrl` + `,` | Toggle settings |
| Double-click a split | Reset that boundary to 50/50 |
| Double-click a workspace | Rename |

## Discord Rich Presence

Off by default — it is the one feature that publishes anything — and turned on from
Settings → Discord, which also previews the card and says whether Discord is answering.

The card names the workspace, counts its panes, lists the agents running in them and
carries the session's elapsed time; both the workspace name and the agent names sit
behind their own toggle, so it can stay generic. The application icon and a button both
link back to this repository.

It speaks to the Discord desktop app over its local IPC — a named pipe on Windows, a
unix socket elsewhere — so nothing travels further than the machine. Discord being
closed is not an error: the connection is retried every fifteen seconds and the
presence appears on its own, without a restart.

The large image is `assets/logo.png`, fetched from this repository rather than uploaded
to Discord — nothing to set up, as long as the file stays where it is on `main`.
The small corner badge is the agent in the focused pane, and it is the one part that
needs art uploaded to the Discord application, under the keys `agent-claude`,
`agent-codex`, `agent-gemini` and `agent-opencode`. Discord drops asset keys it cannot
resolve without complaining, so until they exist the card simply shows no badge. A fork
can point the whole thing at its own Discord application from Settings → Discord →
Application ID.

## Development

Requires Node 20+, pnpm and a Rust toolchain. On Windows you also need the MSVC build
tools; WebView2 ships with Windows 11.

```bash
pnpm install
pnpm tauri dev      # run the app
pnpm tauri build    # produce the installer
```

Check the MCP server without the GUI:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | ./src-tauri/target/debug/hangar-ai --mcp
```

The mark is drawn from one script, and three copies of it are checked in. Redraw them
together, in this order:

```powershell
powershell -File scripts/logo.ps1            # assets/logo.png and .svg, the masters
pnpm tauri icon assets/logo.png              # the OS icon set, src-tauri/icons/
powershell -File scripts/installer-art.ps1   # the installer bitmaps, which embed the icon
```

## Layout

```
src-tauri/src/
  pty.rs           pseudo-terminals, streamed to the frontend
  sessions.rs      Claude/Codex transcript discovery, agent detection
  shells.rs        installed shell discovery
  board.rs         task model and the single-writer store
  server.rs        local HTTP API the agents talk to
  mcp.rs           stdio MCP server (same binary, --mcp)
  install.rs       registers the server across agent tools
  instructions.rs  writes the agent playbook
  store.rs         atomic state persistence
  discord.rs       Rich Presence: one worker thread owning the Discord IPC
src/
  themes.ts        palette catalogue, drives the app's CSS variables
  store.tsx        workspaces, panes, settings
  lib/discord.ts   what the presence says, and when it is republished
  components/
    PaneGrid.tsx     nested split tree with draggable separators
    TerminalPane.tsx xterm.js bound to a PTY
    BoardView.tsx    kanban
```

The frontend owns the state schema; Rust persists it verbatim.

## License

MIT
