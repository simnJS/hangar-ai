# Kits

A kit is a package a person links to a workspace to bring a domain — Roblox,
Unreal, a studio's own tools — into Hangar, without Hangar knowing anything
about that domain. A kit can bring:

- a **Claude Code plugin**: skills, commands, agents and MCP servers, loaded by
  every Claude Code pane of the workspaces the kit is enabled on;
- **tabs**: isolated pages of its own, talking to Hangar through a small,
  declared API, with a [data folder](#a-kits-data-folder) of the kit's and, when
  declared, a local server to talk to;
- **setup commands**: shown to the person verbatim, run in a visible pane once
  they approve, once per machine.

Its plugin may also carry [hooks](#hooks), which Claude Code runs by itself: the
person sees them before enabling the kit.

Kits come from a local folder (linked, read live) or from a GitHub link
(cloned). There is no catalog.

This page is the contract. The code lives in `src-tauri/src/kits.rs` and
`src-tauri/src/kits/`, with the typed wrappers in `src/lib/kits.ts`; a tab's
page is served by `src-tauri/src/kit_scheme.rs` with the SDK in
`src-tauri/kit-sdk/kit.js`, and framed and answered by
`src/components/preview/KitTabView.tsx`, `src/lib/kitTabs.ts` and
`src/lib/kitApi.ts`.

## A minimal kit

```
hello-kit/
├── hangar-kit.json
├── .claude-plugin/
│   └── plugin.json
├── .mcp.json
├── skills/
│   └── hello/
│       └── SKILL.md
├── server/
│   └── index.js
└── tabs/
    └── status/
        └── index.html
```

`hangar-kit.json`:

```json
{
  "schema": 1,
  "id": "hello-kit",
  "name": "Hello kit",
  "version": "0.1.0",
  "description": "A skill, an MCP server and a tab, to see what a kit does.",
  "author": "You",
  "tabs": [
    {
      "id": "status",
      "title": "Status",
      "entry": "tabs/status/index.html",
      "permissions": ["panes.read", "notify"]
    }
  ],
  "setup": [
    {
      "id": "server-deps",
      "title": "Install the MCP server's dependencies",
      "description": "Runs npm in the kit's server folder.",
      "run": {
        "windows": "npm install --prefix server",
        "macos": "npm install --prefix server",
        "linux": "npm install --prefix server"
      }
    }
  ]
}
```

`.claude-plugin/plugin.json` — an ordinary Claude Code plugin. Name it like the
kit: the name prefixes its skills (`/hello-kit:hello`) and its MCP tools.

```json
{ "name": "hello-kit", "version": "0.1.0", "description": "Hello from a kit" }
```

`.mcp.json` — the plugin's MCP servers. `${CLAUDE_PLUGIN_ROOT}` is the plugin's
folder:

```json
{
  "mcpServers": {
    "hello": { "command": "node", "args": ["${CLAUDE_PLUGIN_ROOT}/server/index.js"] }
  }
}
```

`tabs/status/index.html`:

```html
<!doctype html>
<meta charset="utf-8" />
<script src="/_hangar/kit.js"></script>
<pre id="out">…</pre>
<script>
  async function show() {
    const panes = await window.hangar.call("panes.list");
    document.getElementById("out").textContent = JSON.stringify(panes, null, 2);
  }
  window.hangar.on("panes.changed", show);
  show();
</script>
```

The plugin is found by itself here — the root holds `.claude-plugin/` — so the
manifest does not need a `plugin` field.

## The manifest: `hangar-kit.json`

At the kit's root. Schema 1:

| Field         | Type              | Required | Rules                                                                                     |
| ------------- | ----------------- | -------- | ----------------------------------------------------------------------------------------- |
| `schema`      | number            | yes      | `1`. A higher number reads as "needs a newer Hangar".                                     |
| `id`          | string            | yes      | `[a-z0-9-]`, 2 to 40 characters. Unique on a machine: two kits cannot share it.           |
| `name`        | string            | yes      | Not blank.                                                                                |
| `version`     | string            | yes      | Semver: `1.2.0`, `0.3.0-beta.1`. What update checks compare.                              |
| `description` | string            | yes      |                                                                                           |
| `author`      | string            | no       |                                                                                           |
| `homepage`    | string            | no       | An `http://` or `https://` link.                                                          |
| `icon`        | string            | no       | Relative path to an existing `.svg` or `.png` file.                                       |
| `hangar`      | string            | no       | A semver requirement on the Hangar version, like `>=1.6.0`. Unmet, the kit is unusable.   |
| `plugin`      | string            | no       | Relative folder holding `.claude-plugin/plugin.json`. Default below.                      |
| `tabs`        | array of tabs     | no       | See below. Ids unique.                                                                    |
| `setup`       | array of steps    | no       | See below. Ids unique.                                                                    |

`plugin` defaults to `"."` (the kit's root) when `<root>/.claude-plugin` exists,
and to no plugin otherwise. A `plugin` folder without
`.claude-plugin/plugin.json` is an error.

A **tab**:

| Field         | Type     | Required | Rules                                                                      |
| ------------- | -------- | -------- | -------------------------------------------------------------------------- |
| `id`          | string   | yes      | `[a-z0-9-]`, 1 to 40 characters.                                           |
| `title`       | string   | yes      | Not blank.                                                                 |
| `entry`       | string   | yes      | Relative path to an existing `.html` file.                                 |
| `icon`        | string   | no       | Relative path to an existing `.svg` or `.png` file.                        |
| `permissions` | string[] | yes      | Among the [permissions](#tab-permissions). `[]` asks for nothing.          |

A **setup step**:

| Field         | Type   | Required | Rules                                                                                         |
| ------------- | ------ | -------- | --------------------------------------------------------------------------------------------- |
| `id`          | string | yes      | `[a-z0-9-]`, 1 to 40 characters.                                                              |
| `title`       | string | yes      | Not blank.                                                                                    |
| `description` | string | no       |                                                                                               |
| `run`         | object | yes      | `{ "windows"?, "macos"?, "linux"? }`: one shell command string per OS, at least one, none blank. |

Every unix that is not macOS counts as `linux`. A step with no command for the
machine's OS is not offered there.

**Unknown fields are ignored**, at every level: a newer schema's field, or a
kit's own notes, do no harm.

**Paths.** Every path a manifest names is relative to the kit's root. Hangar
keeps them normalized — forward slashes, no `.` parts (`./tabs\status/x.html`
reads `tabs/status/x.html`), `.` for the root itself — and refuses a path that
is absolute (`/x`, `C:\x`, `\\server\x`), names a drive or a stream (any `:`),
climbs out with `..`, or resolves outside the kit folder once symlinks and
junctions are followed. Either slash separates, on every OS.

**Errors.** A manifest is checked whole, and every problem is reported at once,
by the path of its field, joined by `; `:

```
id: must be 2 to 40 characters among a-z, 0-9 and -; tabs[0].permissions: unknown permission "files.write"
```

## The plugin, in the panes

Every pane of a workspace — orchestrator, workers and shell panes included —
is started with the workspace's kit ids (`ptySpawn({ kits })`). Rust resolves
each to its plugin folder and puts the folders on `CLAUDE_CODE_PLUGIN_DIRS` in
the pane's environment, after the hangar-bridge mod's when it comes along and
before whatever folders the user's own environment already lists:

```
CLAUDE_CODE_PLUGIN_DIRS=<hangar-bridge>;<kit A plugin>;<kit B plugin>;<the user's own>
```

(`:` instead of `;` outside Windows.) So a Claude Code started in the pane —
by Hangar or typed by hand — loads them.

- Kits **do not depend on the bridge**: a pane started with `bridge: false`, or
  with the bridge setting off, gets its workspace's kits all the same.
- A pane of another workspace gets that workspace's kits, and none of these.
- A kit that is not installed, whose manifest is broken, whose folder is gone,
  or that needs a newer Hangar is **skipped with a warning** printed in the
  Claude Code pane. It never blocks the pane.
- A machine whose Claude Code managed settings forbid sideloading
  (`"disableSideloadFlags": true`) refuses to start while
  `CLAUDE_CODE_PLUGIN_DIRS` is set, so the kits stay out there entirely — MCP
  servers included — and the pane says so.
- A WSL pane does not carry the environment into Linux: kits do not reach it.

- A GitHub kit whose **hooks** changed since this workspace accepted them is
  [left out](#consent-on-change) until they are accepted again, MCP servers
  included, and the pane says so.

`pty_spawn` answers `{ kitIssues: [{ id, reason, detail? }] }`, `reason` being
`missing`, `broken` (with `detail`, in English) or `forbidden`.

A pane of a workspace with at least one kit also finds their data's root in
[`HANGAR_KITS_DATA`](#a-kits-data-folder), with the bridge or without it. A pane
started with no kit — its workspace has none, or the only ones it has are left
out for their hooks — starts without the variable, even under a Hangar launched
from inside a pane.

### MCP servers, in both MCP modes

Claude Code names a plugin's server `plugin:<plugin name>:<server>`, and its
tools `mcp__plugin_<plugin name>_<server>__<tool>` — for the kit above,
`mcp__plugin_hello-kit_hello__<tool>`. That is the name a kit's skills use.

A workspace in `inherit` MCP mode loads the kits' servers through the plugins.

A workspace in `custom` MCP mode starts Claude Code with `--mcp-config <file>
--strict-mcp-config`, and **`--strict-mcp-config` drops a plugin's servers**
(checked with Claude Code 2.1.316: the plugin's skills load, its `.mcp.json`
servers do not). So `mcp_scope_write` is given the workspace's kits in that
mode, and copies their servers into the file under the same
`plugin:<plugin name>:<server>` names — the tools keep their names — with
`${CLAUDE_PLUGIN_ROOT}` written out as the plugin's folder (forward slashes, as
Claude Code spells it). Other `${VAR}` references are left for Claude Code. The
kits are not passed in `inherit` mode, where the plugins start their servers
themselves and a copy would run each one twice.

A plugin's servers are read from `mcpServers` in its `plugin.json` (inline, or
the relative path of a JSON file), then from its `.mcp.json` for the names not
already declared. A `.mcp.json` may hold them under `mcpServers` or at its top
level.

### Hooks

A plugin's hooks are commands **Claude Code runs by itself**, each time they
match, without asking. So the activation view shows them before anything is
stored, next to the setup steps, verbatim in the same monospace block, and
`kit_inspect` lists them as `hooks: [{ event, matcher, command }]`.

They are read from `hooks/hooks.json` and from `hooks` in
`.claude-plugin/plugin.json` (inline, the relative path of a file, or a list of
either), each hook listed once. `matcher` is `""` for a hook without one. The
reading leans towards showing too much: a hook that is not a `command` (a prompt,
a URL) comes as its JSON, a `modules` entry (a hooks module, code Claude Code
loads) under the event `modules` by its path, and a file that cannot be read as
it stands, under the file's name.

A hook finds the kit's data folder at `$HANGAR_KITS_DATA/<kit id>`.

### Consent on change

For a GitHub-installed kit, **an update that changes the hooks does not reach
new panes until it is accepted again** on the workspace.

What a workspace accepted is stored in its `kits[].granted` beside the tab
permissions, as the pseudo permission `hooks:<hash>`: the first 16 hex of the
SHA-256 of the canonical list, which is the JSON
`[{"event":…,"matcher":…,"command":…}]` sorted by those three fields and without
duplicates (so reformatting the file asks nothing). A manifest cannot ask for it,
and no tab ever holds it.

The hooks go through the same diff as new permissions. When the workspace does
not hold the kit's current `hooks:<hash>`, the kit reads « to review », and its
whole plugin stays out of `ptySpawn` and of the `--mcp-config` file, the pane
saying why. Accepting stores the hash of the hooks the person was shown: hooks
that changed while the view was open stay to be accepted.

- A **linked folder** kit is exempt and stores nothing: the person is writing it,
  it is read live, and every edit would ask again. The view says so.
- A kit with **no hooks** stores nothing, and asks nothing. An update that brings
  hooks to it is a change like any other.
- A git kit **installed before this was asked**, with its hooks as they were,
  behaves on the workspaces that have it as it did: not held back, and not asked,
  though the view lists its hooks. The index marks every kit added since
  (`hooksConsent`), and marks an older one as soon as an update changes its hooks;
  that is what tells it from a kit whose update just brought hooks, both holding
  no `hooks:<hash>`.

Two limits to know. The hash covers **the list, not the scripts it names**: an
update that changes what `node ${CLAUDE_PLUGIN_ROOT}/start.js` does, with the
same command, asks nothing. And this holds back **new panes**: a Claude Code
already running keeps its plugin folder, the clone an update moves, and watches
it for changes, so an update can reach a running session's hooks module before
anyone accepts it. Restarting panes follows the review, as for any kit change.

## Tab permissions

A closed set. A permission not in it is a manifest error.

| Permission        | Lets the tab                                                                                       |
| ----------------- | -------------------------------------------------------------------------------------------------- |
| `workspace.read`  | read the workspace it is shown in (`workspace.info`).                                              |
| `panes.read`      | list the workspace's panes, and hear when they change.                                             |
| `panes.send`      | send text to a pane's agent.                                                                       |
| `files.read`      | read a text file inside the workspace folder.                                                      |
| `data.read`       | read a text file of the kit's own data (`data.read`).                                              |
| `data.write`      | write a text file of the kit's own data (`data.write`).                                            |
| `loopback:<port>` | talk to the local server on that port (`loopback.fetch`), and make it do whatever that server allows. |
| `preview.show`    | open a file in Hangar's preview.                                                                   |
| `notify`          | raise a desktop notification.                                                                      |

`<port>` is written plainly (no sign, no leading zero), from 1024 to 65535:
`loopback:80`, `loopback:abc` or `loopback:08765` is a manifest error. A tab holds
it for that exact port, and no other.

The person accepts a tab's permissions when they enable the kit on a
workspace; what they accepted is the workspace's `kits[].granted`. A tab may
call what it **declares in its manifest and** the workspace granted, both: a
permission granted to the kit that this tab does not declare stays out of it,
and one an update added to the tab waits until the person grants it. Both are
read again at every call, so taking a grant back denies the next call.

## A tab

A tab opens in the workspace's side panel — the preview, beside the
terminals — from the toolbar's **Kits** menu or the command palette, which
list the tabs of the kits enabled on the workspace. It stays open per
workspace, like the files there, and keeps running while another tab or
workspace is in front; a workspace put to sleep lets its tabs' pages go, and
they load again when it wakes. Its bar reloads it, and says what it was
granted.

### Where its page comes from

The `hangar-kit` scheme (`src-tauri/src/kit_scheme.rs`) serves the kits
installed on the machine that can be used — not a missing, broken or
too-new one — at `<base>/<kit id>/<path in the kit>`, the base being
`http://hangar-kit.localhost` on Windows and `hangar-kit://localhost`
elsewhere. A tab opens at its `entry`:

```
http://hangar-kit.localhost/hello-kit/tabs/status/index.html
```

So **use relative URLs** for the kit's own files (`app.js`,
`../shared/style.css`): an absolute one like `/style.css` leaves the kit. The
one absolute URL is the SDK's, `/_hangar/kit.js`.

It serves:

- files inside the kit's folder only, once links are followed; a path with an
  encoded `.`, `..`, slash, backslash, `:` or NUL is refused;
- never a hidden file or folder (`.env`, `.git/…`);
- web files only, each with its type: `.html` `.htm` `.js` `.mjs` `.css`
  `.json` `.map` `.txt` `.md` `.csv` `.svg` `.png` `.jpg` `.jpeg` `.gif`
  `.webp` `.avif` `.ico` `.woff` `.woff2` `.ttf` `.otf` `.wasm` `.mp3` `.wav`
  `.ogg` `.m4a` `.mp4` `.webm`, 64 MB at most; anything else stays on disk;
- with `Access-Control-Allow-Origin: *`, so module scripts and fonts load,
  to the kit's pages and Hangar's own only, and never cached.

### What its page can do

The page runs isolated: this is code written by someone else.

- It is framed with `sandbox="allow-scripts"`, and every answer of the scheme
  carries a `sandbox allow-scripts` Content-Security-Policy as well: the page
  has an **opaque origin**. It reaches neither Hangar's window nor its
  commands, has **no storage** (`localStorage`, `sessionStorage`, IndexedDB
  and cookies throw or stay empty), opens no popup, submits no form, shows no
  `alert`/`confirm`/`prompt`, and cannot navigate Hangar.
- Its policy allows **the kit's own files and nothing else**: no network at
  all — no `fetch`, image, font, style or script from anywhere else, no
  frames, workers or plugins. Inline scripts and styles run, module scripts
  and WebAssembly too; `eval` does not.
- It talks to Hangar by `postMessage` to its parent, and only that (below).
  **The frame still has no network of its own**: `data.read`, `data.write` and
  `loopback.fetch` are done by the Hangar host, and a page calling `fetch` on a
  local server is refused as ever.
- **A tab is one page.** It speaks to Hangar from the page it opened on: if
  the frame loads another document — a link followed, the page sending itself
  elsewhere — Hangar takes the frame down and says so, until the person
  reloads the tab. Hash changes and `history` within the page are fine.

## The tab message protocol

Every message carries `hangar: 1`. Hangar answers only messages whose source
is that tab's frame, from its opaque origin; anything else is dropped without
an answer.

Request, from the page, to `window.parent`:

```json
{ "hangar": 1, "id": "7", "method": "panes.send", "params": { "pane": "Ava", "text": "Build the place" } }
```

`id` is a non-empty string (128 characters at most) or a number; a message
without one is dropped. Response, from Hangar, with the request's `id`:

```json
{ "hangar": 1, "id": "7", "result": { "queued": true, "pane": "Ava" } }
{ "hangar": 1, "id": "7", "error": { "code": "denied", "message": "this tab was not granted panes.send" } }
```

Event, from Hangar:

```json
{ "hangar": 1, "event": "panes.changed", "data": {} }
```

Methods:

| Method           | Params              | Needs            | Result                                                                   |
| ---------------- | ------------------- | ---------------- | ------------------------------------------------------------------------ |
| `workspace.info` | —                   | `workspace.read` | `{ id, name, folder }`: the workspace the tab is shown in, `folder` its absolute path. |
| `panes.list`     | —                   | `panes.read`     | `[{ name, agent, role, state, canSend }]`, in the workspace's order (below). |
| `panes.send`     | `{ pane, text }`    | `panes.send`     | `{ queued: true, pane }`: `text` waits for that pane's agent to be free. |
| `files.read`     | `{ path }`          | `files.read`     | The file's text, a string.                                               |
| `data.read`      | `{ path }`          | `data.read`      | `{ text }`, or the error `not_found` when it is absent.                  |
| `data.write`     | `{ path, text }`    | `data.write`     | `{}`: written whole, its folders made.                                   |
| `loopback.fetch` | `{ port, path, method?, body?, timeoutMs? }` | `loopback:<port>` for that exact port | `{ status, text }`: the local server's answer, whatever its status. |
| `preview.show`   | `{ path }`          | `preview.show`   | `{ opened: true, path }`: the file opens in the workspace's preview, in front. |
| `notify`         | `{ title, body? }`  | `notify`         | `null`: a desktop notification, titled `<kit name> · <title>`.          |

A pane in `panes.list`: `name` (what `panes.send` takes), `agent` (`claude`,
`codex`, `gemini`, `opencode` or `shell`), `role` (`orchestrator`, `worker`,
`manager` or `null`), `state` (`working`, `waiting` for an answer, `yours` —
done, not looked at yet — `idle`, or `null` when Hangar cannot tell), and
`canSend`, whether `panes.send` reaches it now.

- **`panes.send`** reaches the **Claude Code agents of the tab's own
  workspace**, by pane name (any case), and only those whose session runs
  Hangar's bridge: the text waits in Hangar's queue until the agent is free,
  and reaches it labelled as coming from `kit:<kit id>` — never as typed by
  the person, so it cannot approve anything on their behalf. `text` is 1 to
  20 000 characters, sent as given. At most **10 a minute**.
- **`files.read`** and **`preview.show`** take a path **relative to the
  workspace folder**: either slash, no `..`, no drive, no `:`, no hidden file
  or folder, 1 024 characters at most. Hangar checks it again on disk, where
  links must not lead out of the folder. `files.read` reads a UTF-8 text file
  of **1 MB at most** (a BOM is dropped); `preview.show` opens any file the
  preview shows, under the kit's name.
- **`data.read`** and **`data.write`** take a path **relative to that kit's
  data folder**: either slash, no absolute path, no `..`, no drive, no `:`, 1 024
  characters at most. A hidden name is allowed: the folder is the kit's own. Hangar
  checks it again on disk, where no link may lead out of the folder, and a kit
  never reaches another kit's; the kit comes from Hangar's own record of the tab,
  never from the message. Both handle UTF-8 text of **1 MB at most** (a BOM is
  dropped on reading). `data.write` is atomic (a temp file, then a rename), so a
  reader finds the whole of the old text or the whole of the new, and is limited
  to **10 a minute**, as `panes.send` is. See [the data folder](#a-kits-data-folder).
- **`loopback.fetch`** is made by Hangar, not by the page, to `127.0.0.1` only,
  whatever the system's proxy says, for the port the tab holds `loopback:<port>`
  for:
  - `path` starts with one `/` and is URL-encoded already, query included: no
    space, backslash, `#` or character beyond ASCII, 2 048 characters at most. It
    cannot change the host.
  - `method` is `GET` (the default) or `POST`. `body` is a string, on a `POST`
    only, 2 MB at most, sent as `Content-Type: application/json`.
  - No redirect is followed: a `302` comes back as `{ status: 302, text }`. No
    header or cookie comes from the page.
  - The answer may hold **2 MB**, or the call fails. Its text is decoded as UTF-8,
    what is not being replaced.
  - `timeoutMs` is 1 to 15 000, **5 000** by default.
  - A refused or timed-out connection answers the error `unreachable`: an ordinary
    state for a tab (the editor is closed), logged nowhere.
- **Polling.** There is no file watcher: a tab polls. `data.read` and
  `loopback.fetch` are cheap and have no limit of their own, so one call a second
  is fine; a tab that is not visible may slow itself down.
- **`notify`** takes a `title` of 1 to 120 characters and a `body` of 1 000
  at most. At most **5 a minute**.
- A tab has at most **16 calls** waiting for their answer at once.

Errors:

| Code           | When                                                                  |
| -------------- | --------------------------------------------------------------------- |
| `denied`       | The tab may not call the method, a port it does not hold included; or `panes.send` to a pane that is not a Claude Code agent. |
| `invalid`      | The params are not what the method takes.                             |
| `unknown`      | No such method.                                                       |
| `not_found`    | No pane of that name in the workspace; no such file in the kit's data. |
| `rate_limited` | Over a limit above; try again later.                                  |
| `unavailable`  | Hangar cannot do it now: the agent's session does not run the bridge, the workspace is gone. |
| `unreachable`  | `loopback.fetch`: nothing answers on that port, or not in time.       |
| `failed`       | It was tried and failed: a file not found, too large, not text, a link leading out of the data folder… The message says why. |

Events:

| Event           | Needs        | Sent when                                                            |
| --------------- | ------------ | -------------------------------------------------------------------- |
| `panes.changed` | `panes.read` | The workspace's panes change: one added, closed, renamed, or changing state. `data` is `{}`: call `panes.list`. |

### The SDK

`/_hangar/kit.js`, served by the `hangar-kit` scheme to every tab, wraps the
protocol in `window.hangar`. It is small, has no dependencies, and is all a
tab needs:

- `window.hangar.call(method, params?)` → a Promise of the result. It rejects
  with an `Error` whose `code` is the protocol's (`denied`, `invalid`…), or
  `timeout` when Hangar does not answer within 30 seconds, or `unavailable`
  when the page is not open as a tab.
- `window.hangar.on(event, handler)` → a function that stops listening.
- `window.hangar.version` → `1`.

A minimal tab, asking for `panes.read`, `panes.send` and `notify`:

```html
<!doctype html>
<meta charset="utf-8" />
<script src="/_hangar/kit.js"></script>
<ul id="panes"></ul>
<script>
  async function show() {
    const list = document.getElementById("panes");
    list.replaceChildren();
    for (const pane of await window.hangar.call("panes.list")) {
      const item = document.createElement("li");
      item.textContent = `${pane.name} (${pane.agent}, ${pane.state ?? "?"})`;
      if (pane.canSend) {
        const button = document.createElement("button");
        button.textContent = "Ping";
        button.onclick = () =>
          window.hangar
            .call("panes.send", { pane: pane.name, text: "Where are you at?" })
            .catch((error) => window.hangar.call("notify", { title: "Not sent", body: error.message }));
        item.append(" ", button);
      }
      list.append(item);
    }
  }
  window.hangar.on("panes.changed", show);
  show().catch((error) => (document.body.textContent = `${error.code}: ${error.message}`));
</script>
```

The SDK's `call` is generic, so the data folder and a local server need nothing
more. A tab showing a local server's queue and keeping a note, asking for
`data.read`, `data.write` and `loopback:8765`:

```html
<!doctype html>
<meta charset="utf-8" />
<script src="/_hangar/kit.js"></script>
<pre id="queue">…</pre>
<textarea id="note"></textarea>
<script>
  const queue = document.getElementById("queue");
  const note = document.getElementById("note");

  // Once a second. The editor being closed is no failure: it says so, and
  // the next poll finds it open.
  async function poll() {
    try {
      const { status, text } = await window.hangar.call("loopback.fetch", { port: 8765, path: "/queue" });
      queue.textContent = status === 200 ? text : `The server answered ${status}`;
    } catch (error) {
      queue.textContent = error.code === "unreachable" ? "The editor is closed." : error.message;
    }
  }
  setInterval(() => {
    if (!document.hidden) poll();
  }, 1000);
  poll();

  // A file that is not there yet is not_found.
  window.hangar
    .call("data.read", { path: "notes/today.txt" })
    .then(({ text }) => (note.value = text))
    .catch((error) => {
      if (error.code !== "not_found") throw error;
    });
  note.onchange = () => window.hangar.call("data.write", { path: "notes/today.txt", text: note.value });
</script>
```

`src/lib/kits.ts` exports the protocol's constants: `KIT_METHODS` (method →
permission), `KIT_EVENTS`, `KIT_SDK_PATH`, `KIT_FILE_MAX_BYTES`,
`KIT_DATA_MAX_BYTES`, `KIT_LOOPBACK_*`, `KIT_DENIED`, the message types and
`kitMayCall`. The window's side is `src/lib/kitApi.ts` (its limits and error
codes), the tab itself `src/components/preview/KitTabView.tsx`, and the paths
`files.read` and `preview.show` take are checked on disk by `kit_file_read` and
`kit_workspace_path` (`src-tauri/src/kit_scheme/files.rs`); the data folder is
`src-tauri/src/kits/data.rs` and `loopback.fetch` `src-tauri/src/kits/loopback.rs`.

## A kit's data folder

Each kit has a folder of its own, `~/.hangar/kit-data/<kit id>/` — beside
`HANGAR_ENDPOINT_FILE` for a development build given one, like the kits' store.
It is the same on every Claude account and worktree, and never inside a project.

- **A tab** reaches it through `data.read` and `data.write`.
- **A hook, or anything else in a pane**, finds it at
  `$HANGAR_KITS_DATA/<kit id>`: panes of a workspace with at least one kit get
  `HANGAR_KITS_DATA`, which is the root, `~/.hangar/kit-data`. Hangar makes the
  folder of each kit whose plugin comes along when such a pane starts, so a hook
  can write there at once; otherwise the first `data.write` does. A WSL pane does
  not carry the variable into Linux.
- **Removing a kit asks nothing new and leaves its data.** Added again, it finds
  it there; to get rid of it, delete the folder.

Nothing polices what a hook or a server does with the folder: only a tab's calls
are held to it.

## Setup commands

A step's command for the machine's OS is shown to the person **verbatim**, and
runs in a visible pane, opened in the kit's root folder (`dir`), once they
approve it. Hangar never runs one on its own.

Each run is recorded per machine in `~/.hangar/kits/setup.json` as
`{ kit, step, sha256, at }`, the hash being SHA-256 of the command string
exactly as the manifest writes it for this OS. A step whose command is
unchanged is not asked for again; one an update changed is. Removing a kit
forgets its steps: added again, it asks again.

`kit_setup_status(id)` lists the steps that have a command for this OS, each
with `command`, `sha256` and `done`; `kit_setup_mark(id, stepId)` records one
as run, with its current command.

## The store, and where kits come from

Installed kits live in `~/.hangar/kits/` — beside `HANGAR_ENDPOINT_FILE` for a
development build given one, as the bridge's mod does, so a dev build never
touches the installed app's kits. Its `index.json`:

```json
{
  "kits": [
    {
      "id": "hello-kit",
      "source": { "kind": "folder", "path": "D:\\kits\\hello-kit" },
      "dir": "D:\\kits\\hello-kit",
      "installedVersion": "0.1.0",
      "addedAt": 1793460795412
    },
    {
      "id": "roblox",
      "source": { "kind": "git", "url": "https://github.com/owner/kits", "ref": "main", "subdir": "roblox" },
      "dir": "C:\\Users\\me\\.hangar\\kits\\roblox\\roblox",
      "installedVersion": "1.0.0",
      "addedAt": 1793460795999
    }
  ]
}
```

`dir` is the kit's root on disk. `addedAt` is in milliseconds since the epoch.
`hooksConsent`, on a git kit added since hooks are asked about, or one whose
update changed them, says a workspace has to have accepted its hooks
([Consent on change](#consent-on-change)). An index that cannot be read is an
error, never a store that looks empty and gets overwritten.

**Folder kits** are linked, never copied: `dir` is the folder, read in place at
every pane launch, so editing it is editing the kit. Removing one only takes it
out of the index; the folder stays as it is.

**Git kits** come from a GitHub link, `https://` only:

- `https://github.com/<owner>/<repo>` (with or without `.git`, trailing `/`,
  `?query` or `#fragment`): the default branch, the kit at the top;
- `https://github.com/<owner>/<repo>/tree/<ref>/<folder…>`: that branch, tag or
  commit, the kit in that folder. A branch can hold slashes: the longest
  leading part of the path that names one of the remote's branches or tags is
  the ref, the rest the folder.

Links with credentials, other hosts, `http://`, `ssh`, `git@` and `file://`
are refused before anything runs. The repository is cloned with the git CLI,
shallow — one commit, no tags, no submodules, no large files — first aside,
then into `~/.hangar/kits/<id>` once its manifest checks out; `dir` is that
clone, or the folder in it. Symlinks are checked out as plain files
(`core.symlinks=false`). **Cloning never runs anything from the repository.**
Removing a git kit deletes its clone: the only thing Hangar ever deletes.

**Updates.** `kit_check_update(id)` fetches a git kit's ref, reads the version
its manifest declares there, and compares it with `installedVersion`; a folder
kit has nothing to check (`null`). `kit_update(id)` moves a git kit's clone to
the fetched commit — files the repository does not track, like what a setup
step installed there, stay — and records the new version; for a folder kit it
records the version the folder's manifest declares now. A remote whose manifest
now names another kit id is refused as an update.

## Workspaces

A workspace in `state.json` lists the kits enabled on it:

```ts
kits?: { id: string; granted: string[] }[]
```

`granted` is the tab permissions the person accepted, and `hooks:<hash>` for a
GitHub kit's accepted hooks. `state.json` is edited by hand, so it is read through
`workspaceKits(ws)` (`src/lib/kits.ts`): an entry that is not an object with a
valid kit id and a `granted` array is no kit, a duplicate id counts once, and a
permission Hangar does not know is never granted. A missing `kits` is no kit.

Nothing is ever written into a workspace's project folder: kits live in
`~/.hangar/kits/` or where they were linked from, their data in
`~/.hangar/kit-data/`, and the `--mcp-config` file in Hangar's app data.

## Commands and events

Tauri commands, with their wrappers in `src/lib/kits.ts`:

| Command                      | Wrapper                      | Answers                                                                                   |
| ---------------------------- | ---------------------------- | ----------------------------------------------------------------------------------------- |
| `kits_list`                  | `kitsList()`                 | `KitInfo[]`: each kit with `source`, `dir`, `installedVersion`, `addedAt`, `manifest` (null when unreadable or invalid), `error` (null when usable; a manifest can come with one: a kit needing a newer Hangar), `update` (the last check, while the app runs), `hooksPermission` (`hooks:<hash>`, null without hooks), `hooksConsent`. |
| `kit_add_folder(path)`       | `kitAddFolder(path)`         | `KitInfo`. `path` absolute, holding a valid kit.                                           |
| `kit_add_git(url)`           | `kitAddGit(url)`             | `KitInfo`. `url` a GitHub link as above.                                                   |
| `kit_remove(id)`             | `kitRemove(id)`              | Nothing. Deletes only a clone Hangar made.                                                 |
| `kit_check_update(id)`       | `kitCheckUpdate(id)`         | `{ current, latest, available, checkedAt }`, or `null` for a folder kit.                  |
| `kit_update(id)`             | `kitUpdate(id)`              | `KitInfo`.                                                                                 |
| `kit_inspect(id)`            | `kitInspect(id)`             | What it brings: `plugin` (`{ name, dir }`), `skills`, `commands`, `agents`, `mcpServers` (the plugin's names, unprefixed), `hooks` (`[{ event, matcher, command }]`), `hooksPermission`, `tabs`, `setup` (this OS's steps, with `command`). |
| `kit_setup_status(id)`       | `kitSetupStatus(id)`         | `[{ id, title, description?, command, sha256, done }]`.                                   |
| `kit_setup_mark(id, stepId)` | `kitSetupMark(id, stepId)`   | The same list, after recording the step.                                                  |
| `kit_data_read(kit, path)`   | through `KitTabView`         | The file's text. Refuses with `{ code, message }`.                                        |
| `kit_data_write(kit, path, text)` | through `KitTabView`    | Nothing. Refuses with `{ code, message }`.                                                |
| `kit_loopback_fetch(port, path, method?, body?, timeoutMs?)` | through `KitTabView` | `{ status, text }`. Refuses with `{ code, message }`.                        |

Skills are listed from `skills/<name>/SKILL.md`, commands and agents from the
`.md` files under `commands/` and `agents/` (a subfolder joins with `:`), in the
plugin's folder.

Errors are English strings, except those three's, whose `code` a tab tells apart.
`kits:changed` (`onKitsChanged(fn)`) is sent, with
no payload, whenever `kits_list` may answer something new: a kit added,
updated, removed or checked, or the index or an installed kit's manifest
changed on disk — a linked folder being edited, mostly (looked at every two
seconds).

## Strings

The interface around kits is translated in two dictionaries of their own, each
merged into `src/i18n.ts`:

- `src/strings/kitsManage.ts` — Settings › Kits and the activation dialog, keys
  `kitsManage.*`;
- `src/strings/kitsTab.ts` — a kit's tab and its frame, keys `kitsTab.*`.

Each exports `en`, the reference, and `fr: Record<keyof typeof en, string>`:
a French key missing there fails the build, without `i18n.ts` changing.
