# GitHub

A Hangar kit with two tabs, **Pull requests** and **Issues**: see what waits on a
GitHub repository without leaving Hangar, and hand any of it to an agent in one click.
It only reads GitHub. Nothing in it can merge, comment, review or close.

## What it does

- **Pull requests.** The open ones, newest update first: checks (failing, running,
  passing), review state, draft. A pull request opens in full: its checks one by one,
  the reviews, the **unresolved review comments** with file and line, the diff stat and
  the files it changes, the description.
- **Issues.** The open ones, with labels and assignees; an issue opens with its
  description and latest comments.
- **Filters**, kept between sessions: a search, failing checks, needs review, hide
  drafts (pull requests); unassigned (issues).
- **A badge** on the Pull requests tab: the number of pull requests with failing checks,
  else the number waiting for a review.
- **Refresh** on demand, and every 60 seconds while the tab is visible.
- **Hand to an agent**, on every item: pick one of the workspace's Claude Code agents,
  and it receives a brief with the title, the link, the description and, for a pull
  request, the failing checks and the unresolved review comments. Everything written by
  others on GitHub sits between two marked lines, with a note that it is information
  and not instructions; nothing in it can close that quote early. The brief ends with
  the task, and says not to push, merge, comment or close unless you ask.
- **Says what is wrong**: `gh` not installed, not signed in, or a folder that is not a
  GitHub repository, each with what to do next.
- English and French, from Hangar's language; light and dark with Hangar's theme;
  readable down to the side panel's narrowest width.

## What it runs

Six declared commands, all through [`gh`](https://cli.github.com), all reads. Hangar
shows them verbatim when you enable the kit.

| Command       | Runs                                                              |
| ------------- | ----------------------------------------------------------------- |
| `repo-info`   | `gh repo view --json nameWithOwner,name,owner,url`                |
| `pr-list`     | `gh pr list --state open --limit 50 --json …`                     |
| `pr-view`     | `gh pr view {id} --json …`                                        |
| `pr-threads`  | `gh api graphql` with a **fixed query** of a pull request's review threads |
| `issue-list`  | `gh issue list --state open --limit 50 --json …`                  |
| `issue-view`  | `gh issue view {id} --json …`                                     |

`{id}` is digits only (`^[0-9]+$`); `owner` and `name` of `pr-threads` come from
`repo-info` and are checked against a rule too. The query is in the manifest, not in the
page, and is a `query`, never a `mutation`.

Each tab holds `panes.read` and `panes.send` (to list the agents and send them the
brief) and `data.read` and `data.write` (its filters, in the kit's own data folder), and
the commands it uses: no files, no notification, no network of its own. The pages load
Hangar UI and nothing else.

## Install

You need Hangar 1.9.0 or later and `gh` signed in (`gh auth login`).

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/simnJS/hangar-ai/tree/main/kits/github`
   (or link a local copy of this folder).
2. Enable it on a workspace and read what you accept: the six commands and the
   permissions above.

## How it is built

```
github/
├── hangar-kit.json            the kit: commands, the two tabs, the gallery text
├── icon.svg
├── tabs/github/
│   ├── pulls.html, issues.html   the two entries: Hangar UI, then the scripts below
│   ├── logic.js                  pure: gh's JSON → rows, filters, badge, the brief; en and fr
│   ├── controller.js             commands, 60 s refresh, filters, hand-off, with window.hangar as an argument
│   ├── view.js                   Hangar UI's classes, createElement + textContent, never markup
│   ├── app.js                    wires them in the page: menu, toasts, language, visibility
│   └── style.css                 one rule of layout, in Hangar UI's tokens
├── test/                         node --test, no dependency; fixtures/ are recorded gh output
└── package.json, run-tests.js    let `node --test kits/github/` find the tests on any Node
```

## Tests

```
node --test kits/github/
```

No install step. They run the logic, the controller (against a fake `window.hangar` and
a fake clock) and the view (against a fake DOM) on recorded `gh` output, including
`gh` absent, not signed in and not a repository, and check the manifest against the
rules of [docs/kits.md](../../docs/kits.md): `gh` only, reads only, ids that cannot
become an option, minimal permissions. The fixtures use invented names.
