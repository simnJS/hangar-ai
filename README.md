# IaBench

A terminal multiplexer for CLI coding agents. Run Claude Code, Codex and friends
side by side in one window, resume their sessions where you left off, and give them
a shared task board they can read and edit themselves.

Built with Tauri 2 — around 30 MB of RAM at rest, not 300.

## What it does

**Workspaces.** A project directory plus a pane arrangement, saved to disk. Reopen a
workspace and every pane comes back with its agent and its previous conversation.

**Panes.** 1, 2, 4 or 8 real pseudo-terminals in a tiling layout. Every split is
independently resizable — dragging one boundary leaves the others alone. Pick the shell
per pane from whatever is installed: PowerShell, cmd, Git Bash, WSL, MSYS2, Nushell.

**Session resume.** CLI agents do not expose their session id, so IaBench derives it:
it snapshots the transcript directory before launching, then watches for the new file
that appears. On the next launch the pane runs `claude --resume <id>` or
`codex resume <id>` automatically. A global claim prevents two panes in the same
directory from latching onto the same transcript.

**Shared task board.** A kanban board per workspace, stored in `.iabench/board.json`.
Agents reach it over MCP: they can list work, claim a task, report progress and file
new tasks for each other.

**Broadcast.** Send the same instruction to every pane at once.

**Themes.** Twelve palettes (Tokyo Night, Catppuccin, Dracula, Nord, Gruvbox, …) that
restyle the whole window, not just the terminals. Font, size, line height, cursor and
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
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | ./src-tauri/target/debug/iabench --mcp
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
src/
  themes.ts        palette catalogue, drives the app's CSS variables
  store.tsx        workspaces, panes, settings
  components/
    PaneGrid.tsx     nested split tree with draggable separators
    TerminalPane.tsx xterm.js bound to a PTY
    BoardView.tsx    kanban
```

The frontend owns the state schema; Rust persists it verbatim.

## License

MIT
