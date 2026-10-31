# Annoter

A Hangar kit. Describing a visual problem in words is slow. The agent shows a capture, or you
paste one; you circle what is wrong, write a word beside it, and the agent receives the
annotated image and your notes.

## What it does

- **Opens an image three ways**: from the `path` an agent gives, pasted with Ctrl+V, or dropped
  on the tab. A button picks a file too. PNG, JPEG, WebP and GIF; never an SVG or a PDF.
- **Four marks, nothing else**: an arrow, a rectangle, a freehand stroke, and a numbered note
  whose word you write in the list under the image. Five colours, undo, and a zoom that fits the
  image to the panel by default. It is a canvas and no dependency.
- **« Envoyer » (Send) writes two files** in the workspace: the annotated PNG at
  `.hangar/annotations/<date>-<time>.png`, and `<date>-<time>.json` beside it, with the comment,
  the numbered notes and a line per mark. It never replaces a file: a taken name gets `-2`.
  An image too heavy for 10 MB is written at a smaller size, and the message says so.
- **Then it tells the agent** with one message: the path of the PNG (so the agent can read it),
  how many marks of each kind, your comment, and each note with its position in the image's
  pixels. It goes to the agent that opened the tab; otherwise to the pane you name. With no
  pane, the button reads « Enregistrer » (Save): the files are saved and the path shown.
- **English and French**, in Hangar's language; light and dark, in Hangar's theme; legible at the
  side panel's narrowest.

## What it can touch

Three permissions, shown to you before you enable the kit. It runs nothing by itself: no hook,
no MCP server, no command.

| Permission    | What for                                                                                  |
| ------------- | ----------------------------------------------------------------------------------------- |
| `files.read`  | Read the image an agent named by its `path`.                                              |
| `files.write` | Write the annotated PNG and its JSON under `.hangar/annotations/`, never over a file.     |
| `panes.send`  | Tell the agent that asked, or the pane you named, where the image is.                     |

Hangar words `files.write` as changing the workspace's text files. This kit also makes new images
with it (`files.writeBinary`): PNG, JPEG or WebP only, whose first bytes are checked, never
replacing a file, 10 MB at most. Nothing under `.git` can be reached, and nothing leaves the
machine.

**What reaches the agent:** the image's path, counts, your comment and your notes, as you wrote
them, in sentences fixed in [`tabs/annotate/answer.js`](tabs/annotate/answer.js).

What it cannot do: it cannot list the workspace's panes (that needs `panes.read`, one more
permission than it asks for), so without a caller you type the pane's name. Drawing is by pointer;
by keyboard you can write the comment and the notes' words but not place a mark.

## Install

You need Hangar 1.9.0 or later, a version that has `files.writeBinary`.

1. In Hangar, Settings › Kits, add the GitHub link:
   `https://github.com/simnJS/hangar-ai/tree/main/kits/annotate`
   (or link a local copy of this folder).
2. Enable it on a workspace, and read what you accept: the three permissions above.
3. Restart the workspace's Claude Code panes, so they load the skill.

## For agents

The skill in [`skills/annotate/SKILL.md`](skills/annotate/SKILL.md) says it: when you need the
person's eye on something visual, take the capture inside the workspace folder, open it with
`kit_tab_open`, then wait for the answer and read the PNG it names.

```json
{ "kit": "annotate", "tab": "annotate", "params": { "path": ".hangar/capture.png" } }
```

| Param  | What                                                                                 |
| ------ | ------------------------------------------------------------------------------------ |
| `path` | The image, relative to the workspace folder. Without it, the tab waits for a paste.  |

## How it is built

```
annotate/
├── hangar-kit.json            the kit: id, hangar >=1.9.0, the tab, its param and permissions
├── icon.svg
├── .claude-plugin/plugin.json the Claude Code plugin (name = the kit's id)
├── skills/annotate/SKILL.md
├── tabs/annotate/
│   ├── index.html             loads Hangar UI and the SDK, then the scripts below
│   ├── logic.js               the words in en and fr, file names, zoom
│   ├── model.js               the marks, and how they are drawn on any 2D context
│   ├── answer.js              the message the agent gets, and the JSON beside the image
│   ├── controller.js          the behaviour; takes window.hangar as an argument
│   ├── view.js                draws a state with Hangar UI's classes, never markup
│   ├── app.js                 wires them in the page: decoding, paste, drop, the PNG
│   └── style.css              the canvas and the swatches, in Hangar UI's tokens
├── test/                      node --test, no dependency, a fake window.hangar over memory
└── package.json, run-tests.js let `node --test kits/annotate/` find the tests on any Node
```

The only colours that are not Hangar UI's tokens are the five the person picks from to mark the
image, in `model.js`. A test checks that nothing else in the tab writes a colour.

## Tests

```
node --test kits/annotate/
```

No install step. No test writes a real file: the fake Hangar holds its files in memory, and the
canvas is a recorder that lists what is drawn on it.
