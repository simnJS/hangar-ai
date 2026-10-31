# Recette

A Hangar kit for the end of an agent's work. Agents keep finishing with « please
test this » in a chat nobody re-reads. With this kit the agent lists what only a
person can check, the person works through the list in a tab, and each result goes
back to the agent, who fixes what failed.

## What it does

- **The Recette tab.** It opens on a JSON file the agent wrote in the workspace, so
  a long list is no problem for the tab's params:

  ```json
  {
    "title": "Login and logout",
    "steps": [
      { "text": "Open /login, enter a wrong password.", "expect": "An error shows under the field." }
    ]
  }
  ```

  `text` is required, `expect` is what the person should see. Up to 200 steps. One
  tab per list.
- **Each step** is marked **OK**, **Problem** (with a note of what was seen) or
  **Skip**. The tab shows the progress, and its badge goes from running to success or
  failure. Answers are kept in the kit's own data folder, so they survive a restart.
- **« Send to agent »** sends the agent that opened the tab a plain summary: what
  failed, with the notes, then what was skipped or left unanswered. When the person
  opened the tab themselves, there is no agent to answer: the summary can be copied.
- **Testing a fix.** Once the result is sent, opening the same path again (the agent
  does it with `kit_tab_open`) keeps the answers and puts the failed steps back as
  « to redo », with the note of what went wrong.
- **A list that is missing or malformed** is named for what it is, with what to do,
  and nothing else changes. If saving is not allowed, the list still works and says
  its answers will not be kept.
- **A skill** telling agents when to use it: at the end of a work that needs a human
  check, write the list, open it with `kit_tab_open`, and stop until the answer
  arrives; write concrete steps with what to expect, never « check it works ».

English and French, from Hangar's language; light and dark with Hangar's own theme;
readable at the side panel's narrowest. It uses Hangar UI and loads nothing else.

## What it can touch

It runs nothing: no command, no hook, no script. The tab asks for five permissions,
shown to you before you enable the kit:

| Permission   | What it is for                                                                     |
| ------------ | ---------------------------------------------------------------------------------- |
| `files.read` | Read the one list file named in the tab's `path`, in the workspace folder. Hidden files and folders are refused by Hangar. It cannot change a project file. |
| `data.read`  | Read your answers back, from the kit's own data folder.                            |
| `data.write` | Write them there: one small file per list, under `answers/`.                       |
| `panes.send` | Send the summary to the agent that opened the tab, and to no one else.             |
| `notify`     | Tell you once that the result went out.                                            |

The summary holds the steps' text and your notes, and goes to that agent only.

## Install

You need Hangar 1.9.0 or later.

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/simnJS/hangar-ai/tree/main/kits/checklist`
   (or link a local copy of this folder).
2. Enable it on a workspace, and read what you accept: the five permissions above.
3. Restart the workspace's Claude Code panes so they load the skill.

## How it is built

```
checklist/
├── hangar-kit.json            the kit: id, hangar >=1.9.0, the tab, its `path` param, five permissions
├── icon.svg
├── .claude-plugin/plugin.json the Claude Code plugin (name = the kit's id), only a skill
├── skills/checklist/SKILL.md
├── tabs/checklist/
│   ├── index.html             Hangar UI (/_hangar/kit.css, kit.js, kit-ui.js), then the scripts below
│   ├── logic.js               pure: the list checked, answers read and kept, badge, summary; messages in en and fr
│   ├── controller.js          reads the list, saves within data.write's rate, sets badge and title, sends
│   ├── view.js                draws a state with createElement + textContent, never markup
│   ├── app.js                 wires them in the page, in Hangar's language
│   └── style.css              layout of a step only: every colour is Hangar UI's
├── test/                      node --test, no dependency, against a fake window.hangar
└── package.json, run-tests.js let `node --test kits/checklist/` find the tests on any Node
```

Choices worth knowing:

- **A step is known by its text.** Answers follow a step the agent moved, and only a
  step whose text changed loses its answer.
- **The answers' file is named after the list's path**, with a hash, so two lists never
  share one.
- **Writes are spread out.** `data.write` is limited to 10 a minute, so answers and notes
  are written after a short pause and never closer than 7 seconds, and once more when the
  tab is hidden.

## Tests

```
node --test kits/checklist/
```
