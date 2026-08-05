<p align="center">
  <img src="assets/logo.png" alt="" width="96" height="96">
</p>

<h1 align="center">Hangar.AI</h1>

<p align="center">Run Claude Code, Codex and friends side by side, in one window.</p>

<p align="center">
  <a href="https://github.com/simnJS/hangar-ai/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/simnJS/hangar-ai?style=flat-square&color=7aa2f7"></a>
  <img alt="Windows and macOS" src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS-2f3549?style=flat-square">
  <a href="LICENSE"><img alt="MIT" src="https://img.shields.io/github/license/simnJS/hangar-ai?style=flat-square&color=2f3549"></a>
</p>

## Download

**[Get the latest release](https://github.com/simnJS/hangar-ai/releases/latest)**, under
**Assets**:

- **Windows** — `.exe` or `.msi`, 64-bit. WebView2 is required and ships with Windows 11.
- **macOS** — `.dmg`, one universal build for Apple Silicon and Intel. It is not signed by
  an Apple developer account, so the first launch is right-click → **Open** rather than a
  double-click; macOS then remembers it.

The app updates itself from there, so this is a one-time download.

## What it does

- **Tiling panes** — 1, 2, 4 or 8 real terminals. Every split resizes on its own, and each
  pane picks its shell from whatever is installed: PowerShell, cmd, Git Bash, WSL, MSYS2 and
  Nushell on Windows; zsh, bash, fish and Nushell on macOS, opened as login shells so a pane
  starts with the PATH your terminal has.
- **Workspaces** — a project directory plus its pane layout, saved. Reopen one and every
  pane comes back with its agent.
- **Session resume** — agents relaunch on the conversation they were in, with no session
  ids for you to track.
- **Shared task board** — a kanban board per workspace that the agents themselves read and
  write, over MCP.
- **Broadcast** — one instruction, sent to every pane at once.
- **32 themes** — ported from the most-installed VS Code palettes, restyling the whole
  window rather than just the terminals. Font, size, line height, cursor and padding adjust
  live.
- **Rebindable shortcuts**, and an interface in English or French.
- **Discord Rich Presence** — off unless you turn it on.

## The task board

Every workspace gets a board, and agents work it over MCP:

| Tool | What it does |
|---|---|
| `board_list_tasks` | Read the current state of the work |
| `board_next_task` | Highest-priority free task whose dependencies are done |
| `board_claim_task` | Take ownership — fails if someone got there first |
| `board_create_task` | File work for another agent, with priorities and dependencies |
| `board_update_task` | Move between columns, edit, or release |
| `board_comment_task` | Leave notes, which is how agents talk to each other |
| `board_delete_task` | Remove a task |

Claims are atomic: every change goes through a single lock, so two agents racing for the
same task cannot both win it.

## Connecting agents

Settings → MCP registers the server in whichever tools it finds, each in its own format:

| Tool | File |
|---|---|
| Claude Code | `.mcp.json` |
| Codex | `~/.codex/config.toml` |
| Gemini CLI | `~/.gemini/settings.json` |
| Cursor | `.cursor/mcp.json` |
| VS Code | `.vscode/mcp.json` |

Existing configuration is merged rather than overwritten, and a backup is kept on first
write. The API port and its token are resolved at launch and never written into those
files, so the configs stay valid across restarts.

The same panel can drop an agent playbook into `AGENTS.md` and `CLAUDE.md`, inside a
delimited block that leaves the rest of the file alone.

## Keyboard

`Ctrl` on Windows, `Cmd` on macOS:

| Shortcut | Action |
|---|---|
| `Ctrl`/`Cmd` + `1`…`8` | Focus pane N |
| `Ctrl`/`Cmd` + `,` | Settings |
| Double-click a split | Reset that boundary to 50/50 |
| Double-click a workspace | Rename |

Everything else is rebindable from Settings → Shortcuts. The defaults differ where the two
platforms do: a Mac copies with `Cmd`+`C` rather than `Ctrl`+`Shift`+`C`, splits with
`Cmd`+`D`, and moves between panes with `Ctrl`+`Option`+arrow — leaving `Option`+arrow to
the terminal, which uses it to move by word.

## Development

Node 20+, pnpm and a Rust toolchain; on Windows, the MSVC build tools, and on macOS the
Xcode command line tools (`xcode-select --install`).

```bash
pnpm install
pnpm tauri dev      # run it
pnpm tauri build    # build the installers
```

The MCP server is the same binary, run with `--mcp`:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | ./src-tauri/target/debug/hangar-ai --mcp
```

English is the reference for translations in `src/i18n.ts`: the French table is typed
against it, so a missing key fails the build instead of falling back at runtime.

Redrawing the logo regenerates three checked-in copies, in this order. The scripts are
PowerShell, so this one is a Windows job — the artwork they produce is checked in, and a
macOS clone never has to run them:

```powershell
powershell -File scripts/logo.ps1            # the masters, assets/logo.png and .svg
pnpm tauri icon assets/logo.png              # the OS icon set
powershell -File scripts/installer-art.ps1   # the installer bitmaps
```

A fork that wants the Rich Presence badges needs art uploaded to its own Discord
application under the keys `agent-claude`, `agent-codex`, `agent-gemini` and
`agent-opencode`, then its application id set in Settings → Discord. Until then the card
simply shows no badge.

## License

MIT
