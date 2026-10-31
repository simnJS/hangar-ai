# Services

A Hangar kit: a **Services** tab that answers « what is running on this computer, on which port, and whose
is it » without a terminal. Workers each get their own block of 20 ports, so leftovers pile up: a dev server
whose worker is long gone, a container nobody remembers.

## What it does

- **Ports.** Every TCP port something listens on, as a row: the port, the program, its pid, when it started,
  and whether it is reachable from the network or from this computer only. A port in the pool workers draw
  from (`HANGAR_PORT_BASE`, 21000 and up, 20 ports each) says so, and when the program was started in a
  worker's pane it names the worker, or says that its pane is closed: that is a leftover. A filter shows the
  worker ports alone.
- **Containers.** Every Docker container, running or not, with its state, image and published ports, and the
  port block those ports sit in. « Logs » opens the last 200 lines in a log block that opens on its first error.
- **Refresh.** On demand, and every 10 seconds while the tab is shown (not while it is behind another).
- **Ask an agent to stop it.** « Ask an agent to stop this » under a port or a container opens a question:
  you pick the pane, read the brief, and send it. The agent is told to check that the thing is its own before
  stopping it, and to say what it stopped. **The kit itself stops nothing**: no kill, no `docker stop`.
- **States.** Docker not installed, Docker not running, nothing listening, no container, a command not accepted,
  Node or the system's listing tool missing: each says what it is and what to do.
- **A skill** telling agents to open the tab with `kit_tab_open` instead of guessing with `netstat`, and never
  to stop what they did not start.

It is drawn with [Hangar UI](../../docs/kit-ui.md): Hangar's theme, language (English and French) and interface
size, and it holds at the side panel's narrowest.

## What it runs

Only these three, declared in `hangar-kit.json` and shown to you verbatim before you enable the kit. All of
them only read. The tab runs them with your own account; only `{id}` changes, and it must be 12 to 64 lowercase
hexadecimal characters.

| Name          | Command                                  |
| ------------- | ---------------------------------------- |
| `ports`       | `node {kitDir}/tools/ports.js`           |
| `docker-list` | `docker ps -a --format json`             |
| `docker-logs` | `docker logs --tail 200 {id}`            |

`tools/ports.js` is this kit's own script, and is the one thing to read before you accept. It asks the
system for the listening ports and nothing else:

| System  | What it runs                                                                                           |
| ------- | ------------------------------------------------------------------------------------------------------ |
| Windows | one `powershell.exe` call: `Get-NetTCPConnection -State Listen` and `Get-CimInstance Win32_Process` (name, start time) |
| macOS   | `lsof -nP -iTCP -sTCP:LISTEN -F pcn`, `ps -o pid=,lstart=`, and `ps eww -p <pid>` for each program       |
| Linux   | `ss -H -ltnp` (or the same `lsof`), `ps -o pid=,lstart=`, and `/proc/<pid>/environ` for each program    |

**Whose it is.** To name a worker, `ports.js` reads the environment of each listening program on macOS and
Linux (`ps eww`, `/proc/<pid>/environ`), and keeps **only** `HANGAR_PORT_BASE` and `HANGAR_PANE_NAME`, the two
variables Hangar gives a pane. Nothing else of an environment is kept or printed, and a program of another
account is not readable anyway. **Windows does not allow that without reading another program's memory**, which
this kit does not do: there a port in a worker's block is shown as « Port block … » with its range, without
the worker's name, until Hangar tells tabs which block is whose.

The tab also asks Hangar for the workspace's panes (`panes.read`), to tell whether a worker's pane is still
open and to offer them in the question, and sends a message (`panes.send`) only when you press « Send ».
Nothing leaves the machine, and the page loads no external resource.

## Install

You need Hangar 1.9.0 or later and `node` on your PATH. `docker` is optional: without it, the containers
part says so and the ports still show.

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/simnJS/hangar-ai/tree/main/kits/services`
   (or link a local copy of this folder).
2. Enable it on a workspace, and read what you accept: the three commands and the two pane permissions.
3. Restart the workspace's Claude Code panes so they load the skill.

## How it is built

```
services/
├── hangar-kit.json            the kit: id, hangar >=1.9.0, the three commands, the tab
├── icon.svg
├── .claude-plugin/plugin.json the Claude Code plugin (name = the kit's id): a skill, nothing that runs by itself
├── skills/services/SKILL.md
├── tools/
│   ├── ports.js               the `ports` command: runs the system's tool, prints one line of JSON
│   └── parse.js               pure readers of PowerShell's JSON, ss, lsof, ps and an environment
├── tabs/services/
│   ├── index.html             loads Hangar UI (/_hangar/kit.css, kit.js, kit-ui.js), then the files below
│   ├── logic.js               pure: what the tools said → rows, whose a port is, the brief; strings en and fr
│   ├── controller.js          reads, refreshes every 10 s while shown, logs, the question; takes window.hangar
│   ├── view.js                draws the state with createElement + textContent, never markup
│   ├── app.js                 wires them in the page
│   └── style.css              only what the library does not have: no colour, no measure of its own
├── test/                      node --test, no dependency; fixtures/ are the tools' output, scrubbed
└── package.json, run-tests.js let `node --test kits/services/` find the tests on any Node
```

- **A redraw changes only what changed.** The page is redrawn every 10 seconds: each part is rebuilt only
  when what it shows changed, so an open question, a selection or a log's scroll is never lost.
- **Declared commands, not a shell.** Each is an argv. `{id}` is the only placeholder, with a strict rule,
  and Hangar refuses a value starting with `-`.
- **The kit cannot stop anything.** The commands are reads, `ports.js` has no way to kill, and a test fails if
  the manifest, the script or the page gains one.

## Tests

```
node --test kits/services/
```

No install step. They cover each system's output on recorded text (`test/fixtures/README.md` says which
is recorded and which is written from the tool's documented format), the script with a stand-in for the
system so no real process is asked for anything, the page's logic, the controller on a fake Hangar (refresh
every 10 s, hidden tabs, denied, Docker off, the question), what the view draws in each state in both
languages, and the manifest against the rules in [docs/kits.md](../../docs/kits.md), whose validator is Rust.
