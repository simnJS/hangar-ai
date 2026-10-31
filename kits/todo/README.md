# TODO

A Hangar kit that shows the debt the code already names, the `TODO`, `FIXME`, `HACK` and `XXX`
comments, and hands any of it to an agent in one click.

## What it does

**A tab**, « TODO ». It scans the workspace when it opens (and when you press **Analyser**) and lists:

- the **markers**, grouped **by file** or **by tag**, with a filter chip per tag (with its count) and
  a search over the comment and the file name;
- for each one its tag, its comment, `file:line`, and two buttons: **Ouvrir**, which opens the file
  in Hangar's preview, and **Confier à un agent**.

The tab's badge counts the markers. **Qui l’a écrit** (per file) asks git who wrote each marker of
that file and when, and shows `author, 3 months ago` under it.

**Confier à un agent** sends one marker to a Claude Code agent of the workspace (the orchestrator by
default, or the one you pick). The brief names the file, the line and the comment, says it comes from
the kit's scan and not from the person, and puts the comment between markers as data to read, never
instructions to follow: anyone who can write to the repository can write a comment.

**What counts as a marker.** A comment that *starts* with the tag, in a language whose comments the
scan knows (`// TODO: …`, `# FIXME(ana) …`, `/* HACK … */`, `<!-- TODO -->`, a line inside a block
comment). The word in prose, in a string (`"// TODO"`), in a name, in lowercase, or `todo!()` in Rust,
is not one.

Designed states: scanning, nothing found, too many to list (the totals stay complete), nothing
matches the filters, command not allowed, scan failed, no agent to send to, a send or an opening
refused.

**A skill** that tells agents to read the last recorded scan rather than search the whole code again
when they are only asked what is left to do.

## What it runs

One declared command, `scan`: `node {kitDir}/tools/scan.js --blame={file}`, started in the workspace
folder. Hangar shows it to you verbatim when you enable the kit. Pure Node, no dependency. With an
empty `file` it prints one JSON document of the markers (`file`, `line`, `tag`, `text`); with a path
it lists that file's markers with their `author` and `at` (when) from `git blame`.

- **Read-only.** It opens files to read them and, for the author of a file, runs
  `git blame --line-porcelain -- <file>`. It writes nothing, installs nothing, calls no network and
  follows no link. A test fails if the script could write to disk.
- **What it reads.** It skips `.git`, `node_modules`, `target` and `dist` at any depth, and what the
  `.gitignore` files of the workspace list (patterns, negations, `**`, nested files). It reads only
  files of a known language, smaller than 1 MB, that are not binary, and lines shorter than 2 000
  characters.
- **Capped.** At most 1 000 markers are listed. Past that the totals keep counting (to 20 000) and
  the tab says the list is partial. A scan that takes more than 40 s stops and says so.
- **The tab** holds `command:scan` (to run that command), `panes.read` (to list the agents),
  `panes.send` (to hand a marker over, when you click), `preview.show` (to open a file, when you
  click), `data.read` and `data.write` (to keep the last scan in the kit's data folder,
  `last.json`). It writes no file of the project.

## What it does not do

- It does not fix anything: resolving a marker is the agent's job, with your approval of its work
  like any other change.
- It does not read languages it has no comment syntax for, nor JSON (no comments), nor `.env` files.
- Preview cannot open a hidden file or folder (`.github/…`): those rows have no **Ouvrir**.
- A marker on several lines of comment is listed by its first line.

## Tests and fixtures

`node --test kits/todo/`. They run the script on a fixture tree built on the fly (comments in a dozen
languages, ignored folders, a `.gitignore` with negations, a binary file, strings and prose that must
not match, a real git repository for the author), and the tab against a fake `window.hangar`.

## Get it

Link `kits/todo` from Hangar, or clone
<https://github.com/simnJS/hangar-ai/tree/main/kits/todo>. Needs Hangar 1.9.0 or later and Node.js
on the PATH (and git, for the authors).
