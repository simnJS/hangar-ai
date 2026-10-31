# Éditeur

A Hangar kit. When an agent needs a value it cannot or must not set (an API key, a
secret, a machine setting), it opens the file in a tab, you fill it in, and the agent
carries on without ever seeing the secret. The same goes for a config file you should
check or edit.

## What it does

- **A dotenv file opens as a form**: one row per key, values hidden until you reveal
  them. The keys the agent asked for come first, marked missing, under its reason. Keys
  in `.env.example` and missing here are offered. Saving changes only the lines you
  edited: comments, order, quotes and line endings stay as they were.
- **It warns when the file is not ignored by git.** It reads the `.gitignore` files, says
  so plainly, and adds the line only when you click. A template such as `.env.example`
  is meant to be committed and gets no warning.
- **Any other text file opens in a plain editor**: line numbers, the line the agent
  pointed at, a diff before saving. JSON is checked by `JSON.parse`. TOML is parsed for
  its syntax and YAML is scanned for what is certainly wrong (a tab in the indentation, a
  quote or a bracket left open): both say it is a basic check, not a proof. A broken file
  can be saved only after you say you know.
- **It never overwrites silently.** If the file changed while you were editing, nothing is
  written and it shows both sides: a diff for a text file, key by key and hidden for a
  dotenv file, where your keys go on top of what the disk now holds. You choose.
- **The agent is told when you save**, with which keys were set or that the file changed.
  With nothing to change, one click says that too.
- **Opened by you, with no params**, it offers a file picker: env files first, then config
  files.

English and French, in Hangar's language; light and dark, in Hangar's theme; legible at
the side panel's narrowest.

## What it can touch

Three permissions, shown to you before you enable the kit. It runs nothing by itself: no
hook, no MCP server, no command.

| Permission    | What for                                                                       |
| ------------- | ------------------------------------------------------------------------------ |
| `files.read`  | Read the file you or the agent named, `.gitignore`, `.env.example`; list files. |
| `files.write` | Save the file; add a line to `.gitignore` when you click.                       |
| `panes.send`  | Tell the agent that asked that you saved.                                       |

`files.read` and `files.write` together let a tab read hidden files, `.env` included, and
this is the kit that is for. Nothing under `.git` can be reached, and nothing leaves the
machine.

**What reaches the agent:** the file's path, key names and counts, in sentences fixed in
[`tabs/editor/answer.js`](tabs/editor/answer.js). Never a value. A name that is also a
value in the file, or that you typed yourself in the « Add a key » field, is counted and
not named. `test/no-value-reaches-an-agent.test.js` drives the tab over hostile files and
random edits and looks at everything that leaves the page.

What it cannot do: a tab cannot read `.git/info/exclude` or your global excludes, or tell
whether a file is already tracked, so a tracked file listed in `.gitignore` reads as
ignored and a file excluded elsewhere still gets the warning. `files.read` drops a BOM and
the page cannot know there was one, so saving a file that had one saves it without.

## Install

You need Hangar 1.9.0 or later.

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/simnJS/hangar-ai/tree/main/kits/editor`
   (or link a local copy of this folder).
2. Enable it on a workspace, and read what you accept: the three permissions above.
3. Restart the workspace's Claude Code panes, so they load the skill.

## For agents

The skill in [`skills/editor/SKILL.md`](skills/editor/SKILL.md) says it: do not ask for a
secret in chat and do not write a placeholder. Call `kit_tab_open`, then wait for the
answer, and never read the values back.

```json
{ "kit": "editor", "tab": "editor", "params": { "path": ".env", "keys": "STRIPE_KEY,DB_URL", "why": "For the payment tests." } }
```

| Param  | What                                                                       |
| ------ | -------------------------------------------------------------------------- |
| `path` | The file, relative to the workspace folder. Without it, the picker.        |
| `keys` | For a dotenv file, the keys to fill in, separated by commas.               |
| `why`  | One sentence, shown at the top.                                            |
| `line` | For a text file, the line to jump to.                                      |

Hangar keeps one tab per set of params. So ask for all your keys at once: the same file
with other `keys` opens a second tab, and `expected` keeps the two from overwriting each
other.

## How it is built

```
editor/
├── hangar-kit.json            the kit: id, hangar >=1.9.0, the tab, its params and permissions
├── icon.svg
├── .claude-plugin/plugin.json the Claude Code plugin (name = the kit's id)
├── skills/editor/SKILL.md
├── tabs/editor/
│   ├── index.html             loads Hangar UI and the SDK, then the scripts below
│   ├── dotenv.js              reads a dotenv file, and writes back only what changed
│   ├── answer.js              the only sentences an agent ever gets
│   ├── validate.js            JSON, YAML, TOML
│   ├── diff.js                lines compared, and line endings a textarea drops, restored
│   ├── gitignore.js           git's ignore rules
│   ├── logic.js               the words in en and fr, params, the picker's order
│   ├── controller.js          the behaviour; takes window.hangar as an argument
│   ├── view.js                draws a state with Hangar UI's classes, never markup
│   ├── app.js                 wires them in the page
│   └── style.css              the editor's layout, in Hangar UI's tokens
├── test/                      node --test, no dependency, a fake window.hangar over made-up files
└── package.json, run-tests.js let `node --test kits/editor/` find the tests on any Node
```

## Tests

```
node --test kits/editor/
```

No install step. No test reads a real file of yours: the fake Hangar holds its files in
memory, and the check of the ignore rules against `git check-ignore` runs in a temporary
folder.
