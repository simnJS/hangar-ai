# Kits

A kit is a package a person links to a workspace to bring a domain — Roblox,
Unreal, a studio's own tools — into Hangar, without Hangar knowing anything
about that domain. A kit can bring:

- a **Claude Code plugin**: skills, commands, agents and MCP servers, loaded by
  every Claude Code pane of the workspaces the kit is enabled on;
- **tabs**: isolated pages of its own, talking to Hangar through a small,
  declared API, with a [data folder](#a-kits-data-folder) of the kit's and, when
  declared, a local server to talk to and
  [commands to run](#declared-commands) on tools already on the machine;
- **setup commands**: shown to the person verbatim, run in a visible pane once
  they approve, once per machine.

Its plugin may also start things by itself, [hooks](#hooks) and
[more](#what-runs-by-itself): the person sees all of it before enabling the kit.

Kits come from a local folder (linked, read live), from a GitHub link (cloned),
or from [the library](#the-library), which lists public kits pinned to a tag.

The public kits live in their own repositories in the
[`hangar-ai`](https://github.com/hangar-ai) organization, such as
[`kit-editor`](https://github.com/hangar-ai/kit-editor),
[`kit-guard`](https://github.com/hangar-ai/kit-guard) or
[`kit-ci`](https://github.com/hangar-ai/kit-ci): they are good models to read. A new
kit starts from [`kit-template`](https://github.com/hangar-ai/kit-template).

This page is the contract. The code lives in `src-tauri/src/kits.rs` and
`src-tauri/src/kits/`, with the typed wrappers in `src/lib/kits.ts`; a tab's
page is served by `src-tauri/src/kit_scheme.rs` with the SDK in
`src-tauri/kit-sdk/kit.js`, and framed and answered by
`src/components/preview/KitTabView.tsx`, `src/lib/kitTabs.ts` and
`src/lib/kitApi.ts`. How a tab should look is in [Hangar UI](kit-ui.md), the
interface library served to every tab.

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
  "hangar": ">=1.9.0",
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
<link rel="stylesheet" href="/_hangar/kit.css" />
<script src="/_hangar/kit.js"></script>
<main class="kit-page">
  <pre id="out" class="kit-pre" tabindex="0">…</pre>
</main>
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
| `commands`    | object            | no       | What its tabs may run, by name: [declared commands](#declared-commands).                  |

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
| `params`      | object   | no       | What callers may pass it, by name: [below](#a-tab-with-params).            |
| `singleton`   | boolean  | no       | One tab whatever its params, which new ones replace. `false` by default.   |

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
id: must be 2 to 40 characters among a-z, 0-9 and -; tabs[0].permissions: unknown permission "files.delete"
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

- A GitHub kit whose list of [what runs by itself](#what-runs-by-itself) changed
  since this workspace accepted it is [left out](#consent-on-change) until it is
  accepted again, MCP servers included, and the pane says so.

`pty_spawn` answers `{ kitIssues: [{ id, reason, detail? }] }`, `reason` being
`missing`, `broken` (with `detail`, in English) or `forbidden`.

A pane of a workspace with at least one kit also finds their data's root in
[`HANGAR_KITS_DATA`](#a-kits-data-folder), and what a hook needs to
[reach Hangar](#from-a-hook) in `HANGAR_API_URL` and `HANGAR_API_TOKEN`, with the
bridge or without it. A pane started with no kit — its workspace has none, or the
only ones it has are left out for what they run by themselves — starts without these variables,
even under a Hangar launched from inside a pane.

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

A plugin's servers are read in the order Claude Code loads them: its `.mcp.json`,
then `mcpServers` in its `plugin.json` (inline, the relative path of a JSON file,
or a list of either), a name declared later replacing an earlier one. A
`.mcp.json` may hold them under `mcpServers` or at its top level. A bundle
(`.mcpb`, `.dxt`) is shown, but stays out of the `--mcp-config` file: only Claude
Code can unpack it, so a workspace in `custom` mode does not get its servers.

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

### What runs by itself

A plugin has other ways to start code than its hooks. The activation view shows
them all, after the hooks, in the same monospace block, each kind with one sentence
saying when it runs. `kit_inspect` lists them as
`running: [{ kind, name, argv?, text?, when?, description?, env?, headers?, unreadable? }]`,
from the plugin schema of Claude Code 2.1.316, unexpanded and as the plugin writes
them. Showing is all this does: nothing here blocks a kind from running.

| `kind`       | Read from                                                                                                                 | When it runs                                                                        | What is given                                                                                    |
| ------------ | ------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `mcp`        | `.mcp.json`, `mcpServers`                                                                                                 | A session starts: started or connected to, and kept for the session                 | `argv` (command and args), or `text` (its URL), with the names in `env` and `headers`            |
| `mcp`        | `headersHelper` of a server, as `<server> · headersHelper`                                                                | When the server connects                                                            | `text`: the command                                                                              |
| `mcp`        | A `.mcpb` or `.dxt` bundle in `mcpServers`                                                                                | As a server; what it starts is inside                                               | `text`: its path or URL, `unreadable`                                                            |
| `monitor`    | `monitors/monitors.json`, `monitors`, `experimental.monitors`                                                             | A session starts, or the skill its `when` names first runs; until the session ends  | `text`: the command, `when`, `description`                                                       |
| `lsp`        | `.lsp.json`, `lspServers`                                                                                                 | Claude first edits a file of its types; until the session ends                      | `argv`, `when`: its file extensions, the names in `env`                                          |
| `skillHook`  | `hooks` in the frontmatter of `skills/`, `commands/`, what `skills` and `commands` name, a root `SKILL.md`, inline `content` | That skill or command is in use, each time they match                               | `text`: the lines, as written                                                                    |
| `settings`   | `settings.json`, `settings`: `agent`, `subagentStatusLine`                                                                | `agent`: the whole session answers as it. `subagentStatusLine`: every few seconds while subagents work | `text`: the agent, or the command                                                                |
| `grammar`    | `experimental.syntaxHighlighting.hljsLanguages`                                                                           | Claude Code loads this JavaScript to colour code                                    | `text`: its remote, when it has one, `unreadable`                                                |
| `bin`        | The direct children of `bin/`                                                                                             | Never by itself: on the `PATH` of Claude's shell, after the person's own entries    | `text`: `bin/<name>`                                                                             |
| `binary`     | `binaries`                                                                                                                | Downloaded into `bin/` when Claude Code installs a plugin                           | `text`: `sha256:<hash>`, `unreadable`                                                            |
| `dependency` | `dependencies`                                                                                                            | Enabled along with this plugin, when installed, with whatever it brings             | `text`: the entry as written, `unreadable`                                                       |
| `unknown`    | What the rest cannot use                                                                                                  | Hangar cannot say                                                                   | `text`: as written, or only `name` and `unreadable` for a file that does not parse               |

A hooks module (`modules` in a hooks file) stays in `hooks`, under the event
`modules`; the view gives it a group and a sentence of its own: code Claude Code
loads into itself when a session starts. Hangar shows its path, not its code.

**Env values and headers never leave Rust**, only their names do. In a block
Hangar did not understand, `env`, `headers` and `authToken` are replaced by their
names at any depth, and a file that does not parse is listed by its name alone, as
it may hold one.

What Hangar cannot read is said to be so at activation, in plain words, and never
left out: `unreadable` is a bundle, a remote grammar, a download, another plugin, a
file that does not parse, or more `skills` and `commands` files than it reads (500,
four folders deep).

`unknown` is the view's « not understood ». It lists a key Claude Code 2.1.316 does
not know in `plugin.json` or its `experimental`, and a key of `settings` other than
those two, whenever it holds any text at all: telling a path or a command from
anything else would be a guess. A known key in a shape that cannot be read, or a
path that leads nowhere or out of the plugin, comes the same way. A number or a
boolean names nothing, and is not listed.

These do not run by themselves, and are not listed: `commands`, `agents` (Claude
Code ignores `hooks` and `mcpServers` in a plugin agent's frontmatter), `skills`
apart from their hooks, `outputStyles`, `themes`, `workflows`, `channels`,
`userConfig`, `types`, and the plugin's metadata keys.

### Consent on change

For a GitHub-installed kit, **an update that changes what runs by itself does not
reach new panes until it is accepted again** on the workspace.

What a workspace accepted is stored in its `kits[].granted` beside the tab
permissions, as the pseudo permission `hooks:<hash>`: the first 16 hex of a SHA-256.
A manifest cannot ask for it, and no tab ever holds it. For a plugin with hooks and
nothing else, the hash is of the canonical list, the JSON
`[{"event":…,"matcher":…,"command":…}]` sorted by those three fields and without
duplicates. With anything else, it is of the JSON
`[<that list>, [[kind, name, what it wrote], …]]`, sorted and without duplicates,
where « what it wrote » is the item's whole definition with its keys sorted, env
values and headers included, and the bytes of a bundle inside the plugin. So
reformatting a file asks nothing, and changing a value that is never shown does.

The list goes through the same diff as new permissions. When the workspace does
not hold the kit's current `hooks:<hash>`, the kit reads « to review », and its
whole plugin stays out of `ptySpawn` and of the `--mcp-config` file, the pane
saying why. Accepting stores the hash of the list the person was shown: what
changed while the view was open stays to be accepted.

- A **linked folder** kit is exempt and stores nothing: the person is writing it,
  it is read live, and every edit would ask again. The view says so.
- A kit that **runs nothing by itself** stores nothing, and asks nothing. An update
  that brings something to it is a change like any other.
- A kit with **only hooks** keeps the hash of Hangar 1.8.0: a workspace that accepted
  them is not asked again.
- A GitHub kit with **anything else** that a workspace accepted before Hangar listed
  it, an MCP server for one, has a new hash and is asked once: the person was never
  shown those commands, and only the hooks' hash was ever stored, so nothing can
  tell they did not change. Until then it stays out of new panes.
- A git kit **installed before this was asked**, with all that as it was, behaves
  on the workspaces that have it as it did: not held back, and not asked, though the
  view lists it. The index marks every kit added since (`hooksConsent`), and marks
  an older one as soon as an update changes any of it; that is what tells it from
  a kit whose update just brought some, both holding no `hooks:<hash>`.

What to know about its limits:

- The hash covers **the list, not the scripts it names**: an update that changes
  what `node ${CLAUDE_PLUGIN_ROOT}/start.js` does, with the same command, asks
  nothing. The same goes for a file in `bin/` and for a hooks module's code.
- This holds back **new panes**: a Claude Code already running keeps its plugin
  folder, the clone an update moves, and watches it for changes, so an update can
  reach a running session's hooks module before anyone accepts it. Restarting panes
  follows the review, as for any kit change.
- A skill's frontmatter is read **line by line**, with no YAML parser: `hooks` as a
  top-level key is found, an exotic form (an anchor, a mapping on one line at the
  root) is not.
- A `` !`command` `` in the body of a skill or a command is **not listed**: it runs
  when that skill or command is invoked, not by itself.
- A key Hangar does not know, whose text changes with every version, asks again at
  every update. That is the price of leaving nothing out.

## Tab permissions

A closed set. A permission not in it is a manifest error.

| Permission        | Lets the tab                                                                                       |
| ----------------- | -------------------------------------------------------------------------------------------------- |
| `workspace.read`  | read the workspace it is shown in (`workspace.info`).                                              |
| `panes.read`      | list the workspace's panes, and hear when they change.                                             |
| `panes.send`      | send text to a pane's agent.                                                                       |
| `files.read`      | read a text file inside the workspace folder, read an image or a PDF there, and list its files.    |
| `files.write`     | change text files in the project, hidden ones like `.env` included, and [make a new image](#the-tab-message-protocol) (`files.writeBinary`) where there is no file. Held with `files.read`, it [reads hidden files too](#hidden-files-and-secrets). |
| `data.read`       | read a text file of the kit's own data (`data.read`).                                              |
| `data.write`      | write a text file of the kit's own data (`data.write`).                                            |
| `loopback:<port>` | talk to the local server on that port (`loopback.fetch`), and make it do whatever that server allows. |
| `command:<name>`  | run that [declared command](#declared-commands) on the computer (`command.run`), with the person's logins. |
| `preview.show`    | open a file in Hangar's preview.                                                                   |
| `notify`          | raise a desktop notification.                                                                      |

`<port>` is written plainly (no sign, no leading zero), from 1024 to 65535:
`loopback:80`, `loopback:abc` or `loopback:08765` is a manifest error. A tab holds
it for that exact port, and no other.

`<name>` is a command the kit declares under `commands`: naming another is a
manifest error. A tab holds it for that exact command, and no other.

The person accepts a tab's permissions when they enable the kit on a
workspace; what they accepted is the workspace's `kits[].granted`. A tab may
call what it **declares in its manifest and** the workspace granted, both: a
permission granted to the kit that this tab does not declare stays out of it,
and one an update added to the tab waits until the person grants it. Both are
read again at every call, so taking a grant back denies the next call.

## A tab

A tab opens in the workspace's side panel — the preview, beside the
terminals — from the toolbar's **Kits** menu or the command palette, which
list the tabs of the kits enabled on the workspace, or when
[an agent or a hook opens it](#opening-a-tab). It stays open per
workspace, like the files there, and keeps running while another tab or
workspace is in front; a workspace put to sleep lets its tabs' pages go, and
they load again when it wakes. Its bar reloads it, and says what it was
granted.

**A restart brings it back**, with its params and the title its page gave it.
Each window saves the tabs of the workspaces it draws in its own storage, and
restores them the first time it mounts the workspace, so no page runs for a
workspace nobody opened. They come back behind a closed panel, the panel's button
saying they are there; the page loads, and sets its badge again. Files in the
preview do not come back: only kits' tabs.

**Small or large, a tab is the same thing**: it lives in the panel, at the panel's
width. That is 320 interface points at least, so 320 CSS pixels at 100 % interface
size and 288 at the smallest, 90 %; a very narrow window can leave less, as a drawer.
Nothing sets a minimum width for a page but the page.

### Its look: Hangar UI

**A tab should use [Hangar UI](kit-ui.md)**, so that it looks like a part of Hangar
rather than a page framed inside it: `/_hangar/kit.css`, its tokens and components, and
`/_hangar/kit-ui.js`, its behaviours, loaded after the SDK. It asks for no permission and
has nothing to install. With it a page follows the theme of its workspace, live, with
Hangar's language and interface size, and holds at the panel's narrowest. That page also
has [the rules a kit follows](kit-ui.md#the-rules-a-kit-follows): no colour of its own,
designed states for what is empty, loading, failing or denied.

Those addresses exist since Hangar 1.9.0, and an older one answers them with a 404, so a kit
that uses them says `"hangar": ">=1.9.0"` in its manifest. A page that does without the
library starts on a white frame, and has its width and all the themes to answer for itself.

### A tab with params

A tab can declare what callers may pass it. Nothing is ever required: the palette and the
Kits menu open it with none, and a page given none offers its own picker, which Hangar
has nothing to do with.

```json
{
  "tabs": [
    {
      "id": "run",
      "title": "CI run",
      "entry": "tabs/run/index.html",
      "permissions": ["command:ci-run"],
      "params": {
        "run": { "description": "The run's number", "pattern": "[0-9]+" },
        "repo": { "description": "owner/name; the workspace's repository without it" }
      }
    }
  ]
}
```

A **param**, under its name (letters, digits, `_` and `-`, starting with a letter or `_`):

| Field         | Type   | Required | Rules                                                                                |
| ------------- | ------ | -------- | ------------------------------------------------------------------------------------ |
| `description` | string | yes      | Not blank. Listed for agents by `kit_tab_list`.                                      |
| `pattern`     | string | no       | A regular expression a value must match **whole**, whatever its author wrote. No rule: any string. |

A value is a string of 1 024 characters at most. A rule is checked by Rust when the manifest
is read, and a value against it before anything else, in linear time, so a rule cannot hang
Hangar. Hangar's window then holds the value to the same rule in JavaScript: write a rule both
read alike (`[0-9]+`, `[\w.-]+/[\w.-]+`). One JavaScript cannot read, such as `(?i)abc`, refuses
every value, with a message saying so.

**One tab per set of params.** Asked for with the same params, whatever their order, the tab
open comes forward as it is, not reloaded; with others, another tab opens: a tab per CI run.

**`singleton: true`** keeps one tab. New params replace the old, and its page hears the
[`tab.params` event](#the-tab-message-protocol) without being reloaded; the same params, or
none, bring it forward holding what it holds.

Params are what a caller said, and a page handles them as it would anything typed into a form:
`command.run` holds them to its own rules whatever the tab declares.

### Who asked

`tab.params` also says who the tab can answer: `caller` is `{ pane }`, the name of the pane that
opened it through `kit_tab_open` or the hooks' route, and `null` when the person opened it.

The name comes from **Hangar's own record of the request**: the pane the agent's MCP process was
started in, or the running pane a hook's route was given, looked up among the workspace's panes.
Nothing a request's body says of itself is read, and a name that is no pane of the workspace is no
caller.

**Asked for again, a tab's caller becomes the latest to ask**, and its page hears the
[`tab.params` event](#the-tab-message-protocol), singleton or not, even from the same pane: it may
be waiting for another answer. The person bringing the tab forward from the palette changes neither,
so the agent that asked is still the one to answer. The page answers with `panes.send`, as ever, which
holds it to its own rules; the pane may be gone by then, which answers `not_found`.

### Opening a tab

Three doors, for one tab: **the person**, from the palette or the Kits menu, with no params; an
**agent**, through MCP; a **hook of the kit**, through a local route. Hangar stays ignorant of
what the tab is about: the kit brings the logic, deciding when to open which tab with what.

An agent has three tools in the `hangar` MCP server. They act on the workspace it works in, found
from its pane or else its folder, as `preview_show` finds it: **the workspace is never read from
the request**.

| Tool            | Takes                          | Does                                                                                     |
| --------------- | ------------------------------ | ---------------------------------------------------------------------------------------- |
| `kit_tab_list`  | —                              | `{ workspace, tabs: [{ kit, kitName, tab, title, description, singleton, params, toReview? }] }`: the tabs of the kits enabled on the workspace, `description` being the kit's. `toReview` says the person has to accept changes before any can be opened. |
| `kit_tab_open`  | `{ kit, tab, params?, reveal? }` | `{ outcome, kit, tab, params, workspace, visible, note? }`, `outcome` being `opened`, `focused` or `replaced`. |
| `kit_tab_close` | `{ kit, tab, params? }`        | `{ closed: [params…], open: [params…] }`. With params, the tab holding those very ones, and none if nothing does; without, every tab of it open there. It asks nothing of the kit, so the tab of a kit taken off the workspace can still be closed. |

`kit_tab_open` is refused, with a message saying what to do, when:

- the kit is **not enabled** on the workspace, on its own or [on all of them](#workspaces);
- the kit is **to review** there: hooks, commands or permissions an update changed wait for the
  person's answer. The person can still open its tabs from the palette, as they are held;
- the kit is not installed any more, or cannot be used;
- it has **no such tab**;
- a param is **not declared**, is not a string, or **does not match** its rule.

**`reveal`**, `true` by default, opens the side panel if it is closed, with the tab in front.
`false` adds the tab and leaves the panel, and what is in front, as they are; a singleton still
takes its new params, and the panel's button says something is there. Neither ever takes the
keyboard from a terminal: without room beside the terminals the panel does not come over them as a
drawer, as it does for the person, and `visible: false` with a `note` says the person has to look.
A page that focuses something of its own while nobody clicked it has the keyboard handed back to
the terminal it took it from.

The three tools are pane control, like `preview_show`: refused when the person turned that off in
Settings › General › Agents. That goes for the hooks' route too.

### From a hook

A plugin's [hooks](#hooks) run in the pane, and have no MCP. They open a tab through the loopback
API that the bridge also speaks to:

```
POST $HANGAR_API_URL/api/kit/tab/open?pane=$HANGAR_PANE_ID
Authorization: Bearer $HANGAR_API_TOKEN
Content-Type: application/json

{ "kit": "ci-kit", "tab": "run", "params": { "run": "4567" }, "reveal": true }
```

The body is `kit_tab_open`'s, and it answers the same, as `{ "error": "…" }` when refused. The
workspace is the one of the pane in `pane`, and nothing else in the request has a say in it. Without
the token it answers `401`; for a pane Hangar is not running, `404`.

**A hook must fail quietly.** Hangar may be closed, the variables absent (a WSL pane), the kit
disabled: none of that is the agent's problem, and a hook that exits with an error gets in its way.
Give the call two seconds, say nothing, and always exit 0.

A hook that opens the tab for a run after a `git push`, in `hooks/hooks.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "node ${CLAUDE_PLUGIN_ROOT}/hooks/open-ci.js" }]
      }
    ]
  }
}
```

`hooks/open-ci.js`, the kit's own logic and Hangar none of it:

```js
// Never fails: whatever goes wrong here is no reason to bother the agent.
process.on("uncaughtException", () => process.exit(0));
const { HANGAR_API_URL: url, HANGAR_API_TOKEN: token, HANGAR_PANE_ID: pane } = process.env;

let input = "";
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("end", async () => {
  const command = JSON.parse(input).tool_input?.command ?? "";
  if (!url || !token || !pane || !/\bgit push\b/.test(command)) process.exit(0);
  await fetch(`${url}/api/kit/tab/open?pane=${encodeURIComponent(pane)}`, {
    method: "POST",
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
    body: JSON.stringify({ kit: "ci-kit", tab: "run", reveal: true }),
    signal: AbortSignal.timeout(2000),
  }).catch(() => undefined);
  process.exit(0);
});
```

Or from a shell, the same three safeguards in one line:

```sh
curl -fsS --max-time 2 -X POST "$HANGAR_API_URL/api/kit/tab/open?pane=$HANGAR_PANE_ID" \
  -H "Authorization: Bearer $HANGAR_API_TOKEN" -H "Content-Type: application/json" \
  -d '{"kit":"ci-kit","tab":"run","params":{"run":"4567"}}' >/dev/null 2>&1 || true
```

**Where the token goes.** `HANGAR_API_URL` and `HANGAR_API_TOKEN` are set by `pty_spawn`, in the
environment of the process it starts in a pane of Hangar, and of what that process starts in turn; a
pane with at least one kit gets them whether the bridge mod is on or off for it, and `HANGAR_PANE_ID`
is in every pane. Anything else does not:

- a **pane without a kit** starts without them, even under a Hangar launched from inside another
  pane, whose values are removed rather than inherited;
- a **tab's page** never sees them: it has no environment, and its `postMessage` is all it speaks;
- a **WSL pane** does not carry them into Linux;
- nothing is written into a project, and nothing is sent off the machine: the API listens on
  `127.0.0.1` only, on a port chosen at each start, under a token generated at each start.

It is the token of the whole API, not one for this route alone: the one the bridge's mod is given,
and the one the `--mcp` process reads from `~/.hangar/endpoint.json`, where it has always been. As an
environment variable it goes nowhere but those panes, and whatever runs in such a pane can use it
as they can. This is the same trust as the bridge's, and the reason the person sees a kit's hooks
before enabling it.

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
absolute URLs a page uses are Hangar's own, under `/_hangar/`: the SDK, `kit.js`, and
[Hangar UI](kit-ui.md), `kit.css` and `kit-ui.js`.

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
- Its policy allows **the kit's own files and Hangar's under `/_hangar/`, and
  nothing else**: no network at all — no `fetch`, image, font, style or script
  from anywhere else, no frames, workers or plugins. Inline scripts and styles run, module scripts
  and WebAssembly too; `eval` does not.
- It talks to Hangar by `postMessage` to its parent, and only that (below).
  **The frame still has no network of its own**: `data.read`, `data.write`,
  `loopback.fetch` and `command.run` are done by the Hangar host, and a page
  calling `fetch` on a local server is refused as ever.
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
| `files.readBinary` | `{ path }`        | `files.read`     | `{ base64, mime }`: an image or a PDF.                                   |
| `files.list`     | `{ path?, pattern? }` | `files.read`   | `{ files: [{ path, size, modified }], truncated }`: the files under that folder. |
| `files.write`    | `{ path, text, expected? }` | `files.write` | `{}`: written whole, its folders made.                                 |
| `files.writeBinary` | `{ path, base64 }` | `files.write` | `{}`: a new image, written whole, its folders made. Never over a file.  |
| `data.read`      | `{ path }`          | `data.read`      | `{ text }`, or the error `not_found` when it is absent.                  |
| `data.write`     | `{ path, text }`    | `data.write`     | `{}`: written whole, its folders made.                                   |
| `loopback.fetch` | `{ port, path, method?, body?, timeoutMs? }` | `loopback:<port>` for that exact port | `{ status, text }`: the local server's answer, whatever its status. |
| `command.run`    | `{ name, args? }`   | `command:<name>` for that exact name | `{ exitCode, stdout, stderr, timedOut, truncated }`: see [declared commands](#declared-commands). |
| `preview.show`   | `{ path }`          | `preview.show`   | `{ opened: true, path }`: the file opens in the workspace's preview, in front. |
| `notify`         | `{ title, body? }`  | `notify`         | `null`: a desktop notification, titled `<kit name> · <title>`.          |
| `tab.params`     | —                   | —                | `{ params, caller }`: what this tab holds, `{}` for none, and [who asked](#who-asked), `{ pane }` or `null`. |
| `tab.setTitle`   | `{ title }`         | —                | `{}`: its name in the tab strip.                                         |
| `tab.setBadge`   | `{ state, text? }`  | —                | `{}`: its state in the tab strip.                                        |

The three `tab.*` need no permission, and reach this tab alone: the tab is the host's own record,
never the message's.

- **`tab.setTitle`** takes plain text on one line, 40 characters at most, an emoji counting as one:
  what would not show or would turn the line around is refused (`invalid`), and markup is shown as
  it is written. Asking for the tab again leaves the name its page gave it. It lasts across a
  restart until the page says otherwise.
- **`tab.setBadge`** takes a `state` among `"running"`, `"success"`, `"failure"`, `"neutral"`, shown
  as a small glyph where the tab's icon goes (a ring turning, a check, an alert, a dot), and a `text`
  of 12 characters at most beside the title, the same plain text. `state: null` takes it away.
  It lasts until the page loads again, so a page sets it when it starts: a stale « running » is worse
  than none.
- Each may be called **120 times a minute**, enough to count seconds; each change redraws the strip.

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
  preview shows, under the kit's name. A tab holding `files.write` as well has
  `files.read` follow the rules below, with no hidden file refused:
  [hidden files](#hidden-files-and-secrets). `preview.show` stays as it is.
- **`files.write`, `files.writeBinary`, `files.readBinary` and `files.list`** take a path relative to the
  workspace folder as well: either slash, no `..`, no drive, no `:`, no device name on
  Windows, 1 024 characters at most. On disk, no link may lead out of the folder.
  **Nothing under `.git` is ever reached**, at any depth, whatever its case, whether a
  folder or the file a worktree has there, and through a link or not: its hooks are
  programs. `.gitignore` and `.github/` are files like any other.
- **`files.write`** writes UTF-8 text of **1 MB at most**, whole: a temp file in the same
  folder, then a rename, so a reader finds the whole of the old text or the whole of the
  new. It makes the missing folders. What it replaces has to be a text file `files.read`
  could read, and not a read-only one, and keeps its permissions. A link that stays inside
  stays a link, and the file it leads to is what changes. The page sends what it wants:
  line endings and a BOM are its business. At most **10 a minute**, apart from `data.write`'s.
  - `expected`, optional, is the text the page last read. If the file holds anything else,
    or is gone, the call answers `conflict`, and nothing is written or made, so an agent's
    edit is never overwritten blindly. It is looked at before writing and again right
    before the rename; a program writing in that very instant is not held off, as nothing
    can do so without its help.
  - A write is not shown in the workspace's activity: an agent's edits are listed by its
    pane, and a tab has none. It shows where any change does, in what git says of the folder.
- **`files.writeBinary`** makes a **new raster image** from `base64` (standard alphabet, padded),
  **10 MB decoded at most**, whole: a temp file in the same folder, then a link at the name, so a
  reader finds the whole image or nothing. It makes the missing folders.
  - The name ends in `.png`, `.jpg`, `.jpeg` or `.webp`, whatever its case. Not `.svg`, which
    carries script, nor a PDF, nor anything else: `invalid`.
  - The content has to be what the extension says, by its first bytes (PNG signature, JPEG
    `FF D8 FF`, WebP `RIFF`…`WEBP`): else `invalid`. Nothing more of it is looked at.
  - **It never replaces a file**: if anything is at that path, of any kind, the call answers
    `conflict` and the file stays as it is. A tab that wants another version picks another name.
  - It shares `files.write`'s limit of **10 a minute** with `data.write`'s, and the same path rules
    as `files.write`, `.git` never reached and no link leading out. Held with `files.write`, the
    permission the person accepted in the words of `files.write`: text files, and now new images.
- **`files.readBinary`** reads a file of **10 MB at most** whose extension is `.png`, `.jpg`,
  `.jpeg`, `.gif`, `.webp`, `.avif`, `.bmp`, `.ico`, `.svg` or `.pdf`, whatever its case. Its
  contents are not looked at. Anything else answers `unsupported`, a link named like an
  image included when what it leads to is not. A hidden file is held to the same rule as
  in `files.read`. A page shows an image from a `data:` URL, which its policy allows.
- **`files.list`** gives the files under `path`, the workspace folder without one, all the
  way down. `path` is relative to the workspace folder with `/`, what the other calls
  take; `size` is in bytes and `modified` in milliseconds since the epoch.
  - **Dotfiles are listed**, for whoever holds `files.read`: `.env` is what this is for.
    Reading one is another matter.
  - The walk does not go into `.git`, `node_modules`, `target` or `dist`. A folder asked for
    by name is listed all the same, `.git` aside. A link is neither followed nor listed.
  - **2 000 files at most**, the nearest first, then in path order; `truncated` says there
    were more, or that 200 000 entries were looked at before it found them all.
  - `pattern`, 256 characters at most, is a simple glob: `*` is any characters but a slash,
    `?` one such character, `**` any characters at all, and `**/` any folders, none
    included. Nothing else is special, and case counts. Without a slash it is held to a
    file's name, at any depth (`.env*`); with one, to its path from the folder listed
    (`config/**/*.json`).
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
- **Polling.** There is no file watcher: a tab polls. `data.read`, `files.read`,
  `files.list`, `loopback.fetch` and `command.run` have no limit of their own, so one call
  a second is fine; a tab that is not visible may slow itself down.
- **`notify`** takes a `title` of 1 to 120 characters and a `body` of 1 000
  at most. At most **5 a minute**.
- A tab has at most **16 calls** waiting for their answer at once.

Errors:

| Code           | When                                                                  |
| -------------- | --------------------------------------------------------------------- |
| `denied`       | The tab may not call the method, a port or a command it does not hold included; or `panes.send` to a pane that is not a Claude Code agent; or `command.run` on a GitHub kit whose commands were not accepted as they are now. |
| `invalid`      | The params are not what the method takes; `files.writeBinary`: not valid base64, over 10 MB, a name that is not a raster image's, or content that is not one. |
| `unknown`      | No such method.                                                       |
| `not_found`    | No pane of that name in the workspace; no such file in the kit's data; `files.readBinary`, `files.list`, and `files.read` for a tab that holds `files.write`: no such file or folder; `command.run`: the program is not installed, or the command has no argv for this OS. |
| `conflict`     | `files.write`: the file changed since the page read it; `files.writeBinary`: something is already at that path. Nothing was written. |
| `unsupported`  | `files.readBinary`: not an image or a PDF, by its extension.          |
| `rate_limited` | Over a limit above; try again later.                                  |
| `unavailable`  | Hangar cannot do it now: the agent's session does not run the bridge, the workspace or its folder is gone, the kit cannot be used. |
| `unreachable`  | `loopback.fetch`: nothing answers on that port, or not in time.       |
| `bad_args`     | `command.run`: a value is missing, does not match its rule whole, starts with `-`, or is not a string; or an argument the command does not declare. |
| `busy`         | `command.run`: four commands of this kit are running; try again later. |
| `failed`       | It was tried and failed: a file not found, too large, not text, a link leading out of the data folder, a program that is a batch file on Windows… The message says why. |

Events:

| Event           | Needs        | Sent when                                                            |
| --------------- | ------------ | -------------------------------------------------------------------- |
| `panes.changed` | `panes.read` | The workspace's panes change: one added, closed, renamed, or changing state. `data` is `{}`: call `panes.list`. |
| `tab.params`    | —            | A [singleton](#a-tab-with-params) got new params, or an agent or a hook [asked for this tab again](#who-asked). `data` is `{ params, caller }`. |
| `theme`         | —            | The frame's page has loaded, or the theme of the tab's workspace, Hangar's language, its interface size or the terminals' font changed. `data` is `{ scheme, locale, tokens }`: [what the host sends](kit-ui.md#what-the-host-sends). The SDK applies it by itself, before any handler. |

A page that takes params reads them when it starts, and a singleton's, or one that answers who asked,
listens as well. Reading them once it is ready is enough: an event sent before it listens changes what
`tab.params` answers.

```js
async function show({ params }) {
  if (!params.run) return showPicker();
  await window.hangar.call("tab.setTitle", { title: `Run #${params.run}` });
  await window.hangar.call("tab.setBadge", { state: "running" });
  // … command.run, then state: "success" or "failure"
}
window.hangar.on("tab.params", show);
window.hangar.call("tab.params").then(show);
```

### Hidden files and secrets

`files.read` alone refuses a hidden file or folder, and always has. **A tab that holds both
`files.read` and `files.write` reads them too**, `.git` aside, with `files.read` and
`files.readBinary`: without that, a kit that opens the `.env` so the person can fill it in
would have nothing to show them, and no `expected` to send.

Both have to be on the same tab: two tabs holding one each read nothing hidden. The activation
view says it in so many words for such a tab, naming `.env` and secrets, and that is what the person
is agreeing to. A kit whose update adds either reads « to review », and the tab holds the new one
only once accepted. A grant of `files.read` given before `files.write` existed reaches no further than
it did.

**What this means.** Such a page can read your secrets. It has no network of its own, but it
can pass them on if its kit is written to: to an agent through `panes.send`, to a local server
through `loopback.fetch`, to a command through `command.run`. And `files.write` on its own is more than
it sounds: changing `package.json`'s scripts or `.claude/settings.json` changes what runs next, which
is why `.git` is out of reach and the rest is what the person accepts.

Grants are by kit, read for each tab. So a kit that already holds both, on two tabs, can bring them
together on one with an update, which asks nothing new; such a kit could already change what runs.

```js
// Fills in a variable an agent could not set, and tells it so.
const path = ".env";
const { caller } = await window.hangar.call("tab.params");
let lastRead = await window.hangar.call("files.read", { path }).catch((error) => {
  if (error.code === "not_found") return null;
  throw error;
});

async function save(text) {
  try {
    await window.hangar.call("files.write", lastRead === null ? { path, text } : { path, text, expected: lastRead });
    lastRead = text;
    if (caller) await window.hangar.call("panes.send", { pane: caller.pane, text: `${path} is filled in.` });
  } catch (error) {
    if (error.code === "conflict") return showItChanged();
    throw error;
  }
}
```

### The SDK

`/_hangar/kit.js`, served by the `hangar-kit` scheme to every tab, wraps the
protocol in `window.hangar`. It is small, has no dependencies, and is all a
tab needs:

- `window.hangar.call(method, params?)` → a Promise of the result. It rejects
  with an `Error` whose `code` is the protocol's (`denied`, `invalid`…), or
  `timeout` when Hangar does not answer within 30 seconds (130 for `command.run`,
  whose commands may run for 120), or `unavailable` when the page is not open as
  a tab.
- `window.hangar.on(event, handler)` → a function that stops listening.
- `window.hangar.version` → `1`.
- `window.hangar.ui` → [Hangar UI's behaviours](kit-ui.md#behaviours), once
  `/_hangar/kit-ui.js` is loaded.

It also keeps the page in Hangar's theme, by [the `theme` event](kit-ui.md#the-theme):
the tokens of `/_hangar/kit.css`, on the root element.

A minimal tab, asking for `panes.read`, `panes.send` and `notify`:

```html
<!doctype html>
<meta charset="utf-8" />
<link rel="stylesheet" href="/_hangar/kit.css" />
<script src="/_hangar/kit.js"></script>
<main class="kit-page">
  <ul id="panes" class="kit-rows"></ul>
</main>
<script>
  async function show() {
    const list = document.getElementById("panes");
    list.replaceChildren();
    for (const pane of await window.hangar.call("panes.list")) {
      const item = document.createElement("li");
      item.className = "kit-row";
      const name = document.createElement("span");
      name.className = "kit-row__main";
      name.textContent = `${pane.name} (${pane.agent}, ${pane.state ?? "?"})`;
      item.append(name);
      if (pane.canSend) {
        const button = document.createElement("button");
        button.className = "kit-btn kit-btn--small";
        button.type = "button";
        button.textContent = "Ping";
        button.onclick = () =>
          window.hangar
            .call("panes.send", { pane: pane.name, text: "Where are you at?" })
            .catch((error) => window.hangar.call("notify", { title: "Not sent", body: error.message }));
        item.append(button);
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
<link rel="stylesheet" href="/_hangar/kit.css" />
<script src="/_hangar/kit.js"></script>
<main class="kit-page">
  <pre id="queue" class="kit-pre" tabindex="0">…</pre>
  <textarea id="note" class="kit-input" aria-label="Note"></textarea>
</main>
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
`KIT_BINARY_MAX_BYTES`, `KIT_LIST_*`, `KIT_DATA_MAX_BYTES`, `KIT_LOOPBACK_*`,
`KIT_COMMAND_PATTERN`, `KIT_DENIED`, the message types, `kitMayCall` and
`kitReadsHiddenFiles`. The window's side is `src/lib/kitApi.ts` (its
limits and error codes, the `tab.*` methods, which need no permission, among
them), the tab itself `src/components/preview/KitTabView.tsx`, the `theme` event it sends
`kitThemeEvent` in `src/lib/kitTabs.ts`,
and the paths `files.read` and `preview.show` take are checked on disk by
`kit_file_read` and `kit_workspace_path` (`src-tauri/src/kit_scheme/files.rs`), and
those `files.write`, `files.writeBinary`, `files.readBinary` and `files.list` take, and what a tab holding
`files.write` reads, by `src-tauri/src/kits/files.rs`; the
data folder is `src-tauri/src/kits/data.rs`, `loopback.fetch`
`src-tauri/src/kits/loopback.rs` and `command.run` `src-tauri/src/kits/commands.rs`.

## Declared commands

A tab can get data from tools already on the machine — `gh run view`, `docker ps`,
`glab ci status` — without a network, a token or a shell of its own. The kit
declares its commands in its manifest, the person sees them verbatim when enabling
it, and a tab can run those and nothing else.

```json
{
  "commands": {
    "ci-run": {
      "description": "The state of a CI run.",
      "run": ["gh", "run", "view", "{run}", "--repo={repo}", "--json", "status,conclusion,jobs"],
      "args": {
        "run": { "pattern": "[0-9]+", "description": "The run's number" },
        "repo": { "pattern": "[\\w.-]+/[\\w.-]+" }
      },
      "timeoutSeconds": 20
    }
  },
  "tabs": [
    { "id": "ci", "title": "CI", "entry": "tabs/ci/index.html", "permissions": ["command:ci-run"] }
  ]
}
```

A **command**, under its name, `[a-z][a-z0-9-]*`:

| Field            | Type             | Required | Rules                                                                             |
| ---------------- | ---------------- | -------- | --------------------------------------------------------------------------------- |
| `description`    | string           | yes      | Not blank. Shown to the person.                                                   |
| `run`            | array, or object | yes      | An argv, or `{ "windows"?, "macos"?, "linux"? }` holding one per OS, at least one. Never a command line. |
| `args`           | object           | no       | What each placeholder may hold, by name: `{ "pattern", "description"? }`.         |
| `timeoutSeconds` | number           | no       | A whole number from 1 to 120. 30 by default.                                      |

**An argv** is the program, then literal items and placeholders. A placeholder is
`{name}`, a name being letters, digits, `_` and `-`, starting with a letter or `_`.
It may be a whole item (`{run}`) or inside one (`--repo={repo}`). Other braces are
text, so `docker ps --format {{.Names}}` needs nothing special.

These are manifest errors: a placeholder not declared in `args`, an arg without a
`pattern`, a `pattern` that is not a regular expression, a placeholder or any brace
in the program, a `run` written as a string, an arg named `kitDir`.

**The program** is a name found on the PATH: not a path, nor a name ending in a dot
or a space. It is found on the PATH Hangar computes for everything it starts
(`src-tauri/src/path_env.rs`), passing over relative and empty entries, so nothing
is ever run from the project's folder for being there. Absent, the call answers
`not_found`, an ordinary state for a tab (`gh` not installed), and nothing is logged.

**On Windows, a program that resolves to a `.bat` or a `.cmd` is refused** (`failed`):
arguments to a batch file are parsed by cmd.exe, which would bring a shell back. That
includes what npm installs (`npm`, `pnpm`, and the tools they install), which are `.cmd`
files there.

### A script of the kit's own: `{kitDir}`

`{kitDir}` is a reserved placeholder: Hangar fills it in with the kit's root folder, in any item of
the argv except the program.

```json
{
  "commands": {
    "audit": {
      "description": "Audits a package.",
      "run": ["node", "{kitDir}/tools/audit.js", "{id}"],
      "args": { "id": { "pattern": "[0-9]+" } }
    }
  }
}
```

- It needs no `args` entry and no `pattern`. A kit may not declare an arg named `kitDir`, and a tab
  that passes one answers `bad_args`: the folder is Hangar's to say.
- It stays inside the item it is written in, so a kit folder with spaces is still one argument, and it
  is an argument however it is filled in: a program never comes from it.
- This is how a kit reaches what cannot be run directly: `pnpm`, `npm` and `npx` are `.cmd` files
  on Windows, which are refused. A kit runs `node` on a script of its own, and what the script calls
  is its own business.

**What the person accepts is the script's name, not its content.** The activation view shows the argv
as written, `{kitDir}` included, and the hash covers that: an update that changes what
`{kitDir}/tools/audit.js` does, under the same name, asks nothing, exactly as for
[hooks](#consent-on-change). The script runs with the person's logins, like any command.

### Running one

```js
const { exitCode, stdout, stderr, timedOut, truncated } = await window.hangar.call("command.run", {
  name: "ci-run",
  args: { run: "4567", repo: "simnJS/hangar-ai" },
});
```

`exitCode` is the program's, a code other than 0 being an answer and not a failure;
null when it did not end by itself. `truncated` says stdout or stderr went past the cap.

- **No shell, ever.** The argv goes to the program as it is, `Command::new` and its
  arguments. A value holding `;`, `$(…)` or `&&` reaches it as plain text.
- **Each value must match its rule, whole**, whatever the author wrote: the pattern is
  compiled on its own, then anchored at both ends (Rust's `regex` syntax, no
  look-around and no back-reference; it matches in linear time). A value that is missing
  or does not match, a value that is not a string, and an argument the command does not
  declare answer `bad_args`. A value that only fills a placeholder of another OS's argv
  need not be given.
- **A value starting with `-` is refused** even if its pattern allows it, as is an item
  that would start with `-` once filled in (`{a}-{b}` with an empty `a`): a parameter
  never becomes an option. Options are the kit's to write in the argv.
- A value is filled in once: one holding `{other}` stays that text.
- **It starts in the workspace folder of the tab**, with Hangar's own environment and
  nothing added, and no stdin.
- **Stdout and stderr are kept up to 1 MB each.** Past that the rest is read and
  dropped, so the program is not held up, and `truncated` is true. They are decoded as
  UTF-8, what is not being replaced.
- **At the timeout it is killed with its tree**, `timedOut` is true, and what it wrote
  until then comes back. A process it leaves behind when it ends by itself is not
  killed, and not waited for beyond half a second. Closing the tab does not stop a
  command: its timeout does.
- **At most 4 commands of a kit run at once**, whatever the tab: a further call answers
  `busy`.
- A tab is told only what it holds: `command.run` for a command it was not granted
  answers `denied`, as does one the kit does not declare (any more).

### What the person accepts

The activation view shows each command in the same monospace block as the setup steps
and the hooks, its argv on one line, with the rule next to each placeholder and one
sentence: a tab of this kit can run this command on their computer whenever it wants,
with their logins, for instance their `gh` login; only the parts in braces change, and
they must match their rule. An item holding a space or a quote is quoted, so one item
cannot read as two, and what would not show or would turn the line around (control and
format characters) is written out. A tab's `command:<name>` reads in plain words among
its permissions.

`kit_inspect` lists them as `declaredCommands`: `[{ name, description, argv, args,
timeoutSeconds }]` for this OS, the argv verbatim, placeholders included, and `args` the
rules of the placeholders of that argv, `{kitDir}` having none. A command with no argv for
this OS is left out there, and answers `not_found`.

For a GitHub-installed kit, what a workspace accepted is stored in its `kits[].granted`
as the pseudo permission `commands:<hash>`, exactly like `hooks:<hash>`: the first 16 hex
of the SHA-256 of the canonical list for this OS, the JSON
`[{"name":…,"run":[…],"args":{"<arg>":"<pattern>"}}]` sorted by name, `args` by arg. It
covers what can run: a reworded description or another timeout asks nothing, and a wider
rule asks again. A manifest cannot ask for it, and no tab holds it.

**A GitHub kit whose commands changed must be accepted again before its tabs can run
any**, whatever else they hold: `command.run` answers `denied` until then, the kit reads
« to review », and the rest of the kit works as it did, its plugin included. Accepting
stores the hash of what the person was shown. Rust compares it with the kit as it is at
each call, so an update cannot slip a new argv under the grant given for another.

A **linked folder** kit is exempt and stores nothing, as for hooks: the person is writing
it, it is read live, and every edit would ask again. The view says so. A kit with no
command for this OS stores nothing, and asks nothing.

Three limits to know. **A kit can declare a shell**: `["sh", "-c", "{x}"]` with a rule of
`.*` is a valid manifest, and so is `node -e`. Hangar does not keep a list of such
programs, which could never be whole: the person reads the argv before accepting, and that
is what the view is for. The hash covers **the list, not the programs it names**: a `gh`
that changes on the machine asks nothing, and neither does a [script of the kit's](#a-script-of-the-kits-own-kitdir)
that changes under the same name. And the hash is **for this OS**: a kit whose
argv changed only for another system asks nothing here.

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
    },
    {
      "id": "ci-status",
      "source": { "kind": "git", "url": "https://github.com/hangar-ai/ci-status", "ref": "refs/tags/v0.2.0" },
      "dir": "C:\\Users\\me\\.hangar\\kits\\ci-status",
      "installedVersion": "0.2.0",
      "addedAt": 1793460796500
    }
  ]
}
```

`dir` is the kit's root on disk. `addedAt` is in milliseconds since the epoch.
`hooksConsent`, on a git kit added since this is asked about, or one whose update
changed it, says a workspace has to have accepted what it runs by itself
([Consent on change](#consent-on-change)). An index that cannot be read is an
error, never a store that looks empty and gets overwritten.

**Folder kits** are linked, never copied: `dir` is the folder, read in place at
every pane launch, so editing it is editing the kit. Removing one only takes it
out of the index; the folder stays as it is.

**Git kits** come from a GitHub link, `https://` only:

- `https://github.com/<owner>/<repo>` (with or without `.git`, trailing `/`,
  `?query` or `#fragment`): the default branch, the kit at the top;
- `https://github.com/<owner>/<repo>/tree/<ref>/<folder…>`: that branch, tag or
  commit, the kit in that folder, or at the top with no folder. A branch can hold
  slashes: the longest leading part of the path that names one of the remote's
  branches or tags is the ref, the rest the folder. A **tag** pins the kit: its
  `ref` is recorded as `refs/tags/<tag>`, which git reads as that tag and no
  branch of the same name.

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
now names another kit id is refused as an update. A kit
[pinned to a tag](#pinning) does not follow its ref: it follows the catalog.

## The library

Settings › Kits has a **Library**: the public kits of the
[`hangar-ai`](https://github.com/hangar-ai) organization on GitHub, one repository
per kit, one click to install. No server of ours stands behind it — the list is a
static file of the repository `hangar-ai/catalog`, `index.json`:

```json
{
  "schema": 2,
  "kits": [
    {
      "id": "ci-status",
      "name": "CI status",
      "description": "A tab that follows a GitHub Actions run.",
      "repo": "hangar-ai/ci-status",
      "tag": "v0.2.0",
      "version": "0.2.0",
      "permissionsSummary": "Runs gh to read CI runs, shows a tab, sends notifications.",
      "tags": ["ci", "github"],
      "verified": true
    }
  ]
}
```

`id`, `name` and `version` are those of the `hangar-kit.json` at that tag. `repo`
is `owner/name`, the kit at the top of it. `tag` is a tag of that repository, not a
branch: a branch's name there installs a kit that follows that branch.
`permissionsSummary` is one sentence for the card, never what the activation shows
(that is read from the kit itself).

**How a kit gets listed.** By a pull request to
[`hangar-ai/catalog`](https://github.com/hangar-ai/catalog) adding its entry; a new
version is a pull request changing `tag` and `version`. Nothing in this repository
lists a kit.

`kit_gallery_list` reads `https://raw.githubusercontent.com/hangar-ai/catalog/main/index.json`
over https: no redirect, 256 KB at most, ten seconds at most. It is the only thing
the library fetches — no icon, no manifest. Another `schema` than 2 lists nothing.
An entry that is not well formed (an id outside `[a-z0-9-]`, a `repo` that is not
exactly `owner/name` in GitHub's characters, a `tag` git could misread, a version
that is not semver, a repeated id) is dropped and the others stay. The last good
list is kept in `gallery-cache.json` beside the store; when the catalog cannot be
read, that copy is shown and the page says so (`origin: "cached"`), and with none
yet it says the library cannot be reached (`"unavailable"`). A 404, the answer
while the catalog is not public, is not a failure: with no copy kept, the list is
empty and the page says no kit is listed yet (`"live"`). Nothing is logged for any
of it.

A card shows the name, description, repository, tag and tags, what the kit will
ask, whether it was reviewed; whether it is installed, at which version and what it
is pinned to; an update when the catalog names a new tag with a newer version.
**Install** is `kit_add_git` on `https://github.com/<repo>/tree/<tag>` — nothing
runs and nothing is enabled by it. The person sees what the kit does, and accepts
it, in the activation, as for any kit. There is one source in this version, and no
way to add another.

### Pinning

A kit installed from a tag stays on that tag. Its branch moving changes nothing,
and neither does a new tag of its repository: **a new version appears only when the
catalog names a new tag**, so it reaches people once a pull request to the catalog
was reviewed.

- `kit_check_update` and `kit_update` of a pinned kit read the catalog, or the kept
  copy when it cannot be read, for the entry of that `id` and of the repository the
  kit was cloned from. On another tag, they fetch that one, shallow, and go by the
  version its manifest declares. Otherwise nothing is fetched and the kit is up to
  date.
- `kit_update` then moves the clone to that tag and records it as the new `ref`,
  through [consent on change](#consent-on-change) and the permissions' diff like any
  update.
- A kit is never taken back to an older version: a tag whose manifest is not later
  than `installedVersion` is not offered, and updating to it is refused.
- The same `id` in another repository says nothing of an installed kit, which stays
  where it is: to change repository, remove it and install it again. The catalog has
  no say either on a linked folder, or on a kit added by a link to a repository or
  a branch, which follows its branch as ever.
- A tag link pasted by hand pins the same way. If its kit is not in the catalog, it
  stays on that tag.

Settings › Kits shows the version and what the kit is pinned to, on its row and on
its card.

**A tag is not a commit.** The catalog names a tag, and whoever owns the repository
can move it. A kit already installed is not affected, as nothing is fetched for the
tag it is on, but someone installing after that gets what the tag holds then, which
is not what was reviewed.

### Verified

`verified` is shown as a badge when **the catalog says `true` and the repository's
owner is exactly `hangar-ai`**. Hangar checks that itself, on the fetched list and on
the copy it kept, whatever the file says. Anything else is a community kit, and the
card says in one sentence that Hangar did not review it.

It says who looked at the kit, and changes nothing else: installing and activation
are the same for both, and the person is shown and accepts the same things.

## Workspaces

A workspace in `state.json` lists the kits enabled on it:

```ts
kits?: { id: string; granted: string[] }[]
```

`granted` is the tab permissions the person accepted, and `hooks:<hash>` and
`commands:<hash>` for what a GitHub kit runs by itself and the commands its tabs may run, once accepted. `state.json` is edited by hand, so it is read through
`workspaceKits(ws)` (`src/lib/kits.ts`): an entry that is not an object with a
valid kit id and a `granted` array is no kit, a duplicate id counts once, and a
permission Hangar does not know is never granted. A missing `kits` is no kit.

A kit can also be enabled on **all workspaces at once** from Settings › Kits, accepted
once for all of them: `kitsEverywhere` in the settings, of the same shape. What decides
anything about a workspace's kits is `effectiveKits(ws, settings)`: its own entries, then
the others of `kitsEverywhere`, a workspace's own entry winning for its grants. So its
panes load such a kit, and its tabs open and hold what was accepted, the pseudo
permissions included. The workspace's Kits dialog shows it on, its switch locked, and
says it is managed in Settings › Kits.

Hangar writes nothing of a kit's into a workspace's project folder: kits live in
`~/.hangar/kits/` or where they were linked from, their data in
`~/.hangar/kit-data/`, and the `--mcp-config` file in Hangar's app data. What changes
a project's file is a tab granted `files.write`, saying what, or a kit's own code.

## Commands and events

Tauri commands, with their wrappers in `src/lib/kits.ts`:

| Command                      | Wrapper                      | Answers                                                                                   |
| ---------------------------- | ---------------------------- | ----------------------------------------------------------------------------------------- |
| `kits_list`                  | `kitsList()`                 | `KitInfo[]`: each kit with `source`, `dir`, `installedVersion`, `addedAt`, `manifest` (null when unreadable or invalid), `error` (null when usable; a manifest can come with one: a kit needing a newer Hangar), `update` (the last check, while the app runs), `hooksPermission` (`hooks:<hash>` of what its plugin runs by itself, null when it runs nothing), `hooksConsent`, `commandsPermission` (`commands:<hash>` for this OS, null without commands). |
| `kit_add_folder(path)`       | `kitAddFolder(path)`         | `KitInfo`. `path` absolute, holding a valid kit.                                           |
| `kit_add_git(url)`           | `kitAddGit(url)`             | `KitInfo`. `url` a GitHub link as above.                                                   |
| `kit_gallery_list`           | `kitGalleryList()`           | `{ origin: "live" \| "cached" \| "unavailable", kits, fetchedAt }`: the library, see [The library](#the-library). Never refuses.  |
| `kit_remove(id)`             | `kitRemove(id)`              | Nothing. Deletes only a clone Hangar made.                                                 |
| `kit_check_update(id)`       | `kitCheckUpdate(id)`         | `{ current, latest, available, checkedAt }`, or `null` for a folder kit. A pinned kit goes by [the catalog's tag](#pinning). |
| `kit_update(id)`             | `kitUpdate(id)`              | `KitInfo`. A pinned kit goes by [the catalog's tag](#pinning).                             |
| `kit_inspect(id)`            | `kitInspect(id)`             | What it brings: `plugin` (`{ name, dir }`), `skills`, `commands`, `agents`, `mcpServers` (the plugin's names, unprefixed), `hooks` (`[{ event, matcher, command }]`), `running` ([what else it starts by itself](#what-runs-by-itself), never an env value or a header), `hooksPermission`, `tabs`, `setup` (this OS's steps, with `command`), `declaredCommands` (`[{ name, description, argv, args, timeoutSeconds }]`, this OS's, verbatim; `commands` is the plugin's slash commands), `commandsPermission`. |
| `kit_setup_status(id)`       | `kitSetupStatus(id)`         | `[{ id, title, description?, command, sha256, done }]`.                                   |
| `kit_setup_mark(id, stepId)` | `kitSetupMark(id, stepId)`   | The same list, after recording the step.                                                  |
| `kit_data_read(kit, path)`   | through `KitTabView`         | The file's text. Refuses with `{ code, message }`.                                        |
| `kit_data_write(kit, path, text)` | through `KitTabView`    | Nothing. Refuses with `{ code, message }`.                                                |
| `kit_files_read(root, path)` | through `KitTabView`         | The file's text, hidden or not. Refuses with `{ code, message }`.                         |
| `kit_files_read_binary(root, path, hidden)` | through `KitTabView` | `{ base64, mime }`. Refuses with `{ code, message }`.                            |
| `kit_files_list(root, path?, pattern?)` | through `KitTabView` | `{ files, truncated }`. Refuses with `{ code, message }`.                                 |
| `kit_files_write(root, path, text, expected?)` | through `KitTabView` | Nothing. Refuses with `{ code, message }`.                                         |
| `kit_files_write_binary(root, path, base64)` | through `KitTabView` | Nothing. Refuses with `{ code, message }`.                                           |
| `kit_loopback_fetch(port, path, method?, body?, timeoutMs?)` | through `KitTabView` | `{ status, text }`. Refuses with `{ code, message }`.                        |
| `kit_command_run(kit, name, args, cwd, accepted)` | through `KitTabView` | `{ exitCode, stdout, stderr, timedOut, truncated }`. Refuses with `{ code, message }`. |

Skills are listed from `skills/<name>/SKILL.md`, commands and agents from the
`.md` files under `commands/` and `agents/` (a subfolder joins with `:`), in the
plugin's folder.

Errors are English strings, except those eight's, whose `code` a tab tells apart.
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
